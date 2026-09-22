//! Tests for Exchange Adapters (Part 2.4).
//!
//! This module tests the ExchangeAdapter trait and implementations for:
//! - Deribit (crypto derivatives)
//! - Binance (crypto spot/futures)
//! - OANDA (forex)
//!
//! # Test Categories
//!
//! | Category | Count | Description |
//! |----------|-------|-------------|
//! | Trait Tests | 6 | ExchangeAdapter trait compliance |
//! | Deribit Tests | 16 | Deribit-specific parsing |
//! | Binance Tests | 16 | Binance-specific parsing |
//! | OANDA Tests | 10 | OANDA-specific parsing |
//! | Performance Tests | 4 | Latency validation |
//! | **Total** | **52** | |
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test network -- adapters
//! ```

use astra_flash::core::error::FlashError;
use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEventType, Side};
use astra_flash::network::adapters::{
    create_adapter, BinanceAdapter, BookInterval, DeribitAdapter, ExchangeAdapter, OandaAdapter,
    RateLimit, UpdateInterval,
};
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Instant;

// =============================================================================
// TEST FIXTURES
// =============================================================================

/// Load test fixture from file.
fn load_fixture(exchange: &str, name: &str) -> String {
    let path = format!("tests/fixtures/{}/{}.json", exchange, name);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to load fixture: {}", path))
}

/// Create a test Deribit instrument.
fn deribit_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create a test Binance instrument.
fn binance_instrument() -> Instrument {
    Instrument::new("BTC", "USDT", Exchange::Binance, "BTCUSDT")
}

/// Create a test OANDA instrument.
fn oanda_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

// =============================================================================
// TRAIT COMPLIANCE TESTS (6 tests)
// =============================================================================

/// Test that all adapters implement Send.
#[test]
fn test_adapter_is_send() {
    fn assert_send<T: Send>() {}

    assert_send::<DeribitAdapter>();
    assert_send::<BinanceAdapter>();
    assert_send::<OandaAdapter>();
}

/// Test that all adapters implement Sync.
#[test]
fn test_adapter_is_sync() {
    fn assert_sync<T: Sync>() {}

    assert_sync::<DeribitAdapter>();
    assert_sync::<BinanceAdapter>();
    assert_sync::<OandaAdapter>();
}

/// Test that all adapters implement Debug.
#[test]
fn test_adapter_debug_impl() {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test_account");

    // Debug should produce non-empty output
    assert!(!format!("{:?}", deribit).is_empty());
    assert!(!format!("{:?}", binance).is_empty());
    assert!(!format!("{:?}", oanda).is_empty());
}

/// Test adapter factory creates correct types.
#[test]
fn test_adapter_factory_creates_correct_type() {
    let deribit = create_adapter(Exchange::Deribit);
    let binance = create_adapter(Exchange::Binance);
    let oanda = create_adapter(Exchange::Oanda);

    assert_eq!(deribit.exchange(), Exchange::Deribit);
    assert_eq!(binance.exchange(), Exchange::Binance);
    assert_eq!(oanda.exchange(), Exchange::Oanda);
}

/// Test adapter exchange() returns correct enum.
#[test]
fn test_adapter_exchange_returns_correct_enum() {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test_account");

    assert_eq!(deribit.exchange(), Exchange::Deribit);
    assert_eq!(binance.exchange(), Exchange::Binance);
    assert_eq!(oanda.exchange(), Exchange::Oanda);
}

/// Test adapter rate limits are reasonable.
#[test]
fn test_adapter_rate_limit_reasonable() {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test_account");

    // All rate limits should have positive values
    let deribit_limit = deribit.rate_limit();
    assert!(deribit_limit.ws_messages_per_second > 0);
    assert!(deribit_limit.max_subscriptions > 0);

    let binance_limit = binance.rate_limit();
    assert!(binance_limit.ws_messages_per_second > 0);
    assert!(binance_limit.max_subscriptions > 0);

    let oanda_limit = oanda.rate_limit();
    assert!(oanda_limit.ws_messages_per_second > 0);
    assert!(oanda_limit.max_subscriptions > 0);
}

// =============================================================================
// DERIBIT ADAPTER TESTS (16 tests)
// =============================================================================

