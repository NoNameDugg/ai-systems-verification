//! Gateway Types Integration Tests.
//!
//! Tests for OrderBookSnapshot and OrderBookLevel gateway-compatible types.
//!
//! # Test Categories
//!
//! 1. Serialization Tests (5 tests)
//! 2. Conversion Tests (3 tests)
//! 3. Validation Tests (4 tests)
//! 4. Calculation Tests (3 tests)
//!
//! Total: 15+ tests (TDI requirement)

use astra_flash::book::BookSnapshot;
use astra_flash::core::types::{Exchange, Instrument, PriceLevel};
use astra_flash::gateway::{OrderBookLevel, OrderBookSnapshot};
use rust_decimal_macros::dec;

// =============================================================================
// TEST FIXTURES
// =============================================================================

fn test_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

fn test_book_snapshot() -> BookSnapshot {
    let ts = astra_flash::core::types::now_micros();
    BookSnapshot {
        instrument: test_instrument(),
        timestamp: ts,
        bids: vec![
            PriceLevel::new(1.0850, dec!(1000000), ts),
            PriceLevel::new(1.0849, dec!(500000), ts),
        ],
        asks: vec![
            PriceLevel::new(1.0852, dec!(750000), ts),
            PriceLevel::new(1.0853, dec!(250000), ts),
        ],
    }
}

fn test_order_book_snapshot() -> OrderBookSnapshot {
    OrderBookSnapshot::new(
        "EUR_USD",
        "oanda",
        1234567890,
        vec![
            OrderBookLevel::new(1.0850, 1000000.0),
            OrderBookLevel::new(1.0849, 500000.0),
        ],
        vec![
            OrderBookLevel::new(1.0852, 750000.0),
            OrderBookLevel::new(1.0853, 250000.0),
        ],
    )
}

// =============================================================================
// 1. SERIALIZATION TESTS (5 tests)
// =============================================================================

/// Test OrderBookLevel JSON serialization.
#[test]
fn test_order_book_level_json_serialization() {
    let level = OrderBookLevel::new(1.0850, 1000000.0);
    let json = serde_json::to_string(&level).expect("Should serialize");

    assert!(json.contains("price"));
    assert!(json.contains("quantity"));
    assert!(json.contains("1.085"));
    assert!(json.contains("1000000"));
}

/// Test OrderBookLevel JSON deserialization.
#[test]
fn test_order_book_level_json_deserialization() {
    let json = r#"{"price":1.0850,"quantity":1000000.0}"#;
    let level: OrderBookLevel = serde_json::from_str(json).expect("Should deserialize");

    assert!((level.price - 1.0850).abs() < f64::EPSILON);
    assert!((level.quantity - 1000000.0).abs() < f64::EPSILON);
}

/// Test OrderBookSnapshot JSON serialization roundtrip.
#[test]
fn test_order_book_snapshot_json_roundtrip() {
    let snapshot = test_order_book_snapshot();
    let json = snapshot.to_json().expect("Should serialize");
    let deserialized: OrderBookSnapshot = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(snapshot.symbol, deserialized.symbol);
    assert_eq!(snapshot.exchange, deserialized.exchange);
    assert_eq!(snapshot.bids.len(), deserialized.bids.len());
    assert_eq!(snapshot.asks.len(), deserialized.asks.len());
}

/// Test OrderBookSnapshot to_json_bytes.
#[test]
fn test_order_book_snapshot_to_json_bytes() {
    let snapshot = test_order_book_snapshot();
    let bytes = snapshot.to_json_bytes().expect("Should serialize");

    assert!(!bytes.is_empty());
    // Verify bytes can be parsed back
    let deserialized: OrderBookSnapshot =
        serde_json::from_slice(&bytes).expect("Should deserialize");
    assert_eq!(snapshot.symbol, deserialized.symbol);
}

/// Test JSON output format is Gateway-compatible.
#[test]
fn test_json_format_gateway_compatible() {
    let snapshot = test_order_book_snapshot();
    let json = snapshot.to_json().expect("Should serialize");

    // Check required fields are present
    assert!(json.contains("symbol"));
    assert!(json.contains("exchange"));
    assert!(json.contains("timestamp"));
    assert!(json.contains("bids"));
    assert!(json.contains("asks"));

    // Check values
    assert!(json.contains("EUR_USD"));
    assert!(json.contains("oanda"));
}

// =============================================================================
// 2. CONVERSION TESTS (3 tests)
// =============================================================================

/// Test OrderBookLevel from_price_level conversion.
#[test]
fn test_order_book_level_from_price_level() {
    let ts = astra_flash::core::types::now_micros();
    let price_level = PriceLevel::new(1.0850, dec!(1000000), ts);
    let level = OrderBookLevel::from_price_level(&price_level);

    assert!((level.price - 1.0850).abs() < f64::EPSILON);
    assert!((level.quantity - 1000000.0).abs() < f64::EPSILON);
}

