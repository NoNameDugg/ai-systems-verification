//! Tests for Flash configuration system.
//!
//! # Test Categories
//!
//! | Category | Count | Description |
//! |----------|-------|-------------|
//! | Default Values | 5 | All defaults are valid and documented |
//! | YAML Loading | 6 | Load from file, string, with overrides |
//! | Environment Vars | 5 | Override via env, nesting, type coercion |
//! | Validation | 10 | All validation rules enforced |
//! | Error Handling | 4 | Missing file, invalid YAML, invalid values |
//! | Serialization | 3 | Round-trip, format enums |
//! | Thread Safety | 2 | Send + Sync bounds |
//! | Edge Cases | 3 | Empty config, partial config, max values |
//!
//! **Total:** 38 tests

use astra_flash::core::config::{
    BackpressureAction, BackpressureConfig, ExchangeConfig, ExchangesConfig, FlashConfig,
    GapHandling, LogFormat, LogLevel, LoggingConfig, MetricsConfig, OrderBookConfig,
    PublisherConfig, RedisConfig, SerializationFormat, WebSocketConfig,
};
use std::env;

// =============================================================================
// DEFAULT VALUE TESTS (5)
// =============================================================================

/// Verify WebSocket config defaults are sensible.
#[test]
fn test_default_websocket_config_values() {
    let config = WebSocketConfig::default();

    // Verify specific defaults
    assert_eq!(config.connect_timeout_ms, 5000);
    assert_eq!(config.read_timeout_ms, 5000); // Batch 1.3: 5-second read timeout
    assert_eq!(config.ping_interval_ms, 30000);
    assert_eq!(config.pong_timeout_ms, 10000);
    assert_eq!(config.max_reconnect_attempts, 10);
    assert_eq!(config.reconnect_delay_ms, 1000);
    assert_eq!(config.max_reconnect_delay_ms, 30000);
    assert!((config.reconnect_jitter - 0.1).abs() < f64::EPSILON);
}

/// Verify OrderBook config defaults are sensible.
#[test]
fn test_default_orderbook_config_values() {
    let config = OrderBookConfig::default();

    assert_eq!(config.max_depth, 50);
    assert_eq!(config.max_levels, 100);
    assert!(!config.track_orders);
    assert!(config.auto_prune);
    assert_eq!(config.gap_handling, GapHandling::Snapshot);
}

/// Verify Redis config defaults are sensible.
#[test]
fn test_default_redis_config_values() {
    let config = RedisConfig::default();

    assert_eq!(config.url, "redis://127.0.0.1:6379");
    assert_eq!(config.database, 0);
    assert_eq!(config.pool_size, 10);
    assert_eq!(config.connect_timeout_ms, 5000);
    assert_eq!(config.command_timeout_ms, 1000);
    assert_eq!(config.batch_size, 100);
    assert_eq!(config.batch_timeout_ms, 10);
}

/// Verify Publisher config defaults are sensible.
#[test]
fn test_default_publisher_config_values() {
    let config = PublisherConfig::default();

    assert_eq!(config.format, SerializationFormat::Bincode);
    assert!(!config.debug_json_fallback);
    // FUSION WIRING: Changed default to Fusion-compatible topic prefix
    assert_eq!(config.topic_prefix, "astra:signals:flash");
    assert_eq!(config.backpressure.capacity, 10000);
}

/// Verify default FlashConfig passes validation.
#[test]
fn test_default_config_is_valid() {
    let config = FlashConfig::default();
    let result = config.validate();
    assert!(result.is_ok(), "Default config should be valid: {:?}", result);
}

// =============================================================================
// YAML LOADING TESTS (6)
// =============================================================================

/// Test loading configuration from a YAML string.
#[test]
fn test_load_from_yaml_string() {
    let yaml = r#"
        websocket:
            connect_timeout_ms: 10000
        redis:
            url: "redis://custom:6379"
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse valid YAML");

    assert_eq!(config.websocket.connect_timeout_ms, 10000);
    assert_eq!(config.redis.url, "redis://custom:6379");
    // Other values should be defaults
    assert_eq!(config.websocket.ping_interval_ms, 30000);
}

/// Test loading partial YAML uses defaults for missing fields.
#[test]
fn test_load_partial_yaml_uses_defaults() {
    let yaml = r#"
        logging:
            level: debug
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse partial YAML");

    // Specified value
    assert_eq!(config.logging.level, LogLevel::Debug);
    // Default values
    assert_eq!(config.logging.format, LogFormat::Json);
    assert_eq!(config.websocket.connect_timeout_ms, 5000);
}

