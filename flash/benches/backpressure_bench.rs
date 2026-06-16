//! Benchmarks for Backpressure module (Batch 3.3).
//!
//! Run with: `cargo bench --bench backpressure_bench`
//!
//! These benchmarks measure:
//! - try_send performance (non-blocking)
//! - Throughput under backpressure
//! - Metrics update overhead
//! - Flood scenario handling

use astra_flash::publisher::backpressure::{
    BackpressureConfig, BackpressureMetrics, BackpressureSender, send_with_backpressure,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Create a test runtime for async benchmarks.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

// =============================================================================
// METRICS BENCHMARKS
// =============================================================================

/// Benchmark BackpressureMetrics increment operations.
fn bench_metrics_increment(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_increment");

    let metrics = BackpressureMetrics::default();

    group.bench_function("increment_sent", |b| {
        b.iter(|| {
            metrics.increment_sent();
        });
    });

    group.bench_function("increment_dropped", |b| {
        b.iter(|| {
            metrics.increment_dropped();
        });
    });

    group.finish();
}

/// Benchmark drop_rate calculation.
fn bench_metrics_drop_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_drop_rate");

    // Pre-populate metrics
    let metrics = BackpressureMetrics::default();
    for _ in 0..1000 {
        metrics.increment_sent();
    }
    for _ in 0..100 {
        metrics.increment_dropped();
    }

    group.bench_function("drop_rate_calculation", |b| {
        b.iter(|| {
            let rate = black_box(metrics.drop_rate());
            black_box(rate)
        });
    });

    group.bench_function("exceeds_alert_threshold", |b| {
        b.iter(|| {
            let exceeds = black_box(metrics.exceeds_alert_threshold(0.01));
            black_box(exceeds)
        });
    });

    group.finish();
}

// =============================================================================
// TRY_SEND BENCHMARKS
// =============================================================================

/// Benchmark try_send under normal conditions (channel not full).
fn bench_try_send_normal(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("try_send_normal");
    group.measurement_time(Duration::from_secs(5));

    for capacity in [100, 1000, 10_000].iter() {
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::from_parameter(capacity),
            capacity,
            |b, &cap| {
                let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(cap);

                // Spawn a consumer to drain the channel
                let consumer = rt.spawn(async move {
                    while receiver.recv().await.is_some() {}
                });

                b.iter(|| {
                    let _ = black_box(sender.try_send(black_box(42)));
                });

                // Cleanup
                drop(sender);
                rt.block_on(consumer).unwrap();
            },
        );
    }

    group.finish();
}

/// Benchmark try_send under backpressure (channel full).
fn bench_try_send_backpressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("try_send_backpressure");
    group.measurement_time(Duration::from_secs(3));

    // Disable warnings for this benchmark since we're intentionally dropping
    let config = BackpressureConfig::builder()
        .channel_capacity(10)
        .warn_on_drop(false)
        .build();

    let (sender, _receiver) = BackpressureSender::<u64>::new(config);

    // Fill the channel
    for i in 0..10 {
        let _ = sender.try_send(i);
    }

    group.bench_function("try_send_full_channel", |b| {
        b.iter(|| {
            // This will always fail (channel full) but should still be fast
            let _ = black_box(sender.try_send(black_box(99)));
        });
    });

    group.finish();
}

// =============================================================================
// SEND_WITH_BACKPRESSURE BENCHMARKS
// =============================================================================

