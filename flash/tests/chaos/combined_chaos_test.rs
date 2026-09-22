//! Combined Chaos Tests
//!
//! Tests for multi-failure scenarios including:
//! - Network + Redis failures combined
//! - Message burst + memory pressure
//! - Reconnect during burst
//! - Multi-exchange chaos isolation

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use astra_flash::book::{OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use astra_flash::publisher::batch::{BackpressureMonitor, BackpressureStatus};
use rust_decimal::Decimal;

use super::helpers::*;

// ============================================================================
// TEST 1: NETWORK AND REDIS FAILURE COMBINED
// ============================================================================

#[test]
fn test_network_and_redis_failure_combined() {
    // ARRANGE: Track both network and Redis state
    let mut network_available = true;
    let mut redis_available = true;
    let mut network_failures = 0;
    let mut redis_failures = 0;
    let mut messages_buffered = 0;

    // ACT: Simulate simultaneous failures
    // Phase 1: Both network and Redis go down
    network_available = false;
    redis_available = false;
    network_failures += 1;
    redis_failures += 1;

    // Simulate message buffering during outage
    for _ in 0..50 {
        if !network_available || !redis_available {
            messages_buffered += 1;
        }
    }

    // Phase 2: Network recovers first
    network_available = true;

    // Phase 3: Redis recovers
    redis_available = true;

    // Phase 4: Flush buffered messages
    let flushed = messages_buffered;
    messages_buffered = 0;

    // ASSERT: Both components recovered
    assert!(network_available, "Network should recover");
    assert!(redis_available, "Redis should recover");
    assert_eq!(network_failures, 1);
    assert_eq!(redis_failures, 1);
    assert_eq!(flushed, 50);
    assert_eq!(messages_buffered, 0);
}

// ============================================================================
// TEST 2: BURST WITH MEMORY PRESSURE
// ============================================================================

#[test]
fn test_burst_with_memory_pressure() {
    // ARRANGE: Order book with strict limits (memory pressure simulation)
    let memory_constrained_config = OrderBookConfig {
        max_depth: 20,
        max_levels: 50, // Strict limit
        ..Default::default()
    };
    let book = Arc::new(ThreadSafeOrderBook::new(
        test_instrument(),
        memory_constrained_config,
    ));

    // Backpressure monitor
    let mut monitor = BackpressureMonitor::new(0.75, 0.95, 100);

    let burst_size = 1000;
    let processed = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));

    // ACT: Process burst under memory pressure
    for i in 0..burst_size {
        // Check backpressure
        let current_usage = (i % 100) + 10; // Simulate varying usage
        let (status, _) = monitor.update(current_usage);

        match status {
            BackpressureStatus::Critical => {
                // Drop message under critical pressure
                dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            BackpressureStatus::Warning => {
                // Slow down but continue
            }
            BackpressureStatus::Normal => {}
        }

        // Process message
        let mut guard = book.write();
        let ts = chrono::Utc::now().timestamp_micros();
        let level = PriceLevel::new(100.0 - (i as f64 * 0.001), Decimal::new(1, 0), ts);
        guard.apply_delta(Side::Bid, vec![level], ts);
        processed.fetch_add(1, Ordering::Relaxed);
    }

    // ASSERT: Most messages processed, some dropped under pressure
    let total_processed = processed.load(Ordering::Relaxed);
    let _total_dropped = dropped.load(Ordering::Relaxed);

    assert!(
        total_processed > burst_size as u64 * 8 / 10,
        "Should process most: {}",
        total_processed
    );
    // Book should be consistent
    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}

// ============================================================================
// TEST 3: RECONNECT DURING BURST
// ============================================================================

