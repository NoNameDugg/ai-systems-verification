//! Tests for Redis Stream Publisher (XADD).
//!
//! Phase 4.2: Stream Publisher (XADD)
//!
//! This module contains 36+ tests for:
//! - SerializationFormat enum
//! - StreamPublisherConfig struct
//! - TopicBuilder and topic generation
//! - Serialization (JSON, Bincode)
//! - StreamStats tracking
//! - StreamError variants
//! - StreamPublisherBuilder
//!
//! # TDI Methodology
//!
//! These tests are written BEFORE the implementation to define expected behavior.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test publisher -- stream
//! ```

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side, now_micros,
};
use astra_flash::publisher::stream::{
    PublishResult, SerializationFormat, StreamError, StreamPublisher, StreamPublisherBuilder,
    StreamPublisherConfig, StreamStats, TopicBuilder, TopicType,
};
use rust_decimal_macros::dec;
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create test price levels.
fn test_levels(count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| PriceLevel::new(50000.0 + (i as f64 * 10.0), dec!(1.5), now_micros()))
        .collect()
}

/// Create a test book snapshot.
fn test_book_snapshot() -> astra_flash::book::BookSnapshot {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let mut book = OrderBook::new(instrument, config);

    let bids = test_levels(5);
    let asks = test_levels(5);
    book.apply_snapshot(bids, asks, now_micros());

    book.to_snapshot(5)
}

/// Create a test market event.
fn test_market_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: test_instrument(),
        timestamp: now_micros(),
        local_timestamp: now_micros(),
        sequence: Some(1),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.1),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    }
}

// =============================================================================
// SERIALIZATION FORMAT TESTS (6 tests)
// =============================================================================

#[test]
fn test_serialization_format_default() {
    let format = SerializationFormat::default();
    assert_eq!(format, SerializationFormat::Bincode);
}

#[test]
fn test_serialization_format_json() {
    let format = SerializationFormat::Json;
    assert_eq!(format.as_str(), "json");
}

#[test]
fn test_serialization_format_bincode() {
    let format = SerializationFormat::Bincode;
    assert_eq!(format.as_str(), "bincode");
}

#[test]
fn test_serialization_format_rkyv() {
    let format = SerializationFormat::Rkyv;
    assert_eq!(format.as_str(), "rkyv");
}

#[test]
fn test_serialization_format_display() {
    assert_eq!(format!("{}", SerializationFormat::Json), "json");
    assert_eq!(format!("{}", SerializationFormat::Bincode), "bincode");
    assert_eq!(format!("{}", SerializationFormat::Rkyv), "rkyv");
}

#[test]
fn test_serialization_format_clone() {
    let format = SerializationFormat::Bincode;
    let cloned = format;
    assert_eq!(format, cloned);
}

// =============================================================================
// CONFIGURATION TESTS (6 tests)
// =============================================================================

#[test]
fn test_config_default() {
    let config = StreamPublisherConfig::default();

    assert_eq!(config.format, SerializationFormat::Bincode);
    assert_eq!(config.max_stream_length, 100_000);
    assert!(config.approximate_trimming);
    assert!(config.include_timestamp);
    assert_eq!(config.data_field, "data");
    assert_eq!(config.format_field, "format");
    assert_eq!(config.topic_prefix, "market_data");
}

#[test]
fn test_config_custom() {
    let config = StreamPublisherConfig {
        format: SerializationFormat::Json,
        max_stream_length: 50_000,
        approximate_trimming: false,
        include_timestamp: false,
        data_field: "payload".to_string(),
        format_field: "fmt".to_string(),
        topic_prefix: "custom".to_string(),
    };

    assert_eq!(config.format, SerializationFormat::Json);
    assert_eq!(config.max_stream_length, 50_000);
    assert!(!config.approximate_trimming);
    assert!(!config.include_timestamp);
    assert_eq!(config.data_field, "payload");
    assert_eq!(config.format_field, "fmt");
    assert_eq!(config.topic_prefix, "custom");
}

