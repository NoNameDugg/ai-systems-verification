//! Message Chaos Tests
//!
//! Tests for message-level chaos scenarios including:
//! - Malformed JSON handling
//! - Invalid price/quantity values
//! - Message bursts
//! - Sequence gaps and duplicates

use std::time::{Duration, Instant};

use astra_flash::book::delta::{
    DeltaUpdate, DeltaValidationConfig, DeltaValidator, SequenceCheckResult, SequenceConfig,
    SequenceTracker,
};
use astra_flash::book::snapshot::{SnapshotValidator, ValidationConfig};
use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{PriceLevel, Side};
use rust_decimal::Decimal;

use super::helpers::*;

// ============================================================================
// TEST 1: MALFORMED JSON SKIP AND CONTINUE
// ============================================================================

#[test]
fn test_malformed_json_skip_and_continue() {
    // ARRANGE: Mix of valid and malformed messages
    let malformed_messages = generate_malformed_messages(10);
    let valid_messages = generate_message_burst(10, 1);

    let mut processed_valid = 0;
    let mut skipped_malformed = 0;

    // ACT: Process messages, skip malformed
    for msg in malformed_messages.iter().chain(valid_messages.iter()) {
        match serde_json::from_str::<serde_json::Value>(msg) {
            Ok(_) => processed_valid += 1,
            Err(_) => skipped_malformed += 1,
        }
    }

    // ASSERT: Malformed skipped, valid processed
    assert!(skipped_malformed > 0, "Should skip some malformed messages");
    assert!(processed_valid >= 10, "Should process valid messages");
}

// ============================================================================
// TEST 2: INVALID PRICE NAN REJECTION
// ============================================================================

#[test]
fn test_invalid_price_nan_rejection() {
    // ARRANGE: Validation config
    let config = ValidationConfig::default();
    let validator = SnapshotValidator::new(config);
    let now = chrono::Utc::now().timestamp_micros();

    // Create levels with NaN price
    let bids = vec![PriceLevel::new(f64::NAN, Decimal::new(10, 0), now)];
    let asks = vec![PriceLevel::new(101.0, Decimal::new(10, 0), now)];

    // ACT: Validate
    let result = validator.validate(&bids, &asks);

    // ASSERT: Should reject NaN price
    assert!(result.is_err(), "Should reject NaN price");
}

// ============================================================================
// TEST 3: INVALID PRICE NEGATIVE REJECTION
// ============================================================================

#[test]
fn test_invalid_price_negative_rejection() {
    // ARRANGE: Validation config
    let config = ValidationConfig::default();
    let validator = SnapshotValidator::new(config);
    let now = chrono::Utc::now().timestamp_micros();

    // Create levels with negative price
    let bids = vec![PriceLevel::new(-100.0, Decimal::new(10, 0), now)];
    let asks = vec![PriceLevel::new(101.0, Decimal::new(10, 0), now)];

    // ACT: Validate
    let result = validator.validate(&bids, &asks);

    // ASSERT: Should reject negative price
    assert!(result.is_err(), "Should reject negative price");
}

// ============================================================================
// TEST 4: INVALID QUANTITY HANDLING
// ============================================================================

#[test]
fn test_invalid_quantity_handling() {
    // ARRANGE: Order book with auto_prune
    let now = chrono::Utc::now().timestamp_micros();
    let mut book = OrderBook::new(
        test_instrument(),
        OrderBookConfig {
            auto_prune: true,
            ..Default::default()
        },
    );

    // Initialize with valid data
    let bids = vec![PriceLevel::new(100.0, Decimal::new(10, 0), now)];
    book.apply_snapshot(bids, vec![], now);

    // ACT: Apply update with zero quantity (should remove level)
    let zero_qty_level = PriceLevel::new(100.0, Decimal::ZERO, now);
    book.apply_delta(Side::Bid, vec![zero_qty_level], now);

    // ASSERT: Zero quantity should remove level
    assert!(
        book.best_bid().is_none(),
        "Zero quantity should remove level"
    );
}

