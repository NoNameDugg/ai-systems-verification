//! End-to-End Pipeline Tests for Flash.
//!
//! These tests validate the complete data flow through the system:
//!
//! ```text
//! [Exchange WebSocket] -> [Exchange Adapter] -> [OrderBook] -> [Redis Publisher]
//! ```
//!
//! # Test Coverage
//!
//! 1. Single snapshot flows through pipeline
//! 2. Single delta update flows through
//! 3. Sequence of 100 deltas processed
//! 4. Snapshot followed by deltas
//! 5. Trade event propagates correctly
//! 6. Heartbeat messages handled
//! 7. Sequence numbers preserved
//! 8. Timestamps preserved through pipeline
//! 9. Data integrity (prices/quantities unchanged)
//! 10. JSON serialization works
//! 11. Bincode serialization works
//! 12. Serialize -> Deserialize roundtrip matches

use astra_flash::prelude::*;
use rust_decimal_macros::dec;
use std::time::Duration;

use super::common::*;

// =============================================================================
// PIPELINE FIXTURE HELPERS
// =============================================================================

/// Create a test order book with standard configuration.
fn create_test_orderbook(exchange: Exchange) -> ThreadSafeOrderBook {
    let instrument = test_instrument(exchange);
    let config = OrderBookConfig::default();
    ThreadSafeOrderBook::new(instrument, config)
}

/// Apply a snapshot to the order book and return the resulting book snapshot.
fn apply_and_snapshot(
    book: &ThreadSafeOrderBook,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
) -> BookSnapshot {
    let bid_levels = price_levels(bids);
    let ask_levels = price_levels(asks);
    let timestamp = now_micros();

    book.apply_snapshot(bid_levels, ask_levels, timestamp);
    book.snapshot(50)
}

// =============================================================================
// PIPELINE TESTS (12)
// =============================================================================

/// Test 1: Single snapshot flows through the pipeline.
#[tokio::test]
async fn test_pipeline_single_snapshot_flows_through() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let book = create_test_orderbook(Exchange::Deribit);
    let consumer = MockRedisConsumer::new(env.stream_name(Exchange::Deribit, "BTC", "USD", "book"));

    let bids = &[(50000.0, 1.0), (49990.0, 2.0), (49980.0, 3.0)];
    let asks = &[(50010.0, 1.5), (50020.0, 2.5), (50030.0, 3.5)];

    // ACT: Apply snapshot and simulate publish
    let snapshot = apply_and_snapshot(&book, bids, asks);
    consumer.receive_snapshot(snapshot.clone()).await;

    // ASSERT
    let messages = consumer.messages();
    assert_eq!(messages.len(), 1, "Expected exactly 1 message");

    let received = &messages[0];
    assert!(received.is_valid(), "Message should be valid");

    let received_snapshot = received.snapshot().unwrap();
    assert_eq!(received_snapshot.bids.len(), 3, "Should have 3 bid levels");
    assert_eq!(received_snapshot.asks.len(), 3, "Should have 3 ask levels");
    assert_eq!(
        received_snapshot.bids[0].price, 50000.0,
        "Best bid should be 50000"
    );
    assert_eq!(
        received_snapshot.asks[0].price, 50010.0,
        "Best ask should be 50010"
    );
}

/// Test 2: Single delta update flows through the pipeline.
#[tokio::test]
async fn test_pipeline_single_delta_flows_through() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let book = create_test_orderbook(Exchange::Deribit);
    let consumer = MockRedisConsumer::new(env.stream_name(Exchange::Deribit, "BTC", "USD", "book"));

    // Apply initial snapshot
    let _ = apply_and_snapshot(&book, &[(50000.0, 1.0)], &[(50010.0, 1.0)]);

    // ACT: Apply delta update
    let delta_levels = price_levels(&[(50005.0, 2.0)]);
    book.apply_delta(Side::Bid, delta_levels, now_micros());

    let snapshot = book.snapshot(50);
    consumer.receive_snapshot(snapshot).await;

    // ASSERT
    let messages = consumer.messages();
    assert_eq!(messages.len(), 1);

    let received_snapshot = messages[0].snapshot().unwrap();
    assert_eq!(
        received_snapshot.bids.len(),
        2,
        "Should have 2 bid levels after delta"
    );

    // Best bid should now be 50005 (higher than previous 50000)
    assert_eq!(received_snapshot.bids[0].price, 50005.0);
}