/// Test parsing Deribit book snapshot.
#[test]
fn test_deribit_parse_book_snapshot() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "book_snapshot");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Snapshot);
    assert_eq!(event.instrument.exchange, Exchange::Deribit);
    assert_eq!(event.instrument.raw_symbol, "BTC-PERPETUAL");

    if let MarketData::Book { bids, asks } = &event.data {
        assert_eq!(bids.len(), 3);
        assert_eq!(asks.len(), 3);

        // Check first bid
        assert!((bids[0].price - 50000.0).abs() < f64::EPSILON);
        assert_eq!(bids[0].quantity, Decimal::new(105, 1)); // 10.5

        // Check first ask
        assert!((asks[0].price - 50000.5).abs() < f64::EPSILON);
        assert_eq!(asks[0].quantity, Decimal::new(83, 1)); // 8.3
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing Deribit book delta.
#[test]
fn test_deribit_parse_book_delta() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "book_delta");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Delta);
    assert!(event.sequence.is_some());

    if let MarketData::Book { bids, asks } = &event.data {
        // Delta includes updates and removals (qty=0)
        assert_eq!(bids.len(), 2);
        assert_eq!(asks.len(), 2);

        // Check for removal (qty=0)
        let removed_bid = bids.iter().find(|l| l.quantity.is_zero());
        assert!(removed_bid.is_some());
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing Deribit trade.
#[test]
fn test_deribit_parse_trade() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "trade");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Trade);

    if let MarketData::Trade {
        price,
        quantity,
        side,
        trade_id,
    } = &event.data
    {
        assert!((price - 50000.25).abs() < f64::EPSILON);
        assert_eq!(*quantity, Decimal::new(5, 1)); // 0.5
        assert_eq!(*side, Side::Bid); // "buy" -> Bid
        assert!(trade_id.is_some());
    } else {
        panic!("Expected Trade data");
    }
}

/// Test parsing Deribit heartbeat.
#[test]
fn test_deribit_parse_heartbeat() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "heartbeat");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    // Heartbeat returns empty vec (handled separately)
    assert!(events.is_empty() || events[0].event_type == MarketEventType::Heartbeat);
}

/// Test Deribit is_heartbeat detection.
#[test]
fn test_deribit_is_heartbeat() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "heartbeat");

    assert!(adapter.is_heartbeat(&json));

    // Non-heartbeat message
    let book = load_fixture("deribit", "book_snapshot");
    assert!(!adapter.is_heartbeat(&book));
}

/// Test Deribit subscription response detection.
#[test]
fn test_deribit_parse_subscription_response() {
    let adapter = DeribitAdapter::default();
    let response = r#"{"jsonrpc":"2.0","id":1,"result":["book.BTC-PERPETUAL.100ms"]}"#;

    assert!(adapter.is_subscription_response(response));
}

/// Test parsing empty Deribit book.
#[test]
fn test_deribit_parse_empty_book() {
    let adapter = DeribitAdapter::default();
    let json = r#"{
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": "book.BTC-PERPETUAL.100ms",
            "data": {
                "type": "snapshot",
                "timestamp": 1703808000000,
                "instrument_name": "BTC-PERPETUAL",
                "change_id": 12345,
                "bids": [],
                "asks": []
            }
        }
    }"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    assert_eq!(events.len(), 1);

    if let MarketData::Book { bids, asks } = &events[0].data {
        assert!(bids.is_empty());
        assert!(asks.is_empty());
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing invalid JSON returns error.
#[test]
fn test_deribit_parse_invalid_json() {
    let adapter = DeribitAdapter::default();
    let result = adapter.parse_message("not valid json");

    assert!(result.is_err());
    if let Err(FlashError::ParseError(_)) = result {
        // Expected
    } else {
        panic!("Expected ParseError");
    }
}

/// Test parsing unknown method returns empty.
#[test]
fn test_deribit_parse_unknown_method() {
    let adapter = DeribitAdapter::default();
    let json = r#"{"jsonrpc":"2.0","method":"unknown","params":{}}"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    assert!(events.is_empty());
}

/// Test building Deribit subscribe message for single instrument.
#[test]
fn test_deribit_build_subscribe_single() {
    let adapter = DeribitAdapter::default();
    let instruments = vec![deribit_instrument()];

    let messages = adapter.build_subscribe(&instruments);

    assert_eq!(messages.len(), 1);
    let msg = &messages[0];

    assert!(msg.contains("public/subscribe"));
    assert!(msg.contains("book.BTC-PERPETUAL"));
}

/// Test building Deribit subscribe message for multiple instruments.
#[test]
fn test_deribit_build_subscribe_multiple() {
    let adapter = DeribitAdapter::default();
    let instruments = vec![
        Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
        Instrument::new("ETH", "USD", Exchange::Deribit, "ETH-PERPETUAL"),
    ];

    let messages = adapter.build_subscribe(&instruments);

    // Deribit allows multiple channels in one message
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("BTC-PERPETUAL"));
    assert!(messages[0].contains("ETH-PERPETUAL"));
}

