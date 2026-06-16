//! Tests for Dual Output Publisher (Batch 3.2).
//!
//! These tests verify the dual output logic that publishes to BOTH:
//! - `market:orderbook:{symbol}` - Raw OrderBookSnapshot for Gateway UI
//! - `astra:signals:flash:{exchange}:{symbol}` - AlphaSignal for Fusion engine
//!
//! Test Categories:
//! - Configuration Tests (6 tests)
//! - DualPublishResult Tests (4 tests)
//! - Topic Generation Tests (6 tests)
//! - Conversion Tests (6 tests)
//! - Statistics Tests (4 tests)
//! - Builder Tests (4 tests)
//! - Integration Tests (6 tests - requires Redis)
//!
//! Total: 36 tests (TDI requirement)

use astra_flash::book::orderbook::BookSnapshot;
use astra_flash::core::types::{now_micros, Exchange, Instrument, PriceLevel};
use astra_flash::fusion::{AlphaSignal, SignalDirection, FUSION_TOPIC_PREFIX};
use astra_flash::gateway::OrderBookSnapshot as GatewaySnapshot;
use astra_flash::publisher::{
    DualPublishResult, DualPublisher, DualPublisherBuilder, DualPublisherConfig, DualStats,
    PoolConfig, RedisPool, SerializationFormat,
};
use rust_decimal_macros::dec;
use std::sync::Arc;

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Default test Redis URL (local)
const TEST_REDIS_URL: &str = "redis://127.0.0.1:6379";

/// Create a test pool configuration
fn test_config() -> PoolConfig {
    PoolConfig {
        url: TEST_REDIS_URL.to_string(),
        max_size: 10,
        min_idle: Some(2),
        connect_timeout_ms: 5000,
        wait_timeout_ms: 5000,
        health_check_interval_ms: 10000,
        auto_reconnect: true,
        max_reconnect_attempts: 3,
        reconnect_delay_ms: 100,
        database: 0,
        password: None,
        tls_enabled: false,
    }
}

/// Create a test instrument
fn test_instrument() -> Instrument {
    Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")
}

/// Create a test book snapshot
fn test_book_snapshot() -> BookSnapshot {
    BookSnapshot {
        instrument: test_instrument(),
        timestamp: now_micros(),
        bids: vec![
            PriceLevel {
                price: 1.0850,
                quantity: dec!(1000000),
                order_count: Some(5),
                timestamp: now_micros(),
            },
            PriceLevel {
                price: 1.0848,
                quantity: dec!(500000),
                order_count: Some(3),
                timestamp: now_micros(),
            },
        ],
        asks: vec![
            PriceLevel {
                price: 1.0852,
                quantity: dec!(800000),
                order_count: Some(4),
                timestamp: now_micros(),
            },
            PriceLevel {
                price: 1.0854,
                quantity: dec!(600000),
                order_count: Some(2),
                timestamp: now_micros(),
            },
        ],
    }
}

/// Create a test book snapshot with imbalance for signal direction testing
fn test_book_snapshot_with_imbalance(bid_heavy: bool) -> BookSnapshot {
    let instrument = test_instrument();
    let ts = now_micros();

    if bid_heavy {
        // Bid heavy -> LONG signal
        BookSnapshot {
            instrument,
            timestamp: ts,
            bids: vec![PriceLevel {
                price: 1.0850,
                quantity: dec!(2000000), // 2M bid
                order_count: Some(10),
                timestamp: ts,
            }],
            asks: vec![PriceLevel {
                price: 1.0852,
                quantity: dec!(500000), // 500K ask
                order_count: Some(2),
                timestamp: ts,
            }],
        }
    } else {
        // Ask heavy -> SHORT signal
        BookSnapshot {
            instrument,
            timestamp: ts,
            bids: vec![PriceLevel {
                price: 1.0850,
                quantity: dec!(500000), // 500K bid
                order_count: Some(2),
                timestamp: ts,
            }],
            asks: vec![PriceLevel {
                price: 1.0852,
                quantity: dec!(2000000), // 2M ask
                order_count: Some(10),
                timestamp: ts,
            }],
        }
    }
}

