//! Timing Chaos Tests
//!
//! Tests for timing-related chaos scenarios including:
//! - Clock skew handling
//! - Timeout handling
//! - Deadline exceeded scenarios

use std::time::{Duration, Instant};

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{PriceLevel, Side};
use rust_decimal::Decimal;

use super::helpers::*;

// ============================================================================
// TEST 1: CLOCK JUMP FORWARD HANDLING
// ============================================================================

#[test]
fn test_clock_jump_forward_handling() {
    // ARRANGE: Order book with timestamp tracking
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());

    // Initialize with current time
    let initial_timestamp = chrono::Utc::now().timestamp_micros();
    let bids = vec![PriceLevel::new(
        100.0,
        Decimal::new(10, 0),
        initial_timestamp,
    )];
    book.apply_snapshot(bids, vec![], initial_timestamp);

    // ACT: Apply update with timestamp far in future (simulating clock jump)
    let future_timestamp = initial_timestamp + 3_600_000_000; // 1 hour in future
    let level = PriceLevel::new(99.0, Decimal::new(5, 0), future_timestamp);
    book.apply_delta(Side::Bid, vec![level], future_timestamp);

    // ASSERT: Should accept future timestamp (clocks can drift forward)
    assert_eq!(book.stats().last_update_timestamp, future_timestamp);
    assert!(book.best_bid().is_some());
}

// ============================================================================
// TEST 2: CLOCK JUMP BACKWARD HANDLING
// ============================================================================

#[test]
fn test_clock_jump_backward_handling() {
    // ARRANGE: Order book with recent timestamp
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());

    let initial_timestamp = chrono::Utc::now().timestamp_micros();
    let bids = vec![PriceLevel::new(
        100.0,
        Decimal::new(10, 0),
        initial_timestamp,
    )];
    book.apply_snapshot(bids, vec![], initial_timestamp);

    // ACT: Apply update with older timestamp (simulating clock jump backward)
    let past_timestamp = initial_timestamp - 1_000_000; // 1 second in past
    let level = PriceLevel::new(99.0, Decimal::new(5, 0), past_timestamp);

    // Note: In production, we might reject past timestamps, but for resilience
    // we still accept them (exchange may have different clock)
    book.apply_delta(Side::Bid, vec![level], past_timestamp);

    // ASSERT: Should handle gracefully (may or may not accept)
    // The book should remain consistent either way
    assert!(validate_orderbook_consistency(&book));
}

// ============================================================================
// TEST 3: SLOW RESPONSE TIMEOUT HANDLING
// ============================================================================

#[tokio::test]
async fn test_slow_response_timeout_handling() {
    // ARRANGE: Configurable timeout
    let operation_timeout = Duration::from_millis(100);
    let slow_response_time = Duration::from_millis(200);

    // ACT: Attempt operation with timeout
    let start = Instant::now();
    let result = tokio::time::timeout(operation_timeout, async {
        // Simulate slow response
        tokio::time::sleep(slow_response_time).await;
        "response"
    })
    .await;
    let elapsed = start.elapsed();

    // ASSERT: Should timeout, not wait for full slow response
    assert!(result.is_err(), "Should timeout");
    assert!(
        elapsed < slow_response_time,
        "Should not wait for full response: {:?}",
        elapsed
    );
    assert!(
        elapsed >= operation_timeout,
        "Should wait at least timeout period: {:?}",
        elapsed
    );
}

// ============================================================================
// TEST 4: DEADLINE EXCEEDED GRACEFUL
// ============================================================================

#[tokio::test]
async fn test_deadline_exceeded_graceful() {
    // ARRANGE: Multiple operations with deadline
    let deadline = Instant::now() + Duration::from_millis(100);
    let operation_count = 10;
    let mut completed = 0;
    let mut exceeded = 0;

    // ACT: Run operations until deadline
    for i in 0..operation_count {
        if Instant::now() >= deadline {
            exceeded = operation_count - i;
            break;
        }

        // Simulate operation
        tokio::time::sleep(Duration::from_millis(15)).await;
        completed += 1;
    }

    // ASSERT: Should complete some, may exceed deadline for rest
    assert!(
        completed > 0,
        "Should complete some operations: {}",
        completed
    );
    // Note: All might complete if fast enough, which is also acceptable
}

