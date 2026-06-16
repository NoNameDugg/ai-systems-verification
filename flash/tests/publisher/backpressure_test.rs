//! Tests for Backpressure module (Batch 3.3).
//!
//! Run with: `cargo test --test publisher_tests -- backpressure`
//!
//! These tests verify:
//! - try_send logic (non-blocking sends)
//! - dropped_frames metric tracking
//! - Flood test (sending faster than consumer can process)

use astra_flash::publisher::backpressure::{
    BackpressureConfig, BackpressureMetrics, BackpressurePolicy, BackpressureSendError,
    BackpressureSender, send_with_backpressure,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test message for benchmarking.
fn test_message(id: u64) -> TestMessage {
    TestMessage {
        id,
        data: format!("test_payload_{}", id),
        timestamp: std::time::Instant::now(),
    }
}

/// Test message type for backpressure testing.
#[derive(Debug, Clone)]
struct TestMessage {
    id: u64,
    data: String,
    timestamp: std::time::Instant,
}

// =============================================================================
// BACKPRESSURE CONFIG TESTS
// =============================================================================

#[test]
fn test_backpressure_config_default() {
    let config = BackpressureConfig::default();
    assert_eq!(config.channel_capacity, 1000);
    assert_eq!(config.policy, BackpressurePolicy::DropNewest);
    assert!(config.warn_on_drop);
    assert!((config.drop_rate_alert_threshold - 0.01).abs() < f64::EPSILON);
}

#[test]
fn test_backpressure_config_builder() {
    let config = BackpressureConfig::builder()
        .channel_capacity(500)
        .policy(BackpressurePolicy::WarnAndContinue)
        .warn_on_drop(false)
        .drop_rate_alert_threshold(0.05)
        .build();

    assert_eq!(config.channel_capacity, 500);
    assert_eq!(config.policy, BackpressurePolicy::WarnAndContinue);
    assert!(!config.warn_on_drop);
    assert!((config.drop_rate_alert_threshold - 0.05).abs() < f64::EPSILON);
}

#[test]
fn test_backpressure_policy_default() {
    // Default policy must be DropNewest for HFT (never block)
    let policy = BackpressurePolicy::default();
    assert_eq!(policy, BackpressurePolicy::DropNewest);
}

#[test]
fn test_backpressure_policy_display() {
    assert_eq!(BackpressurePolicy::DropNewest.to_string(), "DropNewest");
    assert_eq!(BackpressurePolicy::WarnAndContinue.to_string(), "WarnAndContinue");
}

// =============================================================================
// BACKPRESSURE METRICS TESTS
// =============================================================================

#[test]
fn test_metrics_default() {
    let metrics = BackpressureMetrics::default();
    assert_eq!(metrics.dropped_frames(), 0);
    assert_eq!(metrics.sent_frames(), 0);
    assert!((metrics.drop_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_metrics_increment_dropped() {
    let metrics = BackpressureMetrics::default();
    metrics.increment_dropped();
    metrics.increment_dropped();
    assert_eq!(metrics.dropped_frames(), 2);
}

#[test]
fn test_metrics_increment_sent() {
    let metrics = BackpressureMetrics::default();
    metrics.increment_sent();
    metrics.increment_sent();
    metrics.increment_sent();
    assert_eq!(metrics.sent_frames(), 3);
}

#[test]
fn test_metrics_drop_rate_calculation() {
    let metrics = BackpressureMetrics::default();

    // Send 90, drop 10 = 10% drop rate
    for _ in 0..90 {
        metrics.increment_sent();
    }
    for _ in 0..10 {
        metrics.increment_dropped();
    }

    let rate = metrics.drop_rate();
    assert!((rate - 0.1).abs() < 0.001, "Expected ~0.1, got {}", rate);
}

#[test]
fn test_metrics_drop_rate_zero_total() {
    let metrics = BackpressureMetrics::default();
    // No frames at all - drop rate should be 0
    assert!((metrics.drop_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_metrics_total_frames() {
    let metrics = BackpressureMetrics::default();
    metrics.increment_sent();
    metrics.increment_sent();
    metrics.increment_dropped();
    assert_eq!(metrics.total_frames(), 3);
}

#[test]
fn test_metrics_reset() {
    let metrics = BackpressureMetrics::default();
    metrics.increment_sent();
    metrics.increment_dropped();

    metrics.reset();

    assert_eq!(metrics.dropped_frames(), 0);
    assert_eq!(metrics.sent_frames(), 0);
}

#[test]
fn test_metrics_exceeds_alert_threshold() {
    let metrics = BackpressureMetrics::default();
    let threshold = 0.01; // 1%

    // 98 sent, 2 dropped = 2% drop rate > 1% threshold
    for _ in 0..98 {
        metrics.increment_sent();
    }
    metrics.increment_dropped();
    metrics.increment_dropped();

    assert!(metrics.exceeds_alert_threshold(threshold));
}

#[test]
fn test_metrics_below_alert_threshold() {
    let metrics = BackpressureMetrics::default();
    let threshold = 0.01; // 1%

    // 100 sent, 0 dropped = 0% drop rate < 1% threshold
    for _ in 0..100 {
        metrics.increment_sent();
    }

    assert!(!metrics.exceeds_alert_threshold(threshold));
}

// =============================================================================
// BACKPRESSURE SENDER TESTS
// =============================================================================

#[test]
fn test_backpressure_sender_creation() {
    let config = BackpressureConfig::default();
    let (sender, _receiver) = BackpressureSender::<TestMessage>::new(config);

    assert_eq!(sender.capacity(), 1000);
    assert_eq!(sender.metrics().dropped_frames(), 0);
}

#[test]
fn test_backpressure_sender_with_capacity() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(500);
    assert_eq!(sender.capacity(), 500);
}

#[tokio::test]
async fn test_backpressure_sender_try_send_success() {
    let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(10);

    let result = sender.try_send(42);
    assert!(result.is_ok());
    assert_eq!(sender.metrics().sent_frames(), 1);
    assert_eq!(sender.metrics().dropped_frames(), 0);

    // Verify message was actually sent
    let msg = receiver.recv().await.unwrap();
    assert_eq!(msg, 42);
}

#[tokio::test]
async fn test_backpressure_sender_try_send_multiple() {
    let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(10);

    for i in 0..5 {
        let result = sender.try_send(i);
        assert!(result.is_ok());
    }

    assert_eq!(sender.metrics().sent_frames(), 5);

    // Verify all messages received
    for i in 0..5 {
        let msg = receiver.recv().await.unwrap();
        assert_eq!(msg, i);
    }
}

#[test]
fn test_backpressure_sender_try_send_channel_full() {
    // Create tiny channel that will fill up
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(2);

    // Fill the channel
    assert!(sender.try_send(1).is_ok());
    assert!(sender.try_send(2).is_ok());

    // Third message should be dropped (channel full)
    let result = sender.try_send(3);
    assert!(result.is_err());

    match result {
        Err(BackpressureSendError::ChannelFull { dropped_value }) => {
            assert_eq!(dropped_value, 3);
        }
        _ => panic!("Expected ChannelFull error"),
    }

    // Verify metrics
    assert_eq!(sender.metrics().sent_frames(), 2);
    assert_eq!(sender.metrics().dropped_frames(), 1);
}

#[tokio::test]
async fn test_backpressure_sender_channel_closed() {
    let (sender, receiver) = BackpressureSender::<u64>::with_capacity(10);

    // Drop the receiver to close the channel
    drop(receiver);

    // Now try to send
    let result = sender.try_send(42);

    match result {
        Err(BackpressureSendError::ChannelClosed { dropped_value }) => {
            assert_eq!(dropped_value, 42);
        }
        _ => panic!("Expected ChannelClosed error"),
    }
}

#[test]
fn test_backpressure_sender_available_capacity() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(10);

    assert_eq!(sender.available_capacity(), 10);

    sender.try_send(1).unwrap();
    sender.try_send(2).unwrap();

    assert_eq!(sender.available_capacity(), 8);
}

#[test]
fn test_backpressure_sender_is_full() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(2);

    assert!(!sender.is_full());

    sender.try_send(1).unwrap();
    assert!(!sender.is_full());

    sender.try_send(2).unwrap();
    assert!(sender.is_full());
}

#[test]
fn test_backpressure_sender_is_empty() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(10);
    assert!(sender.is_empty());

    sender.try_send(1).unwrap();
    assert!(!sender.is_empty());
}

// =============================================================================
// send_with_backpressure FUNCTION TESTS
// =============================================================================

#[tokio::test]
async fn test_send_with_backpressure_success() {
    let (tx, mut rx) = mpsc::channel::<u64>(10);
    let metrics = Arc::new(BackpressureMetrics::default());

    send_with_backpressure(&tx, 42, &metrics);

    assert_eq!(metrics.sent_frames(), 1);
    assert_eq!(metrics.dropped_frames(), 0);

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, 42);
}

#[test]
fn test_send_with_backpressure_channel_full() {
    let (tx, _rx) = mpsc::channel::<u64>(2);
    let metrics = Arc::new(BackpressureMetrics::default());

    // Fill channel
    send_with_backpressure(&tx, 1, &metrics);
    send_with_backpressure(&tx, 2, &metrics);

    // This should be dropped
    send_with_backpressure(&tx, 3, &metrics);

    assert_eq!(metrics.sent_frames(), 2);
    assert_eq!(metrics.dropped_frames(), 1);
}

#[test]
fn test_send_with_backpressure_channel_closed() {
    let (tx, rx) = mpsc::channel::<u64>(10);
    let metrics = Arc::new(BackpressureMetrics::default());

    drop(rx); // Close the channel

    send_with_backpressure(&tx, 42, &metrics);

    // Closed channel also counts as dropped
    assert_eq!(metrics.dropped_frames(), 1);
}

// =============================================================================
// FLOOD TEST - CRITICAL VERIFICATION
// =============================================================================
// Verify: Sending faster than consumer can process results in dropped frames

#[tokio::test]
async fn test_flood_fast_producer_slow_consumer() {
    // Small capacity channel to easily trigger backpressure
    let (sender, mut receiver) = BackpressureSender::<TestMessage>::with_capacity(10);
    let metrics = sender.metrics();

    // Producer: Send 1000 messages as fast as possible (non-blocking)
    let producer_handle = tokio::spawn({
        let sender = sender.clone();
        async move {
            for i in 0..1000_u64 {
                let _ = sender.try_send(test_message(i));
            }
        }
    });

    // Consumer: Process slowly (simulates slow Redis writes)
    let consumer_handle = tokio::spawn(async move {
        let mut received = 0;
        while let Ok(msg) = tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await {
            if msg.is_none() {
                break;
            }
            received += 1;
            // Simulate slow processing (1ms per message)
            tokio::time::sleep(Duration::from_micros(100)).await;
        }
        received
    });

    // Wait for producer to finish
    producer_handle.await.unwrap();

    // Drop sender to close channel
    drop(sender);

    // Wait for consumer
    let received = consumer_handle.await.unwrap();

    let dropped = metrics.dropped_frames();
    let sent = metrics.sent_frames();

    // VERIFICATION: Under flood conditions, frames MUST be dropped
    assert!(dropped > 0, "Flood test should have dropped frames, but dropped = 0");

    // Total should equal 1000 (sent + dropped)
    assert_eq!(
        sent + dropped,
        1000,
        "sent ({}) + dropped ({}) should equal 1000",
        sent,
        dropped
    );

    // Verify non-blocking: all 1000 messages processed instantly
    // (producer finished before consumer could process many)

    println!("Flood test results:");
    println!("  Total attempted: 1000");
    println!("  Sent successfully: {}", sent);
    println!("  Dropped (backpressure): {}", dropped);
    println!("  Received by consumer: {}", received);
    println!("  Drop rate: {:.2}%", metrics.drop_rate() * 100.0);
}

#[tokio::test]
async fn test_flood_verify_non_blocking() {
    // This test verifies that try_send never blocks, even under heavy load
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(100);

    let start = std::time::Instant::now();

    // Send 10,000 messages as fast as possible
    for i in 0..10_000_u64 {
        let _ = sender.try_send(i);
    }

    let elapsed = start.elapsed();

    // CRITICAL: If try_send blocked, this would take ~10 seconds
    // With non-blocking sends, should complete in < 10ms
    assert!(
        elapsed < Duration::from_millis(100),
        "try_send blocked! Elapsed: {:?} (should be < 100ms)",
        elapsed
    );

    let metrics = sender.metrics();
    println!("Non-blocking test completed in {:?}", elapsed);
    println!("  Sent: {}", metrics.sent_frames());
    println!("  Dropped: {}", metrics.dropped_frames());
}

#[tokio::test]
async fn test_flood_drop_rate_calculation() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(100);

    // Send 1000 messages, channel can only hold 100
    for i in 0..1000_u64 {
        let _ = sender.try_send(i);
    }

    let metrics = sender.metrics();
    let drop_rate = metrics.drop_rate();

    // With capacity 100 and 1000 messages, expect ~90% drop rate
    assert!(
        drop_rate > 0.8,
        "Expected high drop rate (>80%), got {:.2}%",
        drop_rate * 100.0
    );
}

#[tokio::test]
async fn test_flood_alert_threshold_exceeded() {
    let config = BackpressureConfig::builder()
        .channel_capacity(10)
        .drop_rate_alert_threshold(0.01) // 1% threshold
        .build();

    let (sender, _receiver) = BackpressureSender::<u64>::new(config);

    // Send 100 messages into channel with capacity 10
    for i in 0..100_u64 {
        let _ = sender.try_send(i);
    }

    // Drop rate should exceed 1% threshold
    assert!(
        sender.metrics().exceeds_alert_threshold(0.01),
        "Alert threshold should be exceeded after flood"
    );
}

// =============================================================================
// CONCURRENT FLOOD TEST
// =============================================================================

#[tokio::test]
async fn test_flood_multiple_producers() {
    let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(100);

    // Spawn 4 producers, each sending 1000 messages
    let mut producer_handles = Vec::new();
    for producer_id in 0..4_u64 {
        let sender = sender.clone();
        producer_handles.push(tokio::spawn(async move {
            for i in 0..1000_u64 {
                let _ = sender.try_send(producer_id * 1000 + i);
            }
        }));
    }

    // Slow consumer
    let consumer_handle = tokio::spawn(async move {
        let mut count = 0;
        while let Ok(msg) = tokio::time::timeout(Duration::from_millis(50), receiver.recv()).await {
            if msg.is_none() {
                break;
            }
            count += 1;
        }
        count
    });

    // Wait for all producers
    for handle in producer_handles {
        handle.await.unwrap();
    }

    // Drop sender to close channel
    drop(sender);

    let received = consumer_handle.await.unwrap();

    println!("Multi-producer flood test:");
    println!("  Total attempted: 4000");
    println!("  Received: {}", received);

    // Some messages should have been dropped under concurrent load
    assert!(received < 4000, "Expected some drops under concurrent load");
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

#[test]
fn test_backpressure_sender_clone() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(10);

    // Clone and verify both share the same metrics
    let sender2 = sender.clone();

    sender.try_send(1).unwrap();
    sender2.try_send(2).unwrap();

    // Both clones should share metrics
    assert_eq!(sender.metrics().sent_frames(), 2);
    assert_eq!(sender2.metrics().sent_frames(), 2);
}

#[test]
fn test_backpressure_sender_debug() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(10);

    let debug_str = format!("{:?}", sender);
    assert!(debug_str.contains("BackpressureSender"));
}

#[tokio::test]
async fn test_backpressure_result_success() {
    let (sender, _receiver) = BackpressureSender::<u64>::with_capacity(10);

    let result = sender.try_send(42);
    assert!(result.is_ok());
}

#[test]
fn test_backpressure_send_error_display() {
    let err: BackpressureSendError<u64> = BackpressureSendError::ChannelFull { dropped_value: 42 };
    assert!(err.to_string().contains("Channel full"));

    let err2: BackpressureSendError<u64> = BackpressureSendError::ChannelClosed { dropped_value: 99 };
    assert!(err2.to_string().contains("closed"));
}