/// Check if Redis is available for integration tests
fn redis_available() -> bool {
    std::env::var("REDIS_TEST_URL").is_ok() || std::env::var("TEST_REDIS").is_ok()
}

// =============================================================================
// CONFIGURATION TESTS (6 tests)
// =============================================================================

#[test]
fn test_dual_publisher_config_default() {
    let config = DualPublisherConfig::default();

    assert!(config.orderbook_enabled);
    assert!(config.signal_enabled);
    assert_eq!(config.orderbook_key_prefix, "market:orderbook:");
    assert_eq!(config.signal_key_prefix, FUSION_TOPIC_PREFIX);
    assert_eq!(config.ttl_seconds, 60);
    assert_eq!(config.format, SerializationFormat::Json);
}

#[test]
fn test_dual_publisher_config_orderbook_only() {
    let config = DualPublisherConfig {
        orderbook_enabled: true,
        signal_enabled: false,
        ..DualPublisherConfig::default()
    };

    assert!(config.orderbook_enabled);
    assert!(!config.signal_enabled);
}

#[test]
fn test_dual_publisher_config_signal_only() {
    let config = DualPublisherConfig {
        orderbook_enabled: false,
        signal_enabled: true,
        ..DualPublisherConfig::default()
    };

    assert!(!config.orderbook_enabled);
    assert!(config.signal_enabled);
}

#[test]
fn test_dual_publisher_config_custom_prefixes() {
    let config = DualPublisherConfig {
        orderbook_key_prefix: "custom:orderbook:".to_string(),
        signal_key_prefix: "custom:signals:".to_string(),
        ..DualPublisherConfig::default()
    };

    assert_eq!(config.orderbook_key_prefix, "custom:orderbook:");
    assert_eq!(config.signal_key_prefix, "custom:signals:");
}

#[test]
fn test_dual_publisher_config_ttl() {
    let config = DualPublisherConfig {
        ttl_seconds: 120,
        ..DualPublisherConfig::default()
    };

    assert_eq!(config.ttl_seconds, 120);
}

#[test]
fn test_dual_publisher_config_format() {
    let config = DualPublisherConfig {
        format: SerializationFormat::Bincode,
        ..DualPublisherConfig::default()
    };

    assert_eq!(config.format, SerializationFormat::Bincode);
}

// =============================================================================
// DUAL PUBLISH RESULT TESTS (4 tests)
// =============================================================================

#[test]
fn test_dual_publish_result_both_keys() {
    let result = DualPublishResult {
        orderbook_key: Some("market:orderbook:EUR_USD".to_string()),
        signal_key: Some("astra:signals:flash:oanda:EUR_USD".to_string()),
        orderbook_message_id: Some("1234-0".to_string()),
        signal_message_id: Some("1234-1".to_string()),
        latency_us: 500,
    };

    assert!(result.orderbook_key.is_some());
    assert!(result.signal_key.is_some());
    assert!(result.orderbook_message_id.is_some());
    assert!(result.signal_message_id.is_some());
}

#[test]
fn test_dual_publish_result_orderbook_only() {
    let result = DualPublishResult {
        orderbook_key: Some("market:orderbook:EUR_USD".to_string()),
        signal_key: None,
        orderbook_message_id: Some("1234-0".to_string()),
        signal_message_id: None,
        latency_us: 300,
    };

    assert!(result.orderbook_key.is_some());
    assert!(result.signal_key.is_none());
}

#[test]
fn test_dual_publish_result_signal_only() {
    let result = DualPublishResult {
        orderbook_key: None,
        signal_key: Some("astra:signals:flash:oanda:EUR_USD".to_string()),
        orderbook_message_id: None,
        signal_message_id: Some("1234-1".to_string()),
        latency_us: 200,
    };

    assert!(result.orderbook_key.is_none());
    assert!(result.signal_key.is_some());
}

#[test]
fn test_dual_publish_result_latency() {
    let result = DualPublishResult {
        orderbook_key: Some("market:orderbook:EUR_USD".to_string()),
        signal_key: Some("astra:signals:flash:oanda:EUR_USD".to_string()),
        orderbook_message_id: Some("1234-0".to_string()),
        signal_message_id: Some("1234-1".to_string()),
        latency_us: 750,
    };

    assert_eq!(result.latency_us, 750);
}

