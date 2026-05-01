use crate::error::{AppError, StorageError};
use crate::storage::models::PowerSample;
use sqlx::SqlitePool;

pub async fn insert(
    pool: &SqlitePool,
    sampled_at: i64,
    production_w: f64,
    consumption_w: f64,
    grid_w: f64,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO power_sample (sampled_at, production_w, consumption_w, grid_w)
         VALUES (?, ?, ?, ?)",
    )
    .bind(sampled_at)
    .bind(production_w)
    .bind(consumption_w)
    .bind(grid_w)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(())
}

pub async fn query_range(
    pool: &SqlitePool,
    start: i64,
    end: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<PowerSample>, AppError> {
    sqlx::query_as::<_, PowerSample>(
        "SELECT id, sampled_at, production_w, consumption_w, grid_w
         FROM power_sample
         WHERE sampled_at >= ? AND sampled_at < ?
         ORDER BY sampled_at ASC
         LIMIT ? OFFSET ?",
    )
    .bind(start)
    .bind(end)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn delete_before(pool: &SqlitePool, cutoff: i64) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM power_sample WHERE sampled_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(result.rows_affected())
}