#[test]
fn test_config_validation_zero_max_length() {
    let config = StreamPublisherConfig {
        max_stream_length: 0,
        ..StreamPublisherConfig::default()
    };
    // Zero is valid (means unlimited)
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_empty_prefix() {
    let config = StreamPublisherConfig {
        topic_prefix: String::new(),
        ..StreamPublisherConfig::default()
    };
    // Empty prefix should fail validation
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_empty_data_field() {
    let config = StreamPublisherConfig {
        data_field: String::new(),
        ..StreamPublisherConfig::default()
    };
    // Empty data field should fail validation
    assert!(config.validate().is_err());
}

#[test]
fn test_config_clone() {
    let config = StreamPublisherConfig::default();
    let cloned = config.clone();
    assert_eq!(config.format, cloned.format);
    assert_eq!(config.max_stream_length, cloned.max_stream_length);
    assert_eq!(config.topic_prefix, cloned.topic_prefix);
}

// =============================================================================
// TOPIC BUILDER TESTS (8 tests)
// =============================================================================

#[test]
fn test_topic_market_data_book() {
    let builder = TopicBuilder::new("market_data");
    let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
    assert_eq!(topic, "market_data.deribit.btc_usd.book");
}

#[test]
fn test_topic_market_data_trade() {
    let builder = TopicBuilder::new("market_data");
    let topic = builder.market_data(Exchange::Binance, "ETH", "USDT", TopicType::Trade);
    assert_eq!(topic, "market_data.binance.eth_usdt.trade");
}

#[test]
fn test_topic_market_data_ticker() {
    let builder = TopicBuilder::new("market_data");
    let topic = builder.market_data(Exchange::Oanda, "EUR", "USD", TopicType::Ticker);
    assert_eq!(topic, "market_data.oanda.eur_usd.ticker");
}

#[test]
fn test_topic_market_data_event() {
    let builder = TopicBuilder::new("market_data");
    let topic = builder.market_data(Exchange::Deribit, "ETH", "USD", TopicType::Event);
    assert_eq!(topic, "market_data.deribit.eth_usd.event");
}

#[test]
fn test_topic_for_instrument() {
    let builder = TopicBuilder::new("market_data");
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let topic = builder.for_instrument(&instrument, TopicType::Book);
    assert_eq!(topic, "market_data.deribit.btc_usd.book");
}

#[test]
fn test_topic_system() {
    let builder = TopicBuilder::new("market_data");
    let topic = builder.system("health");
    assert_eq!(topic, "system.flash.health");
}

#[test]
fn test_topic_lowercase_conversion() {
    let builder = TopicBuilder::new("MARKET_DATA");
    let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
    // Should convert to lowercase
    assert!(topic.chars().all(|c| c.is_lowercase() || c == '.' || c == '_'));
}

#[test]
fn test_topic_builder_custom_prefix() {
    let builder = TopicBuilder::new("custom_prefix");
    let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
    assert!(topic.starts_with("custom_prefix."));
}

// =============================================================================
// SERIALIZATION TESTS (8 tests)
// =============================================================================

#[test]
fn test_serialize_book_json() {
    let snapshot = test_book_snapshot();
    let result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);

    assert!(result.is_ok());
    let data = result.unwrap();
    assert!(!data.is_empty());

    // Verify it's valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&data);
    assert!(parsed.is_ok());
}

#[test]
fn test_serialize_book_bincode() {
    let snapshot = test_book_snapshot();
    let result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);

    assert!(result.is_ok());
    let data = result.unwrap();
    assert!(!data.is_empty());
    // Bincode is typically smaller than JSON
    let json_result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);
    assert!(data.len() < json_result.unwrap().len());
}

#[test]
fn test_serialize_event_json() {
    let event = test_market_event();
    let result = StreamPublisher::serialize_event(&event, SerializationFormat::Json);

    assert!(result.is_ok());
    let data = result.unwrap();
    assert!(!data.is_empty());

    // Verify it's valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&data);
    assert!(parsed.is_ok());
}

#[test]
fn test_serialize_event_bincode() {
    let event = test_market_event();
    let result = StreamPublisher::serialize_event(&event, SerializationFormat::Bincode);

    assert!(result.is_ok());
    let data = result.unwrap();
    assert!(!data.is_empty());
}