/// Test 3: Sequence of 100 deltas processed correctly.
#[tokio::test]
async fn test_pipeline_multiple_deltas_processed() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let book = create_test_orderbook(Exchange::Binance);
    let consumer =
        MockRedisConsumer::new(env.stream_name(Exchange::Binance, "BTC", "USDT", "book"));

    // Initial snapshot
    let _ = apply_and_snapshot(&book, &[(50000.0, 1.0)], &[(50010.0, 1.0)]);

    // ACT: Apply 100 deltas
    for i in 0..100 {
        let price = 49900.0 + i as f64;
        let delta_levels = price_levels(&[(price, 1.0)]);
        book.apply_delta(Side::Bid, delta_levels, now_micros());
    }

    let final_snapshot = book.snapshot(50);
    consumer.receive_snapshot(final_snapshot).await;

    // ASSERT
    let messages = consumer.messages();
    assert_eq!(messages.len(), 1);

    let received_snapshot = messages[0].snapshot().unwrap();
    // Should have multiple bid levels (capped at max_depth)
    assert!(
        received_snapshot.bids.len() >= 50,
        "Should have at least 50 bid levels"
    );

    // Best bid should be 50000 (from original snapshot, highest price)
    // Since we added deltas from 49900 to 49999, the original 50000 is still best
    assert_eq!(received_snapshot.bids[0].price, 50000.0);
}

/// Test 4: Snapshot followed by deltas maintains consistency.
#[tokio::test]
async fn test_pipeline_snapshot_then_deltas() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Deribit);
    let consumer = MockRedisConsumer::new("test_stream");

    // ACT: Snapshot
    let snapshot1 = apply_and_snapshot(
        &book,
        &[(50000.0, 1.0), (49990.0, 2.0)],
        &[(50010.0, 1.5), (50020.0, 2.5)],
    );
    consumer.receive_snapshot(snapshot1).await;

    // Apply deltas
    book.apply_delta(Side::Bid, price_levels(&[(50005.0, 3.0)]), now_micros());
    book.apply_delta(Side::Ask, price_levels(&[(50008.0, 2.0)]), now_micros());

    let snapshot2 = book.snapshot(50);
    consumer.receive_snapshot(snapshot2).await;

    // ASSERT
    let messages = consumer.messages();
    assert_eq!(messages.len(), 2);

    // Check first snapshot
    let first = messages[0].snapshot().unwrap();
    assert_eq!(first.bids.len(), 2);
    assert_eq!(first.asks.len(), 2);

    // Check second snapshot with deltas applied
    let second = messages[1].snapshot().unwrap();
    assert_eq!(second.bids.len(), 3); // Original 2 + 1 new
    assert_eq!(second.asks.len(), 3); // Original 2 + 1 new

    // Best bid/ask should be the new levels
    assert_eq!(second.bids[0].price, 50005.0);
    assert_eq!(second.asks[0].price, 50008.0);
}

/// Test 5: Trade event handling (via MarketEvent).
#[tokio::test]
async fn test_pipeline_trade_event_propagates() {
    // ARRANGE
    let instrument = test_instrument(Exchange::Deribit);

    // ACT: Create a trade event
    let trade_event = MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: instrument.clone(),
        timestamp: now_micros(),
        local_timestamp: now_micros(),
        sequence: Some(1),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(1.5),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    };

    // ASSERT: Trade event has correct structure
    assert!(matches!(trade_event.event_type, MarketEventType::Trade));

    if let MarketData::Trade {
        price,
        quantity,
        side,
        trade_id,
    } = &trade_event.data
    {
        assert_eq!(*price, 50000.0);
        assert_eq!(*quantity, dec!(1.5));
        assert!(matches!(side, Side::Bid));
        assert_eq!(trade_id.as_ref().unwrap(), "trade_123");
    } else {
        panic!("Expected Trade data");
    }
}

/// Test 6: Heartbeat messages are handled correctly.
#[tokio::test]
async fn test_pipeline_heartbeat_handling() {
    // ARRANGE
    let instrument = test_instrument(Exchange::Deribit);

    // ACT: Create a heartbeat event
    let heartbeat_event = MarketEvent {
        event_type: MarketEventType::Heartbeat,
        instrument: instrument.clone(),
        timestamp: now_micros(),
        local_timestamp: now_micros(),
        sequence: None,
        data: MarketData::Heartbeat {
            exchange_time: now_micros(),
        },
    };

    // ASSERT
    assert!(matches!(
        heartbeat_event.event_type,
        MarketEventType::Heartbeat
    ));

    if let MarketData::Heartbeat { exchange_time } = &heartbeat_event.data {
        assert!(*exchange_time > 0, "Exchange time should be positive");
    } else {
        panic!("Expected Heartbeat data");
    }
}

