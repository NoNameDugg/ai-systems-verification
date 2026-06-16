//! Monitoring Integration Tests
//!
//! Tests for metrics and monitoring including:
//! - Counter metrics
//! - Gauge metrics
//! - Histogram metrics
//! - Metric labels
//! - Metrics export
//! - Alerting thresholds

use std::collections::HashMap;

use super::helpers::*;

// ============================================================================
// TEST 1: COUNTER METRIC UPDATES
// ============================================================================

#[test]
fn test_metrics_counter() {
    // ARRANGE: Counter metric
    let counter = MockCounter::new("messages_processed_total");

    // ACT: Increment counter
    counter.inc();
    counter.inc();
    counter.add(5);

    // ASSERT: Counter value is correct
    assert_eq!(counter.get(), 7);
    assert_eq!(counter.name, "messages_processed_total");
}

// ============================================================================
// TEST 2: GAUGE METRIC UPDATES
// ============================================================================

#[test]
fn test_metrics_gauge() {
    // ARRANGE: Gauge metric
    let gauge = MockGauge::new("orderbook_depth");

    // ACT: Set gauge values
    gauge.set(100);
    assert_eq!(gauge.get(), 100);

    gauge.set(50);
    assert_eq!(gauge.get(), 50);

    gauge.set(200);

    // ASSERT: Final value is correct
    assert_eq!(gauge.get(), 200);
    assert_eq!(gauge.name, "orderbook_depth");
}

// ============================================================================
// TEST 3: HISTOGRAM METRIC UPDATES
// ============================================================================

#[test]
fn test_metrics_histogram() {
    // ARRANGE: Histogram with buckets
    let buckets = vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0];
    let histogram = MockHistogram::new("request_latency_seconds", buckets);

    // ACT: Record observations
    histogram.observe(0.002);
    histogram.observe(0.008);
    histogram.observe(0.015);
    histogram.observe(0.100);
    histogram.observe(0.250);

    // ASSERT: Histogram stats are correct
    assert_eq!(histogram.count(), 5);
    assert!((histogram.sum() - 0.375).abs() < 0.001);
}

// ============================================================================
// TEST 4: METRIC LABELS HANDLING
// ============================================================================

#[test]
fn test_metrics_labels() {
    // ARRANGE: Counter with labels
    let mut labels = HashMap::new();
    labels.insert("exchange".to_string(), "deribit".to_string());
    labels.insert("instrument".to_string(), "BTC-USD".to_string());

    let counter = MockCounter::with_labels("messages_by_exchange", labels);

    // ACT: Increment
    counter.inc();
    counter.inc();

    // ASSERT: Labels are preserved
    assert_eq!(counter.labels.get("exchange"), Some(&"deribit".to_string()));
    assert_eq!(
        counter.labels.get("instrument"),
        Some(&"BTC-USD".to_string())
    );
    assert_eq!(counter.get(), 2);
}

// ============================================================================
// TEST 5: METRICS EXPORT FORMAT
// ============================================================================

#[test]
fn test_metrics_export() {
    // ARRANGE: Various metrics
    let counter = MockCounter::new("requests_total");
    let gauge = MockGauge::new("connections_active");

    counter.add(100);
    gauge.set(5);

    // ACT: Format metrics (Prometheus-style)
    let counter_line = format!("{} {}", counter.name, counter.get());
    let gauge_line = format!("{} {}", gauge.name, gauge.get());

    // ASSERT: Format is correct
    assert_eq!(counter_line, "requests_total 100");
    assert_eq!(gauge_line, "connections_active 5");
}

// ============================================================================
// TEST 6: ALERTING THRESHOLDS
// ============================================================================

#[test]
fn test_alerting_thresholds() {
    // ARRANGE: Metrics with alerting thresholds
    struct AlertConfig {
        metric_name: String,
        warning_threshold: u64,
        critical_threshold: u64,
    }

    let config = AlertConfig {
        metric_name: "queue_depth".to_string(),
        warning_threshold: 100,
        critical_threshold: 500,
    };

    let gauge = MockGauge::new(&config.metric_name);

    // ACT: Check different levels
    gauge.set(50);
    let normal = gauge.get() < config.warning_threshold;

    gauge.set(150);
    let warning =
        gauge.get() >= config.warning_threshold && gauge.get() < config.critical_threshold;

    gauge.set(600);
    let critical = gauge.get() >= config.critical_threshold;

    // ASSERT: Thresholds trigger correctly
    assert!(normal, "50 should be normal");
    assert!(warning, "150 should be warning");
    assert!(critical, "600 should be critical");
}