/// Test OrderBookSnapshot from_book_snapshot conversion.
#[test]
fn test_order_book_snapshot_from_book_snapshot() {
    let book_snapshot = test_book_snapshot();
    let snapshot = OrderBookSnapshot::from_book_snapshot(&book_snapshot);

    assert_eq!(snapshot.symbol, "EUR_USD");
    assert_eq!(snapshot.exchange, "oanda");
    assert_eq!(snapshot.bids.len(), 2);
    assert_eq!(snapshot.asks.len(), 2);

    // Check level prices
    assert!((snapshot.bids[0].price - 1.0850).abs() < f64::EPSILON);
    assert!((snapshot.asks[0].price - 1.0852).abs() < f64::EPSILON);
}

/// Test OrderBookSnapshot preserves order after conversion.
#[test]
fn test_order_book_snapshot_preserves_order() {
    let book_snapshot = test_book_snapshot();
    let snapshot = OrderBookSnapshot::from_book_snapshot(&book_snapshot);

    // Bids should be descending (best bid first)
    assert!(snapshot.bids[0].price > snapshot.bids[1].price);

    // Asks should be ascending (best ask first)
    assert!(snapshot.asks[0].price < snapshot.asks[1].price);
}

// =============================================================================
// 3. VALIDATION TESTS (4 tests)
// =============================================================================

/// Test validate passes for valid snapshot.
#[test]
fn test_validate_valid_snapshot() {
    let snapshot = test_order_book_snapshot();
    assert!(snapshot.validate().is_ok());
}

/// Test validate fails for empty symbol.
#[test]
fn test_validate_empty_symbol_fails() {
    let snapshot = OrderBookSnapshot::new("", "oanda", 0, vec![], vec![]);
    let result = snapshot.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("symbol"));
}

/// Test validate fails for empty exchange.
#[test]
fn test_validate_empty_exchange_fails() {
    let snapshot = OrderBookSnapshot::new("EUR_USD", "", 0, vec![], vec![]);
    let result = snapshot.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exchange"));
}

/// Test validate fails for unsorted bids.
#[test]
fn test_validate_unsorted_bids_fails() {
    let snapshot = OrderBookSnapshot::new(
        "EUR_USD",
        "oanda",
        0,
        vec![
            OrderBookLevel::new(1.0849, 500000.0),  // Lower first (wrong)
            OrderBookLevel::new(1.0850, 1000000.0), // Higher second (wrong)
        ],
        vec![],
    );

    let result = snapshot.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("bids"));
}

// =============================================================================
// 4. CALCULATION TESTS (3 tests)
// =============================================================================

/// Test best_bid_price and best_ask_price.
#[test]
fn test_best_prices() {
    let snapshot = test_order_book_snapshot();

    let best_bid = snapshot.best_bid_price().unwrap();
    let best_ask = snapshot.best_ask_price().unwrap();

    assert!((best_bid - 1.0850).abs() < f64::EPSILON);
    assert!((best_ask - 1.0852).abs() < f64::EPSILON);
}

/// Test mid_price calculation.
#[test]
fn test_mid_price() {
    let snapshot = test_order_book_snapshot();
    let mid = snapshot.mid_price().unwrap();

    // (1.0850 + 1.0852) / 2 = 1.0851
    assert!((mid - 1.0851).abs() < 0.0001);
}

/// Test spread calculation.
#[test]
fn test_spread() {
    let snapshot = test_order_book_snapshot();
    let spread = snapshot.spread().unwrap();

    // 1.0852 - 1.0850 = 0.0002
    assert!((spread - 0.0002).abs() < 0.00001);
}

// =============================================================================
// 5. EDGE CASE TESTS (3 tests)
// =============================================================================

/// Test empty order book.
#[test]
fn test_empty_order_book() {
    let snapshot = OrderBookSnapshot::new("TEST", "test", 0, vec![], vec![]);

    assert!(snapshot.best_bid_price().is_none());
    assert!(snapshot.best_ask_price().is_none());
    assert!(snapshot.mid_price().is_none());
    assert!(snapshot.spread().is_none());
}

/// Test single level order book.
#[test]
fn test_single_level_order_book() {
    let snapshot = OrderBookSnapshot::new(
        "TEST",
        "test",
        0,
        vec![OrderBookLevel::new(100.0, 10.0)],
        vec![OrderBookLevel::new(101.0, 5.0)],
    );

    assert!(snapshot.validate().is_ok());
    assert!(snapshot.mid_price().is_some());
}

/// Test precision preservation.
#[test]
fn test_precision_preservation() {
    let level = OrderBookLevel::new(1.123456789, 0.00000001);
    let json = serde_json::to_string(&level).expect("Should serialize");
    let deserialized: OrderBookLevel = serde_json::from_str(&json).expect("Should deserialize");

    // f64 should preserve reasonable precision
    assert!((level.price - deserialized.price).abs() < 1e-10);
}
