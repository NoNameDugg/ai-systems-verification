//! Benchmarks for Flash Heartbeat & Health Monitoring.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Health status lookup | < 1 μs |
//! | Pong processing | < 10 μs |
//! | Event emission | < 50 μs |
//! | Jitter calculation | < 100 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench heartbeat_bench
//! ```

use astra_flash::core::config::WebSocketConfig;
use astra_flash::core::metrics::FlashMetrics;
use astra_flash::core::types::Exchange;
use astra_flash::network::connector::Connector;
use astra_flash::network::heartbeat::{HeartbeatConfig, HeartbeatManager};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;

/// Create test configuration.
fn test_config() -> HeartbeatConfig {
    HeartbeatConfig {
        ping_interval_ms: 30_000,
        pong_timeout_ms: 10_000,
        jitter_percent: 0.20,
        degraded_threshold: 1,
        unhealthy_threshold: 3,
        auto_disconnect_unhealthy: false,
        stale_connection_ms: 60_000,
    }
}

/// Create test connector.
fn create_connector() -> Arc<Connector> {
    let ws_config = WebSocketConfig::default();
    let metrics = FlashMetrics::new();
    let (connector, _event_rx, _message_rx) = Connector::new(ws_config, metrics);
    Arc::new(connector)
}

/// Create test heartbeat manager.
fn create_heartbeat() -> HeartbeatManager {
    let config = test_config();
    let connector = create_connector();
    let metrics = FlashMetrics::new();
    let (heartbeat, _event_rx) = HeartbeatManager::new(config, connector, metrics);
    heartbeat
}

/// Benchmark health status lookup.
fn bench_health_status_lookup(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Deribit);

    c.bench_function("heartbeat_health_status_lookup", |b| {
        b.iter(|| {
            black_box(heartbeat.health_status(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark health summary retrieval.
fn bench_health_summary(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Deribit);

    c.bench_function("heartbeat_health_summary", |b| {
        b.iter(|| {
            black_box(heartbeat.health_summary(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark pong recording.
fn bench_record_pong(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);

    c.bench_function("heartbeat_record_pong", |b| {
        b.iter(|| {
            heartbeat.record_pong(black_box(Exchange::Deribit));
        });
    });
}

/// Benchmark pong recording with latency.
fn bench_record_pong_with_latency(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    let latency = Duration::from_micros(500);

    c.bench_function("heartbeat_record_pong_with_latency", |b| {
        b.iter(|| {
            heartbeat.record_pong_with_latency(black_box(Exchange::Deribit), black_box(latency));
        });
    });
}

/// Benchmark missed pong recording.
fn bench_record_missed_pong(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Deribit); // Make healthy first

    c.bench_function("heartbeat_record_missed_pong", |b| {
        b.iter(|| {
            heartbeat.record_missed_pong(black_box(Exchange::Deribit));
            // Reset to healthy to avoid state accumulation
            heartbeat.record_pong(Exchange::Deribit);
        });
    });
}

/// Benchmark jitter calculation.
fn bench_jitter_calculation(c: &mut Criterion) {
    let config = test_config();

    c.bench_function("heartbeat_jitter_calculation", |b| {
        b.iter(|| {
            black_box(config.calculate_next_interval());
        });
    });
}

/// Benchmark is_healthy check.
fn bench_is_healthy(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Deribit);

    c.bench_function("heartbeat_is_healthy", |b| {
        b.iter(|| {
            black_box(heartbeat.is_healthy(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark is_stale check.
fn bench_is_stale(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Deribit);

    c.bench_function("heartbeat_is_stale", |b| {
        b.iter(|| {
            black_box(heartbeat.is_stale(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark all health statuses retrieval.
fn bench_all_health_statuses(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);
    heartbeat.start_monitoring(Exchange::Binance);
    heartbeat.start_monitoring(Exchange::Oanda);
    heartbeat.record_pong(Exchange::Deribit);
    heartbeat.record_pong(Exchange::Binance);
    heartbeat.record_pong(Exchange::Oanda);

    c.bench_function("heartbeat_all_health_statuses", |b| {
        b.iter(|| {
            black_box(heartbeat.all_health_statuses());
        });
    });
}

/// Benchmark start/stop monitoring.
fn bench_start_stop_monitoring(c: &mut Criterion) {
    let heartbeat = create_heartbeat();

    c.bench_function("heartbeat_start_stop_monitoring", |b| {
        b.iter(|| {
            heartbeat.start_monitoring(black_box(Exchange::Deribit));
            heartbeat.stop_monitoring(black_box(Exchange::Deribit));
        });
    });
}

/// Throughput benchmark for pong processing.
fn bench_pong_throughput(c: &mut Criterion) {
    let heartbeat = create_heartbeat();
    heartbeat.start_monitoring(Exchange::Deribit);

    let mut group = c.benchmark_group("heartbeat_throughput");
    group.throughput(Throughput::Elements(1));

    group.bench_function("pong_processing", |b| {
        b.iter(|| {
            heartbeat.record_pong(black_box(Exchange::Deribit));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_health_status_lookup,
    bench_health_summary,
    bench_record_pong,
    bench_record_pong_with_latency,
    bench_record_missed_pong,
    bench_jitter_calculation,
    bench_is_healthy,
    bench_is_stale,
    bench_all_health_statuses,
    bench_start_stop_monitoring,
    bench_pong_throughput,
);

criterion_main!(benches);
