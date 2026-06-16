//! Benchmarks for Flash Order Book Operations.
//!
//! # Performance Targets (per STANDARDS.md)
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Single level update | < 1 μs |
//! | Snapshot apply (50 levels) | < 50 μs |
//! | Best bid/ask lookup | < 10 ns |
//! | Mid price calculation | < 20 ns |
//! | Top N levels | < 200 ns per N |
//! | Throughput | > 50,000 updates/sec |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench orderbook_bench
//! cargo bench --bench orderbook_bench -- --test  # Quick verify
//! ```

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

/// Creates a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Creates a price level.
fn level(price: f64, quantity: Decimal) -> PriceLevel {
    PriceLevel::new(price, quantity, 1234567890)
}

/// Creates N bid levels starting at a price (descending).
fn create_bids(start_price: f64, count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| level(start_price - (i as f64 * 0.5), dec!(1)))
        .collect()
}

/// Creates N ask levels starting at a price (ascending).
fn create_asks(start_price: f64, count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| level(start_price + (i as f64 * 0.5), dec!(1)))
        .collect()
}

/// Creates an order book with N levels per side.
fn create_book_with_levels(n: usize) -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

// =============================================================================
// CONSTRUCTION BENCHMARKS
// =============================================================================

fn bench_orderbook_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_construction");

    group.bench_function("new_empty", |b| {
        b.iter(|| {
            let book = OrderBook::new(
                black_box(test_instrument()),
                black_box(OrderBookConfig::default()),
            );
            black_box(book)
        });
    });

    group.bench_function("with_instrument", |b| {
        b.iter(|| {
            let book = OrderBook::with_instrument(black_box(test_instrument()));
            black_box(book)
        });
    });

    group.finish();
}

// =============================================================================
// SNAPSHOT BENCHMARKS
// =============================================================================