// =============================================================================
// TOPIC GENERATION TESTS (6 tests)
// =============================================================================

#[test]
fn test_orderbook_topic_generation() {
    let snapshot = test_book_snapshot();
    let config = DualPublisherConfig::default();

    let topic = DualPublisher::orderbook_key(&snapshot.instrument, &config);

    assert_eq!(topic, "market:orderbook:EUR_USD");
}

#[test]
fn test_signal_topic_generation() {
    let snapshot = test_book_snapshot();
    let config = DualPublisherConfig::default();

    let topic = DualPublisher::signal_key(&snapshot.instrument, &config);

    assert_eq!(topic, "astra:signals:flash:oanda:EUR_USD");
}

#[test]
fn test_orderbook_topic_btc() {
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let config = DualPublisherConfig::default();

    let topic = DualPublisher::orderbook_key(&instrument, &config);

    assert_eq!(topic, "market:orderbook:BTC_USD");
}

#[test]
fn test_signal_topic_btc() {
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let config = DualPublisherConfig::default();

    let topic = DualPublisher::signal_key(&instrument, &config);

    assert_eq!(topic, "astra:signals:flash:deribit:BTC_USD");
}

#[test]
fn test_orderbook_topic_custom_prefix() {
    let instrument = test_instrument();
    let config = DualPublisherConfig {
        orderbook_key_prefix: "custom:book:".to_string(),
        ..DualPublisherConfig::default()
    };

    let topic = DualPublisher::orderbook_key(&instrument, &config);

    assert_eq!(topic, "custom:book:EUR_USD");
}

#[test]
fn test_signal_topic_custom_prefix() {
    let instrument = test_instrument();
    let config = DualPublisherConfig {
        signal_key_prefix: "custom:sig".to_string(), // No trailing colon - format adds it
        ..DualPublisherConfig::default()
    };

    let topic = DualPublisher::signal_key(&instrument, &config);

    assert_eq!(topic, "custom:sig:oanda:EUR_USD");
}

// =============================================================================
// CONVERSION TESTS (6 tests)
// =============================================================================

#[test]
fn test_book_to_gateway_snapshot() {
    let book = test_book_snapshot();

    let gateway = GatewaySnapshot::from_book_snapshot(&book);

    assert_eq!(gateway.symbol, "EUR_USD");
    assert_eq!(gateway.exchange, "oanda");
    assert_eq!(gateway.bids.len(), 2);
    assert_eq!(gateway.asks.len(), 2);
    assert!((gateway.bids[0].price - 1.0850).abs() < f64::EPSILON);
}

#[test]
fn test_book_to_alpha_signal() {
    let book = test_book_snapshot();

    let signal = AlphaSignal::from_book_snapshot(&book, 0.8);

    assert!(signal.validate().is_ok());
    assert_eq!(signal.symbol, "EUR_USD");
    assert_eq!(signal.metadata.source_strategy, "flash");
    assert!((signal.metadata.confidence - 0.8).abs() < f64::EPSILON);
}

#[test]
fn test_book_to_alpha_signal_bid_heavy() {
    let book = test_book_snapshot_with_imbalance(true);

    let signal = AlphaSignal::from_book_snapshot(&book, 0.9);

    // Bid heavy should generate LONG signal
    assert_eq!(signal.signal, SignalDirection::Long);
    assert!(signal.strength > 0.0);
}

#[test]
fn test_book_to_alpha_signal_ask_heavy() {
    let book = test_book_snapshot_with_imbalance(false);

    let signal = AlphaSignal::from_book_snapshot(&book, 0.9);

    // Ask heavy should generate SHORT signal
    assert_eq!(signal.signal, SignalDirection::Short);
    assert!(signal.strength > 0.0);
}

