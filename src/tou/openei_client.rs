use crate::error::{AppError, TouError};
use reqwest::Client;

pub struct OpenEiClient {
    client: Client,
    api_key: String,
    utility_eia_id: u32,
    rate_label: String,
}

pub struct FetchedSchedule {
    pub utility_name: String,
    pub rate_label: String,
    pub effective_date: Option<String>,
    pub rate_json: String,
}

impl OpenEiClient {
    pub fn new(api_key: String, utility_eia_id: u32, rate_label: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            utility_eia_id,
            rate_label,
        }
    }

    pub async fn fetch(&self) -> Result<FetchedSchedule, AppError> {
        let url = format!(
            "https://api.openei.org/utility_rates?version=7&format=json\
             &eia={}&sector=Residential&detail=full&api_key={}",
            self.utility_eia_id, self.api_key
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Tou(TouError::UpstreamUnavailable(e.to_string())))?;

        if !resp.status().is_success() {
            return Err(AppError::Tou(TouError::UpstreamUnavailable(format!(
                "OpenEI returned {}",
                resp.status()
            ))));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Tou(TouError::ParseError(e.to_string())))?;

        let items = body["items"].as_array().ok_or_else(|| {
            AppError::Tou(TouError::ParseError(
                "missing 'items' array in OpenEI response".into(),
            ))
        })?;

        // Match on the `name` field (human-readable), not `label` (internal doc ID).
        // Pick the most recently fetched version if duplicates exist.
        let item = items
            .iter()
            .filter(|i| i["name"].as_str().unwrap_or("") == self.rate_label)
            .max_by_key(|i| i["startdate"].as_i64().unwrap_or(0))
            .ok_or_else(|| {
                AppError::Tou(TouError::ParseError(format!(
                    "rate '{}' not found in OpenEI response for utility EIA {}",
                    self.rate_label, self.utility_eia_id
                )))
            })?;

        let effective_date = item["startdate"].as_i64().and_then(|ts| {
            use chrono::TimeZone;
            chrono::Utc
                .timestamp_opt(ts, 0)
                .single()
                .map(|dt| dt.format("%Y-%m-%d").to_string())
        });

        let utility_name = item["utility"]
            .as_str()
            .unwrap_or("San Diego Gas & Electric")
            .to_string();

        tracing::info!(
            event = "tou_schedule_fetched",
            rate_label = %self.rate_label,
            effective_date = ?effective_date,
        );

        Ok(FetchedSchedule {
            utility_name,
            rate_label: self.rate_label.clone(),
            effective_date,
            rate_json: item.to_string(),
        })
    }
}
