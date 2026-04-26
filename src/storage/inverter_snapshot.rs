use crate::error::{AppError, StorageError};
use crate::storage::models::MicroinverterSnapshot;
use sqlx::SqlitePool;

pub async fn insert_batch(
    pool: &SqlitePool,
    snapshots: &[MicroinverterSnapshot],
) -> Result<(), AppError> {
    for s in snapshots {
        sqlx::query(
            "INSERT INTO microinverter_snapshot (window_start, serial_number, watts_output, is_online)
             VALUES (?, ?, ?, ?)",
        )
        .bind(s.window_start)
        .bind(&s.serial_number)
        .bind(s.watts_output)
        .bind(s.is_online)
        .execute(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    }
    Ok(())
}

pub async fn query_by_window(
    pool: &SqlitePool,
    window_start: i64,
) -> Result<Vec<MicroinverterSnapshot>, AppError> {
    sqlx::query_as::<_, MicroinverterSnapshot>(
        "SELECT id, window_start, serial_number, watts_output, is_online
         FROM microinverter_snapshot WHERE window_start = ?
         ORDER BY serial_number ASC",
    )
    .bind(window_start)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn query_range(
    pool: &SqlitePool,
    start: i64,
    end: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<MicroinverterSnapshot>, AppError> {
    sqlx::query_as::<_, MicroinverterSnapshot>(
        "SELECT id, window_start, serial_number, watts_output, is_online
         FROM microinverter_snapshot
         WHERE window_start >= ? AND window_start < ?
         ORDER BY window_start ASC, serial_number ASC LIMIT ? OFFSET ?",
    )
    .bind(start)
    .bind(end)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn query_latest_window(
    pool: &SqlitePool,
) -> Result<Option<(i64, Vec<MicroinverterSnapshot>)>, AppError> {
    let row = sqlx::query_as::<_, (i64,)>(
        "SELECT window_start FROM microinverter_snapshot ORDER BY window_start DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;

    let Some((window_start,)) = row else {
        return Ok(None);
    };

    let snapshots = query_by_window(pool, window_start).await?;
    Ok(Some((window_start, snapshots)))
}

pub async fn query_by_serial_range(
    pool: &SqlitePool,
    serial: &str,
    start: i64,
    end: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<MicroinverterSnapshot>, AppError> {
    sqlx::query_as::<_, MicroinverterSnapshot>(
        "SELECT id, window_start, serial_number, watts_output, is_online
         FROM microinverter_snapshot
         WHERE serial_number = ? AND window_start >= ? AND window_start < ?
         ORDER BY window_start ASC LIMIT ? OFFSET ?",
    )
    .bind(serial)
    .bind(start)
    .bind(end)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))
}