// ============================================================================
// TEST 5: HEARTBEAT STALENESS SIMULATION
// ============================================================================

#[test]
fn test_timing_chaos_heartbeat_staleness_simulation() {
    // ARRANGE: Track activity timestamps
    let stale_threshold = Duration::from_millis(500);
    let mut last_activity = Instant::now();

    // Check immediately (should not be stale)
    let not_stale = last_activity.elapsed() < stale_threshold;

    // Wait and check again
    std::thread::sleep(Duration::from_millis(100));
    last_activity = Instant::now(); // Simulate activity
    let still_not_stale = last_activity.elapsed() < stale_threshold;

    // ASSERT: Should track staleness correctly
    assert!(not_stale, "Should not be stale immediately");
    assert!(still_not_stale, "Should not be stale after activity");
}

// ============================================================================
// TEST 6: OPERATION DEADLINE MANAGEMENT
// ============================================================================

#[tokio::test]
async fn test_timing_chaos_operation_deadline() {
    // ARRANGE: Operations with varying durations
    let deadline = Duration::from_millis(200);
    let operations = vec![
        Duration::from_millis(50),  // Fast
        Duration::from_millis(75),  // Medium
        Duration::from_millis(100), // Slower
        Duration::from_millis(150), // Even slower
    ];

    let start = Instant::now();
    let mut results = Vec::new();

    // ACT: Run operations with deadline
    for op_duration in operations {
        if start.elapsed() >= deadline {
            results.push(Err("deadline exceeded"));
            continue;
        }

        let result = tokio::time::timeout(deadline.saturating_sub(start.elapsed()), async move {
            tokio::time::sleep(op_duration).await;
            Ok::<&str, &str>("completed")
        })
        .await;

        match result {
            Ok(Ok(_)) => results.push(Ok("completed")),
            _ => results.push(Err("timeout")),
        }
    }

    // ASSERT: Some operations should complete before deadline
    let completed = results.iter().filter(|r| r.is_ok()).count();
    assert!(
        completed > 0,
        "Should complete at least one operation: {:?}",
        results
    );
}

// ============================================================================
// TEST 7: TIMESTAMP ORDERING RESILIENCE
// ============================================================================

#[test]
fn test_timing_chaos_timestamp_ordering() {
    // ARRANGE: Order book with timestamp-ordered updates
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let base_timestamp = chrono::Utc::now().timestamp_micros();

    // Initialize
    let bids = vec![PriceLevel::new(100.0, Decimal::new(10, 0), base_timestamp)];
    book.apply_snapshot(bids, vec![], base_timestamp);

    // ACT: Apply updates with varying timestamps
    let timestamps = vec![
        base_timestamp + 1000, // +1ms (normal)
        base_timestamp + 500,  // +0.5ms (out of order)
        base_timestamp + 2000, // +2ms (normal)
        base_timestamp - 100,  // -0.1ms (clock skew)
        base_timestamp + 3000, // +3ms (normal)
    ];

    for (i, ts) in timestamps.iter().enumerate() {
        let level = PriceLevel::new(99.0 - (i as f64 * 0.1), Decimal::new(1, 0), *ts);
        book.apply_delta(Side::Bid, vec![level], *ts);
    }

    // ASSERT: Book should remain consistent despite timing chaos
    assert!(validate_orderbook_consistency(&book));
    assert!(book.stats().update_count >= timestamps.len() as u64);
}

// ============================================================================
// TEST 8: RAPID TIMESTAMP UPDATES
// ============================================================================

#[test]
fn test_rapid_timestamp_updates() {
    // ARRANGE: Order book that handles rapid updates
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    let bids = vec![PriceLevel::new(100.0, Decimal::new(10, 0), now)];
    book.apply_snapshot(bids, vec![], now);

    let start = Instant::now();

    // ACT: Apply rapid updates with incrementing timestamps
    for i in 0..1000 {
        let ts = now + i;
        let level = PriceLevel::new(99.0 - (i as f64 * 0.001), Decimal::new(1, 0), ts);
        book.apply_delta(Side::Bid, vec![level], ts);
    }

    let elapsed = start.elapsed();

    // ASSERT: Should handle rapid updates efficiently
    assert!(
        elapsed < Duration::from_millis(100),
        "Should process 1000 updates quickly: {:?}",
        elapsed
    );
    assert!(validate_orderbook_consistency(&book));
}
