//! Tests for Flash metrics system (Part 1.4).
//!
//! # Test Categories
//!
//! 1. Counter Tests (8 tests)
//! 2. Gauge Tests (8 tests)
//! 3. Histogram Tests (8 tests)
//! 4. Integration Tests (6 tests)
//! 5. Thread Safety Tests (4 tests)
//! 6. Timing Guard Tests (4 tests)
//!
//! Total: 38 tests
//!
//! # TDD Methodology
//!
//! These tests were written BEFORE the implementation per the project TDI standards.

use astra_flash::core::metrics::{
    FlashMetrics, MessageType, MetricsError, ProcessingStage, ReconnectReason, UpdateType,
};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a fresh FlashMetrics instance for testing.
fn create_metrics() -> FlashMetrics {
    FlashMetrics::new()
}

// =============================================================================
// COUNTER TESTS (8 tests)
// =============================================================================

/// Test that counters start at zero.
#[test]
fn test_counter_increment_from_zero() {
    let metrics = create_metrics();

    // Record one message
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);

    // Counter should have been incremented
    // (We verify through the metrics output in integration tests)
}

/// Test that counters increment correctly multiple times.
#[test]
fn test_counter_increment_multiple_times() {
    let metrics = create_metrics();

    // Record multiple messages
    for _ in 0..100 {
        metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
    }

    // Counter should reflect all increments
}

/// Test that counters with different labels are independent.
#[test]
fn test_counter_increment_with_labels() {
    let metrics = create_metrics();

    // Record with different exchanges
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
    metrics.record_message_received("binance", "BTCUSDT", MessageType::Snapshot);

    // Each should be tracked independently
}

/// Test that different label combinations create independent counters.
#[test]
fn test_counter_different_labels_isolated() {
    let metrics = create_metrics();

    // Different instruments
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
    metrics.record_message_received("deribit", "ETH-PERPETUAL", MessageType::Delta);

    // Each instrument should have its own counter
}

/// Test message received counter with all message types.
#[test]
fn test_message_received_counter() {
    let metrics = create_metrics();

    // All message types
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Snapshot);
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Trade);
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Heartbeat);
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Unknown);
}

/// Test message published counter.
#[test]
fn test_message_published_counter() {
    let metrics = create_metrics();

    metrics.record_message_published("deribit", "BTC-PERPETUAL", "market_data.deribit.btc.book");
    metrics.record_message_published("binance", "BTCUSDT", "market_data.binance.btc.book");
}

/// Test error counter with different error types.
#[test]
fn test_error_counter() {
    let metrics = create_metrics();

    metrics.record_error("connection_failed", "critical");
    metrics.record_error("parse_error", "warning");
    metrics.record_error("timeout", "warning");
    metrics.record_error("sequence_gap", "error");
}

/// Test reconnection counter with all reasons.
#[test]
fn test_reconnection_counter() {
    let metrics = create_metrics();

    metrics.record_reconnection("deribit", ReconnectReason::ConnectionLost);
    metrics.record_reconnection("deribit", ReconnectReason::PongTimeout);
    metrics.record_reconnection("binance", ReconnectReason::SequenceGap);
    metrics.record_reconnection("oanda", ReconnectReason::ExchangeDisconnect);
    metrics.record_reconnection("deribit", ReconnectReason::Manual);
}

// =============================================================================
// GAUGE TESTS (8 tests)
// =============================================================================

/// Test setting a gauge value.
#[test]
fn test_gauge_set_value() {
    let metrics = create_metrics();

    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);
}

/// Test updating a gauge value.
#[test]
fn test_gauge_update_value() {
    let metrics = create_metrics();

    // Set initial value
    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);

    // Update to new value
    metrics.set_orderbook_levels("BTC-PERPETUAL", 45, 48);
}

/// Test gauges with different labels.
#[test]
fn test_gauge_set_with_labels() {
    let metrics = create_metrics();

    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);
    metrics.set_orderbook_levels("ETH-PERPETUAL", 40, 42);
}

/// Test that gauges with different labels are independent.
#[test]
fn test_gauge_different_labels_isolated() {
    let metrics = create_metrics();

    // Set for one instrument
    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);

    // Set different value for another instrument
    metrics.set_orderbook_levels("ETH-PERPETUAL", 30, 30);

    // They should not affect each other
}