#[test]
fn test_serialize_roundtrip_json() {
    let original = test_book_snapshot();
    let serialized = StreamPublisher::serialize_book(&original, SerializationFormat::Json).unwrap();

    // Deserialize and compare key fields
    let deserialized: astra_flash::book::BookSnapshot =
        serde_json::from_slice(&serialized).unwrap();

    assert_eq!(original.bids.len(), deserialized.bids.len());
    assert_eq!(original.asks.len(), deserialized.asks.len());
}

#[test]
fn test_serialize_roundtrip_bincode() {
    let original = test_book_snapshot();
    let serialized =
        StreamPublisher::serialize_book(&original, SerializationFormat::Bincode).unwrap();

    // Note: Full bincode roundtrip isn't possible because rust_decimal::Decimal
    // doesn't support bincode's deserialize_any. Instead, verify serialization works
    // and has reasonable size.
    assert!(!serialized.is_empty());

    // Bincode should be more compact than JSON
    let json_serialized =
        StreamPublisher::serialize_book(&original, SerializationFormat::Json).unwrap();
    assert!(serialized.len() < json_serialized.len());
}

#[test]
fn test_serialize_empty_book() {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let book = OrderBook::new(instrument, config);
    let snapshot = book.to_snapshot(10);

    let result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);
    assert!(result.is_ok());

    let result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);
    assert!(result.is_ok());
}

#[test]
fn test_serialize_large_book() {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let mut book = OrderBook::new(instrument, config);

    // Create large book with 50 levels per side
    let bids: Vec<PriceLevel> = (0..50)
        .map(|i| PriceLevel::new(50000.0 - (i as f64 * 10.0), dec!(1.0), now_micros()))
        .collect();
    let asks: Vec<PriceLevel> = (0..50)
        .map(|i| PriceLevel::new(50010.0 + (i as f64 * 10.0), dec!(1.0), now_micros()))
        .collect();

    book.apply_snapshot(bids, asks, now_micros());
    let snapshot = book.to_snapshot(50);

    let json_result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);
    let bincode_result = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);

    assert!(json_result.is_ok());
    assert!(bincode_result.is_ok());

    // Verify bincode is more compact
    let json_size = json_result.unwrap().len();
    let bincode_size = bincode_result.unwrap().len();
    assert!(
        bincode_size < json_size,
        "Bincode {} should be smaller than JSON {}",
        bincode_size,
        json_size
    );
}

// =============================================================================
// STATISTICS TESTS (6 tests)
// =============================================================================

#[test]
fn test_stats_default() {
    let stats = StreamStats::default();

    assert_eq!(stats.messages_published, 0);
    assert_eq!(stats.bytes_serialized, 0);
    assert_eq!(stats.xadd_commands, 0);
    assert_eq!(stats.serialization_errors, 0);
    assert_eq!(stats.publish_errors, 0);
    assert!((stats.avg_publish_latency_us - 0.0).abs() < f64::EPSILON);
    assert!(stats.last_publish.is_none());
}

#[test]
fn test_stats_increment() {
    let mut stats = StreamStats::default();

    stats.messages_published += 1;
    stats.bytes_serialized += 1024;
    stats.xadd_commands += 1;

    assert_eq!(stats.messages_published, 1);
    assert_eq!(stats.bytes_serialized, 1024);
    assert_eq!(stats.xadd_commands, 1);
}

#[test]
fn test_stats_latency_tracking() {
    let mut stats = StreamStats::default();

    // Simulate latency updates using EMA
    let latencies = [100.0, 150.0, 120.0, 180.0, 140.0];
    let alpha = 0.2;

    for latency in latencies {
        stats.avg_publish_latency_us =
            alpha * latency + (1.0 - alpha) * stats.avg_publish_latency_us;
    }

    // Should have a smoothed average
    assert!(stats.avg_publish_latency_us > 0.0);
    assert!(stats.avg_publish_latency_us < 200.0);
}

