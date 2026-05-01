use crate::error::{AppError, StorageError};
use crate::storage::models::BoundarySnapshot;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    AlreadyExists,
}

#[allow(dead_code)]
pub async fn insert(
    pool: &SqlitePool,
    window_start: i64,
    production_wh: f64,
    grid_import_cum_wh: f64,
    grid_export_cum_wh: f64,
    captured_at: i64,
    raw_meters_json: &str,
) -> Result<InsertOutcome, AppError> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO boundary_snapshot
         (window_start, production_wh, grid_import_cum_wh, grid_export_cum_wh, captured_at, raw_meters_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(window_start)
    .bind(production_wh)
    .bind(grid_import_cum_wh)
    .bind(grid_export_cum_wh)
    .bind(captured_at)
    .bind(raw_meters_json)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    if result.rows_affected() == 1 {
        Ok(InsertOutcome::Inserted)
    } else {
        Ok(InsertOutcome::AlreadyExists)
    }
}

/// Returns (prev, curr) where curr.window_start == window_start and prev.window_start == window_start - 900.
/// Returns None if either row is absent — a multi-window gap would corrupt recomputed values.
pub async fn query_pair(
    pool: &SqlitePool,
    window_start: i64,
) -> Result<Option<(BoundarySnapshot, BoundarySnapshot)>, AppError> {
    let prev_start = window_start - 900;

    let curr = sqlx::query_as::<_, BoundarySnapshot>(
        "SELECT id, window_start, production_wh, grid_import_cum_wh, grid_export_cum_wh, captured_at, raw_meters_json
         FROM boundary_snapshot WHERE window_start = ?",
    )
    .bind(window_start)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    let prev = sqlx::query_as::<_, BoundarySnapshot>(
        "SELECT id, window_start, production_wh, grid_import_cum_wh, grid_export_cum_wh, captured_at, raw_meters_json
         FROM boundary_snapshot WHERE window_start = ?",
    )
    .bind(prev_start)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    match (prev, curr) {
        (Some(p), Some(c)) => Ok(Some((p, c))),
        _ => Ok(None),
    }
}

/// Reserved for future optional pruning. NOT called by default; boundary_snapshot is permanent.
#[allow(dead_code)]
pub async fn delete_before(pool: &SqlitePool, cutoff: i64) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM boundary_snapshot WHERE window_start < ?")
        .bind(cutoff)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(result.rows_affected())
}
