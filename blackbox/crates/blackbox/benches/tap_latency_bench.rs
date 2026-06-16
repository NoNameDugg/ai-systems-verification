//! Tap Latency Benchmarks (T2.8)
//!
//! This benchmark suite measures the latency impact of tap integration
//! to verify the <100ns overhead target for Phase 2.
//!
//! Run with: cargo bench --package blackbox --bench tap_latency_bench
//!
//! ## Performance Targets
//!
//! | Tap Type | Target | Description |
//! |----------|--------|-------------|
//! | NullTap | <10ns | Should be fully inlined to no-op |
//! | JournalTap (ring buffer) | <100ns | Write to lock-free ring buffer |
//! | Feature flag overhead | <1ns | Compile-time branch elimination |
//!
//! ## Benchmark Categories
//!
//! 1. **Isolation benchmarks**: Tap methods called in isolation
//! 2. **Realistic workload**: Simulated trading patterns
//! 3. **Concurrent access**: Multi-threaded tap usage
//! 4. **Payload size impact**: Different payload sizes (0B to 4KB)
//! 5. **Comparison**: No tap vs NullTap vs JournalTap

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::thread;

use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, NullTap, Tap};
use blackbox_types::{Exchange, Timestamp};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Timestamp for benchmarks (2024-01-01 00:00:00 UTC)
const BENCH_TIMESTAMP: i64 = 1_704_067_200_000_000;

/// Typical WebSocket message size (market data quote)
const TYPICAL_MARKET_DATA: &[u8] =
    br#"{"type":"quote","bid":50000.50,"ask":50001.00,"time":1704067200000}"#;

