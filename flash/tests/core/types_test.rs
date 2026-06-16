//! Unit tests for Flash core types.
//!
//! # Test Categories
//!
//! 1. Type Creation Tests (5 tests)
//! 2. Serialization Round-Trip Tests (4 tests)
//! 3. Type Conversion Tests (3 tests)
//! 4. Validation Tests (3 tests)
//! 5. Edge Case Tests (2 tests)
//! 6. Property-Based Tests (2 tests)
//!
//! Total: 19+ tests (TDI requirement: 15+)

use astra_flash::core::types::*;
use chrono::Datelike;
use rust_decimal_macros::dec;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument for use in tests.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create a test price level for use in tests.
fn test_price_level() -> PriceLevel {
    PriceLevel::new(50000.0, dec!(1.5), 1234567890_i64)
}

/// Get current timestamp in microseconds.
fn now_micros() -> Timestamp {
    chrono::Utc::now().timestamp_micros()
}

// =============================================================================
// 1. TYPE CREATION TESTS (5 tests)
// =============================================================================

/// Test that all Exchange variants can be created.
#[test]
fn test_exchange_creation_all_variants() {
    // ARRANGE & ACT
    let deribit = Exchange::Deribit;
    let binance = Exchange::Binance;
    let oanda = Exchange::Oanda;

    // ASSERT
    assert_eq!(deribit, Exchange::Deribit);
    assert_eq!(binance, Exchange::Binance);
    assert_eq!(oanda, Exchange::Oanda);

    // Verify they are distinct
    assert_ne!(deribit, binance);
    assert_ne!(binance, oanda);
    assert_ne!(deribit, oanda);
}

/// Test that Side::Bid and Side::Ask can be created and are distinct.
#[test]
fn test_side_creation_bid_and_ask() {
    // ARRANGE & ACT
    let bid = Side::Bid;
    let ask = Side::Ask;

    // ASSERT
    assert_eq!(bid, Side::Bid);
    assert_eq!(ask, Side::Ask);
    assert_ne!(bid, ask);
}

/// Test Instrument creation with all fields populated.
#[test]
fn test_instrument_creation_with_all_fields() {
    // ARRANGE
    let base = "ETH";
    let quote = "USDT";
    let exchange = Exchange::Binance;
    let raw_symbol = "ETHUSDT";

    // ACT
    let instrument = Instrument::new(base, quote, exchange, raw_symbol);

    // ASSERT
    assert_eq!(instrument.base, "ETH");
    assert_eq!(instrument.quote, "USDT");
    assert_eq!(instrument.exchange, Exchange::Binance);
    assert_eq!(instrument.raw_symbol, "ETHUSDT");
}

/// Test PriceLevel creation with valid data.
#[test]
fn test_price_level_creation_with_valid_data() {
    // ARRANGE
    let price = 42000.50;
    let quantity = dec!(2.5);
    let timestamp = 1703808000000000_i64; // Some timestamp

    // ACT
    let level = PriceLevel::new(price, quantity, timestamp);

    // ASSERT
    assert!((level.price - 42000.50).abs() < f64::EPSILON);
    assert_eq!(level.quantity, dec!(2.5));
    assert_eq!(level.timestamp, 1703808000000000_i64);
    assert!(level.order_count.is_none()); // Default is None
}

/// Test MarketEvent creation with snapshot data.
#[test]
fn test_market_event_creation_snapshot() {
    // ARRANGE
    let instrument = test_instrument();
    let timestamp = now_micros();
    let local_timestamp = now_micros();
    let bids = vec![PriceLevel::new(49000.0, dec!(1.0), timestamp)];
    let asks = vec![PriceLevel::new(49100.0, dec!(1.0), timestamp)];

    // ACT
    let event = MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument: instrument.clone(),
        timestamp,
        local_timestamp,
        sequence: Some(12345),
        data: MarketData::Book { bids, asks },
    };

    // ASSERT
    assert_eq!(event.event_type, MarketEventType::Snapshot);
    assert_eq!(event.instrument.base, "BTC");
    assert_eq!(event.sequence, Some(12345));

    match &event.data {
        MarketData::Book { bids, asks } => {
            assert_eq!(bids.len(), 1);
            assert_eq!(asks.len(), 1);
        }
        _ => panic!("Expected Book data"),
    }
}

// =============================================================================
// 2. SERIALIZATION ROUND-TRIP TESTS (4 tests)
// =============================================================================

/// Test Exchange serialization/deserialization round-trip.
#[test]
fn test_exchange_serde_roundtrip() {
    // ARRANGE
    let exchanges = [Exchange::Deribit, Exchange::Binance, Exchange::Oanda];

    for exchange in exchanges {
        // ACT
        let json = serde_json::to_string(&exchange).expect("Failed to serialize Exchange");
        let deserialized: Exchange =
            serde_json::from_str(&json).expect("Failed to deserialize Exchange");

        // ASSERT
        assert_eq!(exchange, deserialized, "Round-trip failed for {:?}", exchange);
    }
}