#[test]
fn test_gateway_snapshot_serialization() {
    let book = test_book_snapshot();
    let gateway = GatewaySnapshot::from_book_snapshot(&book);

    let json = gateway.to_json().expect("Should serialize");

    assert!(json.contains("EUR_USD"));
    assert!(json.contains("oanda"));
    assert!(json.contains("bids"));
    assert!(json.contains("asks"));
}

#[test]
fn test_alpha_signal_serialization() {
    let book = test_book_snapshot();
    let signal = AlphaSignal::from_book_snapshot(&book, 0.8);

    let json = signal.to_json().expect("Should serialize");

    assert!(json.contains("signal_id"));
    assert!(json.contains("EUR_USD"));
    assert!(json.contains("flash"));
    assert!(json.contains("strength"));
}

// =============================================================================
// STATISTICS TESTS (4 tests)
// =============================================================================

#[test]
fn test_dual_stats_default() {
    let stats = DualStats::default();

    assert_eq!(stats.orderbook_published, 0);
    assert_eq!(stats.signals_published, 0);
    assert_eq!(stats.total_latency_us, 0);
    assert_eq!(stats.publish_count, 0);
}

#[test]
fn test_dual_stats_increment() {
    let mut stats = DualStats::default();
    stats.orderbook_published += 1;
    stats.signals_published += 1;
    stats.publish_count += 1;
    stats.total_latency_us += 500;

    assert_eq!(stats.orderbook_published, 1);
    assert_eq!(stats.signals_published, 1);
    assert_eq!(stats.publish_count, 1);
    assert_eq!(stats.total_latency_us, 500);
}

#[test]
fn test_dual_stats_average_latency() {
    let mut stats = DualStats::default();
    stats.total_latency_us = 1000;
    stats.publish_count = 4;

    let avg = stats.avg_latency_us();

    assert!((avg - 250.0).abs() < f64::EPSILON);
}

#[test]
fn test_dual_stats_reset() {
    let mut stats = DualStats::default();
    stats.orderbook_published = 100;
    stats.signals_published = 100;
    stats.publish_count = 100;
    stats.total_latency_us = 50000;

    stats.reset();

    assert_eq!(stats.orderbook_published, 0);
    assert_eq!(stats.signals_published, 0);
    assert_eq!(stats.publish_count, 0);
    assert_eq!(stats.total_latency_us, 0);
}

// =============================================================================
// BUILDER TESTS (4 tests)
// =============================================================================

#[test]
fn test_dual_publisher_builder_default() {
    let builder = DualPublisherBuilder::default();
    let config = builder.config();

    assert!(config.orderbook_enabled);
    assert!(config.signal_enabled);
}

#[test]
fn test_dual_publisher_builder_disable_orderbook() {
    let builder = DualPublisherBuilder::default().orderbook_enabled(false);
    let config = builder.config();

    assert!(!config.orderbook_enabled);
    assert!(config.signal_enabled);
}

#[test]
fn test_dual_publisher_builder_disable_signal() {
    let builder = DualPublisherBuilder::default().signal_enabled(false);
    let config = builder.config();

    assert!(config.orderbook_enabled);
    assert!(!config.signal_enabled);
}

#[test]
fn test_dual_publisher_builder_ttl() {
    let builder = DualPublisherBuilder::default().ttl_seconds(300);
    let config = builder.config();

    assert_eq!(config.ttl_seconds, 300);
}

