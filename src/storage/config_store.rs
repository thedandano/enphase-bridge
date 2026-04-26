use crate::error::{AppError, StorageError};
use sqlx::SqlitePool;

pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
    sqlx::query_scalar::<_, String>("SELECT value FROM config_store WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Storage(StorageError::Database(e)))
}

pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<(), AppError> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO config_store (key, value, updated) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated = excluded.updated",
    )
    .bind(key)
    .bind(value)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e)))?;
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
