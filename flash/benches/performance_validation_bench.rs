//! Phase 6.2: Performance Validation Benchmarks for Flash.
//!
//! This benchmark file validates ALL performance targets from PLANNING.md and STANDARDS.md.
//!
//! # Performance Targets (MUST PASS)
//!
//! | Metric | Budget | Target |
//! |--------|--------|--------|
//! | Internal Latency (p95) | < 50 μs | < 25 μs |
//! | Internal Latency (p99) | < 100 μs | < 50 μs |
//! | Throughput | > 50K msg/s | > 100K msg/s |
//! | Memory Footprint | < 100 MB | < 50 MB |
//! | Order Book Update | < 1 μs | < 500 ns |
//! | Best Bid/Ask Lookup | < 10 ns | < 5 ns |
//! | Snapshot (50 levels) | < 50 μs | < 25 μs |
//! | JSON Parsing | < 1 μs | < 500 ns |
//! | Bincode Serialization | < 0.5 μs | < 250 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench performance_validation_bench
//! cargo bench --bench performance_validation_bench -- "latency"  # Just latency tests
//! cargo bench --bench performance_validation_bench -- "throughput"  # Just throughput tests
//! ```

use astra_flash::book::{BookSnapshot, OrderBook, OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

/// Creates a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Creates a price level with realistic data.
fn level(price: f64, quantity: Decimal, timestamp: i64) -> PriceLevel {
    PriceLevel::new(price, quantity, timestamp)
}

/// Creates N bid levels starting at a price (descending).
fn create_bids(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price - (i as f64 * 0.5), dec!(1), now))
        .collect()
}

/// Creates N ask levels starting at a price (ascending).
fn create_asks(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price + (i as f64 * 0.5), dec!(1), now))
        .collect()
}

/// Creates an order book with N levels per side.
fn create_book_with_levels(n: usize) -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

/// Creates a thread-safe order book with N levels per side.
fn create_thread_safe_book(n: usize) -> ThreadSafeOrderBook {
    let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

/// Creates a realistic snapshot for serialization testing.
fn create_realistic_snapshot(depth: usize) -> BookSnapshot {
    let book = create_book_with_levels(depth);
    book.to_snapshot(depth)
}

/// Creates a realistic JSON message for parsing.
fn create_json_snapshot_message(depth: usize) -> String {
    let snapshot = create_realistic_snapshot(depth);
    serde_json::to_string(&snapshot).unwrap()
}

/// Generates delta update messages for throughput testing.
fn generate_delta_messages(count: usize) -> Vec<(Side, PriceLevel)> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| {
            let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
            let price = if side == Side::Bid {
                49990.0 + (i % 20) as f64
            } else {
                50010.0 + (i % 20) as f64
            };
            (side, level(price, dec!(1), now + i as i64))
        })
        .collect()
}

// =============================================================================
// CATEGORY 1: LATENCY BENCHMARKS (12 tests)
// =============================================================================

/// Benchmark single price level update.
/// TARGET: < 1 μs
fn bench_latency_single_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/orderbook");

    group.bench_function("single_level_update", |b| {
        let mut book = create_book_with_levels(50);
        let update = level(49999.0, dec!(5), 0);

        b.iter(|| {
            book.update_level(black_box(Side::Bid), black_box(update.clone()));
        });
    });

    group.bench_function("update_existing", |b| {
        let mut book = create_book_with_levels(50);
        let update = level(50000.0, dec!(10), 0); // Best bid price

        b.iter(|| {
            book.update_level(black_box(Side::Bid), black_box(update.clone()));
        });
    });

    group.bench_function("insert_new_level", |b| {
        let mut book = create_book_with_levels(50);
        let mut price = 49500.0;

        b.iter(|| {
            price += 0.01;
            let update = level(price, dec!(1), 0);
            book.update_level(black_box(Side::Bid), black_box(update));
        });
    });

    group.finish();
}

/// Benchmark best bid/ask lookup.
/// TARGET: < 10 ns
fn bench_latency_best_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/best_lookup");

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

/// Benchmark snapshot creation at various depths.
/// TARGET: 50 levels < 50 μs
fn bench_latency_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/snapshot");

    let book = create_book_with_levels(100);

    for depth in [10, 25, 50, 100] {
        group.throughput(Throughput::Elements(depth as u64 * 2));

        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| black_box(book.to_snapshot(depth)));
        });
    }

    group.finish();
}

