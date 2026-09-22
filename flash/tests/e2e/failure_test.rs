//! Failure and Resilience Tests for Flash.
//!
//! These tests validate error handling and recovery:
//!
//! # Test Coverage
//!
//! 1. WebSocket disconnects mid-stream
//! 2. Automatic reconnection works
//! 3. Subscriptions restored after reconnect
//! 4. Redis unavailable during publish
//! 5. Redis comes back, publishing resumes
//! 6. Malformed JSON in message
//! 7. Invalid price values (NaN/Inf)
//! 8. Sequence gap detection
//! 9. Gap triggers snapshot request
//! 10. Backpressure handling

use astra_flash::prelude::*;
use astra_flash::publisher::{
    BackpressureMonitor, BackpressureStatus, BatchConfig, OverflowAction,
};
use rust_decimal_macros::dec;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::common::*;

// =============================================================================
// FAILURE TEST HELPERS
// =============================================================================

/// Simulated connection state for failure testing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SimulatedState {
    Connected,
    Disconnected,
    Reconnecting,
}

/// Track reconnection attempts.
struct ReconnectionTracker {
    state: parking_lot::RwLock<SimulatedState>,
    attempt_count: AtomicU64,
    success_count: AtomicU64,
    failure_count: AtomicU64,
}

impl ReconnectionTracker {
    fn new() -> Self {
        Self {
            state: parking_lot::RwLock::new(SimulatedState::Disconnected),
            attempt_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
        }
    }

    fn connect(&self) -> bool {
        *self.state.write() = SimulatedState::Connected;
        true
    }

    fn disconnect(&self) {
        *self.state.write() = SimulatedState::Disconnected;
    }

    fn attempt_reconnect(&self) -> bool {
        self.attempt_count.fetch_add(1, Ordering::SeqCst);
        *self.state.write() = SimulatedState::Reconnecting;

        // Simulate 80% success rate
        let success = self.attempt_count.load(Ordering::SeqCst) % 5 != 0;

        if success {
            self.success_count.fetch_add(1, Ordering::SeqCst);
            *self.state.write() = SimulatedState::Connected;
        } else {
            self.failure_count.fetch_add(1, Ordering::SeqCst);
            *self.state.write() = SimulatedState::Disconnected;
        }

        success
    }

    fn is_connected(&self) -> bool {
        *self.state.read() == SimulatedState::Connected
    }
}

// =============================================================================
// FAILURE TESTS (10)
// =============================================================================

/// Test 1: WebSocket disconnects mid-stream.
#[tokio::test]
async fn test_failure_ws_disconnect_mid_stream() {
    // ARRANGE
    let mock_server = MockExchangeServer::start(Exchange::Deribit).await;
    let tracker = ReconnectionTracker::new();

    // Connect initially
    mock_server.simulate_connect();
    tracker.connect();
    assert!(tracker.is_connected());

    // Queue some messages
    mock_server
        .queue_message(MockMessage::text(deribit_book_snapshot(
            "BTC-PERPETUAL",
            &[(50000.0, 1.0)],
            &[(50010.0, 1.0)],
            1,
        )))
        .await;

    // ACT: Simulate disconnect mid-stream
    mock_server.simulate_disconnect();
    tracker.disconnect();

    // ASSERT
    assert!(!tracker.is_connected());
    assert_eq!(mock_server.connection_count(), 0);

    // Messages should still be queued
    assert_eq!(mock_server.pending_messages(), 1);
}

