use enphase_bridge::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta,
};

const WINDOW_START: i64 = 1746057600_i64;

#[test]
fn zero_delta() {
    let reading = CumulativeReading {
        timestamp: WINDOW_START,
        production_wh: 500.0,
        grid_import_cum_wh: 100.0,
        grid_export_cum_wh: 50.0,
    };
    let result = compute_delta(WINDOW_START, &reading, &reading, true);

    assert_eq!(
        result.window_start, WINDOW_START,
        "window_start must equal the supplied value"
    );
    assert_eq!(
        result.formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must equal CURRENT_FORMULA_VERSION — update this test after bumping"
    );
    assert_eq!(
        result.wh_produced,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_import,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_export,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_consumed,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
}

#[test]
fn normal_positive_delta() {
    let prev = CumulativeReading {
        timestamp: WINDOW_START,
        production_wh: 1000.0,
        grid_import_cum_wh: 50.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: WINDOW_START + 900,
        production_wh: 1150.0,
        grid_import_cum_wh: 50.0,
        grid_export_cum_wh: 10.0,
    };
    let result = compute_delta(WINDOW_START, &prev, &curr, true);

    assert_eq!(
        result.window_start, WINDOW_START,
        "window_start must equal the supplied value"
    );
    assert_eq!(
        result.formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must equal CURRENT_FORMULA_VERSION — update this test after bumping"
    );
    assert_eq!(
        result.wh_produced,
        150.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_import,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_export,
        10.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_consumed,
        140.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
}

#[test]
fn counter_rollover() {
    let prev = CumulativeReading {
        timestamp: WINDOW_START,
        production_wh: 999_999.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: WINDOW_START + 900,
        production_wh: 5.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let result = compute_delta(WINDOW_START, &prev, &curr, true);

    assert_eq!(
        result.window_start, WINDOW_START,
        "window_start must equal the supplied value"
    );
    assert_eq!(
        result.formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must equal CURRENT_FORMULA_VERSION — update this test after bumping"
    );
    assert_eq!(
        result.wh_produced,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_consumed,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
}

#[test]
fn net_consumption_import() {
    let prev = CumulativeReading {
        timestamp: WINDOW_START,
        production_wh: 0.0,
        grid_import_cum_wh: 100.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: WINDOW_START + 900,
        production_wh: 0.0,
        grid_import_cum_wh: 150.0,
        grid_export_cum_wh: 0.0,
    };
    let result = compute_delta(WINDOW_START, &prev, &curr, true);

    assert_eq!(
        result.window_start, WINDOW_START,
        "window_start must equal the supplied value"
    );
    assert_eq!(
        result.formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must equal CURRENT_FORMULA_VERSION — update this test after bumping"
    );
    assert_eq!(
        result.wh_grid_import,
        50.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_produced,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_export,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_consumed,
        50.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
}

#[test]
fn net_consumption_export() {
    let prev = CumulativeReading {
        timestamp: WINDOW_START,
        production_wh: 200.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 100.0,
    };
    let curr = CumulativeReading {
        timestamp: WINDOW_START + 900,
        production_wh: 350.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 200.0,
    };
    let result = compute_delta(WINDOW_START, &prev, &curr, true);

    assert_eq!(
        result.window_start, WINDOW_START,
        "window_start must equal the supplied value"
    );
    assert_eq!(
        result.formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must equal CURRENT_FORMULA_VERSION — update this test after bumping"
    );
    assert_eq!(
        result.wh_produced,
        150.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_export,
        100.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_grid_import,
        0.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
    assert_eq!(
        result.wh_consumed,
        50.0,
        "compute_delta output changed. Bump CURRENT_FORMULA_VERSION ({} → {}) \
         then update the expected value in this test.",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION + 1
    );
}