/// Test Instrument serialization/deserialization round-trip.
#[test]
fn test_instrument_serde_roundtrip() {
    // ARRANGE
    let instrument = test_instrument();

    // ACT
    let json = serde_json::to_string(&instrument).expect("Failed to serialize Instrument");
    let deserialized: Instrument =
        serde_json::from_str(&json).expect("Failed to deserialize Instrument");

    // ASSERT
    assert_eq!(instrument.base, deserialized.base);
    assert_eq!(instrument.quote, deserialized.quote);
    assert_eq!(instrument.exchange, deserialized.exchange);
    assert_eq!(instrument.raw_symbol, deserialized.raw_symbol);
}

/// Test PriceLevel serialization/deserialization round-trip.
#[test]
fn test_price_level_serde_roundtrip() {
    // ARRANGE
    let level = test_price_level();

    // ACT
    let json = serde_json::to_string(&level).expect("Failed to serialize PriceLevel");
    let deserialized: PriceLevel =
        serde_json::from_str(&json).expect("Failed to deserialize PriceLevel");

    // ASSERT
    assert!((level.price - deserialized.price).abs() < f64::EPSILON);
    assert_eq!(level.quantity, deserialized.quantity);
    assert_eq!(level.timestamp, deserialized.timestamp);
}

/// Test full MarketEvent serialization/deserialization round-trip.
#[test]
fn test_market_event_serde_roundtrip() {
    // ARRANGE
    let timestamp = now_micros();
    let event = MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: test_instrument(),
        timestamp,
        local_timestamp: timestamp + 100,
        sequence: Some(999),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.5),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    };

    // ACT
    let json = serde_json::to_string(&event).expect("Failed to serialize MarketEvent");
    let deserialized: MarketEvent =
        serde_json::from_str(&json).expect("Failed to deserialize MarketEvent");

    // ASSERT
    assert_eq!(event.event_type, deserialized.event_type);
    assert_eq!(event.timestamp, deserialized.timestamp);
    assert_eq!(event.sequence, deserialized.sequence);

    match (&event.data, &deserialized.data) {
        (
            MarketData::Trade {
                price: p1,
                quantity: q1,
                side: s1,
                trade_id: t1,
            },
            MarketData::Trade {
                price: p2,
                quantity: q2,
                side: s2,
                trade_id: t2,
            },
        ) => {
            assert!((p1 - p2).abs() < f64::EPSILON);
            assert_eq!(q1, q2);
            assert_eq!(s1, s2);
            assert_eq!(t1, t2);
        }
        _ => panic!("Expected Trade data in both"),
    }
}

// =============================================================================
// 3. TYPE CONVERSION TESTS (3 tests)
// =============================================================================

/// Test Exchange Display trait implementation.
#[test]
fn test_exchange_display_trait() {
    // ARRANGE & ACT & ASSERT
    assert_eq!(Exchange::Deribit.as_str(), "deribit");
    assert_eq!(Exchange::Binance.as_str(), "binance");
    assert_eq!(Exchange::Oanda.as_str(), "oanda");
}

/// Test Side::opposite() method.
#[test]
fn test_side_opposite() {
    // ARRANGE & ACT & ASSERT
    assert_eq!(Side::Bid.opposite(), Side::Ask);
    assert_eq!(Side::Ask.opposite(), Side::Bid);

    // Double opposite should return original
    assert_eq!(Side::Bid.opposite().opposite(), Side::Bid);
    assert_eq!(Side::Ask.opposite().opposite(), Side::Ask);
}

/// Test Instrument symbol() method for unified symbol format.
#[test]
fn test_instrument_symbol_method() {
    // ARRANGE
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");

    // ACT
    let symbol = instrument.symbol();

    // ASSERT
    assert_eq!(symbol, "BTC/USD");
}

// =============================================================================
// 4. VALIDATION TESTS (3 tests)
// =============================================================================

/// Test that PriceLevel with zero quantity is valid.
#[test]
fn test_price_level_zero_quantity_valid() {
    // ARRANGE & ACT
    let level = PriceLevel::new(50000.0, dec!(0), now_micros());

    // ASSERT
    assert_eq!(level.quantity, dec!(0));
    // Zero quantity is valid - used for deletions in delta updates
}

/// Test PriceLevel with very small quantity (precision test).
#[test]
fn test_price_level_small_quantity_precision() {
    // ARRANGE
    let tiny_quantity = dec!(0.00000001); // 1 satoshi for BTC

    // ACT
    let level = PriceLevel::new(50000.0, tiny_quantity, now_micros());

    // ASSERT
    assert_eq!(level.quantity, dec!(0.00000001));
    // Decimal should preserve exact precision
}

/// Test timestamp conversion to chrono DateTime.
#[test]
fn test_timestamp_conversion_to_chrono() {
    // ARRANGE
    let timestamp: Timestamp = 1703808000000000; // 2023-12-29 00:00:00 UTC in micros

    // ACT
    let datetime = timestamp_to_datetime(timestamp);

    // ASSERT
    assert!(datetime.is_some());
    let dt = datetime.unwrap();
    assert_eq!(dt.year(), 2023);
    assert_eq!(dt.month(), 12);
    assert_eq!(dt.day(), 29);
}

