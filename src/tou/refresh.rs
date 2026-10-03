use crate::error::AppError;
use crate::storage::tou_schedule;
use crate::tou::openei_client::OpenEiClient;
use sqlx::SqlitePool;
use std::time::Duration;

const REFRESH_TRIGGER_SECS: i64 = 7 * 24 * 3600;

pub async fn run_tou_refresh_loop(
    pool: SqlitePool,
    api_key: String,
    utility_eia_id: u32,
    rate_label: String,
) {
    if needs_refresh_now(&pool, &rate_label, utility_eia_id).await
        && let Err(e) = do_refresh(
            &pool,
            &api_key,
            utility_eia_id,
            &rate_label,
            "https://api.openei.org",
        )
        .await
    {
        tracing::error!(event="tou_refresh_error", error=%e);
    }

    loop {
        tokio::time::sleep(Duration::from_secs(7 * 24 * 3600)).await;
        if let Err(e) = do_refresh(
            &pool,
            &api_key,
            utility_eia_id,
            &rate_label,
            "https://api.openei.org",
        )
        .await
        {
            tracing::error!(event = "tou_refresh_error", error = %e);
        }
    }
}

async fn needs_refresh_now(pool: &SqlitePool, rate_label: &str, utility_eia_id: u32) -> bool {
    let now = crate::util::unix_now();
    match tou_schedule::query_latest(pool, rate_label, utility_eia_id).await {
        Ok(Some(s)) => (now - s.fetched_at) > REFRESH_TRIGGER_SECS,
        Ok(None) => true,
        Err(e) => {
            tracing::warn!(event="tou_refresh_check_failed", error=%e, "attempting refresh after storage check failed");
            true
        }
    }
}

async fn do_refresh(
    pool: &SqlitePool,
    api_key: &str,
    utility_eia_id: u32,
    rate_label: &str,
    base_url: &str,
) -> Result<(), AppError> {
    let client = OpenEiClient::with_base_url(
        api_key.to_string(),
        utility_eia_id,
        rate_label.to_string(),
        base_url.to_string(),
    );
    let fetched = client.fetch_history().await?;
    let fetched_at = crate::util::unix_now();
    tou_schedule::insert_history(pool, utility_eia_id, fetched_at, &fetched).await?;
    tracing::info!(event = "tou_refresh_ok", rate_label = %rate_label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn setup_migrated_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::new()
            .filename(":memory:")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    #[tokio::test]
    async fn other_utility_cannot_suppress_history_refresh() {
        let pool = setup_migrated_pool().await;
        sqlx::query("INSERT INTO tou_rate_schedule (fetched_at, utility_name, rate_label, rate_json, utility_eia_id) VALUES (?, 'Other Utility', 'SameName', '{}', 2)")
            .bind(crate::util::unix_now()).execute(&pool).await.unwrap();
        assert!(needs_refresh_now(&pool, "SameName", 1).await);
        assert!(!needs_refresh_now(&pool, "SameName", 2).await);
    }

    #[tokio::test]
    async fn test_do_refresh_non_200_does_not_modify_db() {
        let pool = setup_migrated_pool().await;

        // Seed one existing schedule row
        sqlx::query(
            "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
             VALUES (1000000, NULL, 'Test', 'TestRate', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/utility_rates".to_string()),
            )
            .with_status(503)
            .create_async()
            .await;

        let result = do_refresh(&pool, "key", 12345, "TestRate", &server.url()).await;
        assert!(result.is_err(), "non-200 should return Err");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tou_rate_schedule")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "DB row should be unchanged after failed refresh");
    }
}