fn bench_apply_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_snapshot");

    for size in [10, 25, 50, 100].iter() {
        let bids = create_bids(50000.0, *size);
        let asks = create_asks(50001.0, *size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
                b.iter(|| {
                    book.apply_snapshot(
                        black_box(bids.clone()),
                        black_box(asks.clone()),
                        black_box(1234567890),
                    );
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// DELTA UPDATE BENCHMARKS
// =============================================================================

fn bench_apply_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_delta");

    // Single level update
    group.bench_function("single_level", |b| {
        let mut book = create_book_with_levels(50);
        let update = vec![level(49999.0, dec!(5))];

        b.iter(|| {
            book.apply_delta(
                black_box(Side::Bid),
                black_box(update.clone()),
                black_box(0),
            );
        });
    });

    // Multi-level updates
    for count in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", count)),
            count,
            |b, &count| {
                let mut book = create_book_with_levels(50);
                let updates: Vec<PriceLevel> = (0..count)
                    .map(|i| level(49990.0 + (i as f64), dec!(1)))
                    .collect();

                b.iter(|| {
                    book.apply_delta(
                        black_box(Side::Bid),
                        black_box(updates.clone()),
                        black_box(0),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_update_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_update_level");

    // Insert new level
    group.bench_function("insert_new", |b| {
        let mut book = create_book_with_levels(50);
        let new_level = level(49975.0, dec!(10));

        b.iter(|| {
            book.update_level(black_box(Side::Bid), black_box(new_level.clone()));
        });
    });

    // Update existing level
    group.bench_function("update_existing", |b| {
        let mut book = create_book_with_levels(50);
        let update = level(50000.0, dec!(15)); // Best bid price

        b.iter(|| {
            book.update_level(black_box(Side::Bid), black_box(update.clone()));
        });
    });

    group.finish();
}

fn bench_remove_level(c: &mut Criterion) {
    c.bench_function("orderbook_remove_level", |b| {
        b.iter_batched(
            || create_book_with_levels(50),
            |mut book| {
                black_box(book.remove_level(Side::Bid, 49999.5));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// =============================================================================
// QUERY BENCHMARKS
// =============================================================================

fn bench_best_price_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_best_price");

    let book = create_book_with_levels(50);

    group.bench_function("best_bid", |b| {
        b.iter(|| black_box(book.best_bid()));
    });

    group.bench_function("best_ask", |b| {
        b.iter(|| black_box(book.best_ask()));
    });

    group.bench_function("mid_price", |b| {
        b.iter(|| black_box(book.mid_price()));
    });

    group.bench_function("spread", |b| {
        b.iter(|| black_box(book.spread()));
    });

    group.bench_function("spread_bps", |b| {
        b.iter(|| black_box(book.spread_bps()));
    });

    group.finish();
}

fn bench_top_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_top_levels");

    let book = create_book_with_levels(100);

    for n in [5, 10, 20, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter(|| {
                let bids = book.top_bids(n);
                let asks = book.top_asks(n);
                black_box((bids, asks))
            });
        });
    }

    group.finish();
}

fn bench_get_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_get_level");

    let book = create_book_with_levels(50);

    group.bench_function("found", |b| {
        b.iter(|| black_box(book.get_level(Side::Bid, 49999.0)));
    });

    group.bench_function("not_found", |b| {
        b.iter(|| black_box(book.get_level(Side::Bid, 12345.0)));
    });

    group.finish();
}

fn bench_quantity_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_quantity");

    let book = create_book_with_levels(50);

    group.bench_function("total_bid_quantity", |b| {
        b.iter(|| black_box(book.total_bid_quantity()));
    });

    group.bench_function("total_ask_quantity", |b| {
        b.iter(|| black_box(book.total_ask_quantity()));
    });

    group.bench_function("imbalance", |b| {
        b.iter(|| black_box(book.imbalance()));
    });

    group.finish();
}

fn bench_state_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_state");

    let book = create_book_with_levels(50);
    let empty_book = OrderBook::new(test_instrument(), OrderBookConfig::default());

    group.bench_function("has_bids", |b| {
        b.iter(|| black_box(book.has_bids()));
    });

    group.bench_function("has_asks", |b| {
        b.iter(|| black_box(book.has_asks()));
    });

    group.bench_function("is_empty_false", |b| {
        b.iter(|| black_box(book.is_empty()));
    });

    group.bench_function("is_empty_true", |b| {
        b.iter(|| black_box(empty_book.is_empty()));
    });

    group.bench_function("bid_count", |b| {
        b.iter(|| black_box(book.bid_count()));
    });

    group.bench_function("ask_count", |b| {
        b.iter(|| black_box(book.ask_count()));
    });

    group.finish();
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

fn bench_to_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_to_snapshot");

    let book = create_book_with_levels(100);

    for depth in [10, 25, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(depth), depth, |b, &depth| {
            b.iter(|| {
                let snapshot = book.to_snapshot(depth);
                black_box(snapshot)
            });
        });
    }

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_throughput");
    group.throughput(Throughput::Elements(1000));

    // 1K delta updates
    group.bench_function("1K_single_deltas", |b| {
        let mut book = create_book_with_levels(50);

        b.iter(|| {
            for i in 0..1000 {
                let price = 49900.0 + (i as f64 % 100.0);
                book.apply_delta(Side::Bid, vec![level(price, dec!(1))], 0);
            }
        });
    });

    // 1K mixed updates
    group.bench_function("1K_mixed_updates", |b| {
        let mut book = create_book_with_levels(50);

        b.iter(|| {
            for i in 0..1000 {
                let price = 50000.0 + (i as f64 % 50.0);
                let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                book.update_level(side, level(price, dec!(1)));
            }
        });
    });

    group.finish();
}

// =============================================================================
// MAX_LEVELS BENCHMARKS (RED TEAM)
// =============================================================================

fn bench_max_levels_enforcement(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_max_levels");

    // Measure overhead of max_levels enforcement
    group.bench_function("enforce_limit", |b| {
        let config = OrderBookConfig {
            max_levels: 50,
            ..OrderBookConfig::default()
        };

        b.iter_batched(
            || {
                let book = OrderBook::new(test_instrument(), config.clone());
                let levels: Vec<PriceLevel> = (0..100)
                    .map(|i| level(50000.0 - (i as f64), dec!(1)))
                    .collect();
                (book, levels)
            },
            |(mut book, levels)| {
                book.apply_delta(Side::Bid, levels, 0);
                black_box(book.bid_count()) // Should be 50
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// =============================================================================
// MEMORY BENCHMARKS
// =============================================================================

fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_memory");

    // Clear operation
    group.bench_function("clear", |b| {
        b.iter_batched(
            || create_book_with_levels(50),
            |mut book| {
                book.clear();
                black_box(book)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Clone cost
    group.bench_function("clone_snapshot", |b| {
        let book = create_book_with_levels(50);
        let snapshot = book.to_snapshot(50);

        b.iter(|| black_box(snapshot.clone()));
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(construction, bench_orderbook_new,);

criterion_group!(
    updates,
    bench_apply_snapshot,
    bench_apply_delta,
    bench_update_level,
    bench_remove_level,
);

criterion_group!(
    queries,
    bench_best_price_queries,
    bench_top_levels,
    bench_get_level,
    bench_quantity_queries,
    bench_state_queries,
);

criterion_group!(serialization, bench_to_snapshot,);

criterion_group!(throughput, bench_throughput,);

criterion_group!(red_team, bench_max_levels_enforcement,);

criterion_group!(memory, bench_memory_patterns,);

criterion_main!(
    construction,
    updates,
    queries,
    serialization,
    throughput,
    red_team,
    memory,
);
