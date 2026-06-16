//! Benchmarks for Flash Thread-Safe Order Book Operations.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | read() uncontended | < 50 ns |
//! | write() uncontended | < 100 ns |
//! | read() 10 readers | < 200 ns |
//! | best_bid() convenience | < 100 ns |
//! | snapshot(50) | < 5 μs |
//! | Lock acquisition overhead | < 30 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench thread_safe_bench
//! cargo bench --bench thread_safe_bench -- --test  # Quick verify
//! ```

use astra_flash::book::{
    new_shared_orderbook, OrderBook, OrderBookConfig, SharedOrderBook, ThreadSafeOrderBook,
};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

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

/// Creates a thread-safe order book with N levels per side.
fn create_book_with_levels(n: usize) -> ThreadSafeOrderBook {
    let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

/// Creates a shared order book with N levels per side.
fn create_shared_book_with_levels(n: usize) -> SharedOrderBook {
    let shared = new_shared_orderbook(test_instrument(), OrderBookConfig::default());
    {
        let mut guard = shared.write();
        guard.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    }
    shared
}

// =============================================================================
// LOCK ACQUISITION BENCHMARKS
// =============================================================================

/// Benchmark read lock acquisition (uncontended).
fn bench_read_lock_uncontended(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/read_lock_uncontended", |b| {
        b.iter(|| {
            let guard = book.read();
            black_box(guard.bid_count());
        });
    });
}

/// Benchmark write lock acquisition (uncontended).
fn bench_write_lock_uncontended(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/write_lock_uncontended", |b| {
        b.iter(|| {
            let guard = book.write();
            black_box(guard.bid_count());
        });
    });
}

/// Benchmark try_read when available.
fn bench_try_read_available(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/try_read_available", |b| {
        b.iter(|| {
            if let Some(guard) = book.try_read() {
                black_box(guard.bid_count());
            }
        });
    });
}

/// Benchmark try_write when available.
fn bench_try_write_available(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/try_write_available", |b| {
        b.iter(|| {
            if let Some(guard) = book.try_write() {
                black_box(guard.bid_count());
            }
        });
    });
}

// =============================================================================
// CONVENIENCE METHOD BENCHMARKS
// =============================================================================

/// Benchmark best_bid convenience method.
fn bench_best_bid(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/best_bid", |b| {
        b.iter(|| black_box(book.best_bid()));
    });
}

/// Benchmark best_ask convenience method.
fn bench_best_ask(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/best_ask", |b| {
        b.iter(|| black_box(book.best_ask()));
    });
}

/// Benchmark mid_price convenience method.
fn bench_mid_price(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/mid_price", |b| {
        b.iter(|| black_box(book.mid_price()));
    });
}

/// Benchmark spread convenience method.
fn bench_spread(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/spread", |b| {
        b.iter(|| black_box(book.spread()));
    });
}

/// Benchmark spread_bps convenience method.
fn bench_spread_bps(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/spread_bps", |b| {
        b.iter(|| black_box(book.spread_bps()));
    });
}

// =============================================================================
// SNAPSHOT BENCHMARKS
// =============================================================================

/// Benchmark snapshot creation at various depths.
fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/snapshot");

    for depth in [10, 25, 50].iter() {
        let book = create_book_with_levels(100);

        group.bench_with_input(BenchmarkId::from_parameter(depth), depth, |b, &depth| {
            b.iter(|| black_box(book.snapshot(depth)));
        });
    }

    group.finish();
}

/// Benchmark top_bids at various sizes.
fn bench_top_bids(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/top_bids");

    for n in [5, 10, 25].iter() {
        let book = create_book_with_levels(50);

        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter(|| black_box(book.top_bids(n)));
        });
    }

    group.finish();
}

/// Benchmark top_asks at various sizes.
fn bench_top_asks(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/top_asks");

    for n in [5, 10, 25].iter() {
        let book = create_book_with_levels(50);

        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter(|| black_box(book.top_asks(n)));
        });
    }

    group.finish();
}

