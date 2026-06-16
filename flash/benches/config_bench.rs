//! Benchmarks for Flash configuration system.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Config from YAML string | < 1 ms |
//! | Config validation | < 1 ms |
//! | Config clone | < 1 ms |
//! | Config default | < 1 us |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench config_bench
//! ```

use astra_flash::core::config::{
    FlashConfig, LogLevel, OrderBookConfig, RedisConfig, SerializationFormat, WebSocketConfig,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// =============================================================================
// DEFAULT CREATION BENCHMARKS
// =============================================================================

/// Benchmark creating FlashConfig with defaults.
fn bench_flash_config_default(c: &mut Criterion) {
    c.bench_function("FlashConfig::default", |b| {
        b.iter(|| black_box(FlashConfig::default()));
    });
}

/// Benchmark creating WebSocketConfig with defaults.
fn bench_websocket_config_default(c: &mut Criterion) {
    c.bench_function("WebSocketConfig::default", |b| {
        b.iter(|| black_box(WebSocketConfig::default()));
    });
}

/// Benchmark creating RedisConfig with defaults.
fn bench_redis_config_default(c: &mut Criterion) {
    c.bench_function("RedisConfig::default", |b| {
        b.iter(|| black_box(RedisConfig::default()));
    });
}

// =============================================================================
// YAML PARSING BENCHMARKS
// =============================================================================

/// Benchmark parsing minimal YAML.
fn bench_from_yaml_minimal(c: &mut Criterion) {
    let yaml = r#"
        websocket:
            connect_timeout_ms: 5000
    "#;

    c.bench_function("FlashConfig::from_yaml (minimal)", |b| {
        b.iter(|| FlashConfig::from_yaml(black_box(yaml)).unwrap());
    });
}

/// Benchmark parsing empty YAML (uses all defaults).
fn bench_from_yaml_empty(c: &mut Criterion) {
    let yaml = "";

    c.bench_function("FlashConfig::from_yaml (empty)", |b| {
        b.iter(|| FlashConfig::from_yaml(black_box(yaml)).unwrap());
    });
}

/// Benchmark parsing comprehensive YAML.
fn bench_from_yaml_comprehensive(c: &mut Criterion) {
    let yaml = r#"
        websocket:
            connect_timeout_ms: 10000
            ping_interval_ms: 30000
            pong_timeout_ms: 10000
            max_reconnect_attempts: 50
            reconnect_delay_ms: 1000
            max_reconnect_delay_ms: 30000
            reconnect_jitter: 0.1

        orderbook:
            max_depth: 100
            max_levels: 200
            track_orders: false
            auto_prune: true
            gap_handling: snapshot

        redis:
            url: "redis://localhost:6379"
            database: 0
            pool_size: 20
            connect_timeout_ms: 5000
            batch_size: 100

        publisher:
            format: bincode
            debug_json_fallback: false
            topic_prefix: market_data
            backpressure:
                capacity: 50000
                warn_threshold: 0.75
                critical_threshold: 0.95
                action: drop_oldest

        logging:
            level: info
            format: json

        metrics:
            enabled: true
            port: 9090
    "#;

    c.bench_function("FlashConfig::from_yaml (comprehensive)", |b| {
        b.iter(|| FlashConfig::from_yaml(black_box(yaml)).unwrap());
    });
}

// =============================================================================
// VALIDATION BENCHMARKS
// =============================================================================

/// Benchmark validating default config.
fn bench_validate_default(c: &mut Criterion) {
    let config = FlashConfig::default();

    c.bench_function("FlashConfig::validate (default)", |b| {
        b.iter(|| black_box(&config).validate());
    });
}

/// Benchmark validating config with all sections modified.
fn bench_validate_modified(c: &mut Criterion) {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 10000;
    config.orderbook.max_depth = 100;
    config.redis.pool_size = 25;
    config.publisher.backpressure.capacity = 50000;

    c.bench_function("FlashConfig::validate (modified)", |b| {
        b.iter(|| black_box(&config).validate());
    });
}

// =============================================================================
// CLONE BENCHMARKS
// =============================================================================

/// Benchmark cloning FlashConfig.
fn bench_flash_config_clone(c: &mut Criterion) {
    let config = FlashConfig::default();

    c.bench_function("FlashConfig::clone", |b| {
        b.iter(|| black_box(&config).clone());
    });
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark serializing config to YAML.
fn bench_to_yaml(c: &mut Criterion) {
    let config = FlashConfig::default();

    c.bench_function("FlashConfig -> YAML", |b| {
        b.iter(|| serde_yaml::to_string(black_box(&config)).unwrap());
    });
}

/// Benchmark round-trip (serialize then deserialize).
fn bench_round_trip(c: &mut Criterion) {
    let config = FlashConfig::default();

    c.bench_function("FlashConfig round-trip", |b| {
        b.iter(|| {
            let yaml = serde_yaml::to_string(black_box(&config)).unwrap();
            let _: FlashConfig = serde_yaml::from_str(&yaml).unwrap();
        });
    });
}

// =============================================================================
// DURATION HELPER BENCHMARKS
// =============================================================================

/// Benchmark Duration conversion helpers.
fn bench_duration_helpers(c: &mut Criterion) {
    let config = WebSocketConfig::default();

    c.bench_function("WebSocketConfig::connect_timeout()", |b| {
        b.iter(|| black_box(&config).connect_timeout());
    });
}

// =============================================================================
// URL PARSING BENCHMARKS
// =============================================================================

/// Benchmark Redis URL parsing.
fn bench_redis_url_parsing(c: &mut Criterion) {
    let config = RedisConfig::default();

    c.bench_function("RedisConfig::parsed_url()", |b| {
        b.iter(|| black_box(&config).parsed_url());
    });
}

// =============================================================================
// ENUM SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark enum serialization.
fn bench_enum_serialization(c: &mut Criterion) {
    let format = SerializationFormat::Bincode;

    c.bench_function("SerializationFormat -> YAML", |b| {
        b.iter(|| serde_yaml::to_string(black_box(&format)).unwrap());
    });
}

/// Benchmark enum deserialization.
fn bench_enum_deserialization(c: &mut Criterion) {
    let yaml = "bincode";

    c.bench_function("YAML -> SerializationFormat", |b| {
        b.iter(|| {
            let _: SerializationFormat = serde_yaml::from_str(black_box(yaml)).unwrap();
        });
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    defaults,
    bench_flash_config_default,
    bench_websocket_config_default,
    bench_redis_config_default,
);

criterion_group!(
    yaml_parsing,
    bench_from_yaml_empty,
    bench_from_yaml_minimal,
    bench_from_yaml_comprehensive,
);

criterion_group!(validation, bench_validate_default, bench_validate_modified,);

criterion_group!(
    serialization,
    bench_flash_config_clone,
    bench_to_yaml,
    bench_round_trip,
);

criterion_group!(helpers, bench_duration_helpers, bench_redis_url_parsing,);

criterion_group!(enums, bench_enum_serialization, bench_enum_deserialization,);

criterion_main!(
    defaults,
    yaml_parsing,
    validation,
    serialization,
    helpers,
    enums
);