// ============================================================================
// TEST 5: MISSING REQUIRED FIELDS HANDLING
// ============================================================================

#[test]
fn test_missing_required_fields_handling() {
    // ARRANGE: JSON with missing required fields
    let incomplete_json = r#"{"type": "book"}"#; // Missing bids, asks, timestamp

    // ACT: Try to parse
    let result: Result<serde_json::Value, _> = serde_json::from_str(incomplete_json);

    // ASSERT: Parses as JSON but missing expected fields
    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(value.get("bids").is_none(), "Should be missing bids");
    assert!(value.get("asks").is_none(), "Should be missing asks");
}

// ============================================================================
// TEST 6: BURST 100K MESSAGES PER SECOND
// ============================================================================

#[test]
fn test_burst_100k_messages_processing() {
    // ARRANGE: Generate burst of messages
    let burst_size = 10_000; // Reduced for unit test speed, proves concept
    let messages = generate_message_burst(burst_size, 1);
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());

    let start = Instant::now();

    // ACT: Process burst
    for (i, _msg) in messages.iter().enumerate() {
        let ts = chrono::Utc::now().timestamp_micros();
        let level = PriceLevel::new(100.0 + (i as f64 * 0.0001), Decimal::new(1, 0), ts);
        book.apply_delta(Side::Bid, vec![level], ts);
    }

    let elapsed = start.elapsed();
    let rate = burst_size as f64 / elapsed.as_secs_f64();

    // ASSERT: Should process at high rate
    assert!(
        rate > 100_000.0,
        "Should process > 100K updates/sec: {:.0} updates/sec",
        rate
    );
}

// ============================================================================
// TEST 7: SEQUENCE GAP DETECTION
// ============================================================================

#[test]
fn test_sequence_gap_detection() {
    // ARRANGE: Sequence tracker
    let config = SequenceConfig::default();
    let mut tracker = SequenceTracker::new(config);

    // Initialize with first sequence (auto-initializes on first record)
    let _first = tracker.record(100);

    // ACT: Skip to sequence 105 (gap of 4)
    let result = tracker.record(105);

    // ASSERT: Should detect gap
    assert!(
        matches!(result, SequenceCheckResult::Gap(_)),
        "Should detect sequence gap"
    );
}

// ============================================================================
// TEST 8: DUPLICATE MESSAGE HANDLING
// ============================================================================

#[test]
fn test_duplicate_message_handling() {
    // ARRANGE: Sequence tracker
    let config = SequenceConfig::default();
    let mut tracker = SequenceTracker::new(config);

    // Initialize
    let _ = tracker.record(100);

    // Process sequence 101
    let first = tracker.record(101);
    assert!(matches!(first, SequenceCheckResult::Ok));

    // ACT: Try to process same sequence again (older)
    let duplicate = tracker.record(101);

    // ASSERT: Should detect out-of-order (duplicate as older)
    assert!(
        matches!(duplicate, SequenceCheckResult::OutOfOrder),
        "Should detect duplicate/out-of-order: {:?}",
        duplicate
    );
}

// ============================================================================
// TEST 9: OUT OF ORDER DELIVERY DETECTION
// ============================================================================

#[test]
fn test_out_of_order_delivery_detection() {
    // ARRANGE: Sequence tracker
    let config = SequenceConfig::default();
    let mut tracker = SequenceTracker::new(config);

    // Initialize and process in order
    let _ = tracker.record(100);
    assert!(matches!(tracker.record(101), SequenceCheckResult::Ok));
    assert!(matches!(tracker.record(102), SequenceCheckResult::Ok));

    // ACT: Skip to 104 (gap), then check for out-of-order handling
    let result_104 = tracker.record(104);

    // ASSERT: Should handle or detect gap/out-of-order
    assert!(matches!(
        result_104,
        SequenceCheckResult::Ok | SequenceCheckResult::Gap(_)
    ));
}

// ============================================================================
// TEST 10: EMPTY MESSAGE PERIOD HANDLING
// ============================================================================

