use enphase_bridge::collector::window_aggregator::{
    CumulativeReading, compute_delta, window_boundary,
};
use enphase_bridge::storage::models::EnergyWindow;

// Simulates the fixed scheduler loop logic for a sequence of ticks.
// Returns (anchor_after_tick, window_produced) for each tick.
fn simulate_ticks(
    initial_anchor: Option<CumulativeReading>,
    ticks: Vec<CumulativeReading>,
) -> Vec<(Option<CumulativeReading>, Option<EnergyWindow>)> {
    let mut anchor = initial_anchor;
    let mut results = Vec::with_capacity(ticks.len());
    for curr in ticks {
        let now = curr.timestamp;
        let (new_anchor, window) = if let Some(prev) = &anchor {
            let prev_window = window_boundary(prev.timestamp);
            let curr_window = window_boundary(now);
            if curr_window > prev_window {
                let w = compute_delta(prev_window, prev, &curr, true);
                (Some(curr), Some(w))
            } else {
                (anchor.clone(), None)
            }
        } else {
            (Some(curr), None)
        };
        anchor = new_anchor.clone();
        results.push((new_anchor, window));
    }
    results
}

// 2024-01-01 00:00:00 UTC — exact 15-min boundary
const BOUNDARY: i64 = 1704067200;
const NEXT_BOUNDARY: i64 = BOUNDARY + 900; // +15 min

fn reading_at(ts: i64, prod: f64, import: f64, export: f64) -> CumulativeReading {
    CumulativeReading {
        timestamp: ts,
        production_wh: prod,
        grid_import_cum_wh: import,
        grid_export_cum_wh: export,
    }
}

// --- Task 2.1 ---
// Mid-window ticks must NOT advance the anchor.
#[test]
fn test_mid_window_ticks_do_not_advance_anchor() {
    let anchor_reading = reading_at(BOUNDARY, 1_000_000.0, 50_000.0, 20_000.0);

    // Three ticks within the same 15-min window (BOUNDARY+60, +120, +180)
    let ticks = vec![
        reading_at(BOUNDARY + 60, 1_000_060.0, 50_010.0, 20_001.0),
        reading_at(BOUNDARY + 120, 1_000_120.0, 50_020.0, 20_002.0),
        reading_at(BOUNDARY + 180, 1_000_180.0, 50_030.0, 20_003.0),
    ];

    let results = simulate_ticks(Some(anchor_reading.clone()), ticks);

    for (tick_idx, (anchor_after, window)) in results.iter().enumerate() {
        assert!(
            window.is_none(),
            "tick {}: no window should be written mid-window",
            tick_idx
        );
        // Anchor must still equal the initial boundary reading
        let a = anchor_after.as_ref().unwrap();
        assert_eq!(
            a.timestamp, BOUNDARY,
            "tick {}: anchor timestamp must remain frozen at boundary",
            tick_idx
        );
        assert!(
            (a.production_wh - anchor_reading.production_wh).abs() < 1e-6,
            "tick {}: anchor production_wh must not advance mid-window",
            tick_idx
        );
    }
}

// --- Task 2.2 ---
// Boundary crossing uses the frozen anchor → delta spans the full 15-min window.
#[test]
fn test_boundary_crossing_delta_spans_full_window() {
    // Steady 3.5 kW production for 15 min → 3500 × (900/3600) = 875 Wh produced
    // Grid import: 0, grid export: 120 Wh in the window
    let prev_anchor = reading_at(BOUNDARY, 100_000.0, 5_000.0, 1_000.0);

    // Ticks inside the window (anchor must stay frozen)
    let mut ticks: Vec<CumulativeReading> = (1..=14)
        .map(|m| {
            let ts = BOUNDARY + m * 60;
            reading_at(
                ts,
                100_000.0 + m as f64 * (875.0 / 15.0),
                5_000.0,
                1_000.0 + m as f64 * (120.0 / 15.0),
            )
        })
        .collect();

    // Final tick: crosses into next window
    ticks.push(reading_at(NEXT_BOUNDARY + 5, 100_875.0, 5_000.0, 1_120.0));

    let results = simulate_ticks(Some(prev_anchor), ticks);

    // All but last should produce no window
    for (i, (_, window)) in results[..results.len() - 1].iter().enumerate() {
        assert!(window.is_none(), "tick {} should not produce a window", i);
    }

    // Last tick must produce a window with the full 15-min delta
    let (anchor_after, window) = results.last().unwrap();
    let w = window
        .as_ref()
        .expect("boundary crossing must produce a window");

    assert!(
        (w.wh_produced - 875.0).abs() < 1.0,
        "wh_produced should be ~875, got {}",
        w.wh_produced
    );
    assert!(
        (w.wh_grid_export - 120.0).abs() < 1.0,
        "wh_grid_export should be ~120, got {}",
        w.wh_grid_export
    );
    assert!(
        (w.wh_grid_import - 0.0).abs() < 1.0,
        "wh_grid_import should be 0, got {}",
        w.wh_grid_import
    );

    // Anchor advances to the boundary-crossing tick
    let a = anchor_after.as_ref().unwrap();
    assert_eq!(
        a.timestamp,
        NEXT_BOUNDARY + 5,
        "anchor must advance after boundary crossing"
    );
}

// --- Task 2.3 ---
// Cold-start: first tick initializes anchor (no window written).
// Mid-window second tick: anchor stays frozen.
// Boundary crossing on later tick: window uses cold-start reading as prev.
#[test]
fn test_cold_start_then_boundary_crossing() {
    // No persisted state
    let initial_anchor = None;

    let first_tick = reading_at(BOUNDARY + 30, 200_000.0, 10_000.0, 5_000.0);
    let mid_tick = reading_at(BOUNDARY + 90, 200_100.0, 10_010.0, 5_010.0);
    // Crosses into next window: 15 min of production ~875 Wh from the first_tick baseline
    let boundary_tick = reading_at(NEXT_BOUNDARY + 10, 200_875.0, 10_000.0, 5_120.0);

    let ticks = vec![first_tick.clone(), mid_tick, boundary_tick];
    let results = simulate_ticks(initial_anchor, ticks);

    // Tick 0: first tick — anchor set, no window
    let (anchor0, window0) = &results[0];
    assert!(window0.is_none(), "first tick must not produce a window");
    assert_eq!(
        anchor0.as_ref().unwrap().timestamp,
        first_tick.timestamp,
        "anchor must be set from first tick"
    );

    // Tick 1: mid-window — anchor stays frozen at first_tick
    let (anchor1, window1) = &results[1];
    assert!(
        window1.is_none(),
        "mid-window tick must not produce a window"
    );
    assert_eq!(
        anchor1.as_ref().unwrap().timestamp,
        first_tick.timestamp,
        "anchor must remain frozen at first_tick on mid-window tick"
    );

    // Tick 2: boundary crossing — window uses first_tick as prev
    let (anchor2, window2) = &results[2];
    let w = window2
        .as_ref()
        .expect("boundary crossing must produce a window");
    assert!(
        (w.wh_produced - 875.0).abs() < 1.0,
        "wh_produced should be ~875 (full delta from cold-start anchor), got {}",
        w.wh_produced
    );
    // Anchor advances to boundary tick
    assert_eq!(
        anchor2.as_ref().unwrap().timestamp,
        NEXT_BOUNDARY + 10,
        "anchor must advance to boundary-crossing tick"
    );
}