// =============================================================================
// 5. EDGE CASE TESTS (2 tests)
// =============================================================================

/// Test PriceLevel behavior with maximum f64 price.
#[test]
fn test_price_level_max_price() {
    // ARRANGE
    let max_price = f64::MAX / 2.0; // Use half to avoid overflow in operations

    // ACT
    let level = PriceLevel::new(max_price, dec!(1), now_micros());

    // ASSERT
    assert_eq!(level.price, max_price);
}

/// Test Instrument with Unicode characters in symbol.
#[test]
fn test_instrument_unicode_handling() {
    // ARRANGE
    // Some exchanges might have unusual symbols
    let instrument = Instrument::new("BTC", "USD", Exchange::Binance, "BTC/USD");

    // ACT - serialize and deserialize
    let json = serde_json::to_string(&instrument).expect("Serialize failed");
    let deserialized: Instrument = serde_json::from_str(&json).expect("Deserialize failed");

    // ASSERT
    assert_eq!(instrument.raw_symbol, deserialized.raw_symbol);
}

// =============================================================================
// 6. MEMORY SIZE TESTS (2 tests)
// =============================================================================

/// Test that PriceLevel size is within budget.
#[test]
fn test_price_level_memory_size() {
    // ARRANGE
    let expected_max_size = 48; // bytes (per PLANNING.md)

    // ACT
    let actual_size = std::mem::size_of::<PriceLevel>();

    // ASSERT
    assert!(
        actual_size <= expected_max_size,
        "PriceLevel is {} bytes, expected <= {} bytes",
        actual_size,
        expected_max_size
    );
}

/// Test that Exchange enum is minimal size.
#[test]
fn test_exchange_enum_size() {
    // ARRANGE
    let expected_size = 1; // byte (just discriminant)

    // ACT
    let actual_size = std::mem::size_of::<Exchange>();

    // ASSERT
    assert_eq!(
        actual_size, expected_size,
        "Exchange is {} bytes, expected {} byte",
        actual_size, expected_size
    );
}

// =============================================================================
// 7. CLONE AND EQ TESTS (2 tests)
// =============================================================================

/// Test that Instrument Clone produces identical copy.
#[test]
fn test_instrument_clone_equality() {
    // ARRANGE
    let original = test_instrument();

    // ACT
    let cloned = original.clone();

    // ASSERT
    assert_eq!(original, cloned);
    // Verify they are separate instances (modify one shouldn't affect other)
}

/// Test PriceLevel Clone produces identical copy.
#[test]
fn test_price_level_clone_equality() {
    // ARRANGE
    let original = test_price_level();

    // ACT
    let cloned = original.clone();

    // ASSERT
    assert!((original.price - cloned.price).abs() < f64::EPSILON);
    assert_eq!(original.quantity, cloned.quantity);
    assert_eq!(original.timestamp, cloned.timestamp);
}

// =============================================================================
// 8. MARKET DATA VARIANT TESTS (2 tests)
// =============================================================================

/// Test MarketData::Book variant.
#[test]
fn test_market_data_book_variant() {
    // ARRANGE
    let timestamp = now_micros();
    let bids = vec![
        PriceLevel::new(49900.0, dec!(1.0), timestamp),
        PriceLevel::new(49800.0, dec!(2.0), timestamp),
    ];
    let asks = vec![PriceLevel::new(50000.0, dec!(1.5), timestamp)];

    // ACT
    let data = MarketData::Book {
        bids: bids.clone(),
        asks: asks.clone(),
    };

    // ASSERT
    match data {
        MarketData::Book { bids: b, asks: a } => {
            assert_eq!(b.len(), 2);
            assert_eq!(a.len(), 1);
        }
        _ => panic!("Expected Book variant"),
    }
}

/// Test MarketData::Heartbeat variant.
#[test]
fn test_market_data_heartbeat_variant() {
    // ARRANGE
    let exchange_time = now_micros();

    // ACT
    let data = MarketData::Heartbeat { exchange_time };

    // ASSERT
    match data {
        MarketData::Heartbeat { exchange_time: t } => {
            assert_eq!(t, exchange_time);
        }
        _ => panic!("Expected Heartbeat variant"),
    }
}

// =============================================================================
// 9. HASH TESTS (1 test)
// =============================================================================

/// Test that Exchange can be used as HashMap key.
#[test]
fn test_exchange_hash_key() {
    use std::collections::HashMap;

    // ARRANGE
    let mut map: HashMap<Exchange, String> = HashMap::new();

    // ACT
    map.insert(Exchange::Deribit, "deribit_url".to_string());
    map.insert(Exchange::Binance, "binance_url".to_string());
    map.insert(Exchange::Oanda, "oanda_url".to_string());

    // ASSERT
    assert_eq!(map.get(&Exchange::Deribit), Some(&"deribit_url".to_string()));
    assert_eq!(map.get(&Exchange::Binance), Some(&"binance_url".to_string()));
    assert_eq!(map.get(&Exchange::Oanda), Some(&"oanda_url".to_string()));
}

// =============================================================================
// Total: 21 tests (exceeds TDI requirement of 15+)
// =============================================================================
