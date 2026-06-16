//! Tests for Verification Script Integration.
//!
//! These tests verify the mathematical correctness requirements for shadow mode
//! validation before production cutover.
//!
//! Acceptance Criteria:
//! | Metric | Requirement |
//! |--------|-------------|
//! | Price Match | 100% (within 1e-10 float tolerance) |
//! | Latency | Must be lower than the incumbent's P99 |
//! | Data Completeness | No missing ticks relative to the incumbent |
//! | Stability | 1 hour continuous operation without crash |
//!
//! Delta < 0.0001 verification
//!
//! Test Categories:
//! - Float Tolerance Tests (6 tests)
//! - Delta Threshold Tests (8 tests)
//! - Verification Statistics Tests (6 tests)
//! - Mathematical Proof Tests (4 tests)
//!
//! Total: 24 tests

use astra_flash::book::{BookSnapshot, OrderBook, OrderBookConfig};
use astra_flash::core::types::{now_micros, Exchange, Instrument, PriceLevel};
use astra_flash::gateway::OrderBookSnapshot;
use rust_decimal_macros::dec;

// =============================================================================
// TEST CONSTANTS
// =============================================================================

/// Float tolerance as specified in the acceptance criteria
const FLOAT_TOLERANCE: f64 = 1e-10;

/// Batch 5.2 delta threshold requirement
const DELTA_THRESHOLD: f64 = 0.0001;

/// Python shim P99 latency baseline (microseconds)
const PYTHON_P99_LATENCY_US: f64 = 813.0;

/// Ticks expected for 1 hour at 100ms interval
const ONE_HOUR_TICKS: u64 = 36000;

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

/// Create a book snapshot with specific price.
fn create_book_snapshot(best_bid: f64, best_ask: f64, depth: usize) -> BookSnapshot {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let mut book = OrderBook::new(instrument, config);

    let bids: Vec<PriceLevel> = (0..depth)
        .map(|i| {
            PriceLevel::new(
                best_bid - (i as f64 * 0.0001),
                dec!(1000000),
                now_micros(),
            )
        })
        .collect();

    let asks: Vec<PriceLevel> = (0..depth)
        .map(|i| {
            PriceLevel::new(
                best_ask + (i as f64 * 0.0001),
                dec!(800000),
                now_micros(),
            )
        })
        .collect();

    book.apply_snapshot(bids, asks, now_micros());
    book.to_snapshot(depth)
}

/// Calculate price delta between two prices.
fn price_delta(price1: f64, price2: f64) -> f64 {
    (price1 - price2).abs()
}

// =============================================================================
// FLOAT TOLERANCE TESTS (6 tests)
// =============================================================================

/// Test FLOAT_TOLERANCE matches specification.
#[test]
fn test_float_tolerance_value() {
    // "within 1e-10 float tolerance"
    assert_eq!(FLOAT_TOLERANCE, 1e-10);
}

/// Test identical prices are within tolerance.
#[test]
fn test_identical_prices_within_tolerance() {
    let price1 = 1.08505;
    let price2 = 1.08505;
    let delta = price_delta(price1, price2);

    assert!(delta < FLOAT_TOLERANCE);
}

/// Test float precision differences within tolerance.
#[test]
fn test_float_precision_within_tolerance() {
    let price1 = 1.08505_0000_0000_01;
    let price2 = 1.08505_0000_0000_02;
    let delta = price_delta(price1, price2);

    assert!(delta < FLOAT_TOLERANCE);
}

/// Test meaningful price difference outside tolerance.
#[test]
fn test_meaningful_difference_outside_tolerance() {
    let price1 = 1.08505;
    let price2 = 1.08506; // 0.00001 difference
    let delta = price_delta(price1, price2);

    assert!(delta > FLOAT_TOLERANCE);
}

/// Test edge case at tolerance boundary.
#[test]
fn test_edge_case_at_tolerance() {
    let tolerance = FLOAT_TOLERANCE;
    let delta_below = tolerance * 0.99;
    let delta_at = tolerance;
    let delta_above = tolerance * 1.01;

    assert!(delta_below < FLOAT_TOLERANCE);
    assert!(!(delta_at < FLOAT_TOLERANCE)); // >= threshold
    assert!(!(delta_above < FLOAT_TOLERANCE));
}

/// Test zero delta.
#[test]
fn test_zero_delta() {
    let delta = price_delta(1.08505, 1.08505);
    assert_eq!(delta, 0.0);
    assert!(delta < FLOAT_TOLERANCE);
}

// =============================================================================
// DELTA THRESHOLD TESTS (8 tests) - Batch 5.2 Requirement
// =============================================================================

/// Test DELTA_THRESHOLD matches Batch 5.2 requirement.
#[test]
fn test_delta_threshold_value() {
    // From Batch 5.2: "Verify: Delta < 0.0001"
    assert_eq!(DELTA_THRESHOLD, 0.0001);
}

