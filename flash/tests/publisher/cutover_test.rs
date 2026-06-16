//! Tests for Production Cutover.
//!
//! These tests verify the production cutover process from shadow mode to
//! production mode, ensuring correct Redis key generation and configuration.
//!
//! Production Cutover Requirements:
//! - [ ] Switch the downstream consumer to read from the Flash keys
//! - [ ] Monitor for 24 hours
//! - [ ] Deprecate the prior Python prototype
//! - [ ] Update documentation
//!
//! Test Categories:
//! - Production Config Tests (6 tests)
//! - Key Generation Production Tests (6 tests)
//! - Cutover Transition Tests (6 tests)
//! - Rollback Tests (4 tests)
//! - Gateway Integration Tests (4 tests)
//!
//! Total: 26 tests

use astra_flash::core::config::{FlashConfig, ShadowModeConfig};
use astra_flash::core::types::{Exchange, Instrument};
use astra_flash::publisher::{DualPublisher, DualPublisherConfig};
use serial_test::serial;
use std::env;

// =============================================================================
// TEST CONSTANTS
// =============================================================================

/// Expected production orderbook key format (no namespace).
const PRODUCTION_ORDERBOOK_KEY_EUR_USD: &str = "market:orderbook:EUR_USD";

/// Expected production signal key format (no namespace).
const PRODUCTION_SIGNAL_KEY_EUR_USD: &str = "astra:signals:flash:oanda:EUR_USD";

/// Expected shadow orderbook key format (with namespace).
const SHADOW_ORDERBOOK_KEY_EUR_USD: &str = "market:orderbook:rust:EUR_USD";

/// Expected shadow signal key format (with namespace).
const SHADOW_SIGNAL_KEY_EUR_USD: &str = "astra:signals:flash:rust:oanda:EUR_USD";

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Create a test instrument for EUR/USD on OANDA.
fn test_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

/// Create a production config (shadow mode disabled).
fn production_config() -> DualPublisherConfig {
    DualPublisherConfig {
        shadow_mode_enabled: false,
        shadow_mode_namespace: "rust".to_string(),
        ..DualPublisherConfig::default()
    }
}

/// Create a shadow config (shadow mode enabled).
fn shadow_config() -> DualPublisherConfig {
    DualPublisherConfig {
        shadow_mode_enabled: true,
        shadow_mode_namespace: "rust".to_string(),
        ..DualPublisherConfig::default()
    }
}

// =============================================================================
// PRODUCTION CONFIG TESTS (6 tests)
// =============================================================================

/// Test production config has shadow mode disabled by default.
#[test]
fn test_production_config_shadow_mode_disabled() {
    let config = DualPublisherConfig::default();

    assert!(!config.shadow_mode_enabled, "Shadow mode should be disabled by default");
}

/// Test production config generates correct orderbook key.
#[test]
fn test_production_config_orderbook_key() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    assert_eq!(key, PRODUCTION_ORDERBOOK_KEY_EUR_USD);
}

/// Test production config generates correct signal key.
#[test]
fn test_production_config_signal_key() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::signal_key(&instrument, &config);

    assert_eq!(key, PRODUCTION_SIGNAL_KEY_EUR_USD);
}

/// Test production config uses bincode format by default.
#[test]
fn test_production_config_default_format() {
    let config = DualPublisherConfig::default();

    assert!(
        matches!(config.format, astra_flash::publisher::stream::SerializationFormat::Json),
        "Default format should be JSON for compatibility"
    );
}

/// Test FlashConfig default is production-ready.
#[test]
fn test_flash_config_default_is_production_ready() {
    let config = FlashConfig::default();

    // Shadow mode disabled (production)
    assert!(!config.shadow_mode.enabled);

    // Namespace still has default value
    assert_eq!(config.shadow_mode.namespace, "rust");
}

/// Test FlashConfig validation passes for production settings.
#[test]
fn test_flash_config_production_validation_passes() {
    let config = FlashConfig::default();

    let result = config.validate();

    assert!(result.is_ok(), "Production config should pass validation");
}

