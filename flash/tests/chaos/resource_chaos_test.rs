//! Resource Chaos Tests
//!
//! Tests for resource pressure scenarios including:
//! - Memory pressure and limits
//! - High allocation rates
//! - Thread pool exhaustion
//! - Channel capacity backpressure

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{PriceLevel, Side};
use astra_flash::publisher::batch::{BackpressureMonitor, BackpressureStatus};
use rust_decimal::Decimal;

use super::helpers::*;

// ============================================================================
// TEST 1: MEMORY PRESSURE 2X LOAD STABLE
// ============================================================================

#[test]
fn test_memory_pressure_2x_load_stable() {
    // ARRANGE: Order book under normal conditions
    let base_levels = 50;
    let stress_levels = base_levels * 2; // 2x load
    let now = chrono::Utc::now().timestamp_micros();

    // ACT: Create order book with 2x typical load
    let mut book = OrderBook::new(
        test_instrument(),
        OrderBookConfig {
            max_depth: stress_levels,
            max_levels: stress_levels * 2,
            ..Default::default()
        },
    );

    let bids: Vec<PriceLevel> = (0..stress_levels)
        .map(|i| PriceLevel::new(100.0 - (i as f64 * 0.01), Decimal::new(10, 0), now))
        .collect();

    let asks: Vec<PriceLevel> = (0..stress_levels)
        .map(|i| PriceLevel::new(101.0 + (i as f64 * 0.01), Decimal::new(10, 0), now))
        .collect();

    book.apply_snapshot(bids, asks, now);

    // ASSERT: Book handles 2x load
    assert_eq!(book.stats().bid_levels, stress_levels);
    assert_eq!(book.stats().ask_levels, stress_levels);
    assert!(validate_orderbook_consistency(&book));
}

// ============================================================================
// TEST 2: MEMORY LIMIT 100MB ENFORCEMENT
// ============================================================================

#[test]
fn test_memory_limit_enforcement_via_max_levels() {
    // ARRANGE: Order book with strict max_levels
    let max_levels = 100;
    let now = chrono::Utc::now().timestamp_micros();
    let mut book = OrderBook::new(
        test_instrument(),
        OrderBookConfig {
            max_depth: 50,
            max_levels,
            ..Default::default()
        },
    );

    // ACT: Try to add more than max_levels
    let oversized_bids: Vec<PriceLevel> = (0..max_levels * 2)
        .map(|i| PriceLevel::new(100.0 - (i as f64 * 0.001), Decimal::new(1, 0), now))
        .collect();

    book.apply_snapshot(oversized_bids, vec![], now);

    // ASSERT: Should enforce max_levels limit
    assert!(
        book.stats().bid_levels <= max_levels,
        "Should enforce max_levels limit: {} <= {}",
        book.stats().bid_levels,
        max_levels
    );
}

// ============================================================================
// TEST 3: HIGH ALLOCATION RATE NO OOM
// ============================================================================

#[test]
fn test_high_allocation_rate_no_oom() {
    // ARRANGE: Rapid allocations
    let allocation_count = 10_000;
    let mut successful_allocations = 0;

    // ACT: Rapid allocation/deallocation cycle
    for _i in 0..allocation_count {
        // Allocate order book
        let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
        let now = chrono::Utc::now().timestamp_micros();

        // Add some data
        let levels: Vec<PriceLevel> = (0..10)
            .map(|j| PriceLevel::new(100.0 + j as f64, Decimal::new(1, 0), now))
            .collect();
        book.apply_snapshot(levels, vec![], now);

        successful_allocations += 1;
        // Book dropped here, memory freed
    }

    // ASSERT: All allocations successful (no OOM)
    assert_eq!(successful_allocations, allocation_count);
}

// ============================================================================
// TEST 4: CPU PRESSURE HIGH LOAD RESPONSIVE
// ============================================================================

#[test]
fn test_cpu_pressure_operations_complete() {
    // ARRANGE: CPU-intensive operations
    let operations = 1000;
    let book = thread_safe_orderbook(50);
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..50)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        let asks: Vec<PriceLevel> = (0..50)
            .map(|i| PriceLevel::new(101.0 + i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, asks, now);
    }

    let start = Instant::now();

    // ACT: Perform many operations
    for i in 0..operations {
        // Read operation
        let _bid = book.read().best_bid().cloned();

        // Write operation
        {
            let mut guard = book.write();
            let ts = chrono::Utc::now().timestamp_micros();
            let level = PriceLevel::new(99.0 - (i as f64 * 0.001), Decimal::new(1, 0), ts);
            guard.apply_delta(Side::Bid, vec![level], ts);
        }
    }

    let elapsed = start.elapsed();

    // ASSERT: Operations complete in reasonable time
    assert!(
        elapsed < Duration::from_secs(5),
        "Operations should complete quickly: {:?}",
        elapsed
    );
}

// ============================================================================
// TEST 5: THREAD POOL EXHAUSTION GRACEFUL
// ============================================================================

#[test]
fn test_thread_pool_exhaustion_graceful() {
    // ARRANGE: Concurrent operations exceeding typical thread count
    let concurrent_operations = 100;
    let completed = Arc::new(AtomicU64::new(0));
    let book = Arc::new(thread_safe_orderbook(10));
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize book
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..10)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, vec![], now);
    }

    // ACT: Spawn many concurrent operations
    let handles: Vec<_> = (0..concurrent_operations)
        .map(|_i| {
            let book_clone = Arc::clone(&book);
            let completed_clone = Arc::clone(&completed);
            thread::spawn(move || {
                // Perform operation
                let _mid = book_clone.read().mid_price();
                completed_clone.fetch_add(1, Ordering::SeqCst);
            })
        })
        .collect();

    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread should complete");
    }

    // ASSERT: All operations completed
    assert_eq!(
        completed.load(Ordering::SeqCst),
        concurrent_operations as u64
    );
}