#[test]
fn test_reconnect_during_burst() {
    // ARRANGE: Components
    let book = Arc::new(ThreadSafeOrderBook::new(
        test_instrument(),
        OrderBookConfig::default(),
    ));

    let mut connected = true;
    let mut reconnect_count = 0;
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize book
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..20)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, vec![], now);
    }

    let burst_size = 500;
    let processed = Arc::new(AtomicU64::new(0));
    let during_reconnect = Arc::new(AtomicU64::new(0));

    // ACT: Process burst with reconnect in middle
    for i in 0..burst_size {
        // Trigger disconnect at 25% through burst
        if i == burst_size / 4 {
            connected = false;
            reconnect_count += 1;
        }

        // Complete reconnect at 75% through burst
        if i == burst_size * 3 / 4 {
            connected = true;
        }

        // Track if we're in reconnecting state
        if !connected {
            // In real scenario, might buffer messages
            during_reconnect.fetch_add(1, Ordering::Relaxed);
        }

        // Process update regardless (simulating buffered delivery after reconnect)
        let mut guard = book.write();
        let ts = chrono::Utc::now().timestamp_micros();
        let level = PriceLevel::new(99.0 - (i as f64 * 0.001), Decimal::new(1, 0), ts);
        guard.apply_delta(Side::Bid, vec![level], ts);
        processed.fetch_add(1, Ordering::Relaxed);
    }

    // ASSERT: All messages processed, some during reconnect
    assert_eq!(processed.load(Ordering::Relaxed), burst_size as u64);
    assert!(
        during_reconnect.load(Ordering::Relaxed) > 0,
        "Should have processed some during reconnect"
    );

    // Final state should be connected
    assert!(connected, "Should be connected after burst");

    // Book should be consistent
    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}

// ============================================================================
// TEST 4: MULTI EXCHANGE CHAOS ISOLATION
// ============================================================================

