//! Benchmarks for Flash Connection Manager.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | State lookup | < 1 μs |
//! | is_connected check | < 1 μs |
//! | connected_exchanges | < 10 μs |
//! | Connector creation | < 100 μs |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench connector_bench
//! ```

use astra_flash::core::config::WebSocketConfig;
use astra_flash::core::metrics::FlashMetrics;
use astra_flash::core::types::Exchange;
use astra_flash::network::connector::{ConnectionState, ConnectionStats, Connector, RawMessage};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Instant;

// =============================================================================
// SETUP
// =============================================================================

fn create_config() -> WebSocketConfig {
    WebSocketConfig::default()
}

fn create_metrics() -> FlashMetrics {
    FlashMetrics::new()
}

fn create_connector() -> (
    Connector,
    tokio::sync::mpsc::Receiver<astra_flash::network::connector::ConnectorEvent>,
    tokio::sync::mpsc::Receiver<RawMessage>,
) {
    Connector::new(create_config(), create_metrics())
}

// =============================================================================
// TYPE BENCHMARKS
// =============================================================================

/// Benchmark ConnectionState::is_connected().
fn bench_connection_state_is_connected(c: &mut Criterion) {
    let states = [
        ConnectionState::Disconnected,
        ConnectionState::Connecting,
        ConnectionState::Connected,
        ConnectionState::Disconnecting,
    ];

    c.bench_function("ConnectionState::is_connected", |b| {
        b.iter(|| {
            for state in &states {
                black_box(state.is_connected());
            }
        });
    });
}

/// Benchmark ConnectionState display.
fn bench_connection_state_display(c: &mut Criterion) {
    let state = ConnectionState::Connected;

    c.bench_function("ConnectionState::to_string", |b| {
        b.iter(|| {
            black_box(state.to_string());
        });
    });
}

/// Benchmark ConnectionStats::default().
fn bench_connection_stats_default(c: &mut Criterion) {
    c.bench_function("ConnectionStats::default", |b| {
        b.iter(|| {
            black_box(ConnectionStats::default());
        });
    });
}

/// Benchmark RawMessage creation.
fn bench_raw_message_creation(c: &mut Criterion) {
    c.bench_function("RawMessage::new", |b| {
        b.iter(|| {
            black_box(RawMessage {
                exchange: Exchange::Deribit,
                payload: r#"{"test": "data"}"#.to_string(),
                received_at: 1234567890,
            });
        });
    });
}

// =============================================================================
// CONNECTOR BENCHMARKS
// =============================================================================

/// Benchmark Connector creation.
fn bench_connector_new(c: &mut Criterion) {
    c.bench_function("Connector::new", |b| {
        b.iter(|| {
            let config = create_config();
            let metrics = create_metrics();
            black_box(Connector::new(config, metrics));
        });
    });
}

/// Benchmark Connector::state() for non-existent exchange.
fn bench_connector_state_nonexistent(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });

    c.bench_function("Connector::state (nonexistent)", |b| {
        b.iter(|| {
            black_box(connector.state(Exchange::Deribit));
        });
    });
}

/// Benchmark Connector::is_connected() for non-existent exchange.
fn bench_connector_is_connected_nonexistent(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });

    c.bench_function("Connector::is_connected (nonexistent)", |b| {
        b.iter(|| {
            black_box(connector.is_connected(Exchange::Deribit));
        });
    });
}

/// Benchmark Connector::connected_exchanges() with no connections.
fn bench_connector_connected_exchanges_empty(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });

    c.bench_function("Connector::connected_exchanges (empty)", |b| {
        b.iter(|| {
            black_box(connector.connected_exchanges());
        });
    });
}

/// Benchmark Connector::stats() for non-existent exchange.
fn bench_connector_stats_nonexistent(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });

    c.bench_function("Connector::stats (nonexistent)", |b| {
        b.iter(|| {
            black_box(connector.stats(Exchange::Deribit));
        });
    });
}

// =============================================================================
// CONCURRENT ACCESS BENCHMARKS
// =============================================================================

/// Benchmark concurrent state lookups.
fn bench_concurrent_state_lookups(c: &mut Criterion) {
    use std::sync::Arc;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });
    let connector = Arc::new(connector);

    c.bench_function("Connector::state (concurrent)", |b| {
        b.iter(|| {
            let c1 = Arc::clone(&connector);
            let c2 = Arc::clone(&connector);
            let c3 = Arc::clone(&connector);

            // Simulate concurrent access
            black_box(c1.state(Exchange::Deribit));
            black_box(c2.state(Exchange::Binance));
            black_box(c3.state(Exchange::Oanda));
        });
    });
}

/// Benchmark concurrent is_connected checks.
fn bench_concurrent_is_connected(c: &mut Criterion) {
    use std::sync::Arc;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });
    let connector = Arc::new(connector);

    c.bench_function("Connector::is_connected (concurrent)", |b| {
        b.iter(|| {
            let c1 = Arc::clone(&connector);
            let c2 = Arc::clone(&connector);
            let c3 = Arc::clone(&connector);

            black_box(c1.is_connected(Exchange::Deribit));
            black_box(c2.is_connected(Exchange::Binance));
            black_box(c3.is_connected(Exchange::Oanda));
        });
    });
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark state lookup throughput.
fn bench_state_lookup_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (connector, _events, _messages) = rt.block_on(async { create_connector() });

    c.bench_function("state_lookup_throughput (10K)", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(connector.state(Exchange::Deribit));
            }
        });
    });
}

// =============================================================================
// CHANNEL BENCHMARKS
// =============================================================================

/// Benchmark channel creation overhead.
fn bench_channel_creation(c: &mut Criterion) {
    c.bench_function("mpsc::channel (1000 capacity)", |b| {
        b.iter(|| {
            let (tx, rx) = tokio::sync::mpsc::channel::<RawMessage>(1000);
            black_box((tx, rx));
        });
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    types,
    bench_connection_state_is_connected,
    bench_connection_state_display,
    bench_connection_stats_default,
    bench_raw_message_creation,
);

criterion_group!(
    connector,
    bench_connector_new,
    bench_connector_state_nonexistent,
    bench_connector_is_connected_nonexistent,
    bench_connector_connected_exchanges_empty,
    bench_connector_stats_nonexistent,
);

criterion_group!(
    concurrent,
    bench_concurrent_state_lookups,
    bench_concurrent_is_connected,
);

criterion_group!(
    throughput,
    bench_state_lookup_throughput,
    bench_channel_creation,
);

criterion_main!(types, connector, concurrent, throughput);