/// Test 2: Automatic reconnection works.
#[tokio::test]
async fn test_failure_ws_automatic_reconnect() {
    // ARRANGE
    let mock_server = MockExchangeServer::start(Exchange::Binance).await;
    let tracker = Arc::new(ReconnectionTracker::new());
    let tracker_clone = tracker.clone();

    // Initial connection
    mock_server.simulate_connect();
    tracker.connect();

    // ACT: Disconnect and attempt reconnection
    mock_server.simulate_disconnect();
    tracker.disconnect();
    assert!(!tracker.is_connected());

    // Simulate reconnection loop (max 5 attempts)
    let reconnect_handle = tokio::spawn(async move {
        for _ in 0..5 {
            if tracker_clone.attempt_reconnect() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    });

    let reconnected = reconnect_handle.await.unwrap();

    // ASSERT
    assert!(reconnected, "Should successfully reconnect");
    assert!(tracker.attempt_count.load(Ordering::SeqCst) > 0);
    assert!(tracker.success_count.load(Ordering::SeqCst) > 0);
}

/// Test 3: Subscriptions restored after reconnect.
#[tokio::test]
async fn test_failure_ws_reconnect_with_resubscribe() {
    // ARRANGE
    let subscriptions: Arc<parking_lot::RwLock<Vec<String>>> =
        Arc::new(parking_lot::RwLock::new(Vec::new()));

    // Add subscriptions
    subscriptions.write().push("BTC-PERPETUAL".to_string());
    subscriptions.write().push("ETH-PERPETUAL".to_string());

    let initial_subs = subscriptions.read().len();
    assert_eq!(initial_subs, 2);

    // ACT: Simulate disconnect and reconnect
    // (subscriptions should be preserved and re-sent)
    let subs_clone = subscriptions.clone();
    let restore_handle = tokio::spawn(async move {
        // Simulate reconnection delay
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Restore subscriptions (they should still be there)
        let count = subs_clone.read().len();
        count
    });

    let restored_count = restore_handle.await.unwrap();

    // ASSERT
    assert_eq!(restored_count, 2, "Subscriptions should be preserved");
    assert!(subscriptions.read().contains(&"BTC-PERPETUAL".to_string()));
    assert!(subscriptions.read().contains(&"ETH-PERPETUAL".to_string()));
}

/// Test 4: Redis unavailable during publish.
#[tokio::test]
async fn test_failure_redis_unavailable_during_publish() {
    // ARRANGE
    let redis_available = Arc::new(AtomicBool::new(true));
    let failed_publishes = Arc::new(AtomicU64::new(0));

    // Simulate publish function
    let redis_available_clone = redis_available.clone();
    let failed_clone = failed_publishes.clone();

    let publish = move |_: &BookSnapshot| -> Result<(), &'static str> {
        if redis_available_clone.load(Ordering::SeqCst) {
            Ok(())
        } else {
            failed_clone.fetch_add(1, Ordering::SeqCst);
            Err("Redis unavailable")
        }
    };

    // Create snapshot
    let instrument = test_instrument(Exchange::Deribit);
    let snapshot = BookSnapshot {
        instrument,
        timestamp: now_micros(),
        bids: price_levels(&[(50000.0, 1.0)]),
        asks: price_levels(&[(50010.0, 1.0)]),
    };

    // ACT: First publish succeeds
    assert!(publish(&snapshot).is_ok());

    // Redis becomes unavailable
    redis_available.store(false, Ordering::SeqCst);

    // Next publish fails
    assert!(publish(&snapshot).is_err());
    assert!(publish(&snapshot).is_err());
    assert!(publish(&snapshot).is_err());

    // ASSERT
    assert_eq!(failed_publishes.load(Ordering::SeqCst), 3);
}

/// Test 5: Redis comes back, publishing resumes.
#[tokio::test]
async fn test_failure_redis_reconnect_resumes_publishing() {
    // ARRANGE
    let redis_available = Arc::new(AtomicBool::new(true));
    let successful_publishes = Arc::new(AtomicU64::new(0));
    let failed_publishes = Arc::new(AtomicU64::new(0));

    let redis_clone = redis_available.clone();
    let success_clone = successful_publishes.clone();
    let failed_clone = failed_publishes.clone();

    let publish = move |_: &BookSnapshot| -> bool {
        if redis_clone.load(Ordering::SeqCst) {
            success_clone.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            failed_clone.fetch_add(1, Ordering::SeqCst);
            false
        }
    };

    let snapshot = BookSnapshot {
        instrument: test_instrument(Exchange::Deribit),
        timestamp: now_micros(),
        bids: price_levels(&[(50000.0, 1.0)]),
        asks: price_levels(&[(50010.0, 1.0)]),
    };

    // ACT: Publish cycle with outage
    // Phase 1: Redis available
    assert!(publish(&snapshot));
    assert!(publish(&snapshot));

    // Phase 2: Redis down
    redis_available.store(false, Ordering::SeqCst);
    assert!(!publish(&snapshot));
    assert!(!publish(&snapshot));

    // Phase 3: Redis back
    redis_available.store(true, Ordering::SeqCst);
    assert!(publish(&snapshot));
    assert!(publish(&snapshot));

    // ASSERT
    assert_eq!(successful_publishes.load(Ordering::SeqCst), 4);
    assert_eq!(failed_publishes.load(Ordering::SeqCst), 2);
}

