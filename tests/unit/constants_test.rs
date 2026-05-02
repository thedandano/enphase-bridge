use enphase_bridge::constants::{
    DAY_SECS, INVERTER_OFFLINE_THRESHOLD_SECS, TOU_REFRESH_INTERVAL_SECS, TOU_STALE_THRESHOLD_SECS,
    WINDOW_SECS,
};

#[test]
fn window_secs_is_900() {
    assert_eq!(WINDOW_SECS, 900);
}

#[test]
fn inverter_offline_threshold_is_1200() {
    assert_eq!(INVERTER_OFFLINE_THRESHOLD_SECS, 1200);
}

#[test]
fn tou_refresh_interval_is_7_days() {
    assert_eq!(TOU_REFRESH_INTERVAL_SECS, 7 * 86_400);
}

#[test]
fn tou_stale_threshold_is_90_days() {
    assert_eq!(TOU_STALE_THRESHOLD_SECS, 90 * 86_400);
}

#[test]
fn day_secs_is_86400() {
    assert_eq!(DAY_SECS, 86_400);
}
