//! Benchmarks for Flash Reconnection Logic.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Status lookup | < 1 μs |
//! | Backoff calculation | < 1 μs |
//! | Subscription registration | < 10 μs |
//! | Stats retrieval | < 5 μs |
//! | Sequence recording | < 1 μs |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench reconnect_bench
//! ```

use astra_flash::core::config::WebSocketConfig;
use astra_flash::core::metrics::FlashMetrics;
use astra_flash::core::types::Exchange;
use astra_flash::network::connector::Connector;
use astra_flash::network::heartbeat::{HeartbeatConfig, HeartbeatManager};
use astra_flash::network::reconnect::{ReconnectionConfig, ReconnectionManager};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;

/// Create test configuration.
fn test_config() -> ReconnectionConfig {
    ReconnectionConfig {
        initial_delay_ms: 1000,
        max_delay_ms: 30_000,
        backoff_multiplier: 2.0,
        jitter_percent: 0.25,
        max_retries: 10,
        auto_reconnect: true,
        restore_subscriptions: true,
        restore_delay_ms: 500,
        request_snapshot_on_reconnect: true,
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
fn create_heartbeat(connector: Arc<Connector>) -> Arc<HeartbeatManager> {
    let config = HeartbeatConfig::default();
    let metrics = FlashMetrics::new();
    let (heartbeat, _event_rx) = HeartbeatManager::new(config, connector, metrics);
    Arc::new(heartbeat)
}

/// Create test reconnection manager.
fn create_reconnection() -> ReconnectionManager {
    let config = test_config();
    let connector = create_connector();
    let heartbeat = create_heartbeat(Arc::clone(&connector));
    let metrics = FlashMetrics::new();
    let (manager, _event_rx) = ReconnectionManager::new(config, connector, heartbeat, metrics);
    manager
}

/// Benchmark status lookup.
fn bench_status_lookup(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    c.bench_function("reconnect_status_lookup", |b| {
        b.iter(|| {
            black_box(manager.status(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark stats retrieval.
fn bench_stats_retrieval(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    c.bench_function("reconnect_stats_retrieval", |b| {
        b.iter(|| {
            black_box(manager.stats(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark is_reconnecting check.
fn bench_is_reconnecting(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    c.bench_function("reconnect_is_reconnecting", |b| {
        b.iter(|| {
            black_box(manager.is_reconnecting(black_box(Exchange::Deribit)));
        });
    });
}

/// Benchmark backoff calculation.
fn bench_backoff_calculation(c: &mut Criterion) {
    let config = test_config();

    c.bench_function("reconnect_backoff_calculation", |b| {
        b.iter(|| {
            black_box(config.calculate_delay(black_box(5)));
        });
    });
}

/// Benchmark backoff calculation no jitter.
fn bench_backoff_no_jitter(c: &mut Criterion) {
    let config = ReconnectionConfig {
        jitter_percent: 0.0,
        ..test_config()
    };

    c.bench_function("reconnect_backoff_no_jitter", |b| {
        b.iter(|| {
            black_box(config.calculate_delay(black_box(5)));
        });
    });
}

/// Benchmark subscription registration.
fn bench_subscription_registration(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    c.bench_function("reconnect_subscription_registration", |b| {
        b.iter(|| {
            manager.register_subscription(
                black_box(Exchange::Deribit),
                black_box(r#"{"channel": "book"}"#.to_string()),
            );
        });
    });
}

/// Benchmark subscription clear.
fn bench_subscription_clear(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    // Register some subscriptions first
    for i in 0..100 {
        manager.register_subscription(Exchange::Deribit, format!("sub_{}", i));
    }

    c.bench_function("reconnect_subscription_clear", |b| {
        b.iter(|| {
            manager.clear_subscriptions(black_box(Exchange::Deribit));
        });
    });
}

/// Benchmark sequence recording.
fn bench_sequence_recording(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    let mut seq = 0u64;

    c.bench_function("reconnect_sequence_recording", |b| {
        b.iter(|| {
            seq += 1;
            black_box(manager.record_sequence(black_box(Exchange::Deribit), black_box(seq)));
        });
    });
}

/// Benchmark enable/disable cycle.
fn bench_enable_disable(c: &mut Criterion) {
    let manager = create_reconnection();

    c.bench_function("reconnect_enable_disable", |b| {
        b.iter(|| {
            manager.enable(
                black_box(Exchange::Deribit),
                black_box("wss://test.example.com"),
            );
            manager.disable(black_box(Exchange::Deribit));
        });
    });
}

/// Benchmark reset operation.
fn bench_reset(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    // Record some sequences
    for i in 1..100 {
        manager.record_sequence(Exchange::Deribit, i);
    }

    c.bench_function("reconnect_reset", |b| {
        b.iter(|| {
            manager.reset(black_box(Exchange::Deribit));
        });
    });
}

/// Throughput benchmark for status lookups.
fn bench_status_throughput(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://test.example.com");

    let mut group = c.benchmark_group("reconnect_throughput");
    group.throughput(Throughput::Elements(1));

    group.bench_function("status_lookup", |b| {
        b.iter(|| {
            black_box(manager.status(black_box(Exchange::Deribit)));
        });
    });

    group.finish();
}

/// Benchmark multiple exchange status lookup.
fn bench_multiple_exchanges(c: &mut Criterion) {
    let manager = create_reconnection();
    manager.enable(Exchange::Deribit, "wss://deribit.example.com");
    manager.enable(Exchange::Binance, "wss://binance.example.com");
    manager.enable(Exchange::Oanda, "wss://oanda.example.com");

    c.bench_function("reconnect_multiple_exchanges", |b| {
        b.iter(|| {
            black_box(manager.status(black_box(Exchange::Deribit)));
            black_box(manager.status(black_box(Exchange::Binance)));
            black_box(manager.status(black_box(Exchange::Oanda)));
        });
    });
}

criterion_group!(
    benches,
    bench_status_lookup,
    bench_stats_retrieval,
    bench_is_reconnecting,
    bench_backoff_calculation,
    bench_backoff_no_jitter,
    bench_subscription_registration,
    bench_subscription_clear,
    bench_sequence_recording,
    bench_enable_disable,
    bench_reset,
    bench_status_throughput,
    bench_multiple_exchanges,
);

criterion_main!(benches);