// =============================================================================
// WRITE OPERATION BENCHMARKS
// =============================================================================

/// Benchmark apply_snapshot convenience method.
fn bench_apply_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/apply_snapshot");

    for size in [10, 25, 50].iter() {
        let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
        let bids = create_bids(50000.0, *size);
        let asks = create_asks(50001.0, *size);

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                book.apply_snapshot(bids.clone(), asks.clone(), 0);
            });
        });
    }

    group.finish();
}

/// Benchmark apply_delta convenience method.
fn bench_apply_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/apply_delta");

    for size in [1, 5, 10].iter() {
        let book = create_book_with_levels(50);
        let levels = create_bids(49999.0, *size);

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                book.apply_delta(Side::Bid, levels.clone(), 0);
            });
        });
    }

    group.finish();
}

/// Benchmark batch_write operations.
fn bench_batch_write(c: &mut Criterion) {
    let book = create_book_with_levels(50);
    let bid_levels = create_bids(49999.0, 5);
    let ask_levels = create_asks(50002.0, 5);

    c.bench_function("thread_safe/batch_write", |b| {
        b.iter(|| {
            let mut handle = book.batch_write();
            handle.apply_delta(Side::Bid, bid_levels.clone(), 0);
            handle.apply_delta(Side::Ask, ask_levels.clone(), 0);
        });
    });
}

// =============================================================================
// OVERHEAD COMPARISON BENCHMARKS
// =============================================================================

/// Compare ThreadSafeOrderBook overhead vs raw OrderBook.
fn bench_overhead_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/overhead");

    // Raw OrderBook
    let mut raw_book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    raw_book.apply_snapshot(create_bids(50000.0, 50), create_asks(50001.0, 50), 0);

    // Thread-safe wrapper
    let safe_book = create_book_with_levels(50);

    // Shared reference (manual locks)
    let shared_book = create_shared_book_with_levels(50);

    group.bench_function("raw_orderbook/mid_price", |b| {
        b.iter(|| black_box(raw_book.mid_price()));
    });

    group.bench_function("thread_safe/mid_price", |b| {
        b.iter(|| black_box(safe_book.mid_price()));
    });

    group.bench_function("shared/mid_price", |b| {
        b.iter(|| black_box(shared_book.read().mid_price()));
    });

    group.finish();
}

// =============================================================================
// CONCURRENT ACCESS BENCHMARKS
// =============================================================================

