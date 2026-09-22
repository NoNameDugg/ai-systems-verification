//! CPU Profiling Benchmarks for Flash (Batch 4.1).
//!
//! This benchmark file is specifically designed for CPU profiling and hotspot identification.
//! It provides detailed timing analysis of the hot path operations.
//!
//! # Purpose
//!
//! - Identify CPU hotspots through systematic benchmarking
//! - Measure allocation pressure in critical code paths
//! - Profile end-to-end message processing pipeline
//! - Compare performance across different operation types
//!
//! # Running Benchmarks
//!
//! ```bash
//! # Run all profiling benchmarks
//! cargo bench --bench cpu_profile_bench
//!
//! # Run specific hotspot analysis
//! cargo bench --bench cpu_profile_bench -- "hotspot"
//!
//! # Run with profiling (criterion profiling mode)
//! cargo bench --bench cpu_profile_bench -- --profile-time 10
//! ```
//!
//! # Profiling with External Tools
//!
//! For Windows flamegraph (requires ETW setup):
//! ```bash
//! cargo flamegraph --bench cpu_profile_bench -- --bench
//! ```
//!
//! For allocation profiling, see the `alloc_profile_test.rs` in tests.

use astra_flash::book::{BookSnapshot, OrderBook, OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use astra_flash::network::adapters::{DeribitAdapter, ExchangeAdapter, OandaAdapter};
use criterion::{
    black_box, criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, BenchmarkId,
    Criterion, Throughput,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::time::Duration;

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

/// Creates a test instrument for profiling.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Creates a price level.
fn level(price: f64, quantity: Decimal, timestamp: i64) -> PriceLevel {
    PriceLevel::new(price, quantity, timestamp)
}

/// Creates N bid levels.
fn create_bids(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price - (i as f64 * 0.5), dec!(1), now))
        .collect()
}

/// Creates N ask levels.
fn create_asks(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price + (i as f64 * 0.5), dec!(1), now))
        .collect()
}

/// Creates an order book with N levels per side.
fn create_book(n: usize) -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

/// Creates a thread-safe order book.
fn create_ts_book(n: usize) -> ThreadSafeOrderBook {
    let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

// =============================================================================
// TEST MESSAGES - EXCHANGE FORMATS
// =============================================================================

/// Deribit order book delta message.
const DERIBIT_DELTA: &str = r#"{
    "jsonrpc": "2.0",
    "method": "subscription",
    "params": {
        "channel": "book.BTC-PERPETUAL.raw",
        "data": {
            "type": "change",
            "timestamp": 1703836800000,
            "instrument_name": "BTC-PERPETUAL",
            "change_id": 12345679,
            "bids": [["change", 42000.0, 10.5]],
            "asks": [["change", 42001.0, 8.0]]
        }
    }
}"#;

/// Deribit snapshot with 10 levels.
const DERIBIT_SNAPSHOT_10: &str = r#"{
    "jsonrpc": "2.0",
    "method": "subscription",
    "params": {
        "channel": "book.BTC-PERPETUAL.raw",
        "data": {
            "type": "snapshot",
            "timestamp": 1703836800000,
            "instrument_name": "BTC-PERPETUAL",
            "change_id": 12345678,
            "bids": [
                ["new", 42000.0, 10.5], ["new", 41999.5, 5.0], ["new", 41999.0, 3.0],
                ["new", 41998.5, 2.5], ["new", 41998.0, 2.0], ["new", 41997.5, 1.5],
                ["new", 41997.0, 1.0], ["new", 41996.5, 0.8], ["new", 41996.0, 0.5],
                ["new", 41995.5, 0.3]
            ],
            "asks": [
                ["new", 42001.0, 8.0], ["new", 42001.5, 4.0], ["new", 42002.0, 3.5],
                ["new", 42002.5, 3.0], ["new", 42003.0, 2.5], ["new", 42003.5, 2.0],
                ["new", 42004.0, 1.5], ["new", 42004.5, 1.0], ["new", 42005.0, 0.8],
                ["new", 42005.5, 0.5]
            ]
        }
    }
}"#;

/// OANDA price update.
const OANDA_PRICE: &str = r#"{
    "type": "PRICE",
    "time": "2023-12-29T12:00:00.123456789Z",
    "instrument": "EUR_USD",
    "bids": [
        {"price": "1.10500", "liquidity": 1000000},
        {"price": "1.10495", "liquidity": 2000000}
    ],
    "asks": [
        {"price": "1.10505", "liquidity": 1000000},
        {"price": "1.10510", "liquidity": 2000000}
    ]
}"#;

// =============================================================================
// HOTSPOT 1: JSON PARSING (TYPICALLY 30-40% OF CPU)
// =============================================================================

