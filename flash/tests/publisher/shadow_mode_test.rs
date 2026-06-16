//! Tests for Shadow Mode.
//!
//! Shadow Mode allows this engine to run in parallel with an existing producer,
//! writing to separate Redis namespaces for validation before cutover.
//!
//! Redis Key Namespaces:
//! - Primary (incumbent): `market:orderbook:EUR_USD`, `astra:signals:flash:oanda:EUR_USD`
//! - Shadow (Flash):      `market:orderbook:rust:EUR_USD`, `astra:signals:flash:rust:oanda:EUR_USD`
//!
//! Test Categories:
//! - ShadowModeConfig Tests (6 tests)
//! - Key Generation Tests (8 tests)
//! - Environment Variable Tests (4 tests)
//! - Integration Tests (4 tests)
//!
//! Total: 22 tests

use astra_flash::core::config::{FlashConfig, ShadowModeConfig};
use astra_flash::core::types::{Exchange, Instrument};
use astra_flash::fusion::FUSION_TOPIC_PREFIX;
use astra_flash::publisher::{DualPublisher, DualPublisherConfig};
use serial_test::serial;
use std::env;

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Create a test instrument for EUR/USD
fn test_instrument_eur_usd() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

/// Create a test instrument for BTC/USD
fn test_instrument_btc_usd() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

// =============================================================================
// SHADOW MODE CONFIG TESTS (6 tests)
// =============================================================================

/// Test ShadowModeConfig has correct defaults.
#[test]
fn test_shadow_mode_config_default_disabled() {
    let config = ShadowModeConfig::default();

    // Shadow mode should be disabled by default (production behavior)
    assert!(!config.enabled);
    assert_eq!(config.namespace, "rust");
}

/// Test ShadowModeConfig can be enabled.
#[test]
fn test_shadow_mode_config_enabled() {
    let config = ShadowModeConfig {
        enabled: true,
        namespace: "rust".to_string(),
    };

    assert!(config.enabled);
    assert_eq!(config.namespace, "rust");
}

/// Test ShadowModeConfig with custom namespace.
#[test]
fn test_shadow_mode_config_custom_namespace() {
    let config = ShadowModeConfig {
        enabled: true,
        namespace: "staging".to_string(),
    };

    assert!(config.enabled);
    assert_eq!(config.namespace, "staging");
}

/// Test ShadowModeConfig validation - namespace must not be empty when enabled.
#[test]
fn test_shadow_mode_config_empty_namespace_validation() {
    let mut flash_config = FlashConfig::default();
    flash_config.shadow_mode.enabled = true;
    flash_config.shadow_mode.namespace = String::new();

    let result = flash_config.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.to_string().contains("namespace")));
}

/// Test ShadowModeConfig validation - disabled mode allows empty namespace.
#[test]
fn test_shadow_mode_config_disabled_allows_empty_namespace() {
    let mut flash_config = FlashConfig::default();
    flash_config.shadow_mode.enabled = false;
    flash_config.shadow_mode.namespace = String::new();

    // Should still pass validation because shadow mode is disabled
    let result = flash_config.validate();
    assert!(result.is_ok());
}

/// Test FlashConfig includes shadow_mode field.
#[test]
fn test_flash_config_includes_shadow_mode() {
    let config = FlashConfig::default();

    // Verify shadow_mode field exists and has default values
    assert!(!config.shadow_mode.enabled);
    assert_eq!(config.shadow_mode.namespace, "rust");
}

// =============================================================================
// KEY GENERATION TESTS (8 tests)
// =============================================================================

/// Test orderbook key generation - production mode (shadow disabled).
#[test]
fn test_orderbook_key_production_mode() {
    let instrument = test_instrument_eur_usd();
    let config = DualPublisherConfig::default();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Production: market:orderbook:EUR_USD (no namespace)
    assert_eq!(key, "market:orderbook:EUR_USD");
}

/// Test orderbook key generation - shadow mode enabled.
#[test]
fn test_orderbook_key_shadow_mode() {
    let instrument = test_instrument_eur_usd();
    let mut config = DualPublisherConfig::default();
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "rust".to_string();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Shadow: market:orderbook:rust:EUR_USD
    assert_eq!(key, "market:orderbook:rust:EUR_USD");
}

/// Test signal key generation - production mode (shadow disabled).
#[test]
fn test_signal_key_production_mode() {
    let instrument = test_instrument_eur_usd();
    let config = DualPublisherConfig::default();

    let key = DualPublisher::signal_key(&instrument, &config);

    // Production: astra:signals:flash:oanda:EUR_USD
    assert_eq!(key, "astra:signals:flash:oanda:EUR_USD");
}