/// Benchmark the standalone send_with_backpressure function.
fn bench_send_with_backpressure_fn(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("send_with_backpressure");
    group.measurement_time(Duration::from_secs(5));

    let (tx, mut rx) = mpsc::channel::<u64>(1000);
    let metrics = Arc::new(BackpressureMetrics::default());

    // Spawn consumer
    let consumer = rt.spawn(async move {
        while rx.recv().await.is_some() {}
    });

    let metrics_clone = Arc::clone(&metrics);
    group.bench_function("normal_send", |b| {
        b.iter(|| {
            send_with_backpressure(&tx, black_box(42), &metrics_clone);
        });
    });

    drop(tx);
    rt.block_on(consumer).unwrap();

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark sustained throughput under various conditions.
fn bench_throughput(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("throughput");
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(1000));

    for capacity in [100, 1000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("burst_1000", capacity),
            capacity,
            |b, &cap| {
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;

                    for _ in 0..iters {
                        let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(cap);

                        // Spawn consumer
                        let consumer = rt.spawn(async move {
                            while receiver.recv().await.is_some() {}
                        });

                        let start = std::time::Instant::now();

                        // Send 1000 messages
                        for i in 0..1000_u64 {
                            let _ = sender.try_send(i);
                        }

                        total_duration += start.elapsed();

                        drop(sender);
                        rt.block_on(consumer).unwrap();
                    }

                    total_duration
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// FLOOD SIMULATION BENCHMARKS
// =============================================================================

/// Benchmark behavior under flood conditions (producer >> consumer speed).
fn bench_flood_scenario(c: &mut Criterion) {
    let mut group = c.benchmark_group("flood_scenario");
    group.measurement_time(Duration::from_secs(5));
    group.throughput(Throughput::Elements(10_000));

    // Disable warnings for flood test
    let config = BackpressureConfig::builder()
        .channel_capacity(100)
        .warn_on_drop(false)
        .build();

    group.bench_function("flood_10k_messages", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;

            for _ in 0..iters {
                let (sender, _receiver) = BackpressureSender::<u64>::new(config.clone());

                let start = std::time::Instant::now();

                // Flood 10k messages into 100-capacity channel
                for i in 0..10_000_u64 {
                    let _ = sender.try_send(i);
                }

                total_duration += start.elapsed();
            }

            total_duration
        });
    });

    group.finish();
}

// =============================================================================
// CONCURRENT BENCHMARKS
// =============================================================================

/// Benchmark concurrent producers.
fn bench_concurrent_producers(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("concurrent");
    group.measurement_time(Duration::from_secs(10));

    for num_producers in [2, 4, 8].iter() {
        group.throughput(Throughput::Elements(1000 * (*num_producers as u64)));

        group.bench_with_input(
            BenchmarkId::new("producers", num_producers),
            num_producers,
            |b, &n| {
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;

                    for _ in 0..iters {
                        let config = BackpressureConfig::builder()
                            .channel_capacity(1000)
                            .warn_on_drop(false)
                            .build();

                        let (sender, mut receiver) = BackpressureSender::<u64>::new(config);

                        // Spawn consumer
                        let consumer = rt.spawn(async move {
                            while receiver.recv().await.is_some() {}
                        });

                        let start = std::time::Instant::now();

                        // Spawn n producers
                        let mut handles = Vec::new();
                        for _ in 0..n {
                            let s = sender.clone();
                            handles.push(rt.spawn(async move {
                                for i in 0..1000_u64 {
                                    let _ = s.try_send(i);
                                }
                            }));
                        }

                        // Wait for producers
                        for handle in handles {
                            rt.block_on(handle).unwrap();
                        }

                        total_duration += start.elapsed();

                        drop(sender);
                        rt.block_on(consumer).unwrap();
                    }

                    total_duration
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// CONFIG CREATION BENCHMARKS
// =============================================================================

/// Benchmark config and sender creation.
fn bench_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("creation");

    group.bench_function("BackpressureConfig::default", |b| {
        b.iter(|| {
            let config = black_box(BackpressureConfig::default());
            black_box(config)
        });
    });

    group.bench_function("BackpressureMetrics::new", |b| {
        b.iter(|| {
            let metrics = black_box(BackpressureMetrics::new());
            black_box(metrics)
        });
    });

    group.bench_function("BackpressureSender::with_capacity_1000", |b| {
        b.iter(|| {
            let (sender, receiver) = BackpressureSender::<u64>::with_capacity(1000);
            black_box((sender, receiver))
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    metrics_benches,
    bench_metrics_increment,
    bench_metrics_drop_rate,
);

criterion_group!(
    send_benches,
    bench_try_send_normal,
    bench_try_send_backpressure,
    bench_send_with_backpressure_fn,
);

criterion_group!(throughput_benches, bench_throughput, bench_flood_scenario,);

criterion_group!(concurrent_benches, bench_concurrent_producers,);

criterion_group!(creation_benches, bench_creation,);

criterion_main!(
    metrics_benches,
    send_benches,
    throughput_benches,
    concurrent_benches,
    creation_benches,
);