/// Profile JSON parsing operations - this is often the biggest CPU hotspot.
fn profile_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot/json_parsing");
    group.measurement_time(Duration::from_secs(5));

    // Raw serde_json parsing (baseline)
    group.bench_function("serde_json_raw_small", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(DERIBIT_DELTA)).unwrap();
            black_box(v)
        });
    });

    group.bench_function("serde_json_raw_medium", |b| {
        b.iter(|| {
            let v: serde_json::Value =
                serde_json::from_str(black_box(DERIBIT_SNAPSHOT_10)).unwrap();
            black_box(v)
        });
    });

    // Adapter parsing (includes type conversion)
    let deribit = DeribitAdapter::default();

    group.bench_function("deribit_parse_delta", |b| {
        b.iter(|| black_box(deribit.parse_message(DERIBIT_DELTA)));
    });

    group.bench_function("deribit_parse_snapshot_10", |b| {
        b.iter(|| black_box(deribit.parse_message(DERIBIT_SNAPSHOT_10)));
    });

    let oanda = OandaAdapter::new("test");

    group.bench_function("oanda_parse_price", |b| {
        b.iter(|| black_box(oanda.parse_message(OANDA_PRICE)));
    });

    group.finish();
}

// =============================================================================
// HOTSPOT 2: ORDER BOOK OPERATIONS (TYPICALLY 20-30% OF CPU)
// =============================================================================

/// Profile order book operations.
fn profile_orderbook_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot/orderbook");
    group.measurement_time(Duration::from_secs(5));

    // Single level update (most frequent operation)
    group.bench_function("update_single_level", |b| {
        let mut book = create_book(50);
        let update = level(49999.0, dec!(5), 0);
        b.iter(|| book.update_level(black_box(Side::Bid), black_box(update.clone())));
    });

    // Best bid/ask lookup (called frequently)
    let book = create_book(50);
    group.bench_function("best_bid_lookup", |b| {
        b.iter(|| black_box(book.best_bid()));
    });

    group.bench_function("mid_price_calc", |b| {
        b.iter(|| black_box(book.mid_price()));
    });

    group.bench_function("spread_bps_calc", |b| {
        b.iter(|| black_box(book.spread_bps()));
    });

    // Snapshot creation (moderately expensive)
    for depth in [10, 25, 50] {
        group.bench_with_input(BenchmarkId::new("snapshot", depth), &depth, |b, &depth| {
            b.iter(|| black_box(book.to_snapshot(depth)));
        });
    }

    group.finish();
}

// =============================================================================
// HOTSPOT 3: SERIALIZATION (TYPICALLY 15-25% OF CPU)
// =============================================================================

/// Profile serialization operations.
fn profile_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot/serialization");
    group.measurement_time(Duration::from_secs(5));

    let book = create_book(50);
    let snapshot = book.to_snapshot(50);

    // JSON serialization (output for Gateway)
    group.bench_function("json_serialize_50", |b| {
        b.iter(|| black_box(serde_json::to_vec(&snapshot).unwrap()));
    });

    // Bincode serialization (internal)
    group.bench_function("bincode_serialize_50", |b| {
        b.iter(|| black_box(bincode::serialize(&snapshot).unwrap()));
    });

    // JSON deserialization
    let json_bytes = serde_json::to_vec(&snapshot).unwrap();
    group.bench_function("json_deserialize_50", |b| {
        b.iter(|| black_box(serde_json::from_slice::<BookSnapshot>(&json_bytes).unwrap()));
    });

    // Smaller snapshot
    let small_snapshot = book.to_snapshot(10);
    group.bench_function("json_serialize_10", |b| {
        b.iter(|| black_box(serde_json::to_vec(&small_snapshot).unwrap()));
    });

    group.bench_function("bincode_serialize_10", |b| {
        b.iter(|| black_box(bincode::serialize(&small_snapshot).unwrap()));
    });

    group.finish();
}

// =============================================================================
// HOTSPOT 4: THREAD-SAFE OPERATIONS (LOCK OVERHEAD)
// =============================================================================

/// Profile thread-safe operations to measure lock overhead.
fn profile_thread_safe(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot/thread_safe");
    group.measurement_time(Duration::from_secs(5));

    // Compare raw vs thread-safe overhead
    let book = create_book(50);
    let ts_book = create_ts_book(50);

    group.bench_function("raw_best_bid", |b| {
        b.iter(|| black_box(book.best_bid()));
    });

    group.bench_function("ts_best_bid", |b| {
        b.iter(|| black_box(ts_book.best_bid()));
    });

    group.bench_function("raw_snapshot_50", |b| {
        b.iter(|| black_box(book.to_snapshot(50)));
    });

    group.bench_function("ts_snapshot_50", |b| {
        b.iter(|| black_box(ts_book.snapshot(50)));
    });

    group.finish();
}

// =============================================================================
// ALLOCATION HOTSPOTS
// =============================================================================