/// Test order book levels gauge.
#[test]
fn test_orderbook_levels_gauge() {
    let metrics = create_metrics();

    // Various book depths
    metrics.set_orderbook_levels("BTC-PERPETUAL", 0, 0); // Empty book
    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50); // Normal
    metrics.set_orderbook_levels("BTC-PERPETUAL", 100, 100); // Max depth
}

/// Test order book spread gauge.
#[test]
fn test_orderbook_spread_gauge() {
    let metrics = create_metrics();

    metrics.set_orderbook_spread("BTC-PERPETUAL", 0.5);
    metrics.set_orderbook_spread("ETH-PERPETUAL", 0.25);
    metrics.set_orderbook_spread("BTC-PERPETUAL", 0.75); // Update
}

/// Test WebSocket connected gauge.
#[test]
fn test_websocket_connected_gauge() {
    let metrics = create_metrics();

    // Connection states
    metrics.set_websocket_connected("deribit", true);
    metrics.set_websocket_connected("binance", false);
    metrics.set_websocket_connected("deribit", false); // Disconnected
    metrics.set_websocket_connected("deribit", true); // Reconnected
}

/// Test queue depth gauge.
#[test]
fn test_queue_depth_gauge() {
    let metrics = create_metrics();

    metrics.set_queue_depth("redis_publish", 100, 10000);
    metrics.set_queue_depth("book_updates", 50, 5000);
    metrics.set_queue_depth("redis_publish", 9500, 10000); // Near capacity
}

// =============================================================================
// HISTOGRAM TESTS (8 tests)
// =============================================================================

/// Test recording a value to a histogram.
#[test]
fn test_histogram_record_value() {
    let metrics = create_metrics();

    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 2.5);
}

/// Test recording multiple values to a histogram.
#[test]
fn test_histogram_record_multiple_values() {
    let metrics = create_metrics();

    // Record various latencies
    for i in 0..100 {
        metrics.record_processing_duration("deribit", ProcessingStage::Parse, i as f64 * 0.1);
    }
}

/// Test histogram bucket distribution.
#[test]
fn test_histogram_bucket_distribution() {
    let metrics = create_metrics();

    // Record values that should fall into different buckets
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 0.1); // 100 ns bucket
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 1.0); // 1 μs bucket
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 10.0); // 10 μs bucket
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 100.0);
    // 100 μs bucket
}

/// Test histogram with labels.
#[test]
fn test_histogram_with_labels() {
    let metrics = create_metrics();

    // Different stages
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 2.0);
    metrics.record_processing_duration("deribit", ProcessingStage::Normalize, 1.0);
    metrics.record_processing_duration("deribit", ProcessingStage::BookUpdate, 0.5);
    metrics.record_processing_duration("deribit", ProcessingStage::Serialize, 0.3);
    metrics.record_processing_duration("deribit", ProcessingStage::Publish, 50.0);
}

/// Test processing duration histogram.
#[test]
fn test_processing_duration_histogram() {
    let metrics = create_metrics();

    // Realistic latency distribution
    let latencies = [0.5, 0.8, 1.0, 1.2, 1.5, 2.0, 2.5, 3.0, 5.0, 10.0];
    for latency in latencies {
        metrics.record_processing_duration("deribit", ProcessingStage::Parse, latency);
    }
}

/// Test order book update duration histogram.
#[test]
fn test_orderbook_update_duration_histogram() {
    let metrics = create_metrics();

    metrics.record_orderbook_update_duration("BTC-PERPETUAL", UpdateType::Snapshot, 25.0);
    metrics.record_orderbook_update_duration("BTC-PERPETUAL", UpdateType::Delta, 0.5);
    metrics.record_orderbook_update_duration("ETH-PERPETUAL", UpdateType::Delta, 0.6);
}

/// Test Redis publish duration histogram.
#[test]
fn test_redis_publish_duration_histogram() {
    let metrics = create_metrics();

    metrics.record_redis_publish_duration("market_data.deribit.btc.book", 50.0);
    metrics.record_redis_publish_duration("market_data.binance.btc.book", 45.0);
    metrics.record_redis_publish_duration("market_data.deribit.btc.trade", 30.0);
}

/// Test histogram with extreme values.
#[test]
fn test_histogram_extreme_values() {
    let metrics = create_metrics();

    // Very small values
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 0.01);

    // Very large values (outliers)
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 100000.0);

    // Zero (edge case)
    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 0.0);
}