// =============================================================================
// INTEGRATION TESTS (6 tests - requires Redis)
// =============================================================================

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_writes_both_keys() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let publisher = DualPublisher::new(pool.clone(), DualPublisherConfig::default());

    let snapshot = test_book_snapshot();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    // Both keys should be written
    assert!(result.orderbook_key.is_some());
    assert!(result.signal_key.is_some());
    assert!(result.orderbook_message_id.is_some());
    assert!(result.signal_message_id.is_some());

    // Verify keys exist in Redis
    let mut conn = pool.get().await.unwrap();

    let orderbook_key = result.orderbook_key.as_ref().unwrap();
    let signal_key = result.signal_key.as_ref().unwrap();

    // Use XLEN to check stream has entries
    let orderbook_len: u64 = redis::cmd("XLEN")
        .arg(orderbook_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert!(orderbook_len >= 1);

    let signal_len: u64 = redis::cmd("XLEN")
        .arg(signal_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert!(signal_len >= 1);

    // Cleanup
    let _: () = redis::cmd("DEL")
        .arg(orderbook_key)
        .arg(signal_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_orderbook_content() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let publisher = DualPublisher::new(pool.clone(), DualPublisherConfig::default());

    let snapshot = test_book_snapshot();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    let mut conn = pool.get().await.unwrap();
    let orderbook_key = result.orderbook_key.as_ref().unwrap();

    // Read the latest entry
    let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg(orderbook_key)
        .arg("-")
        .arg("+")
        .arg("COUNT")
        .arg(1)
        .query_async(&mut *conn)
        .await
        .unwrap();

    assert!(!entries.is_empty());

    // Find the data field
    let fields = &entries[0].1;
    let data_field = fields.iter().find(|(k, _)| k == "data");
    assert!(data_field.is_some());

    let data = &data_field.unwrap().1;
    assert!(data.contains("EUR_USD"));
    assert!(data.contains("oanda"));

    // Cleanup
    let _: () = redis::cmd("DEL")
        .arg(orderbook_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_signal_content() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let publisher = DualPublisher::new(pool.clone(), DualPublisherConfig::default());

    let snapshot = test_book_snapshot();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    let mut conn = pool.get().await.unwrap();
    let signal_key = result.signal_key.as_ref().unwrap();

    // Read the latest entry
    let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg(signal_key)
        .arg("-")
        .arg("+")
        .arg("COUNT")
        .arg(1)
        .query_async(&mut *conn)
        .await
        .unwrap();

    assert!(!entries.is_empty());

    // Find the data field
    let fields = &entries[0].1;
    let data_field = fields.iter().find(|(k, _)| k == "data");
    assert!(data_field.is_some());

    let data = &data_field.unwrap().1;
    assert!(data.contains("signal_id"));
    assert!(data.contains("EUR_USD"));
    assert!(data.contains("flash"));

    // Cleanup
    let _: () = redis::cmd("DEL")
        .arg(signal_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_orderbook_only() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let config = DualPublisherConfig {
        orderbook_enabled: true,
        signal_enabled: false,
        ..DualPublisherConfig::default()
    };
    let publisher = DualPublisher::new(pool.clone(), config);

    let snapshot = test_book_snapshot();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    // Only orderbook should be written
    assert!(result.orderbook_key.is_some());
    assert!(result.signal_key.is_none());

    // Cleanup
    let mut conn = pool.get().await.unwrap();
    let orderbook_key = result.orderbook_key.as_ref().unwrap();
    let _: () = redis::cmd("DEL")
        .arg(orderbook_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_signal_only() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let config = DualPublisherConfig {
        orderbook_enabled: false,
        signal_enabled: true,
        ..DualPublisherConfig::default()
    };
    let publisher = DualPublisher::new(pool.clone(), config);

    let snapshot = test_book_snapshot();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    // Only signal should be written
    assert!(result.orderbook_key.is_none());
    assert!(result.signal_key.is_some());

    // Cleanup
    let mut conn = pool.get().await.unwrap();
    let signal_key = result.signal_key.as_ref().unwrap();
    let _: () = redis::cmd("DEL")
        .arg(signal_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_dual_publish_statistics() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let publisher = DualPublisher::new(pool.clone(), DualPublisherConfig::default());

    let snapshot = test_book_snapshot();

    // Publish twice
    let _ = publisher.publish_dual(&snapshot).await.unwrap();
    let result = publisher.publish_dual(&snapshot).await.unwrap();

    let stats = publisher.stats();

    assert_eq!(stats.orderbook_published, 2);
    assert_eq!(stats.signals_published, 2);
    assert_eq!(stats.publish_count, 2);
    assert!(stats.total_latency_us > 0);

    // Cleanup
    let mut conn = pool.get().await.unwrap();
    let orderbook_key = result.orderbook_key.as_ref().unwrap();
    let signal_key = result.signal_key.as_ref().unwrap();
    let _: () = redis::cmd("DEL")
        .arg(orderbook_key)
        .arg(signal_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
}
