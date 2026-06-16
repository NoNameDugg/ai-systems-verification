//! Benchmarks for Redis connection pool.
//!
//! Run with: `cargo bench --bench pool_bench`
//!
//! Note: Integration benchmarks require a Redis server and are disabled by default.
//! Set `REDIS_BENCH=1` environment variable to enable them.

use astra_flash::publisher::pool::{
    PoolConfig, PoolError, PoolEvent, PoolHealth, PoolHealthStatus, PoolStats, RedisPoolBuilder,
    RedisServerInfo,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Duration;

// =============================================================================
// CONFIGURATION BENCHMARKS
// =============================================================================

/// Benchmark PoolConfig default creation.
fn bench_config_default(c: &mut Criterion) {
    c.bench_function("PoolConfig::default", |b| {
        b.iter(|| {
            let config = PoolConfig::default();
            black_box(config)
        });
    });
}

/// Benchmark PoolConfig cloning.
fn bench_config_clone(c: &mut Criterion) {
    let config = PoolConfig::default();

    c.bench_function("PoolConfig::clone", |b| {
        b.iter(|| {
            let cloned = config.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark PoolConfig validation.
fn bench_config_validate(c: &mut Criterion) {
    let config = PoolConfig::default();

    c.bench_function("PoolConfig::validate", |b| {
        b.iter(|| {
            let result = config.validate();
            black_box(result)
        });
    });
}

// =============================================================================
// BUILDER BENCHMARKS
// =============================================================================

/// Benchmark builder creation.
fn bench_builder_new(c: &mut Criterion) {
    c.bench_function("RedisPoolBuilder::new", |b| {
        b.iter(|| {
            let builder = RedisPoolBuilder::new("redis://127.0.0.1:6379");
            black_box(builder)
        });
    });
}

/// Benchmark builder with all options.
fn bench_builder_full(c: &mut Criterion) {
    c.bench_function("RedisPoolBuilder::full_config", |b| {
        b.iter(|| {
            let builder = RedisPoolBuilder::new("redis://127.0.0.1:6379")
                .max_size(20)
                .min_idle(5)
                .connect_timeout(Duration::from_secs(10))
                .wait_timeout(Duration::from_secs(5))
                .health_check_interval(Duration::from_secs(30))
                .auto_reconnect(true)
                .database(0);
            black_box(builder)
        });
    });
}

// =============================================================================
// STATISTICS BENCHMARKS
// =============================================================================

/// Benchmark PoolStats creation.
fn bench_stats_default(c: &mut Criterion) {
    c.bench_function("PoolStats::default", |b| {
        b.iter(|| {
            let stats = PoolStats::default();
            black_box(stats)
        });
    });
}

/// Benchmark PoolStats cloning.
fn bench_stats_clone(c: &mut Criterion) {
    let mut stats = PoolStats::default();
    stats.connections_created = 1000;
    stats.connections_recycled = 500;
    stats.current_size = 10;
    stats.in_use_connections = 5;
    stats.peak_in_use = 8;
    stats.commands_executed = 10000;

    c.bench_function("PoolStats::clone", |b| {
        b.iter(|| {
            let cloned = stats.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// HEALTH BENCHMARKS
// =============================================================================

/// Benchmark PoolHealth creation.
fn bench_health_default(c: &mut Criterion) {
    c.bench_function("PoolHealth::default", |b| {
        b.iter(|| {
            let health = PoolHealth::default();
            black_box(health)
        });
    });
}

/// Benchmark PoolHealth cloning.
fn bench_health_clone(c: &mut Criterion) {
    let mut health = PoolHealth::default();
    health.status = PoolHealthStatus::Healthy;
    health.avg_latency_us = 250.0;
    health.last_latency_us = 200;
    health.consecutive_successes = 10;
    health.server_info = Some(RedisServerInfo {
        version: "7.0.0".to_string(),
        connected_clients: 5,
        used_memory_bytes: 1024 * 1024,
        uptime_seconds: 3600,
    });

    c.bench_function("PoolHealth::clone", |b| {
        b.iter(|| {
            let cloned = health.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark health status comparison.
fn bench_health_status_eq(c: &mut Criterion) {
    let status1 = PoolHealthStatus::Healthy;
    let status2 = PoolHealthStatus::Healthy;

    c.bench_function("PoolHealthStatus::eq", |b| {
        b.iter(|| {
            let result = status1 == status2;
            black_box(result)
        });
    });
}

// =============================================================================
// EVENT BENCHMARKS
// =============================================================================

/// Benchmark event creation.
fn bench_event_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("PoolEvent");

    group.bench_function("Created", |b| {
        b.iter(|| {
            let event = PoolEvent::Created { size: 10 };
            black_box(event)
        });
    });

    group.bench_function("ConnectionAcquired", |b| {
        b.iter(|| {
            let event = PoolEvent::ConnectionAcquired { idle_remaining: 5 };
            black_box(event)
        });
    });

    group.bench_function("ConnectionReleased", |b| {
        b.iter(|| {
            let event = PoolEvent::ConnectionReleased {
                usage_duration_us: 1000,
            };
            black_box(event)
        });
    });

    group.bench_function("HealthCheckCompleted", |b| {
        b.iter(|| {
            let event = PoolEvent::HealthCheckCompleted {
                status: PoolHealthStatus::Healthy,
                latency_us: 500,
            };
            black_box(event)
        });
    });

    group.finish();
}

// =============================================================================
// ERROR BENCHMARKS
// =============================================================================

/// Benchmark error creation.
fn bench_error_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("PoolError");

    group.bench_function("CreationFailed", |b| {
        b.iter(|| {
            let error = PoolError::CreationFailed("Connection refused".to_string());
            black_box(error)
        });
    });

    group.bench_function("ConnectionTimeout", |b| {
        b.iter(|| {
            let error = PoolError::ConnectionTimeout { timeout_ms: 5000 };
            black_box(error)
        });
    });

    group.bench_function("PoolExhausted", |b| {
        b.iter(|| {
            let error = PoolError::PoolExhausted { waiting: 10 };
            black_box(error)
        });
    });

    group.finish();
}

/// Benchmark error display.
fn bench_error_display(c: &mut Criterion) {
    let error = PoolError::ConnectionTimeout { timeout_ms: 5000 };

    c.bench_function("PoolError::to_string", |b| {
        b.iter(|| {
            let s = error.to_string();
            black_box(s)
        });
    });
}

// =============================================================================
// SERVER INFO BENCHMARKS
// =============================================================================

/// Benchmark server info creation.
fn bench_server_info_creation(c: &mut Criterion) {
    c.bench_function("RedisServerInfo::new", |b| {
        b.iter(|| {
            let info = RedisServerInfo {
                version: "7.0.0".to_string(),
                connected_clients: 5,
                used_memory_bytes: 1024 * 1024,
                uptime_seconds: 3600,
            };
            black_box(info)
        });
    });
}

/// Benchmark server info cloning.
fn bench_server_info_clone(c: &mut Criterion) {
    let info = RedisServerInfo {
        version: "7.0.0".to_string(),
        connected_clients: 5,
        used_memory_bytes: 1024 * 1024,
        uptime_seconds: 3600,
    };

    c.bench_function("RedisServerInfo::clone", |b| {
        b.iter(|| {
            let cloned = info.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark config serialization.
fn bench_config_serialize_json(c: &mut Criterion) {
    let config = PoolConfig::default();

    c.bench_function("PoolConfig::serde_json::to_string", |b| {
        b.iter(|| {
            let json = serde_json::to_string(&config).unwrap();
            black_box(json)
        });
    });
}

/// Benchmark config deserialization.
fn bench_config_deserialize_json(c: &mut Criterion) {
    let config = PoolConfig::default();
    let json = serde_json::to_string(&config).unwrap();

    c.bench_function("PoolConfig::serde_json::from_str", |b| {
        b.iter(|| {
            let parsed: PoolConfig = serde_json::from_str(&json).unwrap();
            black_box(parsed)
        });
    });
}

// =============================================================================
// SIZE BENCHMARKS
// =============================================================================

/// Benchmark memory sizes of types.
fn bench_type_sizes(c: &mut Criterion) {
    use std::mem::size_of;

    c.bench_function("type_sizes", |b| {
        b.iter(|| {
            let config_size = size_of::<PoolConfig>();
            let stats_size = size_of::<PoolStats>();
            let health_size = size_of::<PoolHealth>();
            let status_size = size_of::<PoolHealthStatus>();
            let event_size = size_of::<PoolEvent>();
            let error_size = size_of::<PoolError>();

            black_box((
                config_size,
                stats_size,
                health_size,
                status_size,
                event_size,
                error_size,
            ))
        });
    });

    // Print sizes for reference
    println!("\nType sizes:");
    println!("  PoolConfig: {} bytes", std::mem::size_of::<PoolConfig>());
    println!("  PoolStats: {} bytes", std::mem::size_of::<PoolStats>());
    println!("  PoolHealth: {} bytes", std::mem::size_of::<PoolHealth>());
    println!(
        "  PoolHealthStatus: {} bytes",
        std::mem::size_of::<PoolHealthStatus>()
    );
    println!("  PoolEvent: {} bytes", std::mem::size_of::<PoolEvent>());
    println!("  PoolError: {} bytes", std::mem::size_of::<PoolError>());
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    config_benchmarks,
    bench_config_default,
    bench_config_clone,
    bench_config_validate,
    bench_config_serialize_json,
    bench_config_deserialize_json,
);

criterion_group!(builder_benchmarks, bench_builder_new, bench_builder_full,);

criterion_group!(stats_benchmarks, bench_stats_default, bench_stats_clone,);

criterion_group!(
    health_benchmarks,
    bench_health_default,
    bench_health_clone,
    bench_health_status_eq,
);

criterion_group!(event_benchmarks, bench_event_creation,);

criterion_group!(error_benchmarks, bench_error_creation, bench_error_display,);

criterion_group!(
    server_info_benchmarks,
    bench_server_info_creation,
    bench_server_info_clone,
);

criterion_group!(misc_benchmarks, bench_type_sizes,);

criterion_main!(
    config_benchmarks,
    builder_benchmarks,
    stats_benchmarks,
    health_benchmarks,
    event_benchmarks,
    error_benchmarks,
    server_info_benchmarks,
    misc_benchmarks,
);