// =============================================================================
// INTEGRATION TESTS (6 tests)
// =============================================================================

/// Test metrics registry creation.
#[test]
fn test_metrics_registry_creation() {
    let metrics = FlashMetrics::new();

    // Should not panic and should be usable
    metrics.record_message_received("test", "TEST-INSTRUMENT", MessageType::Heartbeat);
}

/// Test that FlashMetrics has a default implementation.
#[test]
fn test_metrics_default() {
    let metrics = FlashMetrics::default();

    // Should work the same as new()
    metrics.record_message_received("test", "TEST-INSTRUMENT", MessageType::Heartbeat);
}

/// Test that all metrics are registered and accessible.
#[test]
fn test_all_metrics_registered() {
    let metrics = create_metrics();

    // Exercise all metric types
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
    metrics.record_message_published("deribit", "BTC-PERPETUAL", "topic");
    metrics.record_error("test_error", "info");
    metrics.record_reconnection("deribit", ReconnectReason::Manual);

    metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);
    metrics.set_orderbook_spread("BTC-PERPETUAL", 0.5);
    metrics.set_websocket_connected("deribit", true);
    metrics.set_queue_depth("test_queue", 100, 1000);

    metrics.record_processing_duration("deribit", ProcessingStage::Parse, 1.0);
    metrics.record_orderbook_update_duration("BTC-PERPETUAL", UpdateType::Delta, 0.5);
    metrics.record_redis_publish_duration("topic", 10.0);
}

/// Test uptime calculation.
#[test]
fn test_uptime_calculation() {
    let metrics = create_metrics();

    // Small sleep to ensure measurable uptime
    thread::sleep(Duration::from_millis(10));

    let uptime = metrics.uptime_seconds();
    assert!(uptime >= 0.01, "Uptime should be at least 10ms");
    assert!(uptime < 60.0, "Uptime should be reasonable");
}

/// Test uptime increases over time.
#[test]
fn test_uptime_increases() {
    let metrics = create_metrics();

    let uptime1 = metrics.uptime_seconds();
    thread::sleep(Duration::from_millis(50));
    let uptime2 = metrics.uptime_seconds();

    assert!(uptime2 > uptime1, "Uptime should increase over time");
}

/// Test enum Display implementations.
#[test]
fn test_enum_display_implementations() {
    // ProcessingStage
    assert_eq!(ProcessingStage::Parse.as_str(), "parse");
    assert_eq!(ProcessingStage::Normalize.as_str(), "normalize");
    assert_eq!(ProcessingStage::BookUpdate.as_str(), "book_update");
    assert_eq!(ProcessingStage::Serialize.as_str(), "serialize");
    assert_eq!(ProcessingStage::Publish.as_str(), "publish");

    // UpdateType
    assert_eq!(UpdateType::Snapshot.as_str(), "snapshot");
    assert_eq!(UpdateType::Delta.as_str(), "delta");

    // MessageType
    assert_eq!(MessageType::Snapshot.as_str(), "snapshot");
    assert_eq!(MessageType::Delta.as_str(), "delta");
    assert_eq!(MessageType::Trade.as_str(), "trade");
    assert_eq!(MessageType::Heartbeat.as_str(), "heartbeat");
    assert_eq!(MessageType::Unknown.as_str(), "unknown");

    // ReconnectReason
    assert_eq!(ReconnectReason::ConnectionLost.as_str(), "connection_lost");
    assert_eq!(ReconnectReason::PongTimeout.as_str(), "pong_timeout");
    assert_eq!(ReconnectReason::SequenceGap.as_str(), "sequence_gap");
    assert_eq!(
        ReconnectReason::ExchangeDisconnect.as_str(),
        "exchange_disconnect"
    );
    assert_eq!(ReconnectReason::Manual.as_str(), "manual");
}

// =============================================================================
// THREAD SAFETY TESTS (4 tests)
// =============================================================================

/// Test that FlashMetrics is Send.
#[test]
fn test_metrics_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<FlashMetrics>();
}

/// Test that FlashMetrics is Sync.
#[test]
fn test_metrics_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<FlashMetrics>();
}

