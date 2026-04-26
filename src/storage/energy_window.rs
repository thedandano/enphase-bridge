use crate::error::{AppError, StorageError};
use crate::storage::models::EnergyWindow;
use sqlx::SqlitePool;

pub async fn insert(pool: &SqlitePool, window: &EnergyWindow) -> Result<(), AppError> {
    sqlx::query(
        "INSERT OR IGNORE INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(window.window_start)
    .bind(window.wh_produced)
    .bind(window.wh_consumed)
    .bind(window.wh_grid_import)
    .bind(window.wh_grid_export)
    .bind(window.is_complete)
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
) -> Result<Vec<EnergyWindow>, AppError> {
    sqlx::query_as::<_, EnergyWindow>(
        "SELECT id, window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete
         FROM energy_window
         WHERE window_start >= ? AND window_start < ?
         ORDER BY window_start ASC LIMIT ? OFFSET ?",
    )
    .bind(start)
    .bind(end)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn query_latest(pool: &SqlitePool) -> Result<Option<EnergyWindow>, AppError> {
    sqlx::query_as::<_, EnergyWindow>(
        "SELECT id, window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete
         FROM energy_window ORDER BY window_start DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}