/// Test signal key generation - shadow mode enabled.
#[test]
fn test_signal_key_shadow_mode() {
    let instrument = test_instrument_eur_usd();
    let mut config = DualPublisherConfig::default();
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "rust".to_string();

    let key = DualPublisher::signal_key(&instrument, &config);

    // Shadow: astra:signals:flash:rust:oanda:EUR_USD
    assert_eq!(key, "astra:signals:flash:rust:oanda:EUR_USD");
}

/// Test shadow mode with BTC/Deribit instrument.
#[test]
fn test_shadow_mode_keys_btc_deribit() {
    let instrument = test_instrument_btc_usd();
    let mut config = DualPublisherConfig::default();
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "rust".to_string();

    let orderbook_key = DualPublisher::orderbook_key(&instrument, &config);
    let signal_key = DualPublisher::signal_key(&instrument, &config);

    assert_eq!(orderbook_key, "market:orderbook:rust:BTC_USD");
    assert_eq!(signal_key, "astra:signals:flash:rust:deribit:BTC_USD");
}

/// Test shadow mode with custom namespace.
#[test]
fn test_shadow_mode_custom_namespace_keys() {
    let instrument = test_instrument_eur_usd();
    let mut config = DualPublisherConfig::default();
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "canary".to_string();

    let orderbook_key = DualPublisher::orderbook_key(&instrument, &config);
    let signal_key = DualPublisher::signal_key(&instrument, &config);

    assert_eq!(orderbook_key, "market:orderbook:canary:EUR_USD");
    assert_eq!(signal_key, "astra:signals:flash:canary:oanda:EUR_USD");
}

/// Test that disabling shadow mode after enabling reverts to production keys.
#[test]
fn test_shadow_mode_disable_reverts_to_production() {
    let instrument = test_instrument_eur_usd();
    let mut config = DualPublisherConfig::default();

    // Enable shadow mode
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "rust".to_string();
    let shadow_key = DualPublisher::orderbook_key(&instrument, &config);
    assert_eq!(shadow_key, "market:orderbook:rust:EUR_USD");

    // Disable shadow mode
    config.shadow_mode_enabled = false;
    let production_key = DualPublisher::orderbook_key(&instrument, &config);
    assert_eq!(production_key, "market:orderbook:EUR_USD");
}

/// Test shadow mode with custom key prefix and namespace.
#[test]
fn test_shadow_mode_custom_prefix_and_namespace() {
    let instrument = test_instrument_eur_usd();
    let mut config = DualPublisherConfig::default();
    config.orderbook_key_prefix = "custom:book:".to_string();
    config.shadow_mode_enabled = true;
    config.shadow_mode_namespace = "test".to_string();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Custom prefix + shadow namespace: custom:book:test:EUR_USD
    assert_eq!(key, "custom:book:test:EUR_USD");
}

// =============================================================================
// ENVIRONMENT VARIABLE TESTS (4 tests)
// =============================================================================

/// Test shadow mode can be enabled via environment variable.
#[test]
#[serial]
fn test_shadow_mode_env_var_enabled() {
    env::set_var("ASTRA_FLASH_SHADOW_MODE_ENABLED", "true");

    let config = FlashConfig::from_env().expect("Should load from env");

    assert!(config.shadow_mode.enabled);

    env::remove_var("ASTRA_FLASH_SHADOW_MODE_ENABLED");
}

/// Test shadow mode can be disabled via environment variable.
#[test]
#[serial]
fn test_shadow_mode_env_var_disabled() {
    env::set_var("ASTRA_FLASH_SHADOW_MODE_ENABLED", "false");

    let config = FlashConfig::from_env().expect("Should load from env");

    assert!(!config.shadow_mode.enabled);

    env::remove_var("ASTRA_FLASH_SHADOW_MODE_ENABLED");
}

/// Test shadow mode namespace can be set via environment variable.
#[test]
#[serial]
fn test_shadow_mode_env_var_namespace() {
    env::set_var("ASTRA_FLASH_SHADOW_MODE_NAMESPACE", "staging");

    let config = FlashConfig::from_env().expect("Should load from env");

    assert_eq!(config.shadow_mode.namespace, "staging");

    env::remove_var("ASTRA_FLASH_SHADOW_MODE_NAMESPACE");
}