/// Test loading empty YAML uses all defaults.
#[test]
fn test_load_empty_yaml_uses_all_defaults() {
    let yaml = "";
    let config = FlashConfig::from_yaml(yaml).expect("Should handle empty YAML");
    let default = FlashConfig::default();

    assert_eq!(config.websocket.connect_timeout_ms, default.websocket.connect_timeout_ms);
    assert_eq!(config.redis.url, default.redis.url);
}

/// Test loading YAML with nested objects.
#[test]
fn test_load_yaml_with_nested_objects() {
    let yaml = r#"
        publisher:
            format: rkyv
            backpressure:
                capacity: 50000
                action: block
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse nested YAML");

    assert_eq!(config.publisher.format, SerializationFormat::Rkyv);
    assert_eq!(config.publisher.backpressure.capacity, 50000);
    assert_eq!(config.publisher.backpressure.action, BackpressureAction::Block);
}

/// Test loading YAML with exchange configurations.
#[test]
fn test_load_yaml_with_exchanges() {
    let yaml = r#"
        exchanges:
            deribit:
                enabled: true
                instruments:
                    - "BTC-PERPETUAL"
                    - "ETH-PERPETUAL"
            binance:
                enabled: false
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse exchange config");

    assert!(config.exchanges.deribit.enabled);
    assert_eq!(config.exchanges.deribit.instruments.len(), 2);
    assert!(!config.exchanges.binance.enabled);
}

/// Test nonexistent file returns appropriate error.
#[test]
fn test_load_nonexistent_file_returns_error() {
    let result = FlashConfig::load("nonexistent_file_12345.yaml", None);
    assert!(result.is_err());

    let err = result.unwrap_err();
    // Error should indicate file not found
    let msg = err.to_string();
    assert!(msg.contains("not found") || msg.contains("No such file"));
}

// =============================================================================
// ENVIRONMENT VARIABLE TESTS (5)
// =============================================================================

/// Test environment variable overrides YAML value.
#[test]
fn test_env_var_overrides_yaml() {
    // Set environment variable
    env::set_var("ASTRA_FLASH_WEBSOCKET_CONNECT_TIMEOUT_MS", "15000");

    let yaml = r#"
        websocket:
            connect_timeout_ms: 5000
    "#;

    let config = FlashConfig::from_yaml_with_env(yaml).expect("Should parse with env override");

    // Env var should take precedence
    assert_eq!(config.websocket.connect_timeout_ms, 15000);

    // Clean up
    env::remove_var("ASTRA_FLASH_WEBSOCKET_CONNECT_TIMEOUT_MS");
}

/// Test environment variable with nested path.
#[test]
fn test_env_var_nested_path() {
    env::set_var("ASTRA_FLASH_PUBLISHER_BACKPRESSURE_CAPACITY", "25000");

    let config = FlashConfig::from_env().expect("Should load from env");

    assert_eq!(config.publisher.backpressure.capacity, 25000);

    env::remove_var("ASTRA_FLASH_PUBLISHER_BACKPRESSURE_CAPACITY");
}

/// Test environment variable type coercion (string to number).
#[test]
fn test_env_var_type_coercion() {
    env::set_var("ASTRA_FLASH_REDIS_POOL_SIZE", "25");

    let config = FlashConfig::from_env().expect("Should coerce string to number");

    assert_eq!(config.redis.pool_size, 25);

    env::remove_var("ASTRA_FLASH_REDIS_POOL_SIZE");
}

/// Test environment variable for boolean values.
#[test]
fn test_env_var_boolean_coercion() {
    env::set_var("ASTRA_FLASH_METRICS_ENABLED", "false");

    let config = FlashConfig::from_env().expect("Should coerce string to bool");

    assert!(!config.metrics.enabled);

    env::remove_var("ASTRA_FLASH_METRICS_ENABLED");
}

/// Test environment variable for enum values.
#[test]
fn test_env_var_enum_value() {
    env::set_var("ASTRA_FLASH_PUBLISHER_FORMAT", "json");

    let config = FlashConfig::from_env().expect("Should parse enum from env");

    assert_eq!(config.publisher.format, SerializationFormat::Json);

    env::remove_var("ASTRA_FLASH_PUBLISHER_FORMAT");
}

// =============================================================================
// VALIDATION TESTS (10)
// =============================================================================