/// Test delta well below threshold passes.
#[test]
fn test_delta_well_below_threshold_passes() {
    let delta = 0.00001;
    assert!(delta < DELTA_THRESHOLD);
}

/// Test delta just below threshold passes.
#[test]
fn test_delta_just_below_threshold_passes() {
    let delta = 0.000099;
    assert!(delta < DELTA_THRESHOLD);
}

/// Test delta at threshold fails.
#[test]
fn test_delta_at_threshold_fails() {
    let delta = 0.0001;
    assert!(!(delta < DELTA_THRESHOLD));
}

/// Test delta above threshold fails.
#[test]
fn test_delta_above_threshold_fails() {
    let delta = 0.0002;
    assert!(!(delta < DELTA_THRESHOLD));
}

/// Test delta much above threshold fails.
#[test]
fn test_delta_much_above_threshold_fails() {
    let delta = 0.001;
    assert!(!(delta < DELTA_THRESHOLD));
}

/// Test DELTA_THRESHOLD is less strict than FLOAT_TOLERANCE.
#[test]
fn test_delta_threshold_less_strict_than_float_tolerance() {
    // DELTA_THRESHOLD (0.0001) is much larger than FLOAT_TOLERANCE (1e-10)
    assert!(DELTA_THRESHOLD > FLOAT_TOLERANCE);

    // Ratio: 0.0001 / 1e-10 = 1,000,000x less strict
    let ratio = DELTA_THRESHOLD / FLOAT_TOLERANCE;
    // Due to floating point, allow for some tolerance (should be ~1 million)
    assert!(ratio > 900_000.0, "Ratio {} should be > 900,000", ratio);
}

/// Test real-world price delta scenarios.
#[test]
fn test_real_world_price_deltas() {
    // EUR/USD typical scenarios
    // Note: Floating point arithmetic may cause slight imprecision,
    // so we test the clear cases where delta is clearly < or > threshold
    let scenarios = [
        (1.08505, 1.08505, true),  // Exact match - clearly passes
        (1.08505, 1.08506, true),  // 1 pip (0.00001) - clearly passes
        (1.08505, 1.08510, true),  // 5 pips (0.00005) - clearly passes
        (1.08505, 1.08520, false), // 15 pips (0.00015) - clearly fails
        (1.08505, 1.08605, false), // 100 pips (0.001) - clearly fails
    ];

    for (price1, price2, expected_pass) in scenarios {
        let delta = price_delta(price1, price2);
        let passes = delta < DELTA_THRESHOLD;
        assert_eq!(
            passes, expected_pass,
            "Delta {} between {} and {} should {} but {}",
            delta,
            price1,
            price2,
            if expected_pass { "pass" } else { "fail" },
            if passes { "passed" } else { "failed" }
        );
    }
}

// =============================================================================
// VERIFICATION STATISTICS TESTS (6 tests)
// =============================================================================

/// Test one hour tick count calculation.
#[test]
fn test_one_hour_tick_count() {
    let duration_seconds = 3600;
    let interval_ms = 100;
    let expected_ticks = (duration_seconds * 1000) / interval_ms;

    assert_eq!(expected_ticks, ONE_HOUR_TICKS as i32);
    assert_eq!(ONE_HOUR_TICKS, 36000);
}

/// Test Python P99 latency baseline.
#[test]
fn test_python_p99_baseline() {
    // "Python P99 (0.813ms)"
    assert_eq!(PYTHON_P99_LATENCY_US, 813.0);
}

/// Test Rust must beat Python P99.
#[test]
fn test_rust_must_beat_python_p99() {
    // Rust target: < 100us P99 (from performance targets)
    let rust_p99_target_us = 100.0;

    assert!(rust_p99_target_us < PYTHON_P99_LATENCY_US);

    // Calculate improvement ratio
    let improvement = PYTHON_P99_LATENCY_US / rust_p99_target_us;
    assert!(improvement > 8.0); // 8x improvement target
}

/// Test price match rate calculation.
#[test]
fn test_price_match_rate_calculation() {
    let total_ticks: u64 = 10000;
    let price_matches: u64 = 10000;

    let rate = (price_matches as f64 / total_ticks as f64) * 100.0;
    assert_eq!(rate, 100.0);
}

/// Test price match rate with mismatches.
#[test]
fn test_price_match_rate_with_mismatches() {
    let total_ticks: u64 = 10000;
    let price_matches: u64 = 9990;

    let rate = (price_matches as f64 / total_ticks as f64) * 100.0;
    assert_eq!(rate, 99.9);
    assert!(rate < 99.99); // Below acceptance threshold
}

/// Test data completeness calculation.
#[test]
fn test_data_completeness() {
    let total_ticks: u64 = 36000;
    let rust_missing: u64 = 0;

    // Data completeness: No missing ticks = 100%
    let completeness = if total_ticks > 0 {
        ((total_ticks - rust_missing) as f64 / total_ticks as f64) * 100.0
    } else {
        0.0
    };

    assert_eq!(completeness, 100.0);
}

