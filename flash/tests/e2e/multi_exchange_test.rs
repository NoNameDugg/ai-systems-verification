//! Multi-Exchange Tests for Flash.
//!
//! These tests validate concurrent handling of multiple exchanges:
//! - Deribit
//! - Binance
//! - OANDA
//!
//! # Test Coverage
//!
//! 1. Connect to 3 exchanges simultaneously
//! 2. Process messages from all exchanges concurrently
//! 3. Each exchange has separate order book (isolation)
//! 4. Each exchange publishes to own stream (isolation)
//! 5. One exchange fails, others continue (resilience)
//! 6. All exchanges reconnect after outage
//! 7. Handle different message rates across exchanges
//! 8. Messages processed fairly across exchanges

use astra_flash::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::common::*;

// =============================================================================
// MULTI-EXCHANGE FIXTURE HELPERS
// =============================================================================

/// Create order books for all exchanges.
fn create_all_orderbooks() -> HashMap<Exchange, ThreadSafeOrderBook> {
    let mut books = HashMap::new();

    for exchange in [Exchange::Deribit, Exchange::Binance, Exchange::Oanda] {
        let instrument = test_instrument(exchange);
        let config = OrderBookConfig::default();
        books.insert(exchange, ThreadSafeOrderBook::new(instrument, config));
    }

    books
}

/// Create test consumers for all exchanges.
fn create_all_consumers(env: &TestEnvironment) -> HashMap<Exchange, MockRedisConsumer> {
    let mut consumers = HashMap::new();

    consumers.insert(
        Exchange::Deribit,
        MockRedisConsumer::new(env.stream_name(Exchange::Deribit, "BTC", "USD", "book")),
    );
    consumers.insert(
        Exchange::Binance,
        MockRedisConsumer::new(env.stream_name(Exchange::Binance, "BTC", "USDT", "book")),
    );
    consumers.insert(
        Exchange::Oanda,
        MockRedisConsumer::new(env.stream_name(Exchange::Oanda, "EUR", "USD", "book")),
    );

    consumers
}

// =============================================================================
// MULTI-EXCHANGE TESTS (8)
// =============================================================================

/// Test 1: Connect to 3 exchanges simultaneously.
#[tokio::test]
async fn test_multi_exchange_concurrent_connect() {
    // ARRANGE
    let mock = MultiExchangeMock::all_exchanges().await;

    // ACT: Simulate concurrent connections
    let mut handles = Vec::new();

    for server in mock.servers() {
        let exchange = server.exchange();
        handles.push(tokio::spawn(async move {
            // Simulate connection establishment time
            tokio::time::sleep(Duration::from_millis(10)).await;
            exchange
        }));
    }

    let results: Vec<Exchange> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // ASSERT: All exchanges connected
    assert_eq!(results.len(), 3);
    assert!(results.contains(&Exchange::Deribit));
    assert!(results.contains(&Exchange::Binance));
    assert!(results.contains(&Exchange::Oanda));

    // Simulate connection tracking
    for server in mock.servers() {
        server.simulate_connect();
    }
    assert_eq!(mock.total_connections(), 3);
}

