use crate::error::{AppError, StorageError};
use crate::storage::models::TouRateSchedule;
use sqlx::SqlitePool;

pub async fn insert(
    pool: &SqlitePool,
    fetched_at: i64,
    effective_date: Option<&str>,
    utility_name: &str,
    rate_label: &str,
    rate_json: &str,
) -> Result<i64, AppError> {
    let result = sqlx::query(
        "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(fetched_at)
    .bind(effective_date)
    .bind(utility_name)
    .bind(rate_label)
    .bind(rate_json)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    Ok(result.last_insert_rowid())
}

pub async fn query_latest(
    pool: &SqlitePool,
    rate_label: &str,
) -> Result<Option<TouRateSchedule>, AppError> {
    sqlx::query_as::<_, TouRateSchedule>(
        "SELECT id, fetched_at, effective_date, utility_name, rate_label, rate_json
         FROM tou_rate_schedule WHERE rate_label = ?
         ORDER BY fetched_at DESC LIMIT 1",
    )
    .bind(rate_label)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}