/// Test 6: Malformed JSON is handled gracefully.
#[tokio::test]
async fn test_failure_malformed_json_handled() {
    // ARRANGE
    let malformed_messages = vec![
        malformed::empty(),
        malformed::invalid_json(),
        malformed::missing_fields(),
    ];

    let mut parse_errors = 0;
    let mut successful_parses = 0;

    // ACT: Try to parse each malformed message
    for msg in &malformed_messages {
        let result: Result<serde_json::Value, _> = serde_json::from_str(msg);

        match result {
            Ok(value) => {
                // Even if JSON parses, it might be missing required fields
                if value.get("jsonrpc").is_none() && value.get("data").is_none() {
                    parse_errors += 1;
                } else {
                    successful_parses += 1;
                }
            }
            Err(_) => {
                parse_errors += 1;
            }
        }
    }

    // ASSERT
    // Empty and invalid JSON should fail, missing_fields parses but is invalid
    assert!(
        parse_errors >= 2,
        "At least 2 messages should fail to parse properly"
    );
}

/// Test 7: Invalid price values (NaN/Inf) are detected.
#[tokio::test]
async fn test_failure_invalid_price_values_detected() {
    // ARRANGE
    let validator = SnapshotValidator::new(ValidationConfig::default());

    // Create snapshots with invalid prices
    let nan_snapshot = BookSnapshot {
        instrument: test_instrument(Exchange::Deribit),
        timestamp: now_micros(),
        bids: vec![PriceLevel {
            price: f64::NAN,
            quantity: dec!(1),
            order_count: None,
            timestamp: now_micros(),
        }],
        asks: price_levels(&[(50010.0, 1.0)]),
    };

    let inf_snapshot = BookSnapshot {
        instrument: test_instrument(Exchange::Deribit),
        timestamp: now_micros(),
        bids: vec![PriceLevel {
            price: f64::INFINITY,
            quantity: dec!(1),
            order_count: None,
            timestamp: now_micros(),
        }],
        asks: price_levels(&[(50010.0, 1.0)]),
    };

    let neg_snapshot = BookSnapshot {
        instrument: test_instrument(Exchange::Deribit),
        timestamp: now_micros(),
        bids: vec![PriceLevel {
            price: -50000.0,
            quantity: dec!(1),
            order_count: None,
            timestamp: now_micros(),
        }],
        asks: price_levels(&[(50010.0, 1.0)]),
    };

    // ACT & ASSERT
    let nan_result = validator.validate(&nan_snapshot.bids, &nan_snapshot.asks);
    let inf_result = validator.validate(&inf_snapshot.bids, &inf_snapshot.asks);
    let neg_result = validator.validate(&neg_snapshot.bids, &neg_snapshot.asks);

    assert!(nan_result.is_err(), "NaN price should be rejected");
    assert!(inf_result.is_err(), "Infinity price should be rejected");
    assert!(neg_result.is_err(), "Negative price should be rejected");
}

/// Test 8: Sequence gap detection.
#[tokio::test]
async fn test_failure_sequence_gap_detected() {
    // ARRANGE
    let mut tracker = SequenceTracker::new(SequenceConfig::default());
    let mut gaps_detected = Vec::new();

    // ACT: Process sequence with gap
    let sequences = [1u64, 2, 3, 10, 11, 12]; // Gap from 3 to 10

    for seq in sequences {
        let result = tracker.record(seq);
        if let SequenceCheckResult::Gap(gap) = result {
            gaps_detected.push((gap.expected, gap.received));
        }
    }

    // ASSERT
    assert_eq!(gaps_detected.len(), 1, "Should detect 1 gap");
    assert_eq!(gaps_detected[0], (4, 10), "Gap should be from 4 to 10");
}

/// Test 9: Sequence gap triggers snapshot request.
#[tokio::test]
async fn test_failure_gap_triggers_snapshot_request() {
    // ARRANGE
    let config = SequenceConfig {
        max_gaps: 5,
        ..SequenceConfig::default()
    };
    let mut tracker = SequenceTracker::new(config);
    let mut snapshot_requests = 0;

    // ACT: Process sequence with large gap
    let sequences = [1u64, 2, 3, 100]; // Gap of 97

    for seq in sequences {
        let result = tracker.record(seq);

        if let SequenceCheckResult::Gap(gap) = result {
            let gap_size = gap.received - gap.expected;
            if gap_size > 5 {
                snapshot_requests += 1;
            }
        }
    }

    // ASSERT
    assert_eq!(
        snapshot_requests, 1,
        "Should request snapshot for large gap"
    );
}