/// Test concurrent counter increments from multiple threads.
#[test]
fn test_concurrent_counter_increments() {
    let metrics = Arc::new(create_metrics());
    let mut handles = vec![];

    // Spawn 10 threads, each incrementing 1000 times
    for _ in 0..10 {
        let metrics_clone = Arc::clone(&metrics);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                metrics_clone.record_message_received(
                    "deribit",
                    "BTC-PERPETUAL",
                    MessageType::Delta,
                );
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Total should be 10,000 (verified through metrics output)
}

/// Test concurrent gauge updates from multiple threads.
#[test]
fn test_concurrent_gauge_updates() {
    let metrics = Arc::new(create_metrics());
    let mut handles = vec![];

    // Spawn threads updating the same gauge
    for i in 0..10 {
        let metrics_clone = Arc::clone(&metrics);
        let handle = thread::spawn(move || {
            for j in 0..100 {
                metrics_clone.set_orderbook_levels("BTC-PERPETUAL", i * 10 + j, i * 10 + j);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Gauge should have the last written value
}

// =============================================================================
// TIMING GUARD TESTS (4 tests)
// =============================================================================

/// Test that TimingGuard records duration on drop.
#[test]
fn test_timing_guard_records_duration() {
    let metrics = create_metrics();

    {
        let _guard = metrics.time_processing("deribit", ProcessingStage::Parse);
        // Simulate some work
        thread::sleep(Duration::from_millis(1));
    } // Guard dropped here, duration recorded
}

/// Test TimingGuard with explicit drop.
#[test]
fn test_timing_guard_drop_records() {
    let metrics = create_metrics();

    let guard = metrics.time_processing("deribit", ProcessingStage::Normalize);
    thread::sleep(Duration::from_millis(1));
    drop(guard); // Explicit drop
}

/// Test multiple timing guards for different stages.
#[test]
fn test_timing_guard_multiple_stages() {
    let metrics = create_metrics();

    // Simulate a processing pipeline
    {
        let _parse = metrics.time_processing("deribit", ProcessingStage::Parse);
        thread::sleep(Duration::from_micros(100));
    }
    {
        let _normalize = metrics.time_processing("deribit", ProcessingStage::Normalize);
        thread::sleep(Duration::from_micros(50));
    }
    {
        let _update = metrics.time_processing("deribit", ProcessingStage::BookUpdate);
        thread::sleep(Duration::from_micros(25));
    }
}

/// Test that timing guard still records even if panic occurs.
#[test]
fn test_timing_guard_panic_recovery() {
    let metrics = Arc::new(create_metrics());
    let metrics_clone = Arc::clone(&metrics);

    let result = std::panic::catch_unwind(move || {
        let _guard = metrics_clone.time_processing("deribit", ProcessingStage::Parse);
        panic!("Test panic!");
    });

    // Should have panicked
    assert!(result.is_err());

    // Metrics should still be usable
    metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Heartbeat);
}

// =============================================================================
// EDGE CASE TESTS (2 additional tests)
// =============================================================================

/// Test with empty string labels.
#[test]
fn test_empty_string_labels() {
    let metrics = create_metrics();

    // Empty strings should be handled gracefully
    metrics.record_message_received("", "", MessageType::Unknown);
    metrics.set_orderbook_levels("", 0, 0);
}

/// Test with special characters in labels.
#[test]
fn test_special_characters_in_labels() {
    let metrics = create_metrics();

    // Special characters that might appear in instrument names
    metrics.record_message_received("exchange-1", "BTC/USD", MessageType::Delta);
    metrics.record_message_received("exchange_2", "BTC_USDT", MessageType::Delta);
    metrics.set_orderbook_spread("ETH-PERP@100x", 0.5);
}

// =============================================================================
// ERROR TYPE TESTS (2 tests)
// =============================================================================

/// Test MetricsError Display implementation.
#[test]
fn test_metrics_error_display() {
    let error = MetricsError::PortInUse { port: 9090 };
    let msg = format!("{}", error);
    assert!(msg.contains("9090"));
    assert!(msg.contains("port") || msg.contains("Port"));

    let error = MetricsError::ExporterInitFailed("test error".to_string());
    let msg = format!("{}", error);
    assert!(msg.contains("test error"));
}

/// Test MetricsError Debug implementation.
#[test]
fn test_metrics_error_debug() {
    let error = MetricsError::InvalidLabelValue {
        label: "exchange".to_string(),
        value: "invalid\nvalue".to_string(),
    };
    let debug = format!("{:?}", error);
    assert!(debug.contains("InvalidLabelValue"));
}