/// Test environment variable overrides YAML config.
#[test]
#[serial]
fn test_shadow_mode_env_var_overrides_yaml() {
    env::set_var("ASTRA_FLASH_SHADOW_MODE_ENABLED", "true");

    let yaml = r#"
        shadow_mode:
            enabled: false
            namespace: "production"
    "#;

    let config = FlashConfig::from_yaml_with_env(yaml).expect("Should parse with env override");

    // Env var should take precedence
    assert!(config.shadow_mode.enabled);

    env::remove_var("ASTRA_FLASH_SHADOW_MODE_ENABLED");
}

// =============================================================================
// YAML LOADING TESTS (4 tests)
// =============================================================================

/// Test loading shadow mode config from YAML.
#[test]
fn test_shadow_mode_yaml_loading() {
    let yaml = r#"
        shadow_mode:
            enabled: true
            namespace: "rust"
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse shadow mode YAML");

    assert!(config.shadow_mode.enabled);
    assert_eq!(config.shadow_mode.namespace, "rust");
}

/// Test loading partial shadow mode config uses defaults.
#[test]
fn test_shadow_mode_yaml_partial_uses_defaults() {
    let yaml = r#"
        shadow_mode:
            enabled: true
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse partial YAML");

    assert!(config.shadow_mode.enabled);
    assert_eq!(config.shadow_mode.namespace, "rust"); // Default
}

/// Test empty YAML uses shadow mode defaults.
#[test]
fn test_shadow_mode_yaml_empty_uses_defaults() {
    let yaml = "";

    let config = FlashConfig::from_yaml(yaml).expect("Should handle empty YAML");

    assert!(!config.shadow_mode.enabled);
    assert_eq!(config.shadow_mode.namespace, "rust");
}

/// Test shadow mode YAML with custom namespace.
#[test]
fn test_shadow_mode_yaml_custom_namespace() {
    let yaml = r#"
        shadow_mode:
            enabled: true
            namespace: "canary"
    "#;

    let config = FlashConfig::from_yaml(yaml).expect("Should parse custom namespace");

    assert!(config.shadow_mode.enabled);
    assert_eq!(config.shadow_mode.namespace, "canary");
}

// =============================================================================
// DUAL PUBLISHER CONFIG TESTS (4 tests)
// =============================================================================

/// Test DualPublisherConfig includes shadow mode fields.
#[test]
fn test_dual_publisher_config_shadow_mode_fields() {
    let config = DualPublisherConfig::default();

    // Default: shadow mode disabled
    assert!(!config.shadow_mode_enabled);
    assert_eq!(config.shadow_mode_namespace, "rust");
}

/// Test DualPublisherBuilder supports shadow mode.
#[test]
fn test_dual_publisher_builder_shadow_mode() {
    use astra_flash::publisher::DualPublisherBuilder;

    let builder = DualPublisherBuilder::default()
        .shadow_mode_enabled(true)
        .shadow_mode_namespace("staging".to_string());

    let config = builder.config();

    assert!(config.shadow_mode_enabled);
    assert_eq!(config.shadow_mode_namespace, "staging");
}

/// Test DualPublisherConfig can be created from FlashConfig shadow settings.
#[test]
fn test_dual_publisher_config_from_flash_config() {
    let mut flash_config = FlashConfig::default();
    flash_config.shadow_mode.enabled = true;
    flash_config.shadow_mode.namespace = "test".to_string();

    let dual_config = DualPublisherConfig {
        shadow_mode_enabled: flash_config.shadow_mode.enabled,
        shadow_mode_namespace: flash_config.shadow_mode.namespace.clone(),
        ..DualPublisherConfig::default()
    };

    assert!(dual_config.shadow_mode_enabled);
    assert_eq!(dual_config.shadow_mode_namespace, "test");
}

/// Test shadow mode preserves other DualPublisherConfig settings.
#[test]
fn test_shadow_mode_preserves_other_config() {
    let config = DualPublisherConfig {
        orderbook_enabled: true,
        signal_enabled: false, // Deliberately different
        ttl_seconds: 120,
        shadow_mode_enabled: true,
        shadow_mode_namespace: "rust".to_string(),
        ..DualPublisherConfig::default()
    };

    // Shadow mode settings
    assert!(config.shadow_mode_enabled);
    assert_eq!(config.shadow_mode_namespace, "rust");

    // Other settings preserved
    assert!(config.orderbook_enabled);
    assert!(!config.signal_enabled);
    assert_eq!(config.ttl_seconds, 120);
}