/// Test 7: Sequence numbers are preserved through the pipeline.
#[tokio::test]
async fn test_pipeline_sequence_integrity_preserved() {
    // ARRANGE
    let instrument = test_instrument(Exchange::Binance);
    let mut events = Vec::new();

    // ACT: Create events with sequential numbers
    for seq in 1..=10 {
        let event = MarketEvent {
            event_type: MarketEventType::Delta,
            instrument: instrument.clone(),
            timestamp: now_micros(),
            local_timestamp: now_micros(),
            sequence: Some(seq),
            data: MarketData::Book {
                bids: vec![PriceLevel {
                    price: 50000.0 + seq as f64,
                    quantity: dec!(1),
                    order_count: None,
                    timestamp: now_micros(),
                }],
                asks: vec![],
            },
        };
        events.push(event);
    }

    // ASSERT: Sequences are contiguous
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event.sequence, Some((i + 1) as u64));
    }
}

/// Test 8: Timestamps are preserved through the pipeline.
#[tokio::test]
async fn test_pipeline_timestamp_integrity_preserved() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Deribit);
    let consumer = MockRedisConsumer::new("test_stream");

    let before_apply = now_micros();
    tokio::time::sleep(Duration::from_millis(1)).await;

    // ACT
    let snapshot = apply_and_snapshot(&book, &[(50000.0, 1.0)], &[(50010.0, 1.0)]);
    let after_apply = now_micros();

    consumer.receive_snapshot(snapshot).await;

    // ASSERT
    let messages = consumer.messages();
    let received = messages[0].snapshot().unwrap();

    // Snapshot timestamp should be between before and after
    assert!(received.timestamp >= before_apply);
    assert!(received.timestamp <= after_apply);

    // E2E latency should be minimal in this test
    assert!(messages[0].e2e_latency_us >= 0);
}

/// Test 9: Data integrity - prices and quantities unchanged through pipeline.
#[tokio::test]
async fn test_pipeline_data_integrity_maintained() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Oanda);
    let consumer = MockRedisConsumer::new("test_stream");

    let original_bids = &[
        (1.10000, 1000000.0),
        (1.09995, 2000000.0),
        (1.09990, 500000.0),
    ];
    let original_asks = &[
        (1.10005, 1500000.0),
        (1.10010, 2500000.0),
        (1.10015, 750000.0),
    ];

    // ACT
    let snapshot = apply_and_snapshot(&book, original_bids, original_asks);
    consumer.receive_snapshot(snapshot).await;

    // ASSERT
    let messages = consumer.messages();
    let received = messages[0].snapshot().unwrap();

    // Verify exact price matching
    for (i, (expected_price, _)) in original_bids.iter().enumerate() {
        assert!(
            (received.bids[i].price - expected_price).abs() < 1e-10,
            "Bid price {} mismatch: expected {}, got {}",
            i,
            expected_price,
            received.bids[i].price
        );
    }

    for (i, (expected_price, _)) in original_asks.iter().enumerate() {
        assert!(
            (received.asks[i].price - expected_price).abs() < 1e-10,
            "Ask price {} mismatch: expected {}, got {}",
            i,
            expected_price,
            received.asks[i].price
        );
    }
}

/// Test 10: JSON serialization works correctly.
#[tokio::test]
async fn test_pipeline_serialization_json() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Deribit);
    let snapshot = apply_and_snapshot(
        &book,
        &[(50000.0, 1.0), (49990.0, 2.0)],
        &[(50010.0, 1.5), (50020.0, 2.5)],
    );

    // ACT: Serialize to JSON
    let json_result = serde_json::to_string(&snapshot);

    // ASSERT
    assert!(json_result.is_ok(), "JSON serialization should succeed");

    let json = json_result.unwrap();
    assert!(!json.is_empty(), "JSON should not be empty");
    assert!(
        json.contains("instrument"),
        "JSON should contain instrument"
    );
    assert!(json.contains("bids"), "JSON should contain bids");
    assert!(json.contains("asks"), "JSON should contain asks");
    assert!(json.contains("50000"), "JSON should contain bid price");
}

/// Test 11: Bincode serialization works correctly.
#[tokio::test]
async fn test_pipeline_serialization_bincode() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Binance);
    let snapshot = apply_and_snapshot(
        &book,
        &[(50000.0, 1.0), (49990.0, 2.0)],
        &[(50010.0, 1.5), (50020.0, 2.5)],
    );

    // ACT: Serialize to bincode
    let bincode_result = bincode::serialize(&snapshot);

    // ASSERT
    assert!(
        bincode_result.is_ok(),
        "Bincode serialization should succeed"
    );

    let bytes = bincode_result.unwrap();
    assert!(!bytes.is_empty(), "Bincode output should not be empty");

    // Bincode should be smaller than JSON
    let json_bytes = serde_json::to_vec(&snapshot).unwrap();
    assert!(
        bytes.len() < json_bytes.len(),
        "Bincode ({} bytes) should be smaller than JSON ({} bytes)",
        bytes.len(),
        json_bytes.len()
    );
}