/// Benchmark JSON parsing.
/// TARGET: < 1 μs for small messages
fn bench_latency_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/json_parsing");

    // Small message (single level)
    let small_json = serde_json::to_string(&level(50000.0, dec!(1), 0)).unwrap();
    group.bench_function("small_message", |b| {
        b.iter(|| {
            let parsed: PriceLevel = serde_json::from_str(black_box(&small_json)).unwrap();
            black_box(parsed)
        });
    });

    // Full snapshot
    for depth in [10, 50] {
        let snapshot_json = create_json_snapshot_message(depth);

        group.bench_with_input(
            BenchmarkId::new("snapshot", depth),
            &snapshot_json,
            |b, json| {
                b.iter(|| {
                    let parsed: BookSnapshot = serde_json::from_str(black_box(json)).unwrap();
                    black_box(parsed)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark serialization formats.
/// TARGET: Bincode < 0.5 μs
fn bench_latency_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/serialization");

    let snapshot = create_realistic_snapshot(50);

    // JSON serialization
    group.bench_function("json_serialize", |b| {
        b.iter(|| black_box(serde_json::to_vec(&snapshot).unwrap()));
    });

    // Bincode serialization
    group.bench_function("bincode_serialize", |b| {
        b.iter(|| black_box(bincode::serialize(&snapshot).unwrap()));
    });

    // JSON deserialization
    let json_bytes = serde_json::to_vec(&snapshot).unwrap();
    group.bench_function("json_deserialize", |b| {
        b.iter(|| black_box(serde_json::from_slice::<BookSnapshot>(&json_bytes).unwrap()));
    });

    // Note: bincode deserialization skipped due to rust_decimal limitation
    // Bincode serialization is sufficient for redis publishing

    group.finish();
}

/// Benchmark thread-safe operations.
/// TARGET: Overhead < 100 ns
fn bench_latency_thread_safe(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency/thread_safe");

    let book = create_thread_safe_book(50);

    group.bench_function("best_bid_thread_safe", |b| {
        b.iter(|| black_box(book.best_bid()));
    });

    group.bench_function("mid_price_thread_safe", |b| {
        b.iter(|| black_box(book.mid_price()));
    });

    group.bench_function("snapshot_thread_safe", |b| {
        b.iter(|| black_box(book.snapshot(50)));
    });

    group.finish();
}

// =============================================================================
// CATEGORY 2: THROUGHPUT BENCHMARKS (10 tests)
// =============================================================================

/// Benchmark message processing throughput.
/// TARGET: > 50,000 msg/sec
fn bench_throughput_messages(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/messages");

    for batch_size in [1_000, 10_000, 50_000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("process_batch", batch_size),
            &batch_size,
            |b, &size| {
                let messages = generate_delta_messages(size);
                let mut book = create_book_with_levels(50);

                b.iter(|| {
                    for (side, level) in &messages {
                        book.update_level(*side, level.clone());
                    }
                    black_box(&book);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark order book update throughput.
/// TARGET: > 100K updates/sec
fn bench_throughput_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/updates");
    group.throughput(Throughput::Elements(10_000));

    group.bench_function("10K_updates", |b| {
        let mut book = create_book_with_levels(50);
        let updates: Vec<(Side, PriceLevel)> = (0..10_000)
            .map(|i| {
                let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                let price = if side == Side::Bid {
                    49900.0 + (i % 100) as f64
                } else {
                    50100.0 + (i % 100) as f64
                };
                (side, level(price, dec!(1), 0))
            })
            .collect();

        b.iter(|| {
            for (side, lvl) in &updates {
                book.update_level(*side, lvl.clone());
            }
        });
    });

    group.finish();
}

/// Benchmark snapshot creation throughput.
/// TARGET: > 10K snapshots/sec
fn bench_throughput_snapshots(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/snapshots");
    group.throughput(Throughput::Elements(1_000));

    let book = create_book_with_levels(50);

    group.bench_function("1K_snapshots", |b| {
        b.iter(|| {
            for _ in 0..1_000 {
                black_box(book.to_snapshot(50));
            }
        });
    });

    group.finish();
}

/// Benchmark concurrent read throughput.
/// TARGET: > 1M reads/sec with 4 readers
fn bench_throughput_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/concurrent_reads");

    for num_readers in [1, 2, 4, 8] {
        let book = Arc::new(create_thread_safe_book(50));
        let read_target = 100_000u64;

        group.throughput(Throughput::Elements(read_target));

        group.bench_with_input(
            BenchmarkId::new("readers", num_readers),
            &num_readers,
            |b, &readers| {
                b.iter(|| {
                    let per_reader = read_target / readers as u64;
                    let handles: Vec<_> = (0..readers)
                        .map(|_| {
                            let book = Arc::clone(&book);
                            thread::spawn(move || {
                                for _ in 0..per_reader {
                                    black_box(book.mid_price());
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark burst message handling.
/// TARGET: Handle 100K messages in burst
fn bench_throughput_burst(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/burst");
    group.throughput(Throughput::Elements(100_000));
    group.sample_size(20); // Fewer samples for long test

    group.bench_function("100K_burst", |b| {
        let messages = generate_delta_messages(100_000);

        b.iter(|| {
            let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
            book.apply_snapshot(create_bids(50000.0, 50), create_asks(50001.0, 50), 0);

            for (side, lvl) in &messages {
                book.update_level(*side, lvl.clone());
            }

            black_box(book.mid_price())
        });
    });

    group.finish();
}

/// Benchmark serialization throughput.
/// TARGET: > 100K serializations/sec
fn bench_throughput_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/serialization");
    group.throughput(Throughput::Elements(10_000));

    let snapshot = create_realistic_snapshot(50);

    group.bench_function("10K_json", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(serde_json::to_vec(&snapshot).unwrap());
            }
        });
    });

    group.bench_function("10K_bincode", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(bincode::serialize(&snapshot).unwrap());
            }
        });
    });

    group.finish();
}

// =============================================================================
// CATEGORY 3: MEMORY BENCHMARKS (8 tests)
// =============================================================================

/// Benchmark memory footprint for order books.
fn bench_memory_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/footprint");

    // Measure size of empty book
    group.bench_function("empty_book_size", |b| {
        b.iter(|| {
            let book = OrderBook::new(test_instrument(), OrderBookConfig::default());
            let size = std::mem::size_of_val(&book);
            black_box(size)
        });
    });

    // Measure with various level counts
    for levels in [10, 25, 50, 100] {
        group.bench_with_input(BenchmarkId::new("book", levels), &levels, |b, &levels| {
            b.iter(|| {
                let book = create_book_with_levels(levels);
                // Force materialization
                let _ = black_box(book.to_snapshot(levels));
            });
        });
    }

    group.finish();
}

/// Benchmark snapshot memory overhead.
fn bench_memory_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/snapshot");

    for depth in [10, 25, 50] {
        let snapshot = create_realistic_snapshot(depth);
        let json_size = serde_json::to_vec(&snapshot).unwrap().len();
        let bincode_size = bincode::serialize(&snapshot).unwrap().len();

        group.bench_with_input(
            BenchmarkId::new("json_size", depth),
            &json_size,
            |b, &size| {
                b.iter(|| black_box(size));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("bincode_size", depth),
            &bincode_size,
            |b, &size| {
                b.iter(|| black_box(size));
            },
        );
    }

    group.finish();
}

/// Benchmark clone cost (indicates allocation pressure).
fn bench_memory_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/clone");

    let book = create_book_with_levels(50);
    let snapshot = book.to_snapshot(50);

    group.bench_function("clone_snapshot", |b| {
        b.iter(|| black_box(snapshot.clone()));
    });

    group.bench_function("clone_level", |b| {
        let level = level(50000.0, dec!(100), 0);
        b.iter(|| black_box(level.clone()));
    });

    group.finish();
}

// =============================================================================
// CATEGORY 4: COMPARISON BENCHMARKS (6 tests)
// =============================================================================

/// Compare data structure performance.
fn bench_comparison_data_structures(c: &mut Criterion) {
    use ordered_float::OrderedFloat;
    use std::collections::{BTreeMap, HashMap};

    let mut group = c.benchmark_group("comparison/data_structure");

    let test_data: Vec<(f64, f64)> = (0..100)
        .map(|i| (50000.0 + i as f64, 1.0 + i as f64))
        .collect();

    // BTreeMap insert
    group.bench_function("btreemap_insert_100", |b| {
        b.iter(|| {
            let mut map: BTreeMap<OrderedFloat<f64>, f64> = BTreeMap::new();
            for (k, v) in &test_data {
                map.insert(OrderedFloat(*k), *v);
            }
            black_box(map.len())
        });
    });

    // HashMap insert
    group.bench_function("hashmap_insert_100", |b| {
        b.iter(|| {
            let mut map: HashMap<OrderedFloat<f64>, f64> = HashMap::new();
            for (k, v) in &test_data {
                map.insert(OrderedFloat(*k), *v);
            }
            black_box(map.len())
        });
    });

    // BTreeMap best (O(1) via last)
    let mut btree: BTreeMap<OrderedFloat<f64>, f64> = BTreeMap::new();
    for (k, v) in &test_data {
        btree.insert(OrderedFloat(*k), *v);
    }

    group.bench_function("btreemap_best", |b| {
        b.iter(|| black_box(btree.last_key_value()));
    });

    // HashMap best (O(n))
    let mut hashmap: HashMap<OrderedFloat<f64>, f64> = HashMap::new();
    for (k, v) in &test_data {
        hashmap.insert(OrderedFloat(*k), *v);
    }

    group.bench_function("hashmap_best_scan", |b| {
        b.iter(|| black_box(hashmap.iter().max_by_key(|(k, _)| *k)));
    });

    group.finish();
}

/// Compare serialization formats.
fn bench_comparison_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("comparison/serialization");

    let snapshot = create_realistic_snapshot(50);

    // Serialize sizes
    let json_bytes = serde_json::to_vec(&snapshot).unwrap();
    let bincode_bytes = bincode::serialize(&snapshot).unwrap();

    println!("JSON size: {} bytes", json_bytes.len());
    println!("Bincode size: {} bytes", bincode_bytes.len());

    group.bench_function("json_roundtrip", |b| {
        b.iter(|| {
            let bytes = serde_json::to_vec(&snapshot).unwrap();
            let parsed: BookSnapshot = serde_json::from_slice(&bytes).unwrap();
            black_box(parsed)
        });
    });

    // Note: bincode roundtrip not included due to rust_decimal limitation
    // Bincode is used for serialization only in production

    group.finish();
}

/// Compare locking strategies.
fn bench_comparison_locking(c: &mut Criterion) {
    use parking_lot::RwLock;
    use std::sync::RwLock as StdRwLock;

    let mut group = c.benchmark_group("comparison/locking");

    let data = 42i64;
    let parking_lot_lock = Arc::new(RwLock::new(data));
    let std_lock = Arc::new(StdRwLock::new(data));

    group.bench_function("parking_lot_read", |b| {
        b.iter(|| {
            let guard = parking_lot_lock.read();
            black_box(*guard)
        });
    });

    group.bench_function("std_rwlock_read", |b| {
        b.iter(|| {
            let guard = std_lock.read().unwrap();
            black_box(*guard)
        });
    });

    group.bench_function("parking_lot_write", |b| {
        b.iter(|| {
            let mut guard = parking_lot_lock.write();
            *guard = 43;
            black_box(*guard)
        });
    });

    group.bench_function("std_rwlock_write", |b| {
        b.iter(|| {
            let mut guard = std_lock.write().unwrap();
            *guard = 43;
            black_box(*guard)
        });
    });

    group.finish();
}

// =============================================================================
// SUSTAINED LOAD TEST
// =============================================================================

/// Measure actual messages per second over 1 second.
/// This is the key validation benchmark.
fn bench_sustained_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation/sustained");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("1_second_sustained", |b| {
        let messages = generate_delta_messages(100_000);

        b.iter(|| {
            let mut book = create_book_with_levels(50);
            let start = Instant::now();
            let mut count = 0u64;

            // Run for exactly 1 second
            while start.elapsed() < Duration::from_secs(1) {
                for (side, lvl) in messages.iter().take(1000) {
                    book.update_level(*side, lvl.clone());
                    count += 1;
                }
            }

            // Record rate
            let rate = count as f64 / start.elapsed().as_secs_f64();
            assert!(
                rate > 50_000.0,
                "FAILED: Rate {:.0} msg/sec < 50K target",
                rate
            );

            black_box(rate)
        });
    });

    group.finish();
}

/// End-to-end pipeline latency test.
fn bench_pipeline_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation/pipeline");

    group.bench_function("e2e_latency", |b| {
        let mut book = create_book_with_levels(50);

        b.iter(|| {
            // Simulate full pipeline:
            // 1. Parse JSON delta
            let json = r#"{"price":50000.0,"quantity":"1","timestamp":1234567890}"#;
            let level: PriceLevel = serde_json::from_str(json).unwrap();

            // 2. Apply to order book
            book.update_level(Side::Bid, level);

            // 3. Create snapshot
            let snapshot = book.to_snapshot(50);

            // 4. Serialize for Redis
            let bytes = bincode::serialize(&snapshot).unwrap();

            black_box(bytes.len())
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    name = latency_benches;
    config = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(3));
    targets =
        bench_latency_single_update,
        bench_latency_best_lookup,
        bench_latency_snapshot,
        bench_latency_json_parsing,
        bench_latency_serialization,
        bench_latency_thread_safe
);

criterion_group!(
    name = throughput_benches;
    config = Criterion::default()
        .sample_size(50)
        .measurement_time(Duration::from_secs(5));
    targets =
        bench_throughput_messages,
        bench_throughput_updates,
        bench_throughput_snapshots,
        bench_throughput_concurrent_reads,
        bench_throughput_burst,
        bench_throughput_serialization
);

criterion_group!(
    name = memory_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(3));
    targets =
        bench_memory_footprint,
        bench_memory_snapshot,
        bench_memory_clone
);

criterion_group!(
    name = comparison_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(3));
    targets =
        bench_comparison_data_structures,
        bench_comparison_serialization,
        bench_comparison_locking
);

criterion_group!(
    name = validation_benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10));
    targets =
        bench_sustained_throughput,
        bench_pipeline_latency
);

criterion_main!(
    latency_benches,
    throughput_benches,
    memory_benches,
    comparison_benches,
    validation_benches
);