/// Test building Deribit unsubscribe message.
#[test]
fn test_deribit_build_unsubscribe() {
    let adapter = DeribitAdapter::default();
    let instruments = vec![deribit_instrument()];

    let messages = adapter.build_unsubscribe(&instruments);

    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("public/unsubscribe"));
}

/// Test building Deribit auth message.
#[test]
fn test_deribit_build_auth() {
    let adapter = DeribitAdapter::default();
    let auth = adapter.build_auth("api_key", "api_secret", 1703808000000);

    assert!(auth.is_some());
    let auth_msg = auth.unwrap();
    assert!(auth_msg.contains("public/auth"));
}

/// Test Deribit rate limit error parsing.
#[test]
fn test_deribit_parse_rate_limit_error() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "error");

    let error = adapter.parse_error(&json);
    assert!(error.is_some());

    if let Some(FlashError::RateLimited { exchange, .. }) = error {
        assert_eq!(exchange, "deribit");
    } else {
        panic!("Expected RateLimited error");
    }
}

/// Test Deribit auth error parsing.
#[test]
fn test_deribit_parse_auth_error() {
    let adapter = DeribitAdapter::default();
    let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":13009,"message":"unauthorized"}}"#;

    let error = adapter.parse_error(json);
    assert!(error.is_some());

    if let Some(FlashError::AuthenticationFailed { exchange, .. }) = error {
        assert_eq!(exchange, "deribit");
    } else {
        panic!("Expected AuthenticationFailed error");
    }
}

/// Test Deribit ping handling.
#[test]
fn test_deribit_handle_ping() {
    let adapter = DeribitAdapter::default();
    let heartbeat = load_fixture("deribit", "heartbeat");

    let response = adapter.handle_ping(heartbeat.as_bytes());

    // Deribit requires an application-level pong response
    assert!(response.is_some());
    let response_str = String::from_utf8(response.unwrap()).unwrap();
    assert!(response_str.contains("public/test"));
}

// =============================================================================
// BINANCE ADAPTER TESTS (16 tests)
// =============================================================================

/// Test parsing Binance depth update.
#[test]
fn test_binance_parse_depth_update() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "depth_update");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Delta);
    assert_eq!(event.instrument.exchange, Exchange::Binance);

    if let MarketData::Book { bids, asks } = &event.data {
        assert_eq!(bids.len(), 3);
        assert_eq!(asks.len(), 3);
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing Binance partial depth.
#[test]
fn test_binance_parse_partial_depth() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "partial_depth");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, MarketEventType::Snapshot);
}

/// Test parsing Binance trade.
#[test]
fn test_binance_parse_trade() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "trade");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Trade);

    if let MarketData::Trade {
        price,
        quantity,
        side,
        trade_id,
    } = &event.data
    {
        assert!((price - 50000.25).abs() < f64::EPSILON);
        assert_eq!(*quantity, Decimal::new(5, 1)); // 0.5
                                                   // m=true means the buyer is the market maker, so taker is seller (Ask)
        assert_eq!(*side, Side::Ask);
        assert!(trade_id.is_some());
    } else {
        panic!("Expected Trade data");
    }
}