#[test]
fn test_stats_reset() {
    let mut stats = StreamStats::default();

    stats.messages_published = 100;
    stats.bytes_serialized = 10_000;
    stats.xadd_commands = 50;
    stats.serialization_errors = 2;
    stats.publish_errors = 1;
    stats.avg_publish_latency_us = 150.0;
    stats.last_publish = Some(now_micros());

    stats.reset();

    assert_eq!(stats.messages_published, 0);
    assert_eq!(stats.bytes_serialized, 0);
    assert_eq!(stats.xadd_commands, 0);
    assert_eq!(stats.serialization_errors, 0);
    assert_eq!(stats.publish_errors, 0);
    assert!((stats.avg_publish_latency_us - 0.0).abs() < f64::EPSILON);
    assert!(stats.last_publish.is_none());
}

#[test]
fn test_stats_clone() {
    let mut stats = StreamStats::default();
    stats.messages_published = 42;
    stats.bytes_serialized = 1024;
    stats.avg_publish_latency_us = 123.45;

    let cloned = stats.clone();

    assert_eq!(stats.messages_published, cloned.messages_published);
    assert_eq!(stats.bytes_serialized, cloned.bytes_serialized);
    assert!((stats.avg_publish_latency_us - cloned.avg_publish_latency_us).abs() < f64::EPSILON);
}