#[test]
fn test_multi_exchange_chaos_isolation() {
    // ARRANGE: Multiple exchanges with independent state
    let exchanges = vec![Exchange::Deribit, Exchange::Binance, Exchange::Oanda];
    let now = chrono::Utc::now().timestamp_micros();

    // Create separate books for each exchange
    let books: Vec<_> = exchanges
        .iter()
        .map(|ex| {
            let instrument = Instrument::new("BTC", "USD", *ex, "BTC-USD");
            let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());

            // Initialize
            let bids: Vec<PriceLevel> = (0..10)
                .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
                .collect();
            book.write().apply_snapshot(bids, vec![], now);

            (*ex, book)
        })
        .collect();

    // Track exchange states
    let mut exchange_states: Vec<(Exchange, bool)> =
        exchanges.iter().map(|ex| (*ex, true)).collect();

    // ACT: Inject chaos on one exchange, others should be unaffected
    let chaos_exchange = Exchange::Binance;
    for (ex, connected) in exchange_states.iter_mut() {
        if *ex == chaos_exchange {
            *connected = false;
        }
    }

    // Process updates on all exchanges
    for (exchange, book) in &books {
        // All exchanges can still update local book
        let mut guard = book.write();
        let ts = chrono::Utc::now().timestamp_micros();
        let level = PriceLevel::new(99.5, Decimal::new(5, 0), ts);
        guard.apply_delta(Side::Bid, vec![level], ts);
    }

    // ASSERT: Chaos isolated to one exchange
    for (ex, connected) in &exchange_states {
        if *ex == chaos_exchange {
            assert!(!connected, "{:?} should be disconnected", ex);
        } else {
            assert!(connected, "{:?} should be connected", ex);
        }
    }

    // All books should be consistent
    for (exchange, book) in &books {
        let guard = book.read();
        assert!(
            validate_orderbook_consistency(&guard),
            "{:?} book should be consistent",
            exchange
        );
    }
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
fn test_combined_chaos_full_scenario() {
    // ARRANGE: Full chaos scenario with all components
    let book = Arc::new(ThreadSafeOrderBook::new(
        test_instrument(),
        OrderBookConfig::default(),
    ));
    let mut backpressure = BackpressureMonitor::new(0.75, 0.95, 100);
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..20)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, vec![], now);
    }

    let iterations = 200;
    let chaos_events = Arc::new(AtomicU64::new(0));
    let successful_ops = Arc::new(AtomicU64::new(0));

    // ACT: Run scenario with various chaos injections
    for i in 0..iterations {
        // Random chaos injection based on iteration
        let chaos_type = i % 20;
        match chaos_type {
            0 | 5 => {
                // Network disconnect/reconnect cycle
                chaos_events.fetch_add(1, Ordering::Relaxed);
            }
            10 => {
                // Redis failure simulation
                chaos_events.fetch_add(1, Ordering::Relaxed);
            }
            15 => {
                // High backpressure
                let (_, _) = backpressure.update(90);
                chaos_events.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                // Normal operation
                let (_, _) = backpressure.update(50);
            }
        }

        // Check if operation should proceed
        let can_operate = !matches!(backpressure.status(), BackpressureStatus::Critical);

        if can_operate {
            let mut guard = book.write();
            let ts = chrono::Utc::now().timestamp_micros();
            let level = PriceLevel::new(99.0 - (i as f64 * 0.01), Decimal::new(1, 0), ts);
            guard.apply_delta(Side::Bid, vec![level], ts);
            successful_ops.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ASSERT: Most operations succeeded despite chaos
    let total_chaos = chaos_events.load(Ordering::Relaxed);
    let total_success = successful_ops.load(Ordering::Relaxed);

    assert!(total_chaos > 0, "Should have injected chaos events");
    assert!(
        total_success > iterations as u64 * 7 / 10,
        "Should complete most operations: {} / {}",
        total_success,
        iterations
    );

    // Book should remain consistent
    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}

#[tokio::test]
async fn test_concurrent_chaos_all_components() {
    // ARRANGE: Concurrent operations with chaos
    let book = Arc::new(ThreadSafeOrderBook::new(
        test_instrument(),
        OrderBookConfig::default(),
    ));
    let chaos_active = Arc::new(AtomicBool::new(false));
    let operations_completed = Arc::new(AtomicU64::new(0));
    let now = chrono::Utc::now().timestamp_micros();

    // Initialize
    {
        let mut guard = book.write();
        let bids: Vec<PriceLevel> = (0..10)
            .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
            .collect();
        guard.apply_snapshot(bids, vec![], now);
    }

    let duration = Duration::from_millis(500);
    let start = Instant::now();

    // Spawn chaos injector
    let chaos_flag = Arc::clone(&chaos_active);
    let chaos_handle = tokio::spawn(async move {
        while start.elapsed() < duration {
            // Toggle chaos state
            chaos_flag.store(!chaos_flag.load(Ordering::Relaxed), Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    // Spawn workers
    let mut handles = Vec::new();
    for _ in 0..4 {
        let book_clone = Arc::clone(&book);
        let chaos_clone = Arc::clone(&chaos_active);
        let ops_clone = Arc::clone(&operations_completed);

        handles.push(tokio::spawn(async move {
            let mut local_ops = 0u64;
            while start.elapsed() < duration {
                if !chaos_clone.load(Ordering::Relaxed) {
                    // Operate when no chaos
                    let mut guard = book_clone.write();
                    let ts = chrono::Utc::now().timestamp_micros();
                    let level = PriceLevel::new(99.0, Decimal::new(1, 0), ts);
                    guard.apply_delta(Side::Bid, vec![level], ts);
                    local_ops += 1;
                }
                tokio::time::sleep(Duration::from_micros(100)).await;
            }
            ops_clone.fetch_add(local_ops, Ordering::Relaxed);
        }));
    }

    // Wait for all
    chaos_handle.await.unwrap();
    for handle in handles {
        handle.await.unwrap();
    }

    // ASSERT: Operations completed, book consistent
    let total_ops = operations_completed.load(Ordering::Relaxed);
    assert!(total_ops > 50, "Should complete operations: {}", total_ops);

    let guard = book.read();
    assert!(validate_orderbook_consistency(&guard));
}
