use crate::collector::window_aggregator::CURRENT_FORMULA_VERSION;
use crate::error::{AppError, StorageError};
use crate::storage::models::EnergyWindow;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaFilter {
    All,
    CurrentOnly,
    Recomputable,
}

// Task 4.1: bind formula_version in INSERT
pub async fn insert(pool: &SqlitePool, window: &EnergyWindow) -> Result<(), AppError> {
    sqlx::query(
        "INSERT OR IGNORE INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(window.window_start)
    .bind(window.wh_produced)
    .bind(window.wh_consumed)
    .bind(window.wh_grid_import)
    .bind(window.wh_grid_export)
    .bind(window.is_complete)
    .bind(window.formula_version)
    .bind(window.was_clamped)
    .bind(window.avg_production_w)
    .bind(window.avg_consumption_w)
    .bind(window.avg_grid_w)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(())
}

const RANGE_SELECT: &str =
    "SELECT id, window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w
     FROM energy_window WHERE window_start >= ? AND window_start < ?";
const RANGE_ORDER: &str = "ORDER BY window_start ASC LIMIT ? OFFSET ?";

// Task 4.2: accept FormulaFilter; bind CURRENT_FORMULA_VERSION as a parameter for CurrentOnly
// Task 4.3: SELECT includes formula_version
pub async fn query_range(
    pool: &SqlitePool,
    start: i64,
    end: i64,
    limit: i32,
    offset: i32,
    formula_filter: FormulaFilter,
) -> Result<Vec<EnergyWindow>, AppError> {
    let rows = match formula_filter {
        FormulaFilter::All => {
            sqlx::query_as::<_, EnergyWindow>(&format!("{} {}", RANGE_SELECT, RANGE_ORDER))
                .bind(start)
                .bind(end)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
        }
        FormulaFilter::CurrentOnly => {
            sqlx::query_as::<_, EnergyWindow>(&format!(
                "{} AND formula_version = ? {}",
                RANGE_SELECT, RANGE_ORDER
            ))
            .bind(start)
            .bind(end)
            .bind(CURRENT_FORMULA_VERSION)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        FormulaFilter::Recomputable => {
            sqlx::query_as::<_, EnergyWindow>(&format!(
                "{} AND formula_version > 0 {}",
                RANGE_SELECT, RANGE_ORDER
            ))
            .bind(start)
            .bind(end)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(rows)
}

// Task 4.3: SELECT includes formula_version
pub async fn query_latest(pool: &SqlitePool) -> Result<Option<EnergyWindow>, AppError> {
    sqlx::query_as::<_, EnergyWindow>(
        "SELECT id, window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w
         FROM energy_window ORDER BY window_start DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

// Task 4.4: counts rows where formula_version = 0 (permanently unrecomputable)
pub async fn count_unrecomputable(pool: &SqlitePool) -> Result<i64, AppError> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM energy_window WHERE formula_version = 0")
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(row.0)
}

// Task 4.5: counts rows where formula_version > 0 AND formula_version < CURRENT_FORMULA_VERSION
pub async fn count_stale(pool: &SqlitePool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM energy_window WHERE formula_version > 0 AND formula_version < ?",
    )
    .bind(CURRENT_FORMULA_VERSION)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(row.0)
}

// Task 4.6: returns stale rows ordered by window_start
pub async fn query_stale(pool: &SqlitePool) -> Result<Vec<EnergyWindow>, AppError> {
    sqlx::query_as::<_, EnergyWindow>(
        "SELECT id, window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w
         FROM energy_window
         WHERE formula_version > 0 AND formula_version < ?
         ORDER BY window_start ASC",
    )
    .bind(CURRENT_FORMULA_VERSION)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

// Task 4.7: atomically update wh_* and formula_version for a single row
#[allow(clippy::too_many_arguments)]
pub async fn update_recomputed(
    pool: &SqlitePool,
    window_start: i64,
    wh_produced: f64,
    wh_consumed: f64,
    wh_grid_import: f64,
    wh_grid_export: f64,
    new_version: i32,
    was_clamped: bool,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE energy_window
         SET wh_produced = ?, wh_consumed = ?, wh_grid_import = ?, wh_grid_export = ?, formula_version = ?, was_clamped = ?
         WHERE window_start = ?",
    )
    .bind(wh_produced)
    .bind(wh_consumed)
    .bind(wh_grid_import)
    .bind(wh_grid_export)
    .bind(new_version)
    .bind(was_clamped)
    .bind(window_start)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(())
}

// Task 4.3: counts rows where was_clamped = 1
pub async fn count_clamped(pool: &SqlitePool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM energy_window WHERE was_clamped = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(row.0)
}