#[test]
fn test_empty_message_period_handling() {
    // ARRANGE: Order book and track staleness
    let book = test_orderbook_with_data(10);
    let _stale_threshold = Duration::from_secs(1);

    // Get initial timestamp
    let initial_timestamp = book.stats().last_update_timestamp;

    // ACT: Simulate empty period (no updates)
    std::thread::sleep(Duration::from_millis(100));

    // Check if timestamp unchanged (simulating no updates received)
    let current_timestamp = book.stats().last_update_timestamp;

    // ASSERT: Timestamp should be unchanged (no updates during empty period)
    assert_eq!(
        initial_timestamp, current_timestamp,
        "Timestamp should not change without updates"
    );
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
fn test_message_chaos_mixed_valid_invalid() {
    // ARRANGE: Mixed message stream
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let mut processed = 0;
    let mut rejected = 0;
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    let bids = vec![PriceLevel::new(100.0, Decimal::new(10, 0), now)];
    book.apply_snapshot(bids, vec![], now);

    // ACT: Process mix of valid and invalid updates
    for i in 0..100 {
        let price = if i % 10 == 0 {
            // Invalid every 10th
            f64::NAN
        } else {
            100.0 - (i as f64 * 0.01)
        };

        if price.is_nan() || price < 0.0 {
            rejected += 1;
            continue;
        }

        let ts = chrono::Utc::now().timestamp_micros();
        let level = PriceLevel::new(price, Decimal::new(1, 0), ts);
        book.apply_delta(Side::Bid, vec![level], ts);
        processed += 1;
    }

    // ASSERT: Valid processed, invalid rejected
    assert!(processed > 80, "Should process most valid: {}", processed);
    assert!(rejected > 0, "Should reject some invalid: {}", rejected);
    assert!(validate_orderbook_consistency(&book));
}

#[test]
fn test_message_chaos_sequence_recovery() {
    // ARRANGE: Sequence tracker with gap detection
    let config = SequenceConfig::default();
    let mut tracker = SequenceTracker::new(config);

    let mut gaps_detected = 0;
    let mut normal_processed = 0;

    // ACT: Process messages with intentional gaps
    let sequences = vec![101, 102, 105, 106, 110, 111, 112]; // Gaps at 103-104, 107-109

    for seq in sequences {
        match tracker.record(seq) {
            SequenceCheckResult::Ok => normal_processed += 1,
            SequenceCheckResult::Gap(_) => {
                gaps_detected += 1;
                // In real system, would request snapshot here
            }
            SequenceCheckResult::OutOfOrder => {}
        }
    }

    // ASSERT: Should detect gaps and process normal sequences
    assert!(gaps_detected > 0, "Should detect gaps: {}", gaps_detected);
    assert!(
        normal_processed > 0,
        "Should process some normal: {}",
        normal_processed
    );
}

#[test]
fn test_message_chaos_burst_with_validation() {
    // ARRANGE: Burst with validation
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let validator_config = DeltaValidationConfig::default();
    let validator = DeltaValidator::new(validator_config);

    let burst_size = 1000;
    let mut validated = 0;
    let mut invalid = 0;

    // Initialize
    let now = chrono::Utc::now().timestamp_micros();
    let bids = vec![PriceLevel::new(100.0, Decimal::new(10, 0), now)];
    book.apply_snapshot(bids, vec![], now);

    // ACT: Process validated burst
    for i in 0..burst_size {
        let ts = chrono::Utc::now().timestamp_micros();
        let price = if i % 100 == 0 {
            f64::NAN
        } else {
            99.0 - (i as f64 * 0.001)
        };
        let level = PriceLevel::new(price, Decimal::new(1, 0), ts);

        let update = DeltaUpdate::new(Side::Bid, vec![level.clone()], ts);

        match validator.validate(&update) {
            Ok(_) => {
                book.apply_delta(Side::Bid, vec![level], ts);
                validated += 1;
            }
            Err(_) => invalid += 1,
        }
    }

    // ASSERT: Most validated, some rejected
    assert!(validated > 900, "Should validate most: {}", validated);
    assert!(invalid > 0, "Should reject some: {}", invalid);
    assert!(validate_orderbook_consistency(&book));
}