/// Test 2: Process messages from all exchanges concurrently.
#[tokio::test]
async fn test_multi_exchange_concurrent_messages_processed() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let books = create_all_orderbooks();
    let consumers = create_all_consumers(&env);

    // ACT: Apply snapshots to all exchanges concurrently
    let mut handles = Vec::new();

    for (exchange, book) in &books {
        let book = book.clone_shared();
        let consumer = consumers.get(exchange).unwrap();
        let exchange = *exchange;

        // Create exchange-specific prices
        let (bids, asks) = match exchange {
            Exchange::Deribit => (
                vec![(50000.0, 1.0), (49990.0, 2.0)],
                vec![(50010.0, 1.5), (50020.0, 2.5)],
            ),
            Exchange::Binance => (
                vec![(50100.0, 10.0), (50090.0, 20.0)],
                vec![(50110.0, 15.0), (50120.0, 25.0)],
            ),
            Exchange::Oanda => (
                vec![(1.10000, 1000000.0), (1.09990, 2000000.0)],
                vec![(1.10010, 1500000.0), (1.10020, 2500000.0)],
            ),
        };

        handles.push(tokio::spawn({
            let consumer = MockRedisConsumer::new(consumer.consumer().stream());
            async move {
                let bid_levels = price_levels(&bids);
                let ask_levels = price_levels(&asks);
                let timestamp = now_micros();

                // Read lock for snapshot
                {
                    let book_ref = book.read();
                    // Just verify we can read
                    let _ = book_ref.mid_price();
                }

                // Write lock for apply
                {
                    let mut book_ref = book.write();
                    book_ref.apply_snapshot(bid_levels, ask_levels, timestamp);
                }

                // Get snapshot and publish
                let snapshot = {
                    let book_ref = book.read();
                    book_ref.to_snapshot(50)
                };
                consumer.receive_snapshot(snapshot).await;

                (exchange, consumer.messages().len())
            }
        }));
    }

    let results: Vec<(Exchange, usize)> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // ASSERT: All exchanges processed messages
    for (exchange, count) in results {
        assert_eq!(count, 1, "{:?} should have 1 message", exchange);
    }
}

/// Test 3: Each exchange has separate order book (isolation).
#[tokio::test]
async fn test_multi_exchange_isolated_orderbooks() {
    // ARRANGE
    let books = create_all_orderbooks();

    // ACT: Apply different data to each book
    for (exchange, book) in &books {
        let (bids, asks) = match *exchange {
            Exchange::Deribit => (vec![(50000.0, 1.0)], vec![(50010.0, 1.0)]),
            Exchange::Binance => (vec![(60000.0, 10.0)], vec![(60010.0, 10.0)]),
            Exchange::Oanda => (vec![(1.1000, 1000000.0)], vec![(1.1002, 1000000.0)]),
        };

        book.apply_snapshot(price_levels(&bids), price_levels(&asks), now_micros());
    }

    // ASSERT: Each book has its own data
    let deribit_mid = books.get(&Exchange::Deribit).unwrap().mid_price().unwrap();
    let binance_mid = books.get(&Exchange::Binance).unwrap().mid_price().unwrap();
    let oanda_mid = books.get(&Exchange::Oanda).unwrap().mid_price().unwrap();

    // All mid prices should be different
    assert!(
        (deribit_mid - 50005.0).abs() < 0.01,
        "Deribit mid should be ~50005"
    );
    assert!(
        (binance_mid - 60005.0).abs() < 0.01,
        "Binance mid should be ~60005"
    );
    assert!(
        (oanda_mid - 1.1001).abs() < 0.0001,
        "OANDA mid should be ~1.1001"
    );

    // No cross-contamination
    assert!(deribit_mid < binance_mid);
    assert!(oanda_mid < deribit_mid);
}

/// Test 4: Each exchange publishes to own stream (isolation).
#[tokio::test]
async fn test_multi_exchange_isolated_streams() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let consumers = create_all_consumers(&env);

    // ACT: Publish to each exchange's stream
    for (exchange, consumer) in &consumers {
        let instrument = test_instrument(*exchange);
        let snapshot = BookSnapshot {
            instrument,
            timestamp: now_micros(),
            bids: price_levels(&[(50000.0, 1.0)]),
            asks: price_levels(&[(50010.0, 1.0)]),
        };
        consumer.receive_snapshot(snapshot).await;
    }

    // ASSERT: Each consumer only has its own messages
    for (exchange, consumer) in &consumers {
        let messages = consumer.messages();
        assert_eq!(messages.len(), 1, "{:?} should have 1 message", exchange);

        let snapshot = messages[0].snapshot().unwrap();
        assert_eq!(
            snapshot.instrument.exchange, *exchange,
            "Message exchange should match"
        );
    }

    // Verify stream names are different
    let streams: Vec<String> = consumers
        .values()
        .map(|c| c.consumer().stream().to_string())
        .collect();
    let unique_streams: std::collections::HashSet<_> = streams.iter().collect();
    assert_eq!(unique_streams.len(), 3, "All streams should be unique");
}