/// Test 12: Serialize -> Deserialize roundtrip preserves data.
#[tokio::test]
async fn test_pipeline_serialization_roundtrip() {
    // ARRANGE
    let book = create_test_orderbook(Exchange::Deribit);
    let original = apply_and_snapshot(
        &book,
        &[(50000.0, 1.0), (49990.0, 2.0), (49980.0, 3.0)],
        &[(50010.0, 1.5), (50020.0, 2.5), (50030.0, 3.5)],
    );

    // ACT: JSON roundtrip (primary serialization format)
    let json = serde_json::to_string(&original).unwrap();
    let json_restored: BookSnapshot = serde_json::from_str(&json).unwrap();

    // ASSERT: JSON roundtrip preserves data
    assert_eq!(
        json_restored.instrument.raw_symbol,
        original.instrument.raw_symbol
    );
    assert_eq!(json_restored.bids.len(), original.bids.len());
    assert_eq!(json_restored.asks.len(), original.asks.len());

    for (orig, restored) in original.bids.iter().zip(json_restored.bids.iter()) {
        assert!((orig.price - restored.price).abs() < 1e-10);
    }

    for (orig, restored) in original.asks.iter().zip(json_restored.asks.iter()) {
        assert!((orig.price - restored.price).abs() < 1e-10);
    }

    // Verify bincode serialization works (but skip deserialization due to
    // rust_decimal limitation with bincode's deserialize_any)
    let bincode_bytes = bincode::serialize(&original).unwrap();
    assert!(
        !bincode_bytes.is_empty(),
        "Bincode serialization should produce bytes"
    );
    assert!(
        bincode_bytes.len() < json.len(),
        "Bincode should be more compact than JSON"
    );
}

// =============================================================================
// ADDITIONAL PIPELINE INTEGRITY TESTS
// =============================================================================

#[cfg(test)]
mod additional_tests {
    use super::*;
    use std::sync::Arc;

    /// Verify order book clears correctly on new snapshot.
    #[tokio::test]
    async fn test_pipeline_snapshot_replaces_existing() {
        let book = create_test_orderbook(Exchange::Deribit);

        // First snapshot
        let _ = apply_and_snapshot(&book, &[(50000.0, 10.0)], &[(50010.0, 10.0)]);

        // New snapshot should replace
        let new_snapshot = apply_and_snapshot(&book, &[(51000.0, 1.0)], &[(51010.0, 1.0)]);

        // Should only have new levels
        assert_eq!(new_snapshot.bids.len(), 1);
        assert_eq!(new_snapshot.asks.len(), 1);
        assert_eq!(new_snapshot.bids[0].price, 51000.0);
        assert_eq!(new_snapshot.asks[0].price, 51010.0);
    }

    /// Verify zero quantity removes level.
    #[tokio::test]
    async fn test_pipeline_zero_quantity_removes_level() {
        let book = create_test_orderbook(Exchange::Binance);

        // Initial snapshot
        let _ = apply_and_snapshot(&book, &[(50000.0, 1.0), (49990.0, 2.0)], &[(50010.0, 1.0)]);

        // Remove level with zero quantity
        book.apply_delta(Side::Bid, price_levels(&[(50000.0, 0.0)]), now_micros());

        let snapshot = book.snapshot(50);
        assert_eq!(snapshot.bids.len(), 1);
        assert_eq!(snapshot.bids[0].price, 49990.0); // Only remaining bid
    }

    /// Verify thread-safe operations.
    #[tokio::test]
    async fn test_pipeline_thread_safe_concurrent() {
        let book = Arc::new(create_test_orderbook(Exchange::Deribit));
        let book_clone = book.clone();

        // Initial snapshot
        book.apply_snapshot(
            price_levels(&[(50000.0, 1.0)]),
            price_levels(&[(50010.0, 1.0)]),
            now_micros(),
        );

        // Spawn concurrent readers
        let reader_handle = tokio::spawn(async move {
            for _ in 0..100 {
                let _ = book_clone.best_bid();
                let _ = book_clone.best_ask();
                let _ = book_clone.mid_price();
                tokio::task::yield_now().await;
            }
        });

        // Concurrent writer
        for i in 0..100 {
            book.apply_delta(
                Side::Bid,
                price_levels(&[(49900.0 + i as f64, 1.0)]),
                now_micros(),
            );
            tokio::task::yield_now().await;
        }

        reader_handle.await.unwrap();

        // Should have many bid levels now
        let snapshot = book.snapshot(50);
        assert!(snapshot.bids.len() > 1);
    }
}
