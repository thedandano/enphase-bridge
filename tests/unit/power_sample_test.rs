use enphase_bridge::collector::window_aggregator::{CumulativeReading, compute_delta};

#[test]
fn test_compute_delta_avg_fields_are_none() {
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1050.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let w = compute_delta(1704067200, &prev, &curr, true);
    assert!(
        w.avg_production_w.is_none(),
        "compute_delta must not compute avg_production_w"
    );
    assert!(
        w.avg_consumption_w.is_none(),
        "compute_delta must not compute avg_consumption_w"
    );
    assert!(
        w.avg_grid_w.is_none(),
        "compute_delta must not compute avg_grid_w"
    );
}