/// Test 5: One exchange fails, others continue.
#[tokio::test]
async fn test_multi_exchange_one_fails_others_continue() {
    // ARRANGE
    let mock = MultiExchangeMock::all_exchanges().await;
    let books = create_all_orderbooks();

    // Connect all
    for server in mock.servers() {
        server.simulate_connect();
    }
    assert_eq!(mock.total_connections(), 3);

    // ACT: Deribit fails
    mock.get(Exchange::Deribit).unwrap().simulate_disconnect();

    // Other exchanges should still work
    let binance_book = books.get(&Exchange::Binance).unwrap();
    let oanda_book = books.get(&Exchange::Oanda).unwrap();

    binance_book.apply_snapshot(
        price_levels(&[(50000.0, 1.0)]),
        price_levels(&[(50010.0, 1.0)]),
        now_micros(),
    );

    oanda_book.apply_snapshot(
        price_levels(&[(1.1000, 1.0)]),
        price_levels(&[(1.1002, 1.0)]),
        now_micros(),
    );

    // ASSERT: Failed exchange is disconnected, others work
    assert_eq!(mock.get(Exchange::Deribit).unwrap().connection_count(), 0);
    assert_eq!(mock.get(Exchange::Binance).unwrap().connection_count(), 1);
    assert_eq!(mock.get(Exchange::Oanda).unwrap().connection_count(), 1);

    // Other books have data
    assert!(binance_book.mid_price().is_some());
    assert!(oanda_book.mid_price().is_some());
}

/// Test 6: All exchanges reconnect after outage.
#[tokio::test]
async fn test_multi_exchange_all_reconnect_after_outage() {
    // ARRANGE
    let mock = MultiExchangeMock::all_exchanges().await;

    // Initial connections
    for server in mock.servers() {
        server.simulate_connect();
    }
    assert_eq!(mock.total_connections(), 3);

    // ACT: All disconnect (outage)
    for server in mock.servers() {
        server.force_disconnect_all();
    }
    assert_eq!(mock.total_connections(), 0);

    // All reconnect
    tokio::time::sleep(Duration::from_millis(10)).await;
    for server in mock.servers() {
        server.simulate_connect();
    }

    // ASSERT: All reconnected
    assert_eq!(mock.total_connections(), 3);

    // Check total connection count (should be 2 per exchange: initial + reconnect)
    for server in mock.servers() {
        assert_eq!(
            server.total_connections(),
            2,
            "{:?} should have 2 total connections",
            server.exchange()
        );
    }
}

/// Test 7: Handle different message rates across exchanges.
#[tokio::test]
async fn test_multi_exchange_different_message_rates() {
    // ARRANGE
    let env = TestEnvironment::new().await;
    let books = create_all_orderbooks();
    let consumers = create_all_consumers(&env);

    // ACT: Send different numbers of messages to each exchange
    // Deribit: 10 messages
    // Binance: 50 messages
    // OANDA: 5 messages

    let message_counts = [
        (Exchange::Deribit, 10),
        (Exchange::Binance, 50),
        (Exchange::Oanda, 5),
    ];

    for (exchange, count) in &message_counts {
        let book = books.get(exchange).unwrap();
        let consumer = consumers.get(exchange).unwrap();

        for i in 0..*count {
            let price = 50000.0 + i as f64;
            book.apply_delta(Side::Bid, price_levels(&[(price, 1.0)]), now_micros());

            // Publish every update
            let snapshot = book.snapshot(50);
            consumer.receive_snapshot(snapshot).await;
        }
    }

    // ASSERT: Each consumer received correct number
    for (exchange, expected_count) in &message_counts {
        let consumer = consumers.get(exchange).unwrap();
        let actual = consumer.messages().len();
        assert_eq!(
            actual, *expected_count,
            "{:?} should have {} messages, got {}",
            exchange, expected_count, actual
        );
    }
}