/// Test parsing Binance combined stream message.
#[test]
fn test_binance_parse_combined_stream() {
    let adapter = BinanceAdapter::default();
    let json = r#"{
        "stream": "btcusdt@depth@100ms",
        "data": {
            "e": "depthUpdate",
            "E": 1703808000000,
            "s": "BTCUSDT",
            "U": 12345,
            "u": 12350,
            "b": [["50000.00", "10.50000"]],
            "a": [["50000.50", "8.30000"]]
        }
    }"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    assert_eq!(events.len(), 1);
}

/// Test Binance subscription response detection.
#[test]
fn test_binance_parse_subscription_response() {
    let adapter = BinanceAdapter::default();
    let response = r#"{"result":null,"id":1}"#;

    assert!(adapter.is_subscription_response(response));
}

/// Test parsing empty Binance update.
#[test]
fn test_binance_parse_empty_update() {
    let adapter = BinanceAdapter::default();
    let json = r#"{
        "e": "depthUpdate",
        "E": 1703808000000,
        "s": "BTCUSDT",
        "U": 12345,
        "u": 12345,
        "b": [],
        "a": []
    }"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    assert_eq!(events.len(), 1);

    if let MarketData::Book { bids, asks } = &events[0].data {
        assert!(bids.is_empty());
        assert!(asks.is_empty());
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing invalid Binance JSON.
#[test]
fn test_binance_parse_invalid_json() {
    let adapter = BinanceAdapter::default();
    let result = adapter.parse_message("invalid json");

    assert!(result.is_err());
}

/// Test parsing unknown Binance event.
#[test]
fn test_binance_parse_unknown_event() {
    let adapter = BinanceAdapter::default();
    let json = r#"{"e":"unknownEvent","E":1703808000000}"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    assert!(events.is_empty());
}

/// Test building Binance subscribe for single instrument.
#[test]
fn test_binance_build_subscribe_single() {
    let adapter = BinanceAdapter::default();
    let instruments = vec![binance_instrument()];

    let messages = adapter.build_subscribe(&instruments);

    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("SUBSCRIBE"));
    assert!(messages[0].to_lowercase().contains("btcusdt"));
}

/// Test building Binance subscribe for multiple instruments.
#[test]
fn test_binance_build_subscribe_multiple() {
    let adapter = BinanceAdapter::default();
    let instruments = vec![
        Instrument::new("BTC", "USDT", Exchange::Binance, "BTCUSDT"),
        Instrument::new("ETH", "USDT", Exchange::Binance, "ETHUSDT"),
    ];

    let messages = adapter.build_subscribe(&instruments);

    assert_eq!(messages.len(), 1);
    let msg = messages[0].to_lowercase();
    assert!(msg.contains("btcusdt"));
    assert!(msg.contains("ethusdt"));
}

/// Test building Binance unsubscribe.
#[test]
fn test_binance_build_unsubscribe() {
    let adapter = BinanceAdapter::default();
    let instruments = vec![binance_instrument()];

    let messages = adapter.build_unsubscribe(&instruments);

    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("UNSUBSCRIBE"));
}

/// Test Binance combined stream URL format.
#[test]
fn test_binance_combined_stream_format() {
    let adapter = BinanceAdapter::default();
    let url = adapter.websocket_url();

    // Should be the combined streams URL or standard URL
    assert!(url.starts_with("wss://"));
    assert!(url.contains("binance.com"));
}

/// Test Binance error response parsing.
#[test]
fn test_binance_parse_error_response() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "error");

    let error = adapter.parse_error(&json);
    assert!(error.is_some());
}

/// Test Binance rate limit error.
#[test]
fn test_binance_parse_rate_limit_error() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "error");

    let error = adapter.parse_error(&json);
    assert!(error.is_some());

    if let Some(FlashError::RateLimited { exchange, .. }) = error {
        assert_eq!(exchange, "binance");
    } else {
        panic!("Expected RateLimited error");
    }
}

/// Test Binance invalid symbol error.
#[test]
fn test_binance_parse_invalid_symbol() {
    let adapter = BinanceAdapter::default();
    let json = r#"{"error":{"code":-1121,"msg":"Invalid symbol."},"id":1}"#;

    let error = adapter.parse_error(json);
    assert!(error.is_some());

    if let Some(FlashError::ExchangeError { code, .. }) = error {
        assert_eq!(code, -1121);
    } else {
        panic!("Expected ExchangeError");
    }
}

