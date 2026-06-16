//! Benchmarks for Dual Output Publisher (Batch 3.2) and Shadow Mode (Batch 5.1).
//!
//! Run with: `cargo bench --bench dual_bench`
//!
//! These benchmarks measure the performance of dual output publishing:
//! - Configuration creation
//! - Topic key generation (production vs shadow mode)
//! - Snapshot conversion (Book -> Gateway, Book -> AlphaSignal)
//! - Statistics operations
//! - Shadow mode overhead comparison
//!
//! Note: Integration benchmarks require a Redis server and are disabled by default.
//! Set `REDIS_BENCH=1` environment variable to enable them.

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{now_micros, Exchange, Instrument, PriceLevel};
use astra_flash::fusion::AlphaSignal;
use astra_flash::gateway::OrderBookSnapshot as GatewaySnapshot;
use astra_flash::publisher::{
    DualPublisher, DualPublisherBuilder, DualPublisherConfig, DualStats, SerializationFormat,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal_macros::dec;
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

/// Create a test book snapshot with specified depth.
fn test_book_snapshot(depth: usize) -> astra_flash::book::BookSnapshot {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let mut book = OrderBook::new(instrument, config);

    let bids: Vec<PriceLevel> = (0..depth)
        .map(|i| PriceLevel::new(1.0850 - (i as f64 * 0.0001), dec!(1000000), now_micros()))
        .collect();
    let asks: Vec<PriceLevel> = (0..depth)
        .map(|i| PriceLevel::new(1.0852 + (i as f64 * 0.0001), dec!(800000), now_micros()))
        .collect();

    book.apply_snapshot(bids, asks, now_micros());
    book.to_snapshot(depth)
}

// =============================================================================
// CONFIGURATION BENCHMARKS
// =============================================================================

/// Benchmark DualPublisherConfig default creation.
fn bench_config_default(c: &mut Criterion) {
    c.bench_function("DualPublisherConfig::default", |b| {
        b.iter(|| {
            let config = DualPublisherConfig::default();
            black_box(config)
        });
    });
}

/// Benchmark DualStats default creation.
fn bench_stats_default(c: &mut Criterion) {
    c.bench_function("DualStats::default", |b| {
        b.iter(|| {
            let stats = DualStats::default();
            black_box(stats)
        });
    });
}

/// Benchmark DualStats average latency calculation.
fn bench_stats_avg_latency(c: &mut Criterion) {
    let stats = DualStats {
        total_latency_us: 1000000,
        publish_count: 1000,
        ..Default::default()
    };

    c.bench_function("DualStats::avg_latency_us", |b| {
        b.iter(|| {
            let avg = stats.avg_latency_us();
            black_box(avg)
        });
    });
}

// =============================================================================
// TOPIC KEY GENERATION BENCHMARKS
// =============================================================================

/// Benchmark orderbook key generation.
fn bench_orderbook_key(c: &mut Criterion) {
    let instrument = test_instrument();
    let config = DualPublisherConfig::default();

    c.bench_function("DualPublisher::orderbook_key", |b| {
        b.iter(|| {
            let key = DualPublisher::orderbook_key(&instrument, &config);
            black_box(key)
        });
    });
}

/// Benchmark signal key generation.
fn bench_signal_key(c: &mut Criterion) {
    let instrument = test_instrument();
    let config = DualPublisherConfig::default();

    c.bench_function("DualPublisher::signal_key", |b| {
        b.iter(|| {
            let key = DualPublisher::signal_key(&instrument, &config);
            black_box(key)
        });
    });
}

// =============================================================================
// CONVERSION BENCHMARKS
// =============================================================================

/// Benchmark BookSnapshot -> GatewaySnapshot conversion.
fn bench_book_to_gateway(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_to_gateway");
    group.measurement_time(Duration::from_secs(5));

    for depth in [5, 10, 25, 50].iter() {
        let book = test_book_snapshot(*depth);

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &book,
            |b, book| {
                b.iter(|| {
                    let gateway = GatewaySnapshot::from_book_snapshot(book);
                    black_box(gateway)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark BookSnapshot -> AlphaSignal conversion.
fn bench_book_to_signal(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_to_signal");
    group.measurement_time(Duration::from_secs(5));

    for depth in [5, 10, 25, 50].iter() {
        let book = test_book_snapshot(*depth);

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &book,
            |b, book| {
                b.iter(|| {
                    let signal = AlphaSignal::from_book_snapshot(book, 0.8);
                    black_box(signal)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark GatewaySnapshot JSON serialization.
fn bench_gateway_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("gateway_to_json");
    group.measurement_time(Duration::from_secs(5));

    for depth in [5, 10, 25, 50].iter() {
        let book = test_book_snapshot(*depth);
        let gateway = GatewaySnapshot::from_book_snapshot(&book);

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &gateway,
            |b, gateway| {
                b.iter(|| {
                    let json = gateway.to_json().expect("serialize");
                    black_box(json)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark AlphaSignal JSON serialization.
fn bench_signal_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_to_json");
    group.measurement_time(Duration::from_secs(5));

    for depth in [5, 10, 25, 50].iter() {
        let book = test_book_snapshot(*depth);
        let signal = AlphaSignal::from_book_snapshot(&book, 0.8);

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &signal,
            |b, signal| {
                b.iter(|| {
                    let json = signal.to_json().expect("serialize");
                    black_box(json)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// END-TO-END CONVERSION BENCHMARKS
// =============================================================================

/// Benchmark full dual conversion pipeline (without Redis).
fn bench_dual_conversion_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("dual_conversion_pipeline");
    group.measurement_time(Duration::from_secs(5));

    for depth in [5, 10, 25, 50].iter() {
        let book = test_book_snapshot(*depth);
        let config = DualPublisherConfig::default();

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &book,
            |b, book| {
                b.iter(|| {
                    // Generate keys
                    let orderbook_key = DualPublisher::orderbook_key(&book.instrument, &config);
                    let signal_key = DualPublisher::signal_key(&book.instrument, &config);

                    // Convert to Gateway format
                    let gateway = GatewaySnapshot::from_book_snapshot(book);
                    let gateway_json = gateway.to_json().expect("serialize");

                    // Convert to AlphaSignal format
                    let signal = AlphaSignal::from_book_snapshot(book, 0.8);
                    let signal_json = signal.to_json().expect("serialize");

                    black_box((orderbook_key, signal_key, gateway_json, signal_json))
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// BUILDER BENCHMARKS
// =============================================================================

/// Benchmark DualPublisherBuilder chain.
fn bench_builder_chain(c: &mut Criterion) {
    c.bench_function("DualPublisherBuilder::chain", |b| {
        b.iter(|| {
            let builder = DualPublisherBuilder::default()
                .orderbook_enabled(true)
                .signal_enabled(true)
                .ttl_seconds(60)
                .format(SerializationFormat::Json)
                .signal_confidence(0.8);
            black_box(builder)
        });
    });
}

// =============================================================================
// SHADOW MODE BENCHMARKS (Batch 5.1)
// =============================================================================

/// Benchmark orderbook key generation - production vs shadow mode.
fn bench_shadow_mode_orderbook_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("shadow_mode_orderbook_key");
    let instrument = test_instrument();

    // Production mode (shadow disabled)
    let production_config = DualPublisherConfig::default();
    group.bench_function("production", |b| {
        b.iter(|| {
            let key = DualPublisher::orderbook_key(&instrument, &production_config);
            black_box(key)
        });
    });

    // Shadow mode (enabled)
    let mut shadow_config = DualPublisherConfig::default();
    shadow_config.shadow_mode_enabled = true;
    shadow_config.shadow_mode_namespace = "rust".to_string();
    group.bench_function("shadow", |b| {
        b.iter(|| {
            let key = DualPublisher::orderbook_key(&instrument, &shadow_config);
            black_box(key)
        });
    });

    group.finish();
}

/// Benchmark signal key generation - production vs shadow mode.
fn bench_shadow_mode_signal_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("shadow_mode_signal_key");
    let instrument = test_instrument();

    // Production mode (shadow disabled)
    let production_config = DualPublisherConfig::default();
    group.bench_function("production", |b| {
        b.iter(|| {
            let key = DualPublisher::signal_key(&instrument, &production_config);
            black_box(key)
        });
    });

    // Shadow mode (enabled)
    let mut shadow_config = DualPublisherConfig::default();
    shadow_config.shadow_mode_enabled = true;
    shadow_config.shadow_mode_namespace = "rust".to_string();
    group.bench_function("shadow", |b| {
        b.iter(|| {
            let key = DualPublisher::signal_key(&instrument, &shadow_config);
            black_box(key)
        });
    });

    group.finish();
}

/// Benchmark full dual conversion pipeline in shadow mode.
fn bench_shadow_mode_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("shadow_mode_pipeline");
    group.measurement_time(Duration::from_secs(5));

    let book = test_book_snapshot(50);

    // Production mode
    let production_config = DualPublisherConfig::default();
    group.bench_function("production_depth50", |b| {
        b.iter(|| {
            let orderbook_key = DualPublisher::orderbook_key(&book.instrument, &production_config);
            let signal_key = DualPublisher::signal_key(&book.instrument, &production_config);
            let gateway = GatewaySnapshot::from_book_snapshot(&book);
            let gateway_json = gateway.to_json().expect("serialize");
            let signal = AlphaSignal::from_book_snapshot(&book, 0.8);
            let signal_json = signal.to_json().expect("serialize");
            black_box((orderbook_key, signal_key, gateway_json, signal_json))
        });
    });

    // Shadow mode
    let mut shadow_config = DualPublisherConfig::default();
    shadow_config.shadow_mode_enabled = true;
    shadow_config.shadow_mode_namespace = "rust".to_string();
    group.bench_function("shadow_depth50", |b| {
        b.iter(|| {
            let orderbook_key = DualPublisher::orderbook_key(&book.instrument, &shadow_config);
            let signal_key = DualPublisher::signal_key(&book.instrument, &shadow_config);
            let gateway = GatewaySnapshot::from_book_snapshot(&book);
            let gateway_json = gateway.to_json().expect("serialize");
            let signal = AlphaSignal::from_book_snapshot(&book, 0.8);
            let signal_json = signal.to_json().expect("serialize");
            black_box((orderbook_key, signal_key, gateway_json, signal_json))
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    config_benches,
    bench_config_default,
    bench_stats_default,
    bench_stats_avg_latency,
);

criterion_group!(key_benches, bench_orderbook_key, bench_signal_key,);

criterion_group!(
    conversion_benches,
    bench_book_to_gateway,
    bench_book_to_signal,
);

criterion_group!(
    serialization_benches,
    bench_gateway_to_json,
    bench_signal_to_json,
);

criterion_group!(pipeline_benches, bench_dual_conversion_pipeline,);

criterion_group!(builder_benches, bench_builder_chain,);

criterion_group!(
    shadow_mode_benches,
    bench_shadow_mode_orderbook_key,
    bench_shadow_mode_signal_key,
    bench_shadow_mode_pipeline,
);

criterion_main!(
    config_benches,
    key_benches,
    conversion_benches,
    serialization_benches,
    pipeline_benches,
    builder_benches,
    shadow_mode_benches,
);