/// Test validation rejects WebSocket timeout below minimum.
#[test]
fn test_validate_websocket_timeout_below_min() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 100; // Below 1000

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("connect_timeout_ms")));
}

/// Test validation rejects WebSocket timeout above maximum.
#[test]
fn test_validate_websocket_timeout_above_max() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 100_000; // Above 60000

    let result = config.validate();
    assert!(result.is_err());
}

/// Test validation rejects read timeout below minimum (Batch 1.3: Resilience).
#[test]
fn test_validate_read_timeout_below_min() {
    let mut config = FlashConfig::default();
    config.websocket.read_timeout_ms = 500; // Below 1000

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("read_timeout_ms")));
}

/// Test validation rejects read timeout above maximum (Batch 1.3: Resilience).
#[test]
fn test_validate_read_timeout_above_max() {
    let mut config = FlashConfig::default();
    config.websocket.read_timeout_ms = 60_000; // Above 30000

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("read_timeout_ms")));
}

/// Test validation accepts valid read timeout (Batch 1.3: Resilience).
#[test]
fn test_validate_read_timeout_valid_range() {
    let mut config = FlashConfig::default();
    config.websocket.read_timeout_ms = 5000; // Valid: 1000-30000

    let result = config.validate();
    assert!(result.is_ok(), "Valid read_timeout should pass validation");
}

/// Test validation rejects max_levels < max_depth.
#[test]
fn test_validate_orderbook_max_levels_gte_max_depth() {
    let mut config = FlashConfig::default();
    config.orderbook.max_depth = 100;
    config.orderbook.max_levels = 50; // Less than max_depth

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("max_levels")));
}

/// Test validation rejects invalid Redis URL.
#[test]
fn test_validate_redis_url_format() {
    let mut config = FlashConfig::default();
    config.redis.url = "not-a-valid-url".to_string();

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("url")));
}

/// Test validation rejects Redis database out of range.
#[test]
fn test_validate_redis_database_range() {
    let mut config = FlashConfig::default();
    config.redis.database = 16; // Max is 15

    let result = config.validate();
    assert!(result.is_err());
}

/// Test validation rejects warn_threshold > critical_threshold.
#[test]
fn test_validate_backpressure_thresholds_ordering() {
    let mut config = FlashConfig::default();
    config.publisher.backpressure.warn_threshold = 0.95;
    config.publisher.backpressure.critical_threshold = 0.80;

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("threshold")));
}

/// Test validation rejects metrics port below 1024.
#[test]
fn test_validate_metrics_port_range_below() {
    let mut config = FlashConfig::default();
    config.metrics.port = 80; // Below 1024

    let result = config.validate();
    assert!(result.is_err());
}

/// Test validation rejects reconnect_jitter above 0.5.
#[test]
fn test_validate_reconnect_jitter_range() {
    let mut config = FlashConfig::default();
    config.websocket.reconnect_jitter = 0.75; // Above 0.5

    let result = config.validate();
    assert!(result.is_err());
}

/// Test validation rejects empty topic_prefix.
#[test]
fn test_validate_topic_prefix_not_empty() {
    let mut config = FlashConfig::default();
    config.publisher.topic_prefix = "".to_string();

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("topic_prefix")));
}

/// Test validation returns all errors, not just first.
#[test]
fn test_validate_returns_all_errors() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 0; // Invalid
    config.redis.database = 20; // Invalid
    config.publisher.topic_prefix = "".to_string(); // Invalid

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.len() >= 3, "Should return at least 3 errors, got {}", errors.len());
}

// =============================================================================
// ERROR HANDLING TESTS (4)
// =============================================================================

/// Test invalid YAML syntax produces clear error.
#[test]
fn test_invalid_yaml_syntax_error() {
    let yaml = r#"
        websocket:
            connect_timeout_ms: "not closed
    "#;

    let result = FlashConfig::from_yaml(yaml);
    assert!(result.is_err());

    let err = result.unwrap_err();
    // Should be a parse error
    assert!(err.to_string().contains("parse") || err.to_string().contains("syntax"));
}

/// Test invalid enum value produces clear error.
#[test]
fn test_invalid_enum_value_error() {
    let yaml = r#"
        publisher:
            format: "invalid_format"
    "#;

    let result = FlashConfig::from_yaml(yaml);
    assert!(result.is_err());
}