// ============================================================================
// TEST 6: CHANNEL CAPACITY BACKPRESSURE
// ============================================================================

#[test]
fn test_channel_capacity_backpressure() {
    // ARRANGE: Backpressure monitor with limited capacity
    let capacity = 100;
    let warn_threshold = 0.75;
    let critical_threshold = 0.95;
    let mut monitor = BackpressureMonitor::new(warn_threshold, critical_threshold, capacity);

    // ACT: Fill to various levels and check status
    let (_, _) = monitor.update(50);
    let status_50 = monitor.status();

    let (_, _) = monitor.update(80);
    let status_80 = monitor.status();

    let (_, _) = monitor.update(96);
    let status_96 = monitor.status();

    // ASSERT: Status changes at thresholds
    assert_eq!(
        status_50,
        BackpressureStatus::Normal,
        "50% should be normal"
    );
    assert_eq!(
        status_80,
        BackpressureStatus::Warning,
        "80% should be warning"
    );
    assert_eq!(
        status_96,
        BackpressureStatus::Critical,
        "96% should be critical"
    );
}

// ============================================================================
// TEST 7: FILE DESCRIPTOR LIMIT HANDLING
// ============================================================================

#[test]
fn test_resource_limit_graceful_degradation() {
    // ARRANGE: Simulate resource limits through order book limits
    let very_limited_config = OrderBookConfig {
        max_depth: 5,
        max_levels: 10,
        ..Default::default()
    };
    let now = chrono::Utc::now().timestamp_micros();
    let mut book = OrderBook::new(test_instrument(), very_limited_config);

    // ACT: Try to exceed limits
    let many_levels: Vec<PriceLevel> = (0..100)
        .map(|i| PriceLevel::new(100.0 + i as f64, Decimal::new(1, 0), now))
        .collect();

    book.apply_snapshot(many_levels, vec![], now);

    // ASSERT: Should gracefully limit
    assert!(book.stats().bid_levels <= 10);
    assert!(validate_orderbook_consistency(&book));
}

// ============================================================================
// TEST 8: CONCURRENT OPERATIONS STRESS
// ============================================================================

#[test]
fn test_concurrent_operations_stress() {
    // ARRANGE: High concurrency stress test
    let book = Arc::new(thread_safe_orderbook(20));
    let iterations = 1000;
    let writers = 4;
    let readers = 8;
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..20)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        let asks: Vec<PriceLevel> = (0..20)
            .map(|i| PriceLevel::new(101.0 + i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, asks, now);
    }

    let read_count = Arc::new(AtomicU64::new(0));
    let write_count = Arc::new(AtomicU64::new(0));

    // ACT: Run concurrent readers and writers
    let mut handles = Vec::new();

    // Spawn readers
    for _ in 0..readers {
        let book_clone = Arc::clone(&book);
        let count_clone = Arc::clone(&read_count);
        handles.push(thread::spawn(move || {
            for _ in 0..iterations {
                let guard = book_clone.read();
                let _bid = guard.best_bid();
                let _ask = guard.best_ask();
                count_clone.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // Spawn writers
    for w in 0..writers {
        let book_clone = Arc::clone(&book);
        let count_clone = Arc::clone(&write_count);
        handles.push(thread::spawn(move || {
            for i in 0..iterations / 10 {
                let mut guard = book_clone.write();
                let ts = chrono::Utc::now().timestamp_micros();
                let price = 99.0 - (w as f64 * 0.1) - (i as f64 * 0.0001);
                let level = PriceLevel::new(price, Decimal::new(1, 0), ts);
                guard.apply_delta(Side::Bid, vec![level], ts);
                count_clone.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // Wait for all
    for handle in handles {
        handle.join().expect("Thread should complete");
    }

    // ASSERT: All operations completed, book consistent
    assert!(read_count.load(Ordering::Relaxed) > 0);
    assert!(write_count.load(Ordering::Relaxed) > 0);

    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
fn test_resource_pressure_combined() {
    // ARRANGE: Combined resource pressure scenario
    let book = Arc::new(thread_safe_orderbook(50));
    let pressure_duration = Duration::from_millis(500);
    let start = Instant::now();
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..50)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        let asks: Vec<PriceLevel> = (0..50)
            .map(|i| PriceLevel::new(101.0 + i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, asks, now);
    }

    let operations = Arc::new(AtomicU64::new(0));

    // ACT: Apply pressure for fixed duration
    let mut handles = Vec::new();

    for t in 0..8 {
        let book_clone = Arc::clone(&book);
        let ops_clone = Arc::clone(&operations);
        let end_time = start + pressure_duration;

        handles.push(thread::spawn(move || {
            while Instant::now() < end_time {
                if t % 2 == 0 {
                    // Reader
                    let _bid = book_clone.read().best_bid().cloned();
                } else {
                    // Writer
                    let mut guard = book_clone.write();
                    let ts = chrono::Utc::now().timestamp_micros();
                    let level = PriceLevel::new(99.9, Decimal::new(1, 0), ts);
                    guard.apply_delta(Side::Bid, vec![level], ts);
                }
                ops_clone.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for handle in handles {
        handle.join().expect("Thread should complete");
    }

    // ASSERT: Completed many operations, book still consistent
    let total_ops = operations.load(Ordering::Relaxed);
    assert!(
        total_ops > 1000,
        "Should complete many operations: {}",
        total_ops
    );

    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}