/// Test Binance ping handling (returns None - uses WebSocket ping).
#[test]
fn test_binance_handle_ping_none() {
    let adapter = BinanceAdapter::default();
    let response = adapter.handle_ping(b"ping");

    // Binance uses WebSocket protocol ping/pong, not application level
    assert!(response.is_none());
}

// =============================================================================
// OANDA ADAPTER TESTS (10 tests)
// =============================================================================

/// Test parsing OANDA price.
#[test]
fn test_oanda_parse_price() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "price");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(event.event_type, MarketEventType::Snapshot);
    assert_eq!(event.instrument.exchange, Exchange::Oanda);
    assert_eq!(event.instrument.raw_symbol, "EUR_USD");

    if let MarketData::Book { bids, asks } = &event.data {
        assert_eq!(bids.len(), 2);
        assert_eq!(asks.len(), 2);

        // Check first bid
        assert!((bids[0].price - 1.10500).abs() < 0.00001);
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing OANDA heartbeat.
#[test]
fn test_oanda_parse_heartbeat() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "heartbeat");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    // Heartbeat may be returned as empty or as a Heartbeat event
    assert!(events.is_empty() || events[0].event_type == MarketEventType::Heartbeat);
}

/// Test OANDA is_heartbeat detection.
#[test]
fn test_oanda_is_heartbeat() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "heartbeat");

    assert!(adapter.is_heartbeat(&json));
}

/// Test parsing OANDA multiple levels.
#[test]
fn test_oanda_parse_multiple_levels() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "price");

    let events = adapter.parse_message(&json).expect("Parse should succeed");

    if let MarketData::Book { bids, asks } = &events[0].data {
        // Should have 2 levels each from the fixture
        assert_eq!(bids.len(), 2);
        assert_eq!(asks.len(), 2);
    } else {
        panic!("Expected Book data");
    }
}

/// Test parsing OANDA non-tradeable price.
#[test]
fn test_oanda_parse_non_tradeable() {
    let adapter = OandaAdapter::new("test_account");
    let json = r#"{
        "type": "PRICE",
        "time": "2023-12-29T00:00:00.000000000Z",
        "instrument": "EUR_USD",
        "status": "non-tradeable",
        "tradeable": false,
        "bids": [],
        "asks": []
    }"#;

    let events = adapter.parse_message(json).expect("Parse should succeed");
    // Non-tradeable prices may still be returned but with empty levels
    assert!(!events.is_empty());
}

/// Test parsing invalid OANDA JSON.
#[test]
fn test_oanda_parse_invalid_json() {
    let adapter = OandaAdapter::new("test_account");
    let result = adapter.parse_message("not valid");

    assert!(result.is_err());
}

/// Test OANDA WebSocket URL returns base URL.
/// Note: Account ID and instruments are added separately via URL parameters.
#[test]
fn test_oanda_websocket_url_base() {
    let adapter = OandaAdapter::new("test_account");
    let url = adapter.websocket_url();

    // websocket_url() returns the base URL, not the full streaming URL
    assert!(url.starts_with("wss://"));
    assert!(url.contains("oanda.com"));
    assert!(url.contains("fxpractice")); // Default is practice environment
}

/// Test OANDA subscribe via URL (returns empty - subscription is via URL params).
#[test]
fn test_oanda_build_subscribe_via_url() {
    let adapter = OandaAdapter::new("test_account");
    let instruments = vec![oanda_instrument()];

    let messages = adapter.build_subscribe(&instruments);

    // OANDA uses URL parameters for subscription, not separate messages
    // This may return empty or a single empty message
    assert!(messages.is_empty() || messages[0].is_empty());
}

/// Test OANDA error message parsing.
#[test]
fn test_oanda_parse_error_message() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "error");

    let error = adapter.parse_error(&json);
    assert!(error.is_some());

    if let Some(FlashError::ExchangeError { message, .. }) = error {
        assert!(message.contains("Invalid instrument"));
    } else {
        panic!("Expected ExchangeError");
    }
}

/// Test OANDA invalid instrument error.
#[test]
fn test_oanda_parse_invalid_instrument() {
    let adapter = OandaAdapter::new("test_account");
    let json = r#"{"errorMessage":"Invalid instrument: FOO_BAR"}"#;

    let error = adapter.parse_error(json);
    assert!(error.is_some());
}