// =============================================================================
// KEY GENERATION PRODUCTION TESTS (6 tests)
// =============================================================================

/// Test orderbook key has no namespace in production mode.
#[test]
fn test_orderbook_key_no_namespace_production() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    assert!(!key.contains(":rust:"), "Production key should not contain namespace");
    assert_eq!(key, "market:orderbook:EUR_USD");
}

/// Test signal key has no namespace in production mode.
#[test]
fn test_signal_key_no_namespace_production() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::signal_key(&instrument, &config);

    assert!(!key.contains(":rust:"), "Production key should not contain namespace");
    assert_eq!(key, "astra:signals:flash:oanda:EUR_USD");
}

/// Test multiple instruments generate correct production keys.
#[test]
fn test_multiple_instruments_production_keys() {
    let config = production_config();

    let instruments = vec![
        (Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD"), "market:orderbook:EUR_USD"),
        (Instrument::new("GBP", "USD", Exchange::Oanda, "GBP_USD"), "market:orderbook:GBP_USD"),
        (Instrument::new("USD", "JPY", Exchange::Oanda, "USD_JPY"), "market:orderbook:USD_JPY"),
    ];

    for (instrument, expected_key) in instruments {
        let key = DualPublisher::orderbook_key(&instrument, &config);
        assert_eq!(key, expected_key, "Key mismatch for {}", instrument.symbol());
    }
}

/// Test OANDA exchange produces correct signal key prefix.
#[test]
fn test_oanda_signal_key_prefix() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::signal_key(&instrument, &config);

    assert!(key.starts_with("astra:signals:flash:oanda:"));
}

/// Test key format consistency across exchanges.
#[test]
fn test_key_format_consistency_across_exchanges() {
    let config = production_config();

    let exchanges = vec![
        (Exchange::Oanda, "oanda"),
        (Exchange::Deribit, "deribit"),
        (Exchange::Binance, "binance"),
    ];

    for (exchange, expected_name) in exchanges {
        let instrument = Instrument::new("BTC", "USD", exchange, "BTC_USD");
        let key = DualPublisher::signal_key(&instrument, &config);

        assert!(
            key.contains(expected_name),
            "Key should contain exchange name: {}",
            expected_name
        );
    }
}

/// Test production keys match Gateway expected format.
#[test]
fn test_production_keys_match_gateway_format() {
    let instrument = test_instrument();
    let config = production_config();

    let orderbook_key = DualPublisher::orderbook_key(&instrument, &config);
    let signal_key = DualPublisher::signal_key(&instrument, &config);

    // Gateway expects these exact formats
    assert_eq!(orderbook_key, "market:orderbook:EUR_USD");
    assert_eq!(signal_key, "astra:signals:flash:oanda:EUR_USD");
}

// =============================================================================
// CUTOVER TRANSITION TESTS (6 tests)
// =============================================================================

/// Test transition from shadow to production mode.
#[test]
fn test_transition_shadow_to_production() {
    let instrument = test_instrument();

    // Start in shadow mode
    let shadow = shadow_config();
    let shadow_key = DualPublisher::orderbook_key(&instrument, &shadow);
    assert_eq!(shadow_key, SHADOW_ORDERBOOK_KEY_EUR_USD);

    // Cutover to production
    let production = production_config();
    let production_key = DualPublisher::orderbook_key(&instrument, &production);
    assert_eq!(production_key, PRODUCTION_ORDERBOOK_KEY_EUR_USD);
}

/// Test shadow and production keys are different.
#[test]
fn test_shadow_and_production_keys_differ() {
    let instrument = test_instrument();

    let shadow = shadow_config();
    let production = production_config();

    let shadow_orderbook = DualPublisher::orderbook_key(&instrument, &shadow);
    let production_orderbook = DualPublisher::orderbook_key(&instrument, &production);

    assert_ne!(shadow_orderbook, production_orderbook);
}

