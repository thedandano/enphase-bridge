use crate::error::{AppError, StorageError};
use crate::storage::models::PhaseReading;
use sqlx::SqlitePool;

pub async fn insert_batch(pool: &SqlitePool, rows: &[PhaseReading]) -> Result<(), AppError> {
    for row in rows {
        sqlx::query(
            "INSERT OR IGNORE INTO phase_reading
             (sampled_at, meter_eid, channel_eid, active_power_w_at_boundary, energy_dlvd_wh, energy_rcvd_wh)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(row.sampled_at)
        .bind(row.meter_eid)
        .bind(row.channel_eid)
        .bind(row.active_power_w_at_boundary)
        .bind(row.energy_dlvd_wh)
        .bind(row.energy_rcvd_wh)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    }
    Ok(())
}

pub async fn query_range(
    pool: &SqlitePool,
    start: i64,
    end: i64,
    meter_eid: Option<i64>,
    limit: i32,
    offset: i32,
) -> Result<Vec<PhaseReading>, AppError> {
    match meter_eid {
        Some(eid) => sqlx::query_as::<_, PhaseReading>(
            "SELECT id, sampled_at, meter_eid, channel_eid, active_power_w_at_boundary, energy_dlvd_wh, energy_rcvd_wh
             FROM phase_reading
             WHERE sampled_at >= ? AND sampled_at < ? AND meter_eid = ?
             ORDER BY sampled_at ASC, meter_eid ASC, channel_eid ASC
             LIMIT ? OFFSET ?",
        )
        .bind(start)
        .bind(end)
        .bind(eid)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e))),
        None => sqlx::query_as::<_, PhaseReading>(
            "SELECT id, sampled_at, meter_eid, channel_eid, active_power_w_at_boundary, energy_dlvd_wh, energy_rcvd_wh
             FROM phase_reading
             WHERE sampled_at >= ? AND sampled_at < ?
             ORDER BY sampled_at ASC, meter_eid ASC, channel_eid ASC
             LIMIT ? OFFSET ?",
        )
        .bind(start)
        .bind(end)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e))),
    }
}

pub async fn delete_before(pool: &SqlitePool, cutoff: i64) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM phase_reading WHERE sampled_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(result.rows_affected())
}
