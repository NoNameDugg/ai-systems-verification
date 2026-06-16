//! Configuration Validation Tests
//!
//! Tests for production configuration handling including:
//! - Valid configuration loading
//! - Invalid configuration handling
//! - Default values
//! - Environment variable overrides
//! - Per-exchange settings
//! - Configuration validation

use std::collections::HashMap;

use super::helpers::*;

// ============================================================================
// TEST 1: LOAD VALID CONFIGURATION
// ============================================================================

#[test]
fn test_config_load_valid() {
    // ARRANGE: Valid configuration
    let config = MockConfig::valid();

    // ACT: Validate configuration
    let result = config.validate();

    // ASSERT: Should succeed
    assert!(result.is_ok(), "Valid config should validate: {:?}", result);
}

// ============================================================================
// TEST 2: HANDLE INVALID CONFIGURATION
// ============================================================================

#[test]
fn test_config_load_invalid() {
    // ARRANGE: Invalid configuration
    let config = MockConfig::invalid();

    // ACT: Validate configuration
    let result = config.validate();

    // ASSERT: Should fail with errors
    assert!(result.is_err(), "Invalid config should fail validation");
    let errors = result.unwrap_err();
    assert!(!errors.is_empty(), "Should have validation errors");
}

// ============================================================================
// TEST 3: DEFAULT VALUES APPLIED
// ============================================================================

#[test]
fn test_config_default_values() {
    // ARRANGE: Default configuration
    let config = MockConfig::default();

    // ASSERT: Default values are sensible
    assert_eq!(config.exchange, "deribit");
    assert_eq!(config.redis_url, "redis://localhost:6379");
    assert_eq!(config.max_connections, 10);
    assert_eq!(config.timeout_ms, 5000);
    assert_eq!(config.log_level, "info");
    assert_eq!(config.metrics_port, 9090);
}

// ============================================================================
// TEST 4: ENVIRONMENT VARIABLE OVERRIDES
// ============================================================================

#[test]
fn test_config_env_override() {
    // ARRANGE: Configuration with environment overrides
    let mut config = MockConfig::default();
    let mut env = HashMap::new();
    env.insert("ASTRA_EXCHANGE".to_string(), "binance".to_string());
    env.insert("ASTRA_LOG_LEVEL".to_string(), "debug".to_string());
    env.insert("ASTRA_METRICS_PORT".to_string(), "8080".to_string());

    // ACT: Apply environment overrides
    config.apply_env_overrides(&env);

    // ASSERT: Values overridden
    assert_eq!(config.exchange, "binance");
    assert_eq!(config.log_level, "debug");
    assert_eq!(config.metrics_port, 8080);
}

// ============================================================================
// TEST 5: PER-EXCHANGE CONFIGURATION
// ============================================================================

#[test]
fn test_config_exchange_settings() {
    // ARRANGE: Multiple exchange configurations
    let exchanges = vec![
        MockConfig {
            exchange: "deribit".to_string(),
            timeout_ms: 3000,
            ..Default::default()
        },
        MockConfig {
            exchange: "binance".to_string(),
            timeout_ms: 5000,
            ..Default::default()
        },
        MockConfig {
            exchange: "oanda".to_string(),
            timeout_ms: 10000,
            ..Default::default()
        },
    ];

    // ACT: Validate each configuration
    for config in &exchanges {
        let result = config.validate();
        assert!(result.is_ok(), "{} config should be valid", config.exchange);
    }

    // ASSERT: Each has different settings
    assert_eq!(exchanges[0].timeout_ms, 3000);
    assert_eq!(exchanges[1].timeout_ms, 5000);
    assert_eq!(exchanges[2].timeout_ms, 10000);
}

// ============================================================================
// TEST 6: REDIS CONFIGURATION VALIDATION
// ============================================================================

#[test]
fn test_config_redis_settings() {
    // ARRANGE: Various Redis configurations
    let valid_urls = vec![
        "redis://localhost:6379",
        "redis://127.0.0.1:6379",
        "redis://redis-server:6379",
    ];

    let invalid_urls = vec![
        "localhost:6379",        // Missing scheme
        "http://localhost:6379", // Wrong scheme
        "",                      // Empty
    ];

    // ACT & ASSERT: Valid URLs pass
    for url in valid_urls {
        let config = MockConfig {
            redis_url: url.to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok(), "{} should be valid", url);
    }

    // ACT & ASSERT: Invalid URLs fail
    for url in invalid_urls {
        let config = MockConfig {
            redis_url: url.to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err(), "{} should be invalid", url);
    }
}

// ============================================================================
// TEST 7: CONFIGURATION VALIDATION ERRORS
// ============================================================================

#[test]
fn test_config_validation_errors() {
    // ARRANGE: Config with multiple errors
    let config = MockConfig {
        exchange: "".to_string(),
        redis_url: "invalid".to_string(),
        max_connections: 0,
        timeout_ms: 0,
        ..Default::default()
    };

    // ACT: Validate
    let result = config.validate();

    // ASSERT: All errors reported
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(
        errors.len() >= 4,
        "Should have at least 4 errors: {:?}",
        errors
    );
}

// ============================================================================
// TEST 8: CONFIGURATION IMMUTABILITY AFTER LOAD
// ============================================================================

#[test]
fn test_config_immutability() {
    // ARRANGE: Load configuration
    let original = MockConfig::valid();
    let clone = original.clone();

    // ACT: Verify clone is independent
    let mut modified = clone.clone();
    modified.exchange = "modified".to_string();

    // ASSERT: Original unchanged
    assert_eq!(original.exchange, "deribit");
    assert_eq!(modified.exchange, "modified");
}