/// Test wrong type value produces clear error.
#[test]
fn test_wrong_type_value_error() {
    let yaml = r#"
        websocket:
            connect_timeout_ms: "not_a_number"
    "#;

    let result = FlashConfig::from_yaml(yaml);
    assert!(result.is_err());
}

/// Test error message includes field path.
#[test]
fn test_error_message_includes_field_path() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 0;

    let result = config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let error_str = errors[0].to_string();
    // Should mention the field
    assert!(error_str.contains("connect_timeout") || error_str.contains("websocket"));
}

// =============================================================================
// SERIALIZATION TESTS (3)
// =============================================================================

/// Test configuration round-trips through YAML.
#[test]
fn test_config_round_trip_yaml() {
    let config = FlashConfig::default();

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&config).expect("Should serialize");

    // Deserialize back
    let parsed: FlashConfig = serde_yaml::from_str(&yaml).expect("Should deserialize");

    // Key fields should match
    assert_eq!(config.websocket.connect_timeout_ms, parsed.websocket.connect_timeout_ms);
    assert_eq!(config.redis.url, parsed.redis.url);
    assert_eq!(config.publisher.format, parsed.publisher.format);
}

/// Test SerializationFormat enum serializes correctly.
#[test]
fn test_serialization_format_enum_values() {
    assert_eq!(
        serde_yaml::to_string(&SerializationFormat::Json).unwrap().trim(),
        "json"
    );
    assert_eq!(
        serde_yaml::to_string(&SerializationFormat::Bincode).unwrap().trim(),
        "bincode"
    );
    assert_eq!(
        serde_yaml::to_string(&SerializationFormat::Rkyv).unwrap().trim(),
        "rkyv"
    );
}

/// Test LogLevel enum serializes correctly.
#[test]
fn test_log_level_enum_values() {
    assert_eq!(serde_yaml::to_string(&LogLevel::Trace).unwrap().trim(), "trace");
    assert_eq!(serde_yaml::to_string(&LogLevel::Debug).unwrap().trim(), "debug");
    assert_eq!(serde_yaml::to_string(&LogLevel::Info).unwrap().trim(), "info");
    assert_eq!(serde_yaml::to_string(&LogLevel::Warn).unwrap().trim(), "warn");
    assert_eq!(serde_yaml::to_string(&LogLevel::Error).unwrap().trim(), "error");
}

// =============================================================================
// THREAD SAFETY TESTS (2)
// =============================================================================

/// Verify FlashConfig is Send (can be transferred between threads).
#[test]
fn test_flash_config_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<FlashConfig>();
}

/// Verify FlashConfig is Sync (can be shared between threads).
#[test]
fn test_flash_config_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<FlashConfig>();
}

// =============================================================================
// EDGE CASE TESTS (3)
// =============================================================================

/// Test empty instruments list is valid.
#[test]
fn test_empty_instruments_list() {
    let config = FlashConfig::default();
    assert!(config.exchanges.deribit.instruments.is_empty());
    assert!(config.validate().is_ok());
}

/// Test maximum valid values.
#[test]
fn test_maximum_valid_values() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 60000;
    config.websocket.max_reconnect_attempts = 100;
    config.orderbook.max_depth = 1000;
    config.orderbook.max_levels = 10000;
    config.redis.pool_size = 100;
    config.metrics.port = 65535;

    assert!(config.validate().is_ok(), "Max valid values should pass validation");
}

/// Test minimum valid values.
#[test]
fn test_minimum_valid_values() {
    let mut config = FlashConfig::default();
    config.websocket.connect_timeout_ms = 1000;
    config.websocket.max_reconnect_attempts = 1;
    config.orderbook.max_depth = 1;
    config.orderbook.max_levels = 1; // Must be >= max_depth
    config.redis.pool_size = 1;
    config.metrics.port = 1024;

    assert!(config.validate().is_ok(), "Min valid values should pass validation");
}

// =============================================================================
// HELPER METHOD TESTS (2 bonus tests)
// =============================================================================

/// Test Duration conversion helper for WebSocket config.
#[test]
fn test_websocket_config_duration_helper() {
    let config = WebSocketConfig::default();
    let duration = config.connect_timeout();
    assert_eq!(duration.as_millis(), 5000);
}

/// Test Redis URL parsing helper.
#[test]
fn test_redis_config_parsed_url() {
    let config = RedisConfig::default();
    let parsed = config.parsed_url();
    assert!(parsed.is_ok());
    assert_eq!(parsed.unwrap().host_str(), Some("127.0.0.1"));
}