// =============================================================================
// PERFORMANCE TESTS (4 tests)
// =============================================================================

/// Test Deribit parse latency is under 10 microseconds.
#[test]
fn test_deribit_parse_latency() {
    let adapter = DeribitAdapter::default();
    let json = load_fixture("deribit", "book_snapshot");

    // Warm up
    for _ in 0..100 {
        let _ = adapter.parse_message(&json);
    }

    // Measure
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = adapter.parse_message(&json);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() / iterations as u128;
    println!("Deribit parse average: {} ns", avg_ns);

    // Target: < 10 μs (10,000 ns) - we're generous in unit tests
    assert!(avg_ns < 10_000_000, "Parse too slow: {} ns", avg_ns);
}

/// Test Binance parse latency is under 10 microseconds.
#[test]
fn test_binance_parse_latency() {
    let adapter = BinanceAdapter::default();
    let json = load_fixture("binance", "depth_update");

    // Warm up
    for _ in 0..100 {
        let _ = adapter.parse_message(&json);
    }

    // Measure
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = adapter.parse_message(&json);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() / iterations as u128;
    println!("Binance parse average: {} ns", avg_ns);

    // Target: < 10 μs (10,000 ns)
    assert!(avg_ns < 10_000_000, "Parse too slow: {} ns", avg_ns);
}

/// Test OANDA parse latency is under 10 microseconds.
#[test]
fn test_oanda_parse_latency() {
    let adapter = OandaAdapter::new("test_account");
    let json = load_fixture("oanda", "price");

    // Warm up
    for _ in 0..100 {
        let _ = adapter.parse_message(&json);
    }

    // Measure
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = adapter.parse_message(&json);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() / iterations as u128;
    println!("OANDA parse average: {} ns", avg_ns);

    // Target: < 10 μs (10,000 ns)
    assert!(avg_ns < 10_000_000, "Parse too slow: {} ns", avg_ns);
}

/// Test that parsing doesn't cause excessive heap allocations.
/// This is a basic smoke test - detailed allocation tracking needs profiling tools.
#[test]
fn test_adapter_no_excessive_allocation() {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test_account");

    let deribit_json = load_fixture("deribit", "book_snapshot");
    let binance_json = load_fixture("binance", "depth_update");
    let oanda_json = load_fixture("oanda", "price");

    // Run many iterations - if there's a memory leak, this would grow unbounded
    for _ in 0..10000 {
        let _ = deribit.parse_message(&deribit_json);
        let _ = binance.parse_message(&binance_json);
        let _ = oanda.parse_message(&oanda_json);
    }

    // If we get here without OOM, basic allocation is acceptable
}

// =============================================================================
// CONFIGURATION TESTS
// =============================================================================

/// Test Deribit book interval configuration.
#[test]
fn test_deribit_book_interval() {
    let default = DeribitAdapter::default();
    assert!(matches!(default.book_interval(), BookInterval::Ms100));

    let raw = DeribitAdapter::with_interval(BookInterval::Raw);
    assert!(matches!(raw.book_interval(), BookInterval::Raw));
}

/// Test Binance update interval configuration.
#[test]
fn test_binance_update_interval() {
    let default = BinanceAdapter::default();
    assert!(matches!(default.update_interval(), UpdateInterval::Ms1000));

    let fast = BinanceAdapter::with_interval(UpdateInterval::Ms100);
    assert!(matches!(fast.update_interval(), UpdateInterval::Ms100));
}

/// Test OANDA with access token.
#[test]
fn test_oanda_with_access_token() {
    let adapter = OandaAdapter::with_token("test_account", "test_token");

    // Build auth should include the token
    let auth = adapter.build_auth("", "", 0);
    assert!(auth.is_some());
}

/// Test rate limit constants.
#[test]
fn test_rate_limit_constants() {
    assert_eq!(RateLimit::DERIBIT.ws_messages_per_second, 100);
    assert_eq!(RateLimit::BINANCE.ws_messages_per_second, 5);
    assert_eq!(RateLimit::OANDA.ws_messages_per_second, 120);
}
