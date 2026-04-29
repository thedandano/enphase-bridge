use enphase_bridge::collector::window_aggregator::{
    CumulativeReading, WINDOW_SECS, compute_delta, window_boundary,
};

#[test]
fn test_window_boundary_on_exact_multiple() {
    let ts = 1704067200_i64; // 2024-01-01 00:00:00 UTC — divisible by 900
    assert_eq!(window_boundary(ts), ts);
}

#[test]
fn test_window_boundary_floors_to_start() {
    let boundary = 1704067200_i64;
    assert_eq!(window_boundary(boundary + 450), boundary); // 7.5 min in
    assert_eq!(window_boundary(boundary + 899), boundary); // last second
    assert_eq!(window_boundary(boundary + 1), boundary); // first second
}

#[test]
fn test_window_secs_is_fifteen_minutes() {
    assert_eq!(WINDOW_SECS, 900);
}

#[test]
fn test_compute_delta_net_export() {
    // Solar produces more than home consumes → export to grid
    // produced=50, grid_import=0, grid_export=20 → consumed = 50+0-20 = 30
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 100.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1050.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 120.0,
    };

    let w = compute_delta(1704067200, &prev, &curr, true);

    assert!((w.wh_produced - 50.0).abs() < 1e-6);
    assert!((w.wh_consumed - 30.0).abs() < 1e-6);
    assert!((w.wh_grid_export - 20.0).abs() < 1e-6);
    assert!((w.wh_grid_import - 0.0).abs() < 1e-6);
    assert!(w.is_complete);
    assert_eq!(w.window_start, 1704067200);
}

#[test]
fn test_compute_delta_net_import() {
    // Home consumes more than solar produces → import from grid
    // produced=10, grid_import=40, grid_export=0 → consumed = 10+40-0 = 50
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 500.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1010.0,
        grid_import_cum_wh: 540.0,
        grid_export_cum_wh: 0.0,
    };

    let w = compute_delta(1704067200, &prev, &curr, true);

    assert!((w.wh_produced - 10.0).abs() < 1e-6);
    assert!((w.wh_consumed - 50.0).abs() < 1e-6);
    assert!((w.wh_grid_import - 40.0).abs() < 1e-6);
    assert!((w.wh_grid_export - 0.0).abs() < 1e-6);
}

#[test]
fn test_compute_delta_never_negative() {
    // Protect against meter roll-back / reboot (cumulative goes backwards)
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 500.0,
        grid_export_cum_wh: 100.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 999.0,
        grid_import_cum_wh: 499.0,
        grid_export_cum_wh: 99.0,
    };

    let w = compute_delta(1704067200, &prev, &curr, false);
    assert!(w.wh_produced >= 0.0);
    assert!(w.wh_consumed >= 0.0);
    assert!(!w.is_complete);
}

#[test]
fn test_compute_delta_stalled_import_during_export() {
    // Regression: grid_import stalls at 0 while solar is exporting.
    // produced=20, grid_import=0, grid_export=9.863 → consumed = 20+0-9.863 = 10.137 > 0
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 200.0,
        grid_export_cum_wh: 50.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1020.0,
        grid_import_cum_wh: 200.0,
        grid_export_cum_wh: 59.863,
    };

    let w = compute_delta(1704067200, &prev, &curr, true);

    assert!(w.wh_consumed > 0.0, "wh_consumed must be > 0 during export");
    assert!((w.wh_grid_export - 9.863).abs() < 1e-6);
    assert!((w.wh_grid_import - 0.0).abs() < 1e-6);
}
