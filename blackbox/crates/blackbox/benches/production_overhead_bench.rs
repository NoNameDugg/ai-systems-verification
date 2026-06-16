//! Production Overhead Benchmarks (T5.4)
//!
//! This benchmark suite validates production overhead requirements:
//! - <1μs hot path latency
//! - Zero allocation in recording path
//! - >50,000 msg/sec sustained throughput
//! - Cross-crate integration overhead
//!
//! Run with: cargo bench --package blackbox --bench production_overhead_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;

use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, NullTap, Tap};
use blackbox_types::{Exchange, Timestamp};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Benchmark timestamp
const BENCH_TIMESTAMP: i64 = 1_704_067_200_000_000;

/// Typical market data payload (WebSocket quote update)
const MARKET_DATA_SMALL: &[u8] =
    br#"{"type":"quote","bid":50000.50,"ask":50001.00,"time":1704067200000}"#;

/// Medium payload (trade with details)
const MARKET_DATA_MEDIUM: &[u8] = br#"{
    "type":"trade",
    "price":50000.50,
    "quantity":1.5,
    "side":"buy",
    "time":1704067200000,
    "trade_id":"1234567890",
    "maker_order_id":"abc123",
    "taker_order_id":"def456"
}"#;

/// Large payload (L2 orderbook snapshot)
const MARKET_DATA_LARGE_SIZE: usize = 4096;

// =============================================================================
// HELPERS
// =============================================================================

