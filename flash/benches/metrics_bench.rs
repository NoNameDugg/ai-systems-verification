//! Benchmarks for Flash metrics system.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Counter increment | < 50 ns |
//! | Gauge set | < 50 ns |
//! | Histogram record | < 100 ns |
//! | TimingGuard creation + drop | < 200 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench metrics_bench
//! ```

use astra_flash::core::metrics::{
    FlashMetrics, MessageType, ProcessingStage, ReconnectReason, UpdateType,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;

// =============================================================================
// SETUP
// =============================================================================

fn create_metrics() -> FlashMetrics {
    FlashMetrics::new()
}

// =============================================================================
// COUNTER BENCHMARKS
// =============================================================================

/// Benchmark counter increment (message received).
fn bench_counter_message_received(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("counter::record_message_received", |b| {
        b.iter(|| {
            metrics.record_message_received(
                black_box("deribit"),
                black_box("BTC-PERPETUAL"),
                black_box(MessageType::Delta),
            );
        });
    });
}

/// Benchmark counter increment (message published).
fn bench_counter_message_published(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("counter::record_message_published", |b| {
        b.iter(|| {
            metrics.record_message_published(
                black_box("deribit"),
                black_box("BTC-PERPETUAL"),
                black_box("market_data.deribit.btc.book"),
            );
        });
    });
}

/// Benchmark counter increment (error).
fn bench_counter_error(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("counter::record_error", |b| {
        b.iter(|| {
            metrics.record_error(black_box("parse_error"), black_box("warning"));
        });
    });
}

/// Benchmark counter increment (reconnection).
fn bench_counter_reconnection(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("counter::record_reconnection", |b| {
        b.iter(|| {
            metrics.record_reconnection(
                black_box("deribit"),
                black_box(ReconnectReason::ConnectionLost),
            );
        });
    });
}

// =============================================================================
// HISTOGRAM BENCHMARKS
// =============================================================================

/// Benchmark histogram record (processing duration).
fn bench_histogram_processing_duration(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("histogram::record_processing_duration", |b| {
        b.iter(|| {
            metrics.record_processing_duration(
                black_box("deribit"),
                black_box(ProcessingStage::Parse),
                black_box(2.5),
            );
        });
    });
}

/// Benchmark histogram record (orderbook update).
fn bench_histogram_orderbook_update(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("histogram::record_orderbook_update_duration", |b| {
        b.iter(|| {
            metrics.record_orderbook_update_duration(
                black_box("BTC-PERPETUAL"),
                black_box(UpdateType::Delta),
                black_box(0.5),
            );
        });
    });
}

/// Benchmark histogram record (redis publish).
fn bench_histogram_redis_publish(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("histogram::record_redis_publish_duration", |b| {
        b.iter(|| {
            metrics.record_redis_publish_duration(
                black_box("market_data.deribit.btc.book"),
                black_box(50.0),
            );
        });
    });
}

// =============================================================================
// GAUGE BENCHMARKS
// =============================================================================

/// Benchmark gauge set (orderbook levels).
fn bench_gauge_orderbook_levels(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("gauge::set_orderbook_levels", |b| {
        b.iter(|| {
            metrics.set_orderbook_levels(black_box("BTC-PERPETUAL"), black_box(50), black_box(50));
        });
    });
}

/// Benchmark gauge set (orderbook spread).
fn bench_gauge_orderbook_spread(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("gauge::set_orderbook_spread", |b| {
        b.iter(|| {
            metrics.set_orderbook_spread(black_box("BTC-PERPETUAL"), black_box(0.5));
        });
    });
}

/// Benchmark gauge set (websocket connected).
fn bench_gauge_websocket_connected(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("gauge::set_websocket_connected", |b| {
        b.iter(|| {
            metrics.set_websocket_connected(black_box("deribit"), black_box(true));
        });
    });
}

/// Benchmark gauge set (queue depth).
fn bench_gauge_queue_depth(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("gauge::set_queue_depth", |b| {
        b.iter(|| {
            metrics.set_queue_depth(black_box("redis_publish"), black_box(100), black_box(10000));
        });
    });
}

// =============================================================================
// TIMING GUARD BENCHMARKS
// =============================================================================

/// Benchmark timing guard creation and drop (minimal work).
fn bench_timing_guard_minimal(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("timing_guard::minimal", |b| {
        b.iter(|| {
            let _guard =
                metrics.time_processing(black_box("deribit"), black_box(ProcessingStage::Parse));
            // Immediately dropped
        });
    });
}

/// Benchmark timing guard with simulated work.
fn bench_timing_guard_with_work(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("timing_guard::with_work", |b| {
        b.iter(|| {
            let _guard =
                metrics.time_processing(black_box("deribit"), black_box(ProcessingStage::Parse));
            // Simulate minimal work
            black_box(1 + 1);
        });
    });
}

// =============================================================================
// UTILITY BENCHMARKS
// =============================================================================

/// Benchmark uptime calculation.
fn bench_uptime_seconds(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("utility::uptime_seconds", |b| {
        b.iter(|| {
            black_box(metrics.uptime_seconds());
        });
    });
}

/// Benchmark FlashMetrics creation.
fn bench_metrics_new(c: &mut Criterion) {
    c.bench_function("utility::FlashMetrics::new", |b| {
        b.iter(|| {
            black_box(FlashMetrics::new());
        });
    });
}

/// Benchmark FlashMetrics clone.
fn bench_metrics_clone(c: &mut Criterion) {
    let metrics = create_metrics();

    c.bench_function("utility::FlashMetrics::clone", |b| {
        b.iter(|| {
            black_box(metrics.clone());
        });
    });
}

// =============================================================================
// CONCURRENT BENCHMARKS
// =============================================================================

/// Benchmark concurrent counter increments.
fn bench_concurrent_counters(c: &mut Criterion) {
    let metrics = Arc::new(create_metrics());

    c.bench_function("concurrent::counter_increment", |b| {
        b.iter(|| {
            let metrics = Arc::clone(&metrics);
            metrics.record_message_received(
                black_box("deribit"),
                black_box("BTC-PERPETUAL"),
                black_box(MessageType::Delta),
            );
        });
    });
}

// =============================================================================
// ENUM BENCHMARKS
// =============================================================================

/// Benchmark ProcessingStage::as_str().
fn bench_processing_stage_as_str(c: &mut Criterion) {
    c.bench_function("enum::ProcessingStage::as_str", |b| {
        b.iter(|| {
            black_box(ProcessingStage::Parse.as_str());
            black_box(ProcessingStage::Normalize.as_str());
            black_box(ProcessingStage::BookUpdate.as_str());
            black_box(ProcessingStage::Serialize.as_str());
            black_box(ProcessingStage::Publish.as_str());
        });
    });
}

/// Benchmark MessageType::as_str().
fn bench_message_type_as_str(c: &mut Criterion) {
    c.bench_function("enum::MessageType::as_str", |b| {
        b.iter(|| {
            black_box(MessageType::Snapshot.as_str());
            black_box(MessageType::Delta.as_str());
            black_box(MessageType::Trade.as_str());
            black_box(MessageType::Heartbeat.as_str());
            black_box(MessageType::Unknown.as_str());
        });
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    counters,
    bench_counter_message_received,
    bench_counter_message_published,
    bench_counter_error,
    bench_counter_reconnection,
);

criterion_group!(
    histograms,
    bench_histogram_processing_duration,
    bench_histogram_orderbook_update,
    bench_histogram_redis_publish,
);

criterion_group!(
    gauges,
    bench_gauge_orderbook_levels,
    bench_gauge_orderbook_spread,
    bench_gauge_websocket_connected,
    bench_gauge_queue_depth,
);

criterion_group!(
    timing_guards,
    bench_timing_guard_minimal,
    bench_timing_guard_with_work,
);

criterion_group!(
    utilities,
    bench_uptime_seconds,
    bench_metrics_new,
    bench_metrics_clone,
);

criterion_group!(concurrent, bench_concurrent_counters,);

criterion_group!(
    enums,
    bench_processing_stage_as_str,
    bench_message_type_as_str,
);

criterion_main!(
    counters,
    histograms,
    gauges,
    timing_guards,
    utilities,
    concurrent,
    enums
);
