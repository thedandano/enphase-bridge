use crate::error::{AppError, StorageError};
use crate::storage::models::TrueUpEstimate;
use sqlx::SqlitePool;

pub async fn insert(pool: &SqlitePool, estimate: &TrueUpEstimate) -> Result<i64, AppError> {
    let result = sqlx::query(
        "INSERT INTO true_up_estimate
         (computed_at, period_start, period_end, net_cost_usd,
          peak_import_kwh, peak_export_kwh,
          offpeak_import_kwh, offpeak_export_kwh,
          super_offpeak_import_kwh, super_offpeak_export_kwh,
          tou_schedule_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(estimate.computed_at)
    .bind(estimate.period_start)
    .bind(estimate.period_end)
    .bind(estimate.net_cost_usd)
    .bind(estimate.peak_import_kwh)
    .bind(estimate.peak_export_kwh)
    .bind(estimate.offpeak_import_kwh)
    .bind(estimate.offpeak_export_kwh)
    .bind(estimate.super_offpeak_import_kwh)
    .bind(estimate.super_offpeak_export_kwh)
    .bind(estimate.tou_schedule_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    Ok(result.last_insert_rowid())
}
