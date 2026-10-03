use crate::error::{AppError, StorageError};
use crate::storage::models::{ScheduleVersion, TouRateSchedule};
use crate::tou::openei_client::FetchedSchedule;
use sqlx::SqlitePool;

pub async fn query_latest(
    pool: &SqlitePool,
    rate_label: &str,
    utility_eia_id: u32,
) -> Result<Option<TouRateSchedule>, AppError> {
    sqlx::query_as::<_, TouRateSchedule>(
        "SELECT id, fetched_at, effective_date, utility_name, rate_label, rate_json
         FROM tou_rate_schedule WHERE rate_label = ? AND utility_eia_id = ?
         ORDER BY fetched_at DESC, id DESC LIMIT 1",
    )
    .bind(rate_label)
    .bind(utility_eia_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn insert_history(
    pool: &SqlitePool,
    utility_eia_id: u32,
    fetched_at: i64,
    revisions: &[FetchedSchedule],
) -> Result<Vec<i64>, AppError> {
    let mut transaction = pool.begin().await.map_err(StorageError::Database)?;
    let mut ids = Vec::new();
    for revision in revisions {
        let result = sqlx::query("INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json, utility_eia_id, source_id, effective_start, effective_end) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(fetched_at).bind(&revision.effective_date).bind(&revision.utility_name).bind(&revision.rate_label)
            .bind(&revision.rate_json).bind(utility_eia_id).bind(&revision.source_id)
            .bind(revision.effective_start).bind(revision.effective_end)
            .execute(&mut *transaction).await.map_err(StorageError::Database)?;
        ids.push(result.last_insert_rowid());
    }
    transaction.commit().await.map_err(StorageError::Database)?;
    Ok(ids)
}

pub async fn query_history(
    pool: &SqlitePool,
    utility_eia_id: u32,
    rate_label: &str,
) -> Result<Vec<ScheduleVersion>, AppError> {
    sqlx::query_as::<_, ScheduleVersion>("SELECT * FROM tou_rate_schedule WHERE utility_eia_id = ? AND rate_label = ? AND source_id IS NOT NULL AND effective_start IS NOT NULL ORDER BY effective_start, fetched_at, id")
        .bind(utility_eia_id).bind(rate_label).fetch_all(pool).await.map_err(|e| AppError::Storage(StorageError::Database(e)))
}
