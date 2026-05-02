use crate::constants::{DAY_SECS, TOU_STALE_THRESHOLD_SECS};
use crate::storage::tou_schedule;
use sqlx::SqlitePool;

// Alias kept so existing tests (which use `use super::*`) continue to compile unchanged.
#[allow(unused_imports)]
pub(crate) use crate::constants::TOU_STALE_THRESHOLD_SECS as STALE_THRESHOLD_SECS;

pub async fn probe_tou_schedule(pool: &SqlitePool, rate_label: &str) {
    let now = crate::util::unix_now();
    match tou_schedule::query_latest(pool, rate_label).await {
        Ok(Some(schedule)) => {
            let age_days = (now - schedule.fetched_at) / DAY_SECS;
            if now - schedule.fetched_at > TOU_STALE_THRESHOLD_SECS {
                tracing::warn!(event = "tou_schedule_stale", age_days = age_days);
            } else {
                tracing::info!(event = "tou_schedule_ok", age_days = age_days);
            }
        }
        Ok(None) => {
            tracing::warn!(event = "tou_schedule_stale", age_days = Option::<i64>::None);
        }
        Err(e) => {
            tracing::warn!(event = "tou_schedule_stale", age_days = Option::<i64>::None, error = %e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use tracing_test::traced_test;

    async fn setup_pool() -> SqlitePool {
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
    #[traced_test]
    async fn test_probe_emits_stale_when_no_schedule() {
        let pool = setup_pool().await;
        probe_tou_schedule(&pool, "TOU-DR-2").await;
        assert!(logs_contain("tou_schedule_stale"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_probe_emits_ok_when_fresh_schedule() {
        let pool = setup_pool().await;
        let fresh_fetched_at = crate::util::unix_now();
        sqlx::query(
            "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
             VALUES (?, NULL, 'Test', 'TOU-DR-2', '{}')",
        )
        .bind(fresh_fetched_at)
        .execute(&pool)
        .await
        .unwrap();

        probe_tou_schedule(&pool, "TOU-DR-2").await;
        assert!(logs_contain("tou_schedule_ok"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_probe_emits_stale_when_schedule_too_old() {
        let pool = setup_pool().await;
        let old_fetched_at = crate::util::unix_now() - STALE_THRESHOLD_SECS - 1;
        sqlx::query(
            "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
             VALUES (?, NULL, 'Test', 'TOU-DR-2', '{}')",
        )
        .bind(old_fetched_at)
        .execute(&pool)
        .await
        .unwrap();

        probe_tou_schedule(&pool, "TOU-DR-2").await;
        assert!(logs_contain("tou_schedule_stale"));
    }
}
