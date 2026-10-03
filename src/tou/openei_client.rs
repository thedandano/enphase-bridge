use crate::error::{AppError, TouError};
use crate::storage::models::TouRateSchedule;
use crate::trueup::calculator::ScheduleResolver;
use chrono::TimeZone;
use reqwest::Client;
use std::collections::HashSet;

pub struct OpenEiClient {
    client: Client,
    api_key: String,
    utility_eia_id: u32,
    rate_label: String,
    base_url: String,
}

#[derive(Debug)]
pub struct FetchedSchedule {
    pub utility_name: String,
    pub rate_label: String,
    pub effective_date: Option<String>,
    pub rate_json: String,
    pub source_id: String,
    pub effective_start: i64,
    pub effective_end: Option<i64>,
}

impl OpenEiClient {
    pub fn with_base_url(
        api_key: String,
        utility_eia_id: u32,
        rate_label: String,
        base_url: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            utility_eia_id,
            rate_label,
            base_url,
        }
    }

    pub async fn fetch_history(&self) -> Result<Vec<FetchedSchedule>, AppError> {
        const PAGE_SIZE: usize = 500;
        let mut revisions = Vec::new();
        let mut offset = 0;
        let mut seen_pages = HashSet::new();
        loop {
            let response = self
                .client
                .get(format!("{}/utility_rates", self.base_url))
                .query(&[
                    ("version", "7".to_string()),
                    ("format", "json".to_string()),
                    ("eia", self.utility_eia_id.to_string()),
                    ("sector", "Residential".to_string()),
                    ("detail", "full".to_string()),
                    ("api_key", self.api_key.clone()),
                    ("limit", PAGE_SIZE.to_string()),
                    ("offset", offset.to_string()),
                    ("orderby", "label".to_string()),
                    ("direction", "asc".to_string()),
                ])
                .send()
                .await
                .map_err(|e| {
                    AppError::Tou(TouError::UpstreamUnavailable(e.without_url().to_string()))
                })?;
            if !response.status().is_success() {
                return Err(AppError::Tou(TouError::UpstreamUnavailable(format!(
                    "OpenEI returned {}",
                    response.status()
                ))));
            }
            let body: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AppError::Tou(TouError::ParseError(e.without_url().to_string())))?;
            let items = body["items"]
                .as_array()
                .ok_or_else(|| parse_error("missing items array"))?;
            if !seen_pages.insert(body["items"].to_string()) {
                return Err(parse_error(
                    "OpenEI repeated a page; history refresh aborted",
                ));
            }
            for item in items
                .iter()
                .filter(|item| item["name"].as_str() == Some(&self.rate_label))
            {
                revisions.push(self.parse_item(item)?);
            }
            if items.len() < PAGE_SIZE {
                break;
            }
            offset += PAGE_SIZE;
        }
        if revisions.is_empty() {
            tracing::error!(event="tou_rate_not_found", rate_label=%self.rate_label, utility_eia_id=self.utility_eia_id);
            return Err(parse_error("configured plan not found in OpenEI response"));
        }
        revisions.sort_by_key(|revision| revision.effective_start);
        Ok(revisions)
    }

    fn parse_item(&self, item: &serde_json::Value) -> Result<FetchedSchedule, AppError> {
        let source_id = item["label"]
            .as_str()
            .filter(|label| !label.is_empty())
            .ok_or_else(|| parse_error("missing revision label"))?
            .to_string();
        let effective_start = item["startdate"]
            .as_i64()
            .ok_or_else(|| parse_error("missing integer startdate"))?;
        let start = chrono::Utc
            .timestamp_opt(effective_start, 0)
            .single()
            .ok_or_else(|| parse_error("invalid revision startdate"))?;
        let effective_end = match item.get("enddate") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .filter(|end| chrono::Utc.timestamp_opt(*end, 0).single().is_some())
                    .ok_or_else(|| parse_error("invalid revision enddate"))?,
            ),
        };
        if effective_end.is_some_and(|end| end <= effective_start) {
            tracing::warn!(event="tou_revision_invalid_bounds", source_id=%source_id, effective_start, effective_end=?effective_end, "archiving revision without using it for historical coverage");
        }
        let eia = item
            .get("eiaid")
            .ok_or_else(|| parse_error("missing revision utility identity"))?;
        // OpenEI may encode EIA IDs as numbers, strings, or arrays.
        let matches = match eia {
            serde_json::Value::Array(ids) => {
                ids.iter().any(|id| eia_matches(id, self.utility_eia_id))
            }
            id => eia_matches(id, self.utility_eia_id),
        };
        if !matches {
            return Err(parse_error(
                "revision utility identity differs from requested utility",
            ));
        }
        let utility_name = item["utility"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| parse_error("missing utility name"))?
            .to_string();
        let fetched = FetchedSchedule {
            source_id,
            effective_start,
            effective_end,
            utility_name,
            rate_label: self.rate_label.clone(),
            effective_date: Some(start.format("%Y-%m-%d").to_string()),
            rate_json: item.to_string(),
        };
        ScheduleResolver::new(&TouRateSchedule {
            id: 0,
            fetched_at: 0,
            effective_date: fetched.effective_date.clone(),
            utility_name: fetched.utility_name.clone(),
            rate_label: fetched.rate_label.clone(),
            rate_json: fetched.rate_json.clone(),
        })?;
        Ok(fetched)
    }
}

fn eia_matches(value: &serde_json::Value, expected: u32) -> bool {
    value.as_u64() == Some(u64::from(expected))
        || value.as_str().and_then(|id| id.parse::<u32>().ok()) == Some(expected)
}

fn parse_error(message: &str) -> AppError {
    AppError::Tou(TouError::ParseError(message.to_string()))
}
