use crate::error::{AppError, TouError};
use crate::storage::{
    models::{EnergyWindow, ScheduleVersion},
    tou_schedule,
};
use crate::trueup::calculator::{
    self, CalculatorResult, PeriodSummary, ScheduleResolver, TouPeriod,
};
use chrono::{LocalResult, Offset, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleMeta {
    pub id: i64,
    pub source_id: String,
    pub utility_eia_id: i64,
    pub rate_label: String,
    pub effective_date: Option<String>,
    pub effective_start: i64,
    pub effective_end: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TouInterval {
    pub start: i64,
    pub end: i64,
    pub bracket: TouPeriod,
    pub schedule_id: i64,
}

pub struct History {
    versions: Vec<ScheduleVersion>,
}

impl History {
    pub async fn load(pool: &SqlitePool, utility: u32, label: &str) -> Result<Self, AppError> {
        let rows = tou_schedule::query_history(pool, utility, label).await?;
        // A later fetch of one upstream revision supersedes its earlier stored copies.
        let mut by_source: HashMap<String, ScheduleVersion> = HashMap::new();
        for row in rows {
            match by_source.get(&row.source_id) {
                Some(old)
                    if (old.schedule.fetched_at, old.schedule.id)
                        > (row.schedule.fetched_at, row.schedule.id) => {}
                _ => {
                    by_source.insert(row.source_id.clone(), row);
                }
            }
        }
        let mut versions: Vec<_> = by_source
            .into_values()
            .filter(|version| {
                version
                    .effective_end
                    .is_none_or(|end| end > version.effective_start)
            })
            .collect();
        versions.sort_by_key(|v| (v.effective_start, v.schedule.fetched_at, v.schedule.id));
        Ok(Self { versions })
    }

    fn version_at(&self, timestamp: i64) -> Result<usize, AppError> {
        let index = self
            .versions
            .partition_point(|v| v.effective_start <= timestamp)
            .checked_sub(1)
            .ok_or_else(|| unavailable(timestamp))?;
        let version = &self.versions[index];
        if version.effective_end.is_some_and(|end| timestamp >= end) {
            return Err(unavailable(timestamp));
        }
        Ok(index)
    }

    fn meta(&self, index: usize) -> ScheduleMeta {
        let version = &self.versions[index];
        ScheduleMeta {
            id: version.schedule.id,
            source_id: version.source_id.clone(),
            utility_eia_id: version.utility_eia_id,
            rate_label: version.schedule.rate_label.clone(),
            effective_date: version.schedule.effective_date.clone(),
            effective_start: version.effective_start,
            effective_end: match (version.effective_end, self.versions.get(index + 1)) {
                (Some(end), Some(next)) => Some(end.min(next.effective_start)),
                (None, Some(next)) => Some(next.effective_start),
                (end, None) => end,
            },
        }
    }

    fn revision_boundaries(&self, start: i64, end: i64) -> BTreeSet<i64> {
        let mut boundaries = BTreeSet::from([start, end]);
        for version in &self.versions {
            for boundary in [Some(version.effective_start), version.effective_end]
                .into_iter()
                .flatten()
            {
                if boundary > start && boundary < end {
                    boundaries.insert(boundary);
                }
            }
        }
        boundaries
    }

    pub fn intervals(
        &self,
        start: i64,
        end: i64,
        timezone: Tz,
    ) -> Result<(Vec<TouInterval>, Vec<ScheduleMeta>), AppError> {
        let mut boundaries = self.revision_boundaries(start, end);
        add_calendar_boundaries(&mut boundaries, start, end, timezone)?;
        let boundaries: Vec<_> = boundaries.into_iter().collect();
        let mut intervals: Vec<TouInterval> = Vec::new();
        let mut resolvers = HashMap::new();
        let mut used = BTreeSet::new();
        for pair in boundaries.windows(2) {
            let index = self.version_at(pair[0])?;
            if let std::collections::hash_map::Entry::Vacant(entry) = resolvers.entry(index) {
                entry.insert(ScheduleResolver::new(&self.versions[index].schedule)?);
            }
            let bracket = resolvers[&index].bracket_at(pair[0], timezone)?;
            let schedule_id = self.versions[index].schedule.id;
            used.insert(index);
            if let Some(last) = intervals.last_mut()
                && last.bracket == bracket
                && last.schedule_id == schedule_id
            {
                last.end = pair[1];
            } else {
                intervals.push(TouInterval {
                    start: pair[0],
                    end: pair[1],
                    bracket,
                    schedule_id,
                });
            }
        }
        Ok((
            intervals,
            used.into_iter().map(|index| self.meta(index)).collect(),
        ))
    }

    pub fn ensure_coverage(&self, start: i64, end: i64) -> Result<(), AppError> {
        for timestamp in self
            .revision_boundaries(start, end)
            .into_iter()
            .filter(|t| *t < end)
        {
            self.version_at(timestamp)?;
        }
        Ok(())
    }

    pub fn calculate(
        &self,
        start: i64,
        end: i64,
        timezone: Tz,
        windows: &[EnergyWindow],
    ) -> Result<(CalculatorResult, Vec<ScheduleMeta>), AppError> {
        self.ensure_coverage(start, end)?;
        let mut groups: BTreeMap<usize, Vec<EnergyWindow>> = BTreeMap::new();
        for window in windows {
            groups
                .entry(self.version_at(window.window_start)?)
                .or_default()
                .push(window.clone());
        }
        let mut total = CalculatorResult::default();
        let mut schedules = Vec::new();
        for (index, windows) in groups {
            let result = calculator::calculate(&self.versions[index].schedule, timezone, &windows)?;
            add_summary(&mut total.peak, &result.peak);
            add_summary(&mut total.off_peak, &result.off_peak);
            add_summary(&mut total.super_off_peak, &result.super_off_peak);
            total.net_cost_usd += result.net_cost_usd;
            schedules.push(self.meta(index));
        }
        Ok((total, schedules))
    }
}

fn add_summary(total: &mut PeriodSummary, value: &PeriodSummary) {
    total.import_kwh += value.import_kwh;
    total.export_kwh += value.export_kwh;
    total.import_cost_usd += value.import_cost_usd;
    total.export_credit_usd += value.export_credit_usd;
}

fn unavailable(timestamp: i64) -> AppError {
    AppError::Tou(TouError::HistoryUnavailable(format!(
        "no verified revision covers Unix timestamp {timestamp}; refresh TOU history"
    )))
}

fn instant(timestamp: i64) -> Result<chrono::DateTime<chrono::Utc>, AppError> {
    chrono::Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or_else(|| {
            AppError::Tou(TouError::ParseError(format!(
                "invalid timestamp {timestamp}"
            )))
        })
}

fn add_calendar_boundaries(
    boundaries: &mut BTreeSet<i64>,
    start: i64,
    end: i64,
    timezone: Tz,
) -> Result<(), AppError> {
    let first = instant(start)?;
    let last = instant(end)?;
    let mut date = first
        .with_timezone(&timezone)
        .date_naive()
        .pred_opt()
        .ok_or_else(|| unavailable(start))?;
    let last_date = last
        .with_timezone(&timezone)
        .date_naive()
        .succ_opt()
        .ok_or_else(|| unavailable(end))?;
    while date <= last_date {
        for hour in 0..24 {
            let local = date
                .and_hms_opt(hour, 0, 0)
                .ok_or_else(|| unavailable(start))?;
            let candidates = match timezone.from_local_datetime(&local) {
                LocalResult::None => [None, None],
                LocalResult::Single(dt) => [Some(dt.timestamp()), None],
                LocalResult::Ambiguous(a, b) => [Some(a.timestamp()), Some(b.timestamp())],
            };
            for timestamp in candidates.into_iter().flatten() {
                if timestamp > start && timestamp < end {
                    boundaries.insert(timestamp);
                }
            }
        }
        date = date.succ_opt().ok_or_else(|| unavailable(end))?;
    }
    // Offset changes can occur between local hourly boundaries (e.g. Lord Howe's half-hour DST).
    let offset = |timestamp| -> Result<i32, AppError> {
        Ok(instant(timestamp)?
            .with_timezone(&timezone)
            .offset()
            .fix()
            .local_minus_utc())
    };
    let mut left = start;
    while left < end {
        let right = (left + 3600).min(end);
        if offset(left)? != offset(right)? {
            let mut lo = left;
            let mut hi = right;
            let old_offset = offset(left)?;
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if offset(mid)? == old_offset {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if hi < end {
                boundaries.insert(hi);
            }
        }
        left = right;
    }
    Ok(())
}