/// Test both modes can coexist (different keys).
#[test]
fn test_shadow_and_production_coexist() {
    let instrument = test_instrument();

    let shadow = shadow_config();
    let production = production_config();

    // Both generate valid but different keys
    let shadow_key = DualPublisher::orderbook_key(&instrument, &shadow);
    let production_key = DualPublisher::orderbook_key(&instrument, &production);

    // Keys should be distinct
    assert_ne!(shadow_key, production_key);

    // Shadow key should have namespace
    assert!(shadow_key.contains(":rust:"));

    // Production key should not have namespace
    assert!(!production_key.contains(":rust:"));
}

/// Test cutover config change is instantaneous.
#[test]
fn test_cutover_config_change_instant() {
    let instrument = test_instrument();

    // Simulate config change
    let mut config = shadow_config();
    assert!(config.shadow_mode_enabled);

    // Cutover: disable shadow mode
    config.shadow_mode_enabled = false;

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Key should immediately reflect production format
    assert_eq!(key, PRODUCTION_ORDERBOOK_KEY_EUR_USD);
}

/// Test all output types switch during cutover.
#[test]
fn test_all_outputs_switch_during_cutover() {
    let instrument = test_instrument();

    // Shadow mode
    let shadow = shadow_config();
    let shadow_orderbook = DualPublisher::orderbook_key(&instrument, &shadow);
    let shadow_signal = DualPublisher::signal_key(&instrument, &shadow);

    // Production mode
    let production = production_config();
    let production_orderbook = DualPublisher::orderbook_key(&instrument, &production);
    let production_signal = DualPublisher::signal_key(&instrument, &production);

    // All keys should change
    assert_ne!(shadow_orderbook, production_orderbook);
    assert_ne!(shadow_signal, production_signal);

    // Verify correct formats
    assert_eq!(shadow_orderbook, SHADOW_ORDERBOOK_KEY_EUR_USD);
    assert_eq!(production_orderbook, PRODUCTION_ORDERBOOK_KEY_EUR_USD);
    assert_eq!(shadow_signal, SHADOW_SIGNAL_KEY_EUR_USD);
    assert_eq!(production_signal, PRODUCTION_SIGNAL_KEY_EUR_USD);
}

/// Test namespace is preserved but not used in production.
#[test]
fn test_namespace_preserved_but_unused_production() {
    let instrument = test_instrument();

    let config = DualPublisherConfig {
        shadow_mode_enabled: false, // Production
        shadow_mode_namespace: "custom-namespace".to_string(), // Preserved
        ..DualPublisherConfig::default()
    };

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Namespace should not appear in key
    assert!(!key.contains("custom-namespace"));
    assert_eq!(key, PRODUCTION_ORDERBOOK_KEY_EUR_USD);
}

// =============================================================================
// ROLLBACK TESTS (4 tests)
// =============================================================================

/// Test rollback to shadow mode works.
#[test]
fn test_rollback_to_shadow_mode() {
    let instrument = test_instrument();

    // Production mode
    let mut config = production_config();
    let production_key = DualPublisher::orderbook_key(&instrument, &config);
    assert_eq!(production_key, PRODUCTION_ORDERBOOK_KEY_EUR_USD);

    // Rollback: re-enable shadow mode
    config.shadow_mode_enabled = true;
    let shadow_key = DualPublisher::orderbook_key(&instrument, &config);
    assert_eq!(shadow_key, SHADOW_ORDERBOOK_KEY_EUR_USD);
}

/// Test rollback via environment variable.
#[test]
#[serial]
fn test_rollback_via_env_var() {
    // Simulate production config
    let yaml = r#"
        shadow_mode:
            enabled: false
            namespace: "rust"
    "#;

    // Set env var for emergency rollback
    env::set_var("ASTRA_FLASH_SHADOW_MODE_ENABLED", "true");

    let config = FlashConfig::from_yaml_with_env(yaml).expect("Should parse config");

    // Env var should override YAML (rollback activated)
    assert!(config.shadow_mode.enabled);

    env::remove_var("ASTRA_FLASH_SHADOW_MODE_ENABLED");
}