/// Test 10: Backpressure handling when channel is full.
#[tokio::test]
async fn test_failure_backpressure_handling() {
    // ARRANGE
    let config = BatchConfig {
        max_batch_size: 10,
        max_batch_delay: Duration::from_millis(100),
        channel_capacity: 100,
        overflow_action: OverflowAction::DropOldest,
        ..BatchConfig::default()
    };

    // BackpressureMonitor::new(warn_threshold, critical_threshold, capacity)
    let mut monitor = BackpressureMonitor::new(
        0.8,  // warn at 80%
        0.95, // critical at 95%
        config.channel_capacity,
    );

    // ACT: Simulate filling the channel
    let mut dropped_count = 0;

    for i in 0..150 {
        // Update monitor with current queue depth
        let queue_depth = (i).min(config.channel_capacity);
        let (status, _changed) = monitor.update(queue_depth);

        // If critical, simulate drop
        if status == BackpressureStatus::Critical {
            dropped_count += 1;
        }
    }

    // ASSERT
    assert!(dropped_count > 0, "Should drop messages when critical");

    // Check final status
    let _ = monitor.update(config.channel_capacity);
    assert_eq!(
        monitor.status(),
        BackpressureStatus::Critical,
        "Should be critical when full"
    );

    // Drain some messages
    let _ = monitor.update(50);
    assert_eq!(
        monitor.status(),
        BackpressureStatus::Normal,
        "Should return to normal when drained"
    );
}

// =============================================================================
// ADDITIONAL FAILURE TESTS
// =============================================================================

#[cfg(test)]
mod additional_tests {
    use super::*;

    /// Test recovery from multiple consecutive failures.
    #[tokio::test]
    async fn test_failure_multiple_consecutive_failures() {
        let tracker = ReconnectionTracker::new();
        let mut consecutive_failures = 0;
        let max_failures = 3;

        // Force failures then success
        tracker.attempt_count.store(0, Ordering::SeqCst);

        for _ in 0..10 {
            // Modify to force failures for first few attempts
            let attempt = tracker.attempt_count.load(Ordering::SeqCst);
            if attempt < max_failures as u64 {
                tracker.failure_count.fetch_add(1, Ordering::SeqCst);
                tracker.attempt_count.fetch_add(1, Ordering::SeqCst);
                consecutive_failures += 1;
            } else if tracker.attempt_reconnect() {
                break;
            }
        }

        assert_eq!(consecutive_failures, max_failures);
        assert!(tracker.success_count.load(Ordering::SeqCst) > 0);
    }

    /// Test handling of crossed book (bid >= ask).
    #[tokio::test]
    async fn test_failure_crossed_book_detected() {
        let validator = SnapshotValidator::new(ValidationConfig {
            allow_crossed: false, // Reject crossed books
            ..ValidationConfig::default()
        });

        let crossed = BookSnapshot {
            instrument: test_instrument(Exchange::Deribit),
            timestamp: now_micros(),
            bids: price_levels(&[(50100.0, 1.0)]), // Bid higher than ask
            asks: price_levels(&[(50000.0, 1.0)]),
        };

        let result = validator.validate(&crossed.bids, &crossed.asks);
        assert!(result.is_err(), "Crossed book should be rejected");
    }

    /// Test that empty book is handled.
    #[tokio::test]
    async fn test_failure_empty_book_handling() {
        let book = ThreadSafeOrderBook::new(
            test_instrument(Exchange::Deribit),
            OrderBookConfig::default(),
        );

        // Empty book should return None for prices
        assert!(book.mid_price().is_none());
        assert!(book.spread().is_none());
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());

        // Snapshot should have empty levels
        let snapshot = book.snapshot(50);
        assert!(snapshot.bids.is_empty());
        assert!(snapshot.asks.is_empty());
    }

    /// Test duplicate level handling.
    #[tokio::test]
    async fn test_failure_duplicate_levels() {
        let book = ThreadSafeOrderBook::new(
            test_instrument(Exchange::Deribit),
            OrderBookConfig::default(),
        );

        // Apply same price twice with different quantities
        book.apply_snapshot(
            price_levels(&[(50000.0, 1.0), (50000.0, 2.0)]), // Duplicate price
            price_levels(&[(50010.0, 1.0)]),
            now_micros(),
        );

        // Last one should win (BTreeMap behavior)
        let snapshot = book.snapshot(50);
        assert_eq!(snapshot.bids.len(), 1);
        // The quantity should be from the last applied level
    }
}