/// Benchmark concurrent reads (multiple threads).
fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/concurrent_reads");

    for num_readers in [2, 4, 8].iter() {
        let book = Arc::new(create_book_with_levels(50));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_readers),
            num_readers,
            |b, &num_readers| {
                b.iter_custom(|iters| {
                    let barrier = Arc::new(Barrier::new(num_readers + 1));
                    let stop = Arc::new(AtomicBool::new(false));
                    let total_ops = Arc::new(std::sync::atomic::AtomicU64::new(0));

                    // Spawn reader threads
                    let handles: Vec<_> = (0..num_readers)
                        .map(|_| {
                            let book = Arc::clone(&book);
                            let barrier = Arc::clone(&barrier);
                            let stop = Arc::clone(&stop);
                            let ops = Arc::clone(&total_ops);

                            thread::spawn(move || {
                                barrier.wait();
                                let mut count = 0u64;
                                while !stop.load(Ordering::Relaxed) {
                                    black_box(book.mid_price());
                                    count += 1;
                                }
                                ops.fetch_add(count, Ordering::Relaxed);
                            })
                        })
                        .collect();

                    // Start readers
                    barrier.wait();

                    // Let them run for a fixed duration based on iters
                    let duration_per_iter = Duration::from_nanos(100);
                    thread::sleep(duration_per_iter * iters as u32);

                    // Stop readers
                    stop.store(true, Ordering::Relaxed);

                    // Wait for threads
                    for h in handles {
                        h.join().unwrap();
                    }

                    // Calculate average time per operation
                    let ops = total_ops.load(Ordering::Relaxed);
                    if ops > 0 {
                        duration_per_iter * iters as u32 / (ops as u32 / num_readers as u32)
                    } else {
                        duration_per_iter
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark read-write contention.
fn bench_read_write_contention(c: &mut Criterion) {
    let book = Arc::new(create_book_with_levels(50));
    let levels = create_bids(49999.0, 1);

    c.bench_function("thread_safe/read_write_contention", |b| {
        b.iter_custom(|iters| {
            let barrier = Arc::new(Barrier::new(3)); // 1 writer + 1 reader + main
            let stop = Arc::new(AtomicBool::new(false));

            let book_w = Arc::clone(&book);
            let barrier_w = Arc::clone(&barrier);
            let stop_w = Arc::clone(&stop);
            let levels_w = levels.clone();

            // Writer thread
            let writer = thread::spawn(move || {
                barrier_w.wait();
                while !stop_w.load(Ordering::Relaxed) {
                    book_w.apply_delta(Side::Bid, levels_w.clone(), 0);
                }
            });

            let book_r = Arc::clone(&book);
            let barrier_r = Arc::clone(&barrier);
            let stop_r = Arc::clone(&stop);
            let read_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let read_count_clone = Arc::clone(&read_count);

            // Reader thread
            let reader = thread::spawn(move || {
                barrier_r.wait();
                let mut count = 0u64;
                while !stop_r.load(Ordering::Relaxed) {
                    black_box(book_r.mid_price());
                    count += 1;
                }
                read_count_clone.store(count, Ordering::Relaxed);
            });

            barrier.wait();
            let duration = Duration::from_nanos(1000) * iters as u32;
            thread::sleep(duration);
            stop.store(true, Ordering::Relaxed);

            writer.join().unwrap();
            reader.join().unwrap();

            let reads = read_count.load(Ordering::Relaxed);
            if reads > 0 {
                duration / reads as u32
            } else {
                duration
            }
        });
    });
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark write throughput (updates per second).
fn bench_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_safe/throughput");
    group.throughput(Throughput::Elements(1));

    let book = create_book_with_levels(50);
    let levels = create_bids(49999.0, 1);

    group.bench_function("single_update", |b| {
        b.iter(|| {
            book.apply_delta(Side::Bid, levels.clone(), 0);
        });
    });

    group.finish();
}

/// Benchmark clone_shared performance.
fn bench_clone_shared(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    c.bench_function("thread_safe/clone_shared", |b| {
        b.iter(|| black_box(book.clone_shared()));
    });
}

/// Benchmark thread_stats access.
fn bench_thread_stats(c: &mut Criterion) {
    let book = create_book_with_levels(50);

    // Generate some stats
    for _ in 0..100 {
        let _ = book.read();
        let _ = book.write();
    }

    c.bench_function("thread_safe/thread_stats", |b| {
        b.iter(|| black_box(book.thread_stats()));
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    lock_benches,
    bench_read_lock_uncontended,
    bench_write_lock_uncontended,
    bench_try_read_available,
    bench_try_write_available,
);

criterion_group!(
    convenience_benches,
    bench_best_bid,
    bench_best_ask,
    bench_mid_price,
    bench_spread,
    bench_spread_bps,
);

criterion_group!(
    snapshot_benches,
    bench_snapshot,
    bench_top_bids,
    bench_top_asks,
);

criterion_group!(
    write_benches,
    bench_apply_snapshot,
    bench_apply_delta,
    bench_batch_write,
);

criterion_group!(overhead_benches, bench_overhead_comparison,);

criterion_group!(
    concurrent_benches,
    bench_concurrent_reads,
    bench_read_write_contention,
);

criterion_group!(
    misc_benches,
    bench_write_throughput,
    bench_clone_shared,
    bench_thread_stats,
);

criterion_main!(
    lock_benches,
    convenience_benches,
    snapshot_benches,
    write_benches,
    overhead_benches,
    concurrent_benches,
    misc_benches,
);