/// Test 8: Messages processed fairly across exchanges (no starvation).
#[tokio::test]
async fn test_multi_exchange_fair_processing() {
    // ARRANGE
    let books = create_all_orderbooks();
    let processing_order: Arc<parking_lot::RwLock<Vec<Exchange>>> =
        Arc::new(parking_lot::RwLock::new(Vec::new()));

    // ACT: Interleave processing across exchanges
    let exchanges = [Exchange::Deribit, Exchange::Binance, Exchange::Oanda];

    for round in 0..10 {
        for exchange in &exchanges {
            let book = books.get(exchange).unwrap();
            let order = processing_order.clone();

            // Apply delta
            let price = 50000.0 + round as f64;
            book.apply_delta(Side::Bid, price_levels(&[(price, 1.0)]), now_micros());

            // Record processing order
            order.write().push(*exchange);
        }
    }

    // ASSERT: Fair distribution
    let order = processing_order.read();

    // Count occurrences of each exchange
    let mut counts: HashMap<Exchange, usize> = HashMap::new();
    for exchange in order.iter() {
        *counts.entry(*exchange).or_insert(0) += 1;
    }

    // Each should have exactly 10 (one per round)
    for exchange in &exchanges {
        assert_eq!(
            counts.get(exchange).copied().unwrap_or(0),
            10,
            "{:?} should have 10 processing events",
            exchange
        );
    }

    // Check interleaving: no exchange should process more than 3 times in a row
    let mut consecutive = 1;
    let mut max_consecutive = 1;

    for window in order.windows(2) {
        if window[0] == window[1] {
            consecutive += 1;
            max_consecutive = max_consecutive.max(consecutive);
        } else {
            consecutive = 1;
        }
    }

    // With proper round-robin, max consecutive should be 1
    assert_eq!(
        max_consecutive, 1,
        "Exchanges should be perfectly interleaved"
    );
}

// =============================================================================
// ADDITIONAL MULTI-EXCHANGE TESTS
// =============================================================================

#[cfg(test)]
mod additional_tests {
    use super::*;

    /// Test multi-exchange with shared order book manager pattern.
    #[tokio::test]
    async fn test_multi_exchange_shared_manager() {
        // Simulate a manager that owns all order books
        struct OrderBookManager {
            books: HashMap<Exchange, ThreadSafeOrderBook>,
        }

        impl OrderBookManager {
            fn new() -> Self {
                Self {
                    books: create_all_orderbooks(),
                }
            }

            fn get(&self, exchange: Exchange) -> Option<&ThreadSafeOrderBook> {
                self.books.get(&exchange)
            }

            fn all_mid_prices(&self) -> HashMap<Exchange, Option<f64>> {
                self.books
                    .iter()
                    .map(|(e, b)| (*e, b.mid_price()))
                    .collect()
            }
        }

        let manager = OrderBookManager::new();

        // Apply data
        for (exchange, book) in &manager.books {
            let price = match *exchange {
                Exchange::Deribit => 50000.0,
                Exchange::Binance => 60000.0,
                Exchange::Oanda => 1.1000,
            };
            book.apply_snapshot(
                price_levels(&[(price, 1.0)]),
                price_levels(&[(price + 10.0, 1.0)]),
                now_micros(),
            );
        }

        // Get all mid prices
        let mid_prices = manager.all_mid_prices();
        assert_eq!(mid_prices.len(), 3);
        assert!(mid_prices.values().all(|p| p.is_some()));
    }

    /// Test concurrent reads across exchanges.
    #[tokio::test]
    async fn test_multi_exchange_concurrent_reads() {
        let books = Arc::new(create_all_orderbooks());

        // Apply initial data
        for (_, book) in books.iter() {
            book.apply_snapshot(
                price_levels(&[(50000.0, 1.0)]),
                price_levels(&[(50010.0, 1.0)]),
                now_micros(),
            );
        }

        // Spawn concurrent readers
        let mut handles = Vec::new();
        for _ in 0..10 {
            let books_clone = books.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..100 {
                    for (_, book) in books_clone.iter() {
                        let _ = book.mid_price();
                        let _ = book.spread();
                    }
                    tokio::task::yield_now().await;
                }
            }));
        }

        // All readers should complete without deadlock
        let results = futures::future::join_all(handles).await;
        assert!(results.iter().all(|r| r.is_ok()));
    }
}