/// Large payload (L2 orderbook snapshot)
const LARGE_PAYLOAD_SIZE: usize = 4096;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Create a temporary journal file path for benchmarks.
fn temp_journal_path(suffix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "tap_latency_bench_{}_{}_{}.journal",
        std::process::id(),
        suffix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Create a JournalTap for benchmarking with minimal overhead.
fn create_benchmark_journal_tap(suffix: &str) -> (Arc<JournalTap>, std::path::PathBuf) {
    let path = temp_journal_path(suffix);
    let config = WriterConfig {
        ring_buffer_capacity: 65536, // Large buffer to avoid blocking
        file_size: 64 * 1024 * 1024, // 64MB file
        compress_schema: false,      // No compression overhead
        sync_on_close: false,        // No sync overhead
        prefault_pages: false,       // No prefault overhead
    };
    let writer = JournalWriter::new(&path, config).expect("create writer");
    (Arc::new(JournalTap::new(writer)), path)
}

/// Simulate a realistic trading event (ingress + internal + egress pattern).
#[inline(never)]
fn simulate_trading_event<T: Tap>(tap: &T, payload: &[u8], ts: Timestamp) {
    // Ingress: Market data received
    tap.record_ingress(Exchange::Deribit, payload, ts);

    // Internal: State change (book update)
    tap.record_internal(0x0010, payload, ts);
}

/// Simulate a trading event with order submission.
#[inline(never)]
fn simulate_trading_event_with_order<T: Tap>(tap: &T, payload: &[u8], ts: Timestamp) {
    // Ingress: Market data received
    tap.record_ingress(Exchange::Deribit, payload, ts);

    // Internal: State change
    tap.record_internal(0x0010, payload, ts);

    // Egress: Order submission
    tap.record_egress(Exchange::Deribit, b"{\"order\":\"buy\"}", ts);
}

// =============================================================================
// BENCHMARK: NULL TAP OVERHEAD (Target: <10ns)
// =============================================================================

fn bench_null_tap_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("null_tap_overhead");
    group.significance_level(0.01).sample_size(10000);

    let tap = NullTap;
    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Benchmark: is_active check (should be compiled away)
    group.bench_function("is_active", |b| {
        b.iter(|| black_box(black_box(&tap).is_active()))
    });

    // Benchmark: record_ingress (no-op)
    group.bench_function("record_ingress", |b| {
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Benchmark: record_internal (no-op)
    group.bench_function("record_internal", |b| {
        b.iter(|| {
            black_box(&tap).record_internal(
                black_box(0x0010),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Benchmark: record_egress (no-op)
    group.bench_function("record_egress", |b| {
        b.iter(|| {
            black_box(&tap).record_egress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Benchmark: record_checkpoint (no-op)
    let hash = [0u8; 32];
    group.bench_function("record_checkpoint", |b| {
        b.iter(|| black_box(&tap).record_checkpoint(black_box(&hash), black_box(ts)))
    });

    // Benchmark: Full trading cycle (ingress + internal)
    group.bench_function("trading_cycle", |b| {
        b.iter(|| {
            simulate_trading_event(
                black_box(&tap),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Benchmark: Full trading cycle with order
    group.bench_function("trading_cycle_with_order", |b| {
        b.iter(|| {
            simulate_trading_event_with_order(
                black_box(&tap),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: JOURNAL TAP OVERHEAD (Target: <100ns)
// =============================================================================

fn bench_journal_tap_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_tap_overhead");
    group.significance_level(0.01).sample_size(1000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Benchmark: is_active check (mutex-free)
    group.bench_function("is_active", |b| {
        let (tap, path) = create_benchmark_journal_tap("is_active");
        b.iter(|| black_box(black_box(&*tap).is_active()));
        std::fs::remove_file(&path).ok();
    });

    // Benchmark: record_ingress (ring buffer write)
    group.bench_function("record_ingress", |b| {
        let (tap, path) = create_benchmark_journal_tap("ingress");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    // Benchmark: record_internal (ring buffer write)
    group.bench_function("record_internal", |b| {
        let (tap, path) = create_benchmark_journal_tap("internal");
        b.iter(|| {
            tap.record_internal(
                black_box(0x0010),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    // Benchmark: record_egress (ring buffer write)
    group.bench_function("record_egress", |b| {
        let (tap, path) = create_benchmark_journal_tap("egress");
        b.iter(|| {
            tap.record_egress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    // Benchmark: record_checkpoint (ring buffer write)
    let hash = [0u8; 32];
    group.bench_function("record_checkpoint", |b| {
        let (tap, path) = create_benchmark_journal_tap("checkpoint");
        b.iter(|| tap.record_checkpoint(black_box(&hash), black_box(ts)));
        std::fs::remove_file(&path).ok();
    });

    // Benchmark: Full trading cycle (ingress + internal)
    group.bench_function("trading_cycle", |b| {
        let (tap, path) = create_benchmark_journal_tap("cycle");
        b.iter(|| {
            simulate_trading_event(
                black_box(&*tap),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: PAYLOAD SIZE IMPACT
// =============================================================================

fn bench_payload_size_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("payload_size_impact");
    group.significance_level(0.01).sample_size(500);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Test various payload sizes
    let payload_sizes = [0, 64, 256, 1024, LARGE_PAYLOAD_SIZE];

    for size in payload_sizes.iter() {
        let payload = vec![0x42u8; *size];

        // NullTap - should not change with payload size
        group.bench_with_input(
            BenchmarkId::new("null_tap", size),
            &payload,
            |b, payload| {
                let tap = NullTap;
                b.iter(|| {
                    black_box(&tap).record_ingress(
                        black_box(Exchange::Deribit),
                        black_box(payload),
                        black_box(ts),
                    )
                })
            },
        );

        // JournalTap - may increase with payload size (due to payload.to_vec())
        group.bench_with_input(
            BenchmarkId::new("journal_tap", size),
            &payload,
            |b, payload| {
                let (tap, path) = create_benchmark_journal_tap(&format!("payload_{}", size));
                b.iter(|| {
                    tap.record_ingress(
                        black_box(Exchange::Deribit),
                        black_box(payload),
                        black_box(ts),
                    )
                });
                std::fs::remove_file(&path).ok();
            },
        );
    }

    group.finish();
}

// =============================================================================
// BENCHMARK: CONCURRENT ACCESS
// =============================================================================

fn bench_concurrent_tap(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_tap");
    group.significance_level(0.01).sample_size(100);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);
    const OPS_PER_THREAD: usize = 10_000;

    // Benchmark: NullTap with concurrent access (should remain near-zero)
    group.throughput(Throughput::Elements((OPS_PER_THREAD * 4) as u64));
    group.bench_function("null_tap_4_threads", |b| {
        let tap = Arc::new(NullTap);
        b.iter(|| {
            let mut handles = vec![];
            for _ in 0..4 {
                let tap_clone = Arc::clone(&tap);
                handles.push(thread::spawn(move || {
                    for _ in 0..OPS_PER_THREAD {
                        tap_clone.record_ingress(Exchange::Deribit, TYPICAL_MARKET_DATA, ts);
                    }
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
        })
    });

    // Benchmark: JournalTap with concurrent access
    group.bench_function("journal_tap_4_threads", |b| {
        let (tap, path) = create_benchmark_journal_tap("concurrent");
        b.iter(|| {
            let mut handles = vec![];
            for _ in 0..4 {
                let tap_clone = Arc::clone(&tap);
                handles.push(thread::spawn(move || {
                    for _ in 0..OPS_PER_THREAD {
                        tap_clone.record_ingress(Exchange::Deribit, TYPICAL_MARKET_DATA, ts);
                    }
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: REALISTIC TRADING WORKLOAD
// =============================================================================

fn bench_realistic_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_workload");
    group.significance_level(0.01).sample_size(100);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Simulate 1 second of trading at 1000 msg/sec
    // Pattern: 100 market data, 10 book updates, 1 order per 100 ticks
    const TICKS_PER_SECOND: u64 = 1000;
    group.throughput(Throughput::Elements(TICKS_PER_SECOND));

    let market_data = br#"{"bid":50000.50,"ask":50001.00}"#;
    let book_state = br#"{"bids":[[50000,1.5]],"asks":[[50001,2.0]]}"#;
    let order = br#"{"type":"limit","side":"buy","qty":0.1}"#;
    let hash = [0x42u8; 32];

    // Benchmark: Realistic workload with NullTap
    group.bench_function("null_tap_1s_workload", |b| {
        let tap = NullTap;
        b.iter(|| {
            for i in 0..TICKS_PER_SECOND {
                // Every tick: market data ingress
                tap.record_ingress(Exchange::Deribit, market_data, ts);

                // Every 10 ticks: book update
                if i % 10 == 0 {
                    tap.record_internal(0x0010, book_state, ts);
                }

                // Every 100 ticks: order submission
                if i % 100 == 0 {
                    tap.record_egress(Exchange::Deribit, order, ts);
                }

                // Every 1000 ticks: checkpoint
                if i % 1000 == 0 {
                    tap.record_checkpoint(&hash, ts);
                }
            }
        })
    });

    // Benchmark: Realistic workload with JournalTap
    group.bench_function("journal_tap_1s_workload", |b| {
        let (tap, path) = create_benchmark_journal_tap("realistic");
        b.iter(|| {
            for i in 0..TICKS_PER_SECOND {
                // Every tick: market data ingress
                tap.record_ingress(Exchange::Deribit, market_data, ts);

                // Every 10 ticks: book update
                if i % 10 == 0 {
                    tap.record_internal(0x0010, book_state, ts);
                }

                // Every 100 ticks: order submission
                if i % 100 == 0 {
                    tap.record_egress(Exchange::Deribit, order, ts);
                }

                // Every 1000 ticks: checkpoint
                if i % 1000 == 0 {
                    tap.record_checkpoint(&hash, ts);
                }
            }
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: DYN TRAIT OVERHEAD
// =============================================================================

fn bench_dyn_trait_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("dyn_trait_overhead");
    group.significance_level(0.01).sample_size(5000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Direct call (monomorphized)
    group.bench_function("null_tap_direct", |b| {
        let tap = NullTap;
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Via Box<dyn Tap> (vtable lookup)
    group.bench_function("null_tap_box_dyn", |b| {
        let tap: Box<dyn Tap> = Box::new(NullTap);
        b.iter(|| {
            black_box(&*tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Via Arc<dyn Tap> (vtable lookup + Arc)
    group.bench_function("null_tap_arc_dyn", |b| {
        let tap: Arc<dyn Tap> = Arc::new(NullTap);
        b.iter(|| {
            black_box(&*tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Generic function (monomorphized)
    fn record_generic<T: Tap>(tap: &T, payload: &[u8], ts: Timestamp) {
        tap.record_ingress(Exchange::Deribit, payload, ts);
    }
    group.bench_function("null_tap_generic_fn", |b| {
        let tap = NullTap;
        b.iter(|| {
            record_generic(
                black_box(&tap),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: FEATURE FLAG SIMULATION
// =============================================================================

fn bench_feature_flag_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("feature_flag_pattern");
    group.significance_level(0.01).sample_size(5000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Pattern 1: Conditional tap (checking is_active)
    group.bench_function("conditional_null_tap", |b| {
        let tap = NullTap;
        b.iter(|| {
            if tap.is_active() {
                tap.record_ingress(Exchange::Deribit, TYPICAL_MARKET_DATA, ts);
            }
        })
    });

    // Pattern 2: Always call tap (tap does the check internally)
    group.bench_function("unconditional_null_tap", |b| {
        let tap = NullTap;
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // Pattern 3: Option<Tap> pattern (None case)
    group.bench_function("option_none_tap", |b| {
        let tap: Option<NullTap> = None;
        b.iter(|| {
            if let Some(ref t) = tap {
                t.record_ingress(Exchange::Deribit, TYPICAL_MARKET_DATA, ts);
            }
        })
    });

    // Pattern 4: Option<Tap> pattern (Some case with NullTap)
    group.bench_function("option_some_null_tap", |b| {
        let tap: Option<NullTap> = Some(NullTap);
        b.iter(|| {
            if let Some(ref t) = tap {
                t.record_ingress(
                    black_box(Exchange::Deribit),
                    black_box(TYPICAL_MARKET_DATA),
                    black_box(ts),
                );
            }
        })
    });

    group.finish();
}

// =============================================================================
// BENCHMARK: EXCHANGE VARIANT OVERHEAD
// =============================================================================

fn bench_exchange_variants(c: &mut Criterion) {
    let mut group = c.benchmark_group("exchange_variants");
    group.significance_level(0.01).sample_size(5000);

    let tap = NullTap;
    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // Test all Exchange variants
    for exchange in [
        Exchange::Unknown,
        Exchange::Deribit,
        Exchange::Binance,
        Exchange::Bybit,
        Exchange::OKX,
    ] {
        group.bench_with_input(
            BenchmarkId::new("null_tap", format!("{:?}", exchange)),
            &exchange,
            |b, &exchange| {
                b.iter(|| {
                    black_box(&tap).record_ingress(
                        black_box(exchange),
                        black_box(TYPICAL_MARKET_DATA),
                        black_box(ts),
                    )
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// BENCHMARK: LATENCY PERCENTILE ESTIMATION
// =============================================================================

fn bench_latency_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_distribution");
    // Use 10000 samples for better percentile estimation
    group.significance_level(0.01).sample_size(10000);

    let ts = Timestamp::from_micros(BENCH_TIMESTAMP);

    // NullTap: Should have very tight distribution near 0
    group.bench_function("null_tap_p99", |b| {
        let tap = NullTap;
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        })
    });

    // JournalTap: Distribution depends on lock contention and buffer state
    group.bench_function("journal_tap_p99", |b| {
        let (tap, path) = create_benchmark_journal_tap("p99");
        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(TYPICAL_MARKET_DATA),
                black_box(ts),
            )
        });
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    name = null_tap_benches;
    config = Criterion::default();
    targets = bench_null_tap_overhead
);

criterion_group!(
    name = journal_tap_benches;
    config = Criterion::default();
    targets = bench_journal_tap_overhead
);

criterion_group!(
    name = comparison_benches;
    config = Criterion::default();
    targets =
        bench_payload_size_impact,
        bench_concurrent_tap,
        bench_realistic_workload,
        bench_dyn_trait_overhead,
        bench_feature_flag_pattern,
        bench_exchange_variants,
        bench_latency_distribution
);

criterion_main!(null_tap_benches, journal_tap_benches, comparison_benches);