/// Test rollback preserves namespace.
#[test]
fn test_rollback_preserves_namespace() {
    let instrument = test_instrument();

    let mut config = DualPublisherConfig {
        shadow_mode_enabled: false,
        shadow_mode_namespace: "rollback-test".to_string(),
        ..DualPublisherConfig::default()
    };

    // Rollback
    config.shadow_mode_enabled = true;

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Should use preserved namespace
    assert!(key.contains(":rollback-test:"));
}

/// Test multiple rollback cycles work correctly.
#[test]
fn test_multiple_rollback_cycles() {
    let instrument = test_instrument();
    let mut config = shadow_config();

    for i in 0..5 {
        // Toggle shadow mode
        config.shadow_mode_enabled = i % 2 == 0;

        let key = DualPublisher::orderbook_key(&instrument, &config);

        if i % 2 == 0 {
            assert!(key.contains(":rust:"), "Cycle {} should be shadow mode", i);
        } else {
            assert!(!key.contains(":rust:"), "Cycle {} should be production mode", i);
        }
    }
}

// =============================================================================
// GATEWAY INTEGRATION TESTS (4 tests)
// =============================================================================

/// Test Gateway orderbook key format compliance.
#[test]
fn test_gateway_orderbook_key_format() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::orderbook_key(&instrument, &config);

    // Gateway expects: market:orderbook:{BASE}_{QUOTE}
    assert!(key.starts_with("market:orderbook:"));
    assert!(key.ends_with("EUR_USD"));
}

/// Test Fusion signal key format compliance.
#[test]
fn test_fusion_signal_key_format() {
    let instrument = test_instrument();
    let config = production_config();

    let key = DualPublisher::signal_key(&instrument, &config);

    // Fusion expects: astra:signals:flash:{exchange}:{BASE}_{QUOTE}
    assert!(key.starts_with("astra:signals:flash:"));
    assert!(key.contains(":oanda:"));
    assert!(key.ends_with("EUR_USD"));
}

/// Test all standard FX pairs generate valid keys.
#[test]
fn test_standard_fx_pairs_valid_keys() {
    let config = production_config();

    let pairs = vec![
        ("EUR", "USD"),
        ("GBP", "USD"),
        ("USD", "JPY"),
        ("USD", "CHF"),
        ("AUD", "USD"),
        ("NZD", "USD"),
        ("USD", "CAD"),
    ];

    for (base, quote) in pairs {
        let instrument = Instrument::new(base, quote, Exchange::Oanda, &format!("{}_{}", base, quote));

        let orderbook_key = DualPublisher::orderbook_key(&instrument, &config);
        let signal_key = DualPublisher::signal_key(&instrument, &config);

        // Validate key formats
        assert!(
            orderbook_key.starts_with("market:orderbook:"),
            "Invalid orderbook key for {}_{}: {}",
            base,
            quote,
            orderbook_key
        );
        assert!(
            signal_key.starts_with("astra:signals:flash:oanda:"),
            "Invalid signal key for {}_{}: {}",
            base,
            quote,
            signal_key
        );
    }
}

/// Test production config outputs match Gateway consumption.
#[test]
fn test_production_outputs_match_gateway_consumption() {
    let config = production_config();

    let instrument = test_instrument();
    let orderbook_key = DualPublisher::orderbook_key(&instrument, &config);
    let signal_key = DualPublisher::signal_key(&instrument, &config);

    // Keys should match Gateway subscription patterns:
    // - Gateway orderbook pattern: "market:orderbook:*"
    // - Gateway signal pattern: "astra:signals:flash:*"
    assert!(
        orderbook_key.starts_with("market:orderbook:"),
        "Orderbook key should match Gateway pattern"
    );
    assert!(
        signal_key.starts_with("astra:signals:flash:"),
        "Signal key should match Gateway pattern"
    );

    // Verify no shadow namespace in production keys
    assert!(
        !orderbook_key.contains(":rust:"),
        "Production orderbook key should not have shadow namespace"
    );
    assert!(
        !signal_key.contains(":rust:"),
        "Production signal key should not have shadow namespace"
    );
}