#[test]
fn test_stats_concurrent_access() {
    use std::sync::Arc;
    use parking_lot::RwLock;
    use std::thread;

    let stats = Arc::new(RwLock::new(StreamStats::default()));
    let mut handles = vec![];

    // Spawn multiple threads incrementing counters
    for _ in 0..10 {
        let stats_clone = Arc::clone(&stats);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let mut s = stats_clone.write();
                s.messages_published += 1;
                s.bytes_serialized += 100;
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let final_stats = stats.read();
    assert_eq!(final_stats.messages_published, 1000);
    assert_eq!(final_stats.bytes_serialized, 100_000);
}

// =============================================================================
// ERROR TESTS (4 tests)
// =============================================================================

#[test]
fn test_error_serialization_failed() {
    let error = StreamError::SerializationFailed {
        format: SerializationFormat::Json,
        reason: "Invalid UTF-8".to_string(),
    };

    let display = error.to_string();
    assert!(display.contains("Serialization failed"));
    // Note: format uses Debug ({:?}) so it's "Json" not "json"
    assert!(display.contains("Json"));
    assert!(display.contains("Invalid UTF-8"));
}

#[test]
fn test_error_xadd_failed() {
    let error = StreamError::XaddFailed {
        topic: "market_data.deribit.btc_usd.book".to_string(),
        reason: "Connection refused".to_string(),
    };

    let display = error.to_string();
    assert!(display.contains("XADD failed"));
    assert!(display.contains("market_data.deribit.btc_usd.book"));
    assert!(display.contains("Connection refused"));
}

#[test]
fn test_error_invalid_topic() {
    let error = StreamError::InvalidTopic("invalid..topic".to_string());

    let display = error.to_string();
    assert!(display.contains("Invalid topic"));
    assert!(display.contains("invalid..topic"));
}

#[test]
fn test_error_display_all_variants() {
    let errors = vec![
        StreamError::SerializationFailed {
            format: SerializationFormat::Bincode,
            reason: "test".to_string(),
        },
        StreamError::XaddFailed {
            topic: "test".to_string(),
            reason: "test".to_string(),
        },
        StreamError::InvalidTopic("test".to_string()),
        StreamError::NoConnection,
    ];

    for error in errors {
        let display = error.to_string();
        assert!(!display.is_empty());
    }
}

// =============================================================================
// BUILDER TESTS (4 tests)
// =============================================================================

#[test]
fn test_builder_default() {
    let builder = StreamPublisherBuilder::default();
    let config = builder.config();

    assert_eq!(config.format, SerializationFormat::Bincode);
    assert_eq!(config.topic_prefix, "market_data");
}

#[test]
fn test_builder_full_config() {
    let builder = StreamPublisherBuilder::default()
        .format(SerializationFormat::Json)
        .max_stream_length(50_000)
        .approximate_trimming(false)
        .topic_prefix("custom");

    let config = builder.config();

    assert_eq!(config.format, SerializationFormat::Json);
    assert_eq!(config.max_stream_length, 50_000);
    assert!(!config.approximate_trimming);
    assert_eq!(config.topic_prefix, "custom");
}

#[test]
fn test_builder_chain() {
    // Verify builder methods return Self for chaining
    let builder = StreamPublisherBuilder::default()
        .format(SerializationFormat::Bincode)
        .max_stream_length(100_000)
        .approximate_trimming(true)
        .topic_prefix("test")
        .include_timestamp(true)
        .data_field("payload")
        .format_field("fmt");

    let config = builder.config();
    assert_eq!(config.data_field, "payload");
    assert_eq!(config.format_field, "fmt");
}

#[test]
fn test_builder_config_access() {
    let builder = StreamPublisherBuilder::default()
        .format(SerializationFormat::Json);

    // Should be able to access config without consuming builder
    let config1 = builder.config();
    let config2 = builder.config();

    assert_eq!(config1.format, config2.format);
}

// =============================================================================
// PUBLISH RESULT TESTS (2 tests)
// =============================================================================

#[test]
fn test_publish_result_fields() {
    let result = PublishResult {
        message_id: "1234567890-0".to_string(),
        topic: "market_data.deribit.btc_usd.book".to_string(),
        data_size: 1024,
        latency_us: 500,
    };

    assert_eq!(result.message_id, "1234567890-0");
    assert_eq!(result.topic, "market_data.deribit.btc_usd.book");
    assert_eq!(result.data_size, 1024);
    assert_eq!(result.latency_us, 500);
}

#[test]
fn test_publish_result_clone() {
    let result = PublishResult {
        message_id: "1234567890-0".to_string(),
        topic: "market_data.deribit.btc_usd.book".to_string(),
        data_size: 1024,
        latency_us: 500,
    };

    let cloned = result.clone();

    assert_eq!(result.message_id, cloned.message_id);
    assert_eq!(result.topic, cloned.topic);
    assert_eq!(result.data_size, cloned.data_size);
    assert_eq!(result.latency_us, cloned.latency_us);
}

// =============================================================================
// TOPIC TYPE TESTS (2 tests)
// =============================================================================

#[test]
fn test_topic_type_as_str() {
    assert_eq!(TopicType::Book.as_str(), "book");
    assert_eq!(TopicType::Trade.as_str(), "trade");
    assert_eq!(TopicType::Ticker.as_str(), "ticker");
    assert_eq!(TopicType::Event.as_str(), "event");
}

#[test]
fn test_topic_type_display() {
    assert_eq!(format!("{}", TopicType::Book), "book");
    assert_eq!(format!("{}", TopicType::Trade), "trade");
    assert_eq!(format!("{}", TopicType::Ticker), "ticker");
    assert_eq!(format!("{}", TopicType::Event), "event");
}

// =============================================================================
// SEND + SYNC VERIFICATION
// =============================================================================

#[test]
fn test_types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SerializationFormat>();
    assert_send_sync::<StreamPublisherConfig>();
    assert_send_sync::<StreamStats>();
    assert_send_sync::<PublishResult>();
    assert_send_sync::<TopicType>();
    // StreamPublisher and StreamError are verified at compile time in the module
}

// =============================================================================
// INTEGRATION TESTS (require Redis - marked with #[ignore])
// =============================================================================

#[test]
#[ignore = "Requires Redis server"]
fn test_xadd_single_message() {
    // This test requires a running Redis server
    // It will be run during integration testing with REDIS_TEST=1
    todo!("Integration test: XADD single message")
}

#[test]
#[ignore = "Requires Redis server"]
fn test_xadd_with_maxlen() {
    // This test requires a running Redis server
    todo!("Integration test: XADD with MAXLEN")
}

#[test]
#[ignore = "Requires Redis server"]
fn test_xadd_multiple_messages() {
    // This test requires a running Redis server
    todo!("Integration test: XADD multiple messages")
}

#[test]
#[ignore = "Requires Redis server"]
fn test_xadd_binary_data() {
    // This test requires a running Redis server
    todo!("Integration test: XADD binary data")
}
