use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use enphase_bridge::api::server::{AppState, create_router};
use enphase_bridge::storage::models::{EnergyWindow, TouRateSchedule};
use enphase_bridge::trueup::calculator;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tower::ServiceExt;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn state(pool: SqlitePool, timezone: chrono_tz::Tz) -> AppState {
    AppState {
        pool,
        token_expires_at: i64::MAX,
        started_at: 0,
        arrays: Default::default(),
        tou_timezone: timezone,
        tou_api_key: String::new(),
        tou_utility_eia_id: 1,
        tou_rate_label: "Rate".into(),
        tou_openei_base_url: String::new(),
    }
}

async fn seed(
    pool: &SqlitePool,
    start: i64,
    end: Option<i64>,
    source: &str,
    fetched: i64,
    eia: i64,
    json: &str,
) -> i64 {
    sqlx::query("INSERT INTO tou_rate_schedule (fetched_at, utility_name, rate_label, rate_json, utility_eia_id, source_id, effective_start, effective_end) VALUES (?, 'Utility', 'Rate', ?, ?, ?, ?, ?)")
        .bind(fetched).bind(json).bind(eia).bind(source).bind(start).bind(end).execute(pool).await.unwrap().last_insert_rowid()
}

async fn get(state: AppState, path: &str) -> (StatusCode, serde_json::Value) {
    let response = create_router(state)
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn intervals_are_clipped_half_open_and_use_configured_timezone() {
    let pool = pool().await;
    let id = seed(
        &pool,
        0,
        None,
        "v1",
        1,
        1,
        &super::common::fixture_rate_json(),
    )
    .await;
    let (status, body) = get(
        state(pool, chrono_tz::UTC),
        "/api/tou/intervals?start=1704153601&end=1704182401",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["timezone"], "UTC");
    assert_eq!(body["intervals"][0]["start"], 1704153601_i64);
    assert_eq!(body["intervals"][0]["end"], 1704175200_i64); // 06:00 UTC
    assert_eq!(body["intervals"][0]["bracket"], "super_off_peak");
    assert_eq!(body["intervals"][1]["bracket"], "off_peak");
    assert_eq!(body["intervals"][1]["end"], 1704182401_i64);
    assert_eq!(body["intervals"][0]["schedule_id"], id);
}

#[tokio::test]
async fn intervals_reject_invalid_bounds_and_missing_history() {
    let pool = pool().await;
    for query in [
        "",
        "?start=1",
        "?start=x&end=2",
        "?start=2&end=1",
        "?start=1&end=1",
        "?start=0&end=2678401",
        "?start=9223372036854775807&end=9223372036854775807",
    ] {
        assert_eq!(
            get(
                state(pool.clone(), chrono_tz::UTC),
                &format!("/api/tou/intervals{query}")
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (status, body) = get(
        state(pool, chrono_tz::UTC),
        "/api/tou/intervals?start=1&end=2",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "tou_history_unavailable");
}

#[tokio::test]
async fn revisions_do_not_leak_across_utilities_or_resurrect_expired_versions() {
    let pool = pool().await;
    let json = super::common::fixture_rate_json();
    let old = seed(&pool, 0, None, "old", 1, 1, &json).await;
    let stale = seed(&pool, 100, Some(200), "new", 2, 1, &json).await;
    let latest = seed(&pool, 100, Some(200), "new", 2, 1, &json).await;
    seed(&pool, 100, Some(99), "invalid-old-record", 100, 1, &json).await;
    seed(&pool, 0, None, "other-utility", 100, 2, &json).await;
    let (status, body) = get(
        state(pool.clone(), chrono_tz::UTC),
        "/api/tou/intervals?start=99&end=101",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["intervals"][0]["schedule_id"], old);
    assert_eq!(body["intervals"][0]["end"], 100);
    assert_eq!(body["intervals"][1]["schedule_id"], latest);
    assert_ne!(latest, stale);
    assert_eq!(body["schedules"][0]["effective_end"], 100);
    assert_eq!(
        get(
            state(pool.clone(), chrono_tz::UTC),
            "/api/tou/intervals?start=199&end=201"
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get(
            state(pool, chrono_tz::UTC),
            "/api/tou/intervals?start=200&end=201"
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn malformed_saved_schedule_is_an_explicit_parse_error() {
    let pool = pool().await;
    seed(&pool, 0, None, "bad", 1, 1, "{}").await;
    let (status, body) = get(
        state(pool, chrono_tz::UTC),
        "/api/tou/intervals?start=1&end=2",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"], "upstream_parse_error");
}

fn hourly_json() -> String {
    let row: Vec<usize> = (0..24).map(|hour| hour % 3).collect();
    serde_json::json!({"energyweekdayschedule":vec![row.clone();12], "energyweekendschedule":vec![row;12],
        "energyratestructure":[[{"rate":0.1,"sell":0.1}],[{"rate":0.2,"sell":0.2}],[{"rate":0.3,"sell":0.3}]]}).to_string()
}

#[tokio::test]
async fn calendar_boundaries_match_costs_across_dst_midnight_and_non_hour_offsets() {
    let cases = [
        (chrono_tz::America::Los_Angeles, 1710057600, 1710072000), // spring: 00:00-05:00 local
        (chrono_tz::America::Los_Angeles, 1730617200, 1730631600), // fall: repeated 01:00
        (chrono_tz::Australia::Lord_Howe, 1728140400, 1728154800), // half-hour spring change
        (chrono_tz::Asia::Kolkata, 1704132000, 1704164400),        // midnight, UTC+05:30
    ];
    for (timezone, start, end) in cases {
        let pool = pool().await;
        let json = hourly_json();
        let id = seed(&pool, 0, None, "hourly", 1, 1, &json).await;
        let (status, body) = get(
            state(pool, timezone),
            &format!("/api/tou/intervals?start={start}&end={end}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let intervals = body["intervals"].as_array().unwrap();
        let schedule = TouRateSchedule {
            id,
            fetched_at: 1,
            effective_date: None,
            utility_name: "Utility".into(),
            rate_label: "Rate".into(),
            rate_json: json,
        };
        for timestamp in (start..end).step_by(60) {
            let interval = intervals
                .iter()
                .find(|interval| {
                    interval["start"].as_i64().unwrap() <= timestamp
                        && timestamp < interval["end"].as_i64().unwrap()
                })
                .unwrap();
            let window = EnergyWindow {
                id: 0,
                window_start: timestamp,
                wh_produced: 0.0,
                wh_consumed: 0.0,
                wh_grid_import: 1000.0,
                wh_grid_export: 0.0,
                is_complete: true,
                formula_version: 1,
                was_clamped: false,
                avg_production_w: None,
                avg_consumption_w: None,
                avg_grid_w: None,
            };
            let cost = calculator::calculate(&schedule, timezone, &[window]).unwrap();
            let bracket = if cost.peak.import_kwh == 1.0 {
                "peak"
            } else if cost.off_peak.import_kwh == 1.0 {
                "off_peak"
            } else {
                "super_off_peak"
            };
            assert_eq!(
                interval["bracket"], bracket,
                "timestamp {timestamp} in {timezone}"
            );
        }
        for pair in intervals.windows(2) {
            assert_eq!(pair[0]["end"], pair[1]["start"]);
        }
        if start == 1730617200 {
            assert!(
                intervals
                    .iter()
                    .any(|i| i["end"].as_i64().unwrap() - i["start"].as_i64().unwrap() == 7200)
            );
        }
    }
}

#[tokio::test]
async fn historical_estimates_use_each_revision_and_persist_all_ids() {
    let pool = pool().await;
    let mut json: serde_json::Value = serde_json::from_str(&hourly_json()).unwrap();
    let old = seed(&pool, 0, None, "old", 1, 1, &json.to_string()).await;
    for period in json["energyratestructure"].as_array_mut().unwrap() {
        period[0]["rate"] = serde_json::json!(period[0]["rate"].as_f64().unwrap() * 2.0);
    }
    let boundary = 1704153600; // 2024-01-02 UTC
    let new = seed(&pool, boundary, None, "new", 2, 1, &json.to_string()).await;
    for timestamp in [boundary - 86400, boundary] {
        sqlx::query("INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version) VALUES (?, 0, 0, 1000, 0, 1, 1)")
            .bind(timestamp).execute(&pool).await.unwrap();
    }
    let (status, body) = get(
        state(pool.clone(), chrono_tz::UTC),
        "/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-02T00:00:00Z",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["net_cost_usd"], 0.3);
    assert_eq!(body["tou_schedule"]["id"], new);
    assert_eq!(body["tou_schedules"][0]["id"], old);
    assert_eq!(body["tou_schedules"][1]["id"], new);
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT schedule_id FROM true_up_schedule ORDER BY schedule_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ids, vec![old, new]);
}

#[tokio::test]
async fn month_and_weekend_changes_use_utility_local_midnight() {
    let pool = pool().await;
    let mut json: serde_json::Value =
        serde_json::from_str(&super::common::fixture_rate_json()).unwrap();
    json["energyweekdayschedule"][1] = serde_json::json!(vec![2; 24]);
    json["energyweekendschedule"][1] = serde_json::json!(vec![2; 24]);
    seed(&pool, 0, None, "seasonal", 1, 1, &json.to_string()).await;
    for (boundary, entering) in [(1706745600_i64, "peak"), (1704672000_i64, "super_off_peak")] {
        let (status, body) = get(
            state(pool.clone(), chrono_tz::UTC),
            &format!(
                "/api/tou/intervals?start={}&end={}",
                boundary - 1,
                boundary + 1
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["intervals"][0]["end"], boundary);
        assert_eq!(body["intervals"][1]["start"], boundary);
        assert_eq!(body["intervals"][1]["bracket"], entering);
    }
}

#[tokio::test]
async fn extreme_valid_unix_timestamps_are_rejected_before_calendar_arithmetic() {
    let pool = pool().await;
    for start in [
        chrono::DateTime::<chrono::Utc>::MIN_UTC.timestamp(),
        chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp() - 1,
    ] {
        assert_eq!(
            get(
                state(pool.clone(), chrono_tz::America::Los_Angeles),
                &format!("/api/tou/intervals?start={start}&end={}", start + 1)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
}

#[tokio::test]
async fn a_newer_invalid_copy_cannot_resurrect_older_revision_coverage() {
    let pool = pool().await;
    let json = super::common::fixture_rate_json();
    seed(&pool, 0, None, "same-source", 1, 1, &json).await;
    seed(&pool, 100, Some(99), "same-source", 2, 1, &json).await;
    assert_eq!(
        get(
            state(pool, chrono_tz::UTC),
            "/api/tou/intervals?start=1&end=2"
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