/// Profile operations that cause heap allocations.
fn profile_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocs/operations");
    group.measurement_time(Duration::from_secs(5));

    // String allocations in parsing
    let deribit = DeribitAdapter::default();
    group.bench_function("parse_with_string_allocs", |b| {
        b.iter(|| {
            let events = deribit.parse_message(DERIBIT_SNAPSHOT_10).unwrap();
            black_box(events)
        });
    });

    // Vec allocations in snapshot
    let book = create_book(50);
    group.bench_function("snapshot_vec_allocs", |b| {
        b.iter(|| {
            let snapshot = book.to_snapshot(50);
            black_box(snapshot)
        });
    });

    // Clone operations
    let snapshot = book.to_snapshot(50);
    group.bench_function("snapshot_clone", |b| {
        b.iter(|| black_box(snapshot.clone()));
    });

    // Level clone
    let lvl = level(50000.0, dec!(100), 0);
    group.bench_function("level_clone", |b| {
        b.iter(|| black_box(lvl.clone()));
    });

    group.finish();
}

// =============================================================================
// END-TO-END PIPELINE PROFILING
// =============================================================================

/// Profile the complete message processing pipeline.
fn profile_pipeline(c: &mut Criterion) {
    use astra_flash::core::types::MarketData;

    let mut group = c.benchmark_group("pipeline/e2e");
    group.measurement_time(Duration::from_secs(10));

    // Full pipeline: parse -> update book -> snapshot -> serialize
    group.bench_function("full_pipeline_deribit", |b| {
        let deribit = DeribitAdapter::default();
        let mut book = create_book(50);

        b.iter(|| {
            // 1. Parse incoming message
            let events = deribit.parse_message(DERIBIT_DELTA).unwrap();

            // 2. Apply to order book
            for event in &events {
                if let MarketData::Book { bids, asks } = &event.data {
                    for level in bids {
                        book.update_level(Side::Bid, level.clone());
                    }
                    for level in asks {
                        book.update_level(Side::Ask, level.clone());
                    }
                }
            }

            // 3. Create snapshot
            let snapshot = book.to_snapshot(50);

            // 4. Serialize for Redis
            let bytes = bincode::serialize(&snapshot).unwrap();

            black_box(bytes.len())
        });
    });

    group.bench_function("full_pipeline_oanda", |b| {
        let oanda = OandaAdapter::new("test");
        let mut book = OrderBook::new(
            Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD"),
            OrderBookConfig::default(),
        );

        b.iter(|| {
            // 1. Parse incoming message
            let events = oanda.parse_message(OANDA_PRICE).unwrap();

            // 2. Apply to order book
            for event in &events {
                if let MarketData::Book { bids, asks } = &event.data {
                    for level in bids {
                        book.update_level(Side::Bid, level.clone());
                    }
                    for level in asks {
                        book.update_level(Side::Ask, level.clone());
                    }
                }
            }

            // 3. Create snapshot
            let snapshot = book.to_snapshot(10);

            // 4. Serialize for Redis
            let bytes = bincode::serialize(&snapshot).unwrap();

            black_box(bytes.len())
        });
    });

    group.finish();
}

// =============================================================================
// THROUGHPUT PROFILING
// =============================================================================

/// Profile throughput to find scaling bottlenecks.
fn profile_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput/scaling");
    group.measurement_time(Duration::from_secs(10));

    // Measure throughput at different batch sizes
    for batch_size in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("updates", batch_size),
            &batch_size,
            |b, &size| {
                let updates: Vec<(Side, PriceLevel)> = (0..size)
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

                let mut book = create_book(50);

                b.iter(|| {
                    for (side, lvl) in &updates {
                        book.update_level(*side, lvl.clone());
                    }
                });
            },
        );
    }

    // Parsing throughput
    for batch_size in [100, 1_000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        let deribit = DeribitAdapter::default();

        group.bench_with_input(
            BenchmarkId::new("parses", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for _ in 0..size {
                        black_box(deribit.parse_message(DERIBIT_DELTA));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// CPU CACHE BEHAVIOR
// =============================================================================

/// Profile operations that may have cache implications.
fn profile_cache_behavior(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/behavior");
    group.measurement_time(Duration::from_secs(5));

    // Sequential vs random access patterns
    let book = create_book(100);

    // Sequential iteration (cache-friendly)
    group.bench_function("sequential_iteration", |b| {
        b.iter(|| {
            let bids = book.top_bids(100);
            for bid in bids {
                black_box(bid.price);
            }
        });
    });

    // Repeated access to same data (should be cache-hot)
    group.bench_function("repeated_best_bid", |b| {
        b.iter(|| {
            for _ in 0..100 {
                black_box(book.best_bid());
            }
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    name = hotspot_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));
    targets =
        profile_json_parsing,
        profile_orderbook_ops,
        profile_serialization,
        profile_thread_safe
);

criterion_group!(
    name = alloc_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));
    targets = profile_allocations
);

criterion_group!(
    name = pipeline_benches;
    config = Criterion::default()
        .sample_size(50)
        .measurement_time(Duration::from_secs(10));
    targets = profile_pipeline
);

criterion_group!(
    name = throughput_benches;
    config = Criterion::default()
        .sample_size(50)
        .measurement_time(Duration::from_secs(10));
    targets = profile_throughput
);

criterion_group!(
    name = cache_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));
    targets = profile_cache_behavior
);

criterion_main!(
    hotspot_benches,
    alloc_benches,
    pipeline_benches,
    throughput_benches,
    cache_benches
);