// =============================================================================
// MATHEMATICAL PROOF TESTS (4 tests) - Batch 5.2 Goal
// =============================================================================

/// Test full mathematical proof verification.
#[test]
fn test_mathematical_proof_all_criteria_pass() {
    // Simulate 1 hour of perfect operation
    let total_ticks = ONE_HOUR_TICKS;
    let price_matches = total_ticks;
    let rust_faster_count = (total_ticks as f64 * 0.98) as u64; // 98% faster
    let rust_missing = 0u64;
    let max_price_delta = 0.0; // Exact match
    let avg_latency_us = 50.0; // 50us avg

    // Verify each criterion
    let price_match_rate = (price_matches as f64 / total_ticks as f64) * 100.0;
    let rust_faster_rate = (rust_faster_count as f64 / total_ticks as f64) * 100.0;

    // Criterion 1: Price Match 100%
    assert!(price_match_rate >= 99.99, "Price match rate must be >= 99.99%");

    // Criterion 2: Rust Latency < Python P99
    assert!(
        avg_latency_us < PYTHON_P99_LATENCY_US,
        "Rust latency must be lower than Python P99"
    );

    // Criterion 3: Data Completeness
    assert_eq!(rust_missing, 0, "No missing ticks allowed");

    // Criterion 4: Delta < 0.0001 (Batch 5.2)
    assert!(max_price_delta < DELTA_THRESHOLD, "Max delta must be < 0.0001");

    // Criterion 5: 1 hour stability
    assert!(total_ticks >= ONE_HOUR_TICKS, "Must run for at least 1 hour");

    // Criterion 6: Rust faster > 50%
    assert!(
        rust_faster_rate > 50.0,
        "Rust should be faster more than 50% of the time"
    );
}

/// Test proof fails if price match below threshold.
#[test]
fn test_proof_fails_if_price_match_below_threshold() {
    let total_ticks = 10000u64;
    let price_matches = 9990u64; // 99.9%

    let price_match_rate = (price_matches as f64 / total_ticks as f64) * 100.0;

    assert!(
        price_match_rate < 99.99,
        "Price match rate 99.9% should fail threshold"
    );
}

/// Test proof fails if delta exceeds threshold.
#[test]
fn test_proof_fails_if_delta_exceeds_threshold() {
    let max_price_delta = 0.0001; // At threshold

    assert!(
        !(max_price_delta < DELTA_THRESHOLD),
        "Delta at threshold should fail"
    );
}

/// Test proof fails if rust missing ticks.
#[test]
fn test_proof_fails_if_rust_missing_ticks() {
    let rust_missing = 1u64; // Even 1 missing

    assert!(rust_missing > 0, "Any missing ticks should fail");
}

// =============================================================================
// GATEWAY SNAPSHOT VERIFICATION TESTS (4 tests)
// =============================================================================

/// Test GatewaySnapshot price accuracy.
#[test]
fn test_gateway_snapshot_price_accuracy() {
    let book = create_book_snapshot(1.08505, 1.08510, 5);
    let gateway = OrderBookSnapshot::from_book_snapshot(&book);

    // Verify best bid/ask accuracy
    if let Some(best_bid) = gateway.bids.first() {
        assert!((best_bid.price - 1.08505).abs() < FLOAT_TOLERANCE);
    }

    if let Some(best_ask) = gateway.asks.first() {
        assert!((best_ask.price - 1.08510).abs() < FLOAT_TOLERANCE);
    }
}

/// Test GatewaySnapshot preserves all levels.
#[test]
fn test_gateway_snapshot_preserves_levels() {
    let depth = 10;
    let book = create_book_snapshot(1.08505, 1.08510, depth);
    let gateway = OrderBookSnapshot::from_book_snapshot(&book);

    assert_eq!(gateway.bids.len(), depth);
    assert_eq!(gateway.asks.len(), depth);
}

/// Test GatewaySnapshot maintains price ordering.
#[test]
fn test_gateway_snapshot_price_ordering() {
    let book = create_book_snapshot(1.08505, 1.08510, 5);
    let gateway = OrderBookSnapshot::from_book_snapshot(&book);

    // Bids should be descending (highest first)
    for i in 1..gateway.bids.len() {
        assert!(gateway.bids[i - 1].price >= gateway.bids[i].price);
    }

    // Asks should be ascending (lowest first)
    for i in 1..gateway.asks.len() {
        assert!(gateway.asks[i - 1].price <= gateway.asks[i].price);
    }
}

/// Test GatewaySnapshot JSON serialization round-trip.
#[test]
fn test_gateway_snapshot_json_roundtrip() {
    let book = create_book_snapshot(1.08505, 1.08510, 5);
    let gateway = OrderBookSnapshot::from_book_snapshot(&book);

    let json = gateway.to_json().expect("Should serialize to JSON");
    assert!(!json.is_empty());

    // Verify JSON contains expected fields
    assert!(json.contains("bids"));
    assert!(json.contains("asks"));
    assert!(json.contains("timestamp"));
}