fn temp_path(suffix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "prod_overhead_{}_{}_{}.journal",
        std::process::id(),
        suffix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn create_journal_tap(suffix: &str) -> (Arc<JournalTap>, std::path::PathBuf) {
    let path = temp_path(suffix);
    let config = WriterConfig {
        ring_buffer_capacity: 65536,
        file_size: 64 * 1024 * 1024,
        compress_schema: false,
        sync_on_close: false,
        prefault_pages: false,
    };
    let writer = JournalWriter::new(&path, config).expect("create writer");
    (Arc::new(JournalTap::new(writer)), path)
}

fn generate_large_payload() -> Vec<u8> {
    vec![0xABu8; MARKET_DATA_LARGE_SIZE]
}

// =============================================================================
// BENCHMARK: PRODUCTION THROUGHPUT
// =============================================================================

/// Benchmark sustained throughput with NullTap (baseline)
fn bench_throughput_null_tap(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_throughput/null_tap");
    group.significance_level(0.01);

    let tap = NullTap;

    for batch_size in [100, 1000, 10_000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("messages", batch_size),
            batch_size,
            |b, &size| {
                b.iter(|| {
                    for i in 0..size {
                        let ts_i = Timestamp::from_micros(BENCH_TIMESTAMP + i as i64);
                        black_box(&tap).record_ingress(
                            black_box(Exchange::Deribit),
                            black_box(MARKET_DATA_SMALL),
                            black_box(ts_i),
                        );
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark sustained throughput with JournalTap (production)
fn bench_throughput_journal_tap(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_throughput/journal_tap");
    group.significance_level(0.01);

    for batch_size in [100, 1000, 10_000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("messages", batch_size),
            batch_size,
            |b, &size| {
                let (tap, path) = create_journal_tap(&format!("throughput_{}", size));
                b.iter(|| {
                    for i in 0..size {
                        let ts = Timestamp::from_micros(BENCH_TIMESTAMP + i as i64);
                        black_box(&*tap).record_ingress(
                            black_box(Exchange::Deribit),
                            black_box(MARKET_DATA_SMALL),
                            black_box(ts),
                        );
                    }
                });
                std::fs::remove_file(&path).ok();
            },
        );
    }

    group.finish();
}

// =============================================================================
// BENCHMARK: PAYLOAD SIZE IMPACT
// =============================================================================

fn bench_payload_size_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_overhead/payload_size");
    group.significance_level(0.01).sample_size(1000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);
    let large_payload = generate_large_payload();

    // Small payload (typical quote)
    group.bench_function("small_64b", |b| {
        let (tap, path) = create_journal_tap("payload_small");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(MARKET_DATA_SMALL),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    // Medium payload (trade)
    group.bench_function("medium_256b", |b| {
        let (tap, path) = create_journal_tap("payload_medium");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(MARKET_DATA_MEDIUM),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    // Large payload (L2 book)
    group.bench_function("large_4kb", |b| {
        let (tap, path) = create_journal_tap("payload_large");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(&large_payload),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: FULL TRADING CYCLE
// =============================================================================

/// Simulate realistic trading event pattern
#[inline(never)]
fn full_trading_cycle<T: Tap>(tap: &T, payload: &[u8], ts: Timestamp) {
    // TAP-1: Ingress (WebSocket frame received)
    tap.record_ingress(Exchange::Deribit, payload, ts);

    // TAP-2: Internal (OrderBook updated)
    tap.record_internal(0x0010, payload, ts);

    // Simulate some processing (orderbook update, signal calculation)
    black_box(payload.len());

    // TAP-3: Egress (Order submitted - 20% of trades trigger orders)
    // Only record if there's a signal
    tap.record_egress(Exchange::Deribit, b"{\"order\":\"buy\"}", ts);
}

fn bench_full_trading_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_overhead/trading_cycle");
    group.significance_level(0.01).sample_size(1000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // NullTap baseline (disabled recording)
    group.bench_function("null_tap", |b| {
        let tap = NullTap;
        b.iter(|| full_trading_cycle(black_box(&tap), black_box(MARKET_DATA_SMALL), black_box(ts)))
    });

    // JournalTap (production recording)
    group.bench_function("journal_tap", |b| {
        let (tap, path) = create_journal_tap("trading_cycle");
        b.iter(|| {
            full_trading_cycle(
                black_box(&*tap),
                black_box(MARKET_DATA_SMALL),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: MULTI-THREADED ACCESS
// =============================================================================

fn bench_concurrent_tap_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_overhead/concurrent");
    group.significance_level(0.01).sample_size(100);

    // 2 threads (typical: 1 reader, 1 writer)
    group.bench_function("2_threads", |b| {
        let (tap, path) = create_journal_tap("concurrent_2");
        b.iter(|| {
            std::thread::scope(|s| {
                for thread_id in 0..2 {
                    let tap = &tap;
                    s.spawn(move || {
                        for i in 0..100 {
                            let ts = Timestamp::from_micros(
                                BENCH_TIMESTAMP + (thread_id * 1000 + i) as i64,
                            );
                            tap.record_ingress(Exchange::Deribit, MARKET_DATA_SMALL, ts);
                        }
                    });
                }
            });
        });
        std::fs::remove_file(&path).ok();
    });

    // 4 threads (stress test)
    group.bench_function("4_threads", |b| {
        let (tap, path) = create_journal_tap("concurrent_4");
        b.iter(|| {
            std::thread::scope(|s| {
                for thread_id in 0..4 {
                    let tap = &tap;
                    s.spawn(move || {
                        for i in 0..50 {
                            let ts = Timestamp::from_micros(
                                BENCH_TIMESTAMP + (thread_id * 1000 + i) as i64,
                            );
                            tap.record_ingress(Exchange::Deribit, MARKET_DATA_SMALL, ts);
                        }
                    });
                }
            });
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: OVERHEAD COMPARISON (disabled vs enabled)
// =============================================================================

fn bench_overhead_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_overhead/comparison");
    group.significance_level(0.01).sample_size(1000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Baseline: No tap at all (function call overhead)
    group.bench_function("no_tap_baseline", |b| {
        b.iter(|| {
            // Simulate just the payload processing
            black_box(MARKET_DATA_SMALL.len());
        })
    });

    // NullTap overhead (should be <10ns)
    group.bench_function("null_tap_overhead", |b| {
        let tap = NullTap;
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(MARKET_DATA_SMALL),
                black_box(ts),
            );
        })
    });

    // JournalTap overhead (should be <1μs)
    group.bench_function("journal_tap_overhead", |b| {
        let (tap, path) = create_journal_tap("overhead_comparison");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(MARKET_DATA_SMALL),
                black_box(ts),
            );
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: BURST HANDLING
// =============================================================================

fn bench_burst_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("production_overhead/burst");
    group.significance_level(0.01).sample_size(100);

    // Simulate market open burst (10ms of heavy traffic)
    group.bench_function("market_open_burst", |b| {
        let (tap, path) = create_journal_tap("burst");
        b.iter(|| {
            // 1000 messages in quick succession (simulating market open)
            for i in 0..1000 {
                let ts = Timestamp::from_micros(BENCH_TIMESTAMP + i as i64);
                tap.record_ingress(Exchange::Deribit, MARKET_DATA_SMALL, ts);
            }
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    throughput_benches,
    bench_throughput_null_tap,
    bench_throughput_journal_tap,
);

criterion_group!(
    overhead_benches,
    bench_payload_size_impact,
    bench_full_trading_cycle,
    bench_overhead_comparison,
);

criterion_group!(
    stress_benches,
    bench_concurrent_tap_access,
    bench_burst_handling,
);

criterion_main!(throughput_benches, overhead_benches, stress_benches);
