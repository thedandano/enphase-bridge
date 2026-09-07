/// Duration of one energy-collection window in seconds (15 minutes).
/// See CLAUDE.md "Cumulative-to-delta conversion" and `collector/window_aggregator.rs`.
pub const WINDOW_SECS: i64 = 15 * 60;

/// Inverter offline threshold in seconds (20 minutes).
/// See CLAUDE.md "Inverter online threshold": `OFFLINE_THRESHOLD_SECS = 1200`.
pub const INVERTER_OFFLINE_THRESHOLD_SECS: i64 = 20 * 60;

/// TOU refresh interval in seconds (7 days).
/// See CLAUDE.md "TOU schedule lifecycle" — `tou/refresh.rs` re-fetches every 7 days.
pub const TOU_REFRESH_INTERVAL_SECS: i64 = 7 * 24 * 3600;

/// TOU stale alarm threshold in seconds (90 days).
/// See CLAUDE.md "TOU schedule lifecycle" — health/probe flags schedule stale after 90 days.
pub const TOU_STALE_THRESHOLD_SECS: i64 = 90 * 24 * 3600;

/// Seconds in one calendar day (86 400).
/// Used for end-bound normalization in `api/handlers/trueup.rs::get_estimate`.
pub const DAY_SECS: i64 = 24 * 3600;
