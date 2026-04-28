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
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        consumption_wh: 800.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1050.0,
        consumption_wh: 830.0,
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
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        consumption_wh: 800.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1010.0,
        consumption_wh: 850.0,
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
        consumption_wh: 800.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 999.0,
        consumption_wh: 799.0,
    };

    let w = compute_delta(1704067200, &prev, &curr, false);
    assert!(w.wh_produced >= 0.0);
    assert!(w.wh_consumed >= 0.0);
    assert!(!w.is_complete);
}
