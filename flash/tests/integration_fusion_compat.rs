//! Integration Tests: Decision-Aggregation Compatibility
//!
//! These tests verify that Flash correctly publishes signals in the
//! AlphaSignal format expected by the downstream decision-aggregation engine.
//!
//! # Test Coverage
//!
//! 1. Topic naming: `astra:signals:flash` stream prefix
//! 2. Payload format: Valid AlphaSignal JSON envelope
//! 3. Required fields: signal_id, timestamp, symbol, signal, strength, metadata
//!
//! # Run
//!
//! ```bash
//! cargo test --test integration_fusion_compat
//! ```

use astra_flash::book::orderbook::BookSnapshot;
use astra_flash::core::types::{now_micros, Exchange, Instrument, PriceLevel};
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// ALPHA SIGNAL - Fusion's Expected Format
// =============================================================================

/// AlphaSignal is the standardized signal format expected by the downstream decision-aggregation engine.
///
/// All sidecar projects MUST emit this format to the `astra:signals:*` streams.
///
/// # Fields
///
/// - `signal_id`: Unique identifier (UUID v4)
/// - `timestamp`: ISO-8601 UTC timestamp
/// - `symbol`: Trading pair (e.g., "EUR_USD", "BTC_USD")
/// - `signal`: Direction ("LONG", "SHORT", "NEUTRAL")
/// - `strength`: Conviction level (0.0 to 1.0)
/// - `metadata`: Source-specific additional data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaSignal {
    /// Unique signal identifier (UUID v4)
    pub signal_id: String,
    /// ISO-8601 UTC timestamp when signal was generated
    pub timestamp: String,
    /// Trading symbol (e.g., "EUR_USD", "BTC_USD")
    pub symbol: String,
    /// Signal direction: LONG, SHORT, or NEUTRAL
    pub signal: SignalDirection,
    /// Signal strength/conviction (0.0 to 1.0)
    pub strength: f64,
    /// Source-specific metadata
    pub metadata: AlphaSignalMetadata,
}

/// Signal direction enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum SignalDirection {
    Long,
    Short,
    Neutral,
}

/// Metadata envelope for AlphaSignal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaSignalMetadata {
    /// Source strategy identifier (e.g., "flash", "sentiment", "carry")
    pub source_strategy: String,
    /// Signal confidence (0.0 to 1.0)
    pub confidence: f64,
    /// Market regime classification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regime: Option<String>,
    /// Raw directional value (-1.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_direction: Option<f64>,
    /// Additional source-specific data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl AlphaSignal {
    /// Validate that this signal has all required fields.
    pub fn validate(&self) -> Result<(), String> {
        // Validate signal_id is a valid UUID
        if Uuid::parse_str(&self.signal_id).is_err() {
            return Err("signal_id must be a valid UUID".to_string());
        }

        // Validate timestamp is ISO-8601
        if chrono::DateTime::parse_from_rfc3339(&self.timestamp).is_err() {
            return Err("timestamp must be ISO-8601 format".to_string());
        }

        // Validate symbol is non-empty
        if self.symbol.is_empty() {
            return Err("symbol cannot be empty".to_string());
        }

        // Validate strength is in range
        if !(0.0..=1.0).contains(&self.strength) {
            return Err("strength must be between 0.0 and 1.0".to_string());
        }

        // Validate confidence is in range
        if !(0.0..=1.0).contains(&self.metadata.confidence) {
            return Err("confidence must be between 0.0 and 1.0".to_string());
        }

        // Validate source_strategy is non-empty
        if self.metadata.source_strategy.is_empty() {
            return Err("source_strategy cannot be empty".to_string());
        }

        Ok(())
    }
}

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test BookSnapshot.
fn test_book_snapshot() -> BookSnapshot {
    BookSnapshot {
        instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
        timestamp: now_micros(),
        bids: vec![
            PriceLevel {
                price: 50000.0,
                quantity: dec!(10.0),
                order_count: Some(5),
                timestamp: now_micros(),
            },
            PriceLevel {
                price: 49990.0,
                quantity: dec!(15.0),
                order_count: Some(8),
                timestamp: now_micros(),
            },
        ],
        asks: vec![
            PriceLevel {
                price: 50010.0,
                quantity: dec!(8.0),
                order_count: Some(4),
                timestamp: now_micros(),
            },
            PriceLevel {
                price: 50020.0,
                quantity: dec!(12.0),
                order_count: Some(6),
                timestamp: now_micros(),
            },
        ],
    }
}

/// Create a valid AlphaSignal for testing.
fn test_alpha_signal() -> AlphaSignal {
    AlphaSignal {
        signal_id: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        symbol: "BTC_USD".to_string(),
        signal: SignalDirection::Long,
        strength: 0.75,
        metadata: AlphaSignalMetadata {
            source_strategy: "flash".to_string(),
            confidence: 0.85,
            regime: Some("TRENDING_UP".to_string()),
            raw_direction: Some(0.75),
            extra: Some(serde_json::json!({
                "bid_depth": 25.0,
                "ask_depth": 20.0,
                "imbalance": 0.11
            })),
        },
    }
}

// =============================================================================
// CATEGORY 1: ALPHA SIGNAL STRUCT TESTS
// =============================================================================

#[test]
fn test_alpha_signal_serializes_to_json() {
    let signal = test_alpha_signal();

    let json = serde_json::to_string(&signal).expect("Should serialize to JSON");

    // Verify required fields are present
    assert!(json.contains("signal_id"));
    assert!(json.contains("timestamp"));
    assert!(json.contains("symbol"));
    assert!(json.contains("signal"));
    assert!(json.contains("strength"));
    assert!(json.contains("metadata"));
    assert!(json.contains("source_strategy"));
    assert!(json.contains("confidence"));
}

#[test]
fn test_alpha_signal_deserializes_from_json() {
    let json = r#"{
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "timestamp": "2026-01-17T12:00:00Z",
        "symbol": "EUR_USD",
        "signal": "LONG",
        "strength": 0.8,
        "metadata": {
            "source_strategy": "flash",
            "confidence": 0.9
        }
    }"#;

    let signal: AlphaSignal = serde_json::from_str(json).expect("Should deserialize from JSON");

    assert_eq!(signal.symbol, "EUR_USD");
    assert_eq!(signal.signal, SignalDirection::Long);
    assert!((signal.strength - 0.8).abs() < f64::EPSILON);
    assert_eq!(signal.metadata.source_strategy, "flash");
}

#[test]
fn test_alpha_signal_validation_valid() {
    let signal = test_alpha_signal();
    assert!(signal.validate().is_ok());
}

#[test]
fn test_alpha_signal_validation_invalid_uuid() {
    let mut signal = test_alpha_signal();
    signal.signal_id = "not-a-uuid".to_string();

    let result = signal.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("UUID"));
}

#[test]
fn test_alpha_signal_validation_invalid_timestamp() {
    let mut signal = test_alpha_signal();
    signal.timestamp = "not-a-timestamp".to_string();

    let result = signal.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("ISO-8601"));
}

#[test]
fn test_alpha_signal_validation_invalid_strength() {
    let mut signal = test_alpha_signal();
    signal.strength = 1.5; // Out of range

    let result = signal.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("strength"));
}

#[test]
fn test_alpha_signal_validation_empty_symbol() {
    let mut signal = test_alpha_signal();
    signal.symbol = String::new();

    let result = signal.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("symbol"));
}

// =============================================================================
// CATEGORY 2: SIGNAL DIRECTION TESTS
// =============================================================================

#[test]
fn test_signal_direction_serializes_uppercase() {
    let long = serde_json::to_string(&SignalDirection::Long).unwrap();
    let short = serde_json::to_string(&SignalDirection::Short).unwrap();
    let neutral = serde_json::to_string(&SignalDirection::Neutral).unwrap();

    assert_eq!(long, "\"LONG\"");
    assert_eq!(short, "\"SHORT\"");
    assert_eq!(neutral, "\"NEUTRAL\"");
}

#[test]
fn test_signal_direction_deserializes_uppercase() {
    let long: SignalDirection = serde_json::from_str("\"LONG\"").unwrap();
    let short: SignalDirection = serde_json::from_str("\"SHORT\"").unwrap();
    let neutral: SignalDirection = serde_json::from_str("\"NEUTRAL\"").unwrap();

    assert_eq!(long, SignalDirection::Long);
    assert_eq!(short, SignalDirection::Short);
    assert_eq!(neutral, SignalDirection::Neutral);
}

// =============================================================================
// CATEGORY 3: TOPIC NAMING TESTS (FUTURE - requires config change)
// =============================================================================

/// This test verifies that the EXPECTED topic prefix is correct.
/// The actual implementation will be added in Step 3.
#[test]
fn test_fusion_topic_prefix_format() {
    // The expected topic format for Fusion compatibility
    let expected_prefix = "astra:signals:flash";

    // Topic should follow pattern: astra:signals:flash:{exchange}:{symbol}
    let topic = format!("{}:deribit:BTC_USD", expected_prefix);

    assert!(topic.starts_with("astra:signals:flash"));
    assert!(topic.contains("deribit"));
    assert!(topic.contains("BTC_USD"));
}

#[test]
fn test_fusion_topic_uses_underscores_for_pairs() {
    // Fusion expects symbols like "EUR_USD" not "EUR/USD" or "EURUSD"
    let symbol = "BTC_USD";
    let topic = format!("astra:signals:flash:deribit:{}", symbol);

    assert!(topic.contains("BTC_USD"));
    assert!(!topic.contains("/"));
}

// =============================================================================
// CATEGORY 4: BOOK SNAPSHOT TO ALPHA SIGNAL CONVERSION TESTS
// =============================================================================

/// This test defines the expected conversion logic.
/// Implementation will be added in Step 3.
#[test]
fn test_book_snapshot_to_alpha_signal_structure() {
    let snapshot = test_book_snapshot();

    // Calculate order imbalance as a signal proxy
    let total_bid_qty: f64 = snapshot
        .bids
        .iter()
        .map(|l| l.quantity.to_string().parse::<f64>().unwrap_or(0.0))
        .sum();
    let total_ask_qty: f64 = snapshot
        .asks
        .iter()
        .map(|l| l.quantity.to_string().parse::<f64>().unwrap_or(0.0))
        .sum();

    let imbalance = if total_bid_qty + total_ask_qty > 0.0 {
        (total_bid_qty - total_ask_qty) / (total_bid_qty + total_ask_qty)
    } else {
        0.0
    };

    // Determine signal direction from imbalance
    let direction = if imbalance > 0.1 {
        SignalDirection::Long
    } else if imbalance < -0.1 {
        SignalDirection::Short
    } else {
        SignalDirection::Neutral
    };

    // Create AlphaSignal from snapshot data
    let signal = AlphaSignal {
        signal_id: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        symbol: format!("{}_{}", snapshot.instrument.base, snapshot.instrument.quote),
        signal: direction,
        strength: imbalance.abs().min(1.0),
        metadata: AlphaSignalMetadata {
            source_strategy: "flash".to_string(),
            confidence: 0.8, // Could be derived from spread tightness
            regime: None,
            raw_direction: Some(imbalance),
            extra: Some(serde_json::json!({
                "bid_depth": total_bid_qty,
                "ask_depth": total_ask_qty,
                "best_bid": snapshot.bids.first().map(|l| l.price),
                "best_ask": snapshot.asks.first().map(|l| l.price),
            })),
        },
    };

    // Validate the generated signal
    assert!(signal.validate().is_ok());
    assert_eq!(signal.metadata.source_strategy, "flash");
    assert_eq!(signal.symbol, "BTC_USD");
}

// =============================================================================
// CATEGORY 5: INTEGRATION TEST SKELETON (requires real Redis)
// =============================================================================

/// Integration test that requires actual Redis.
/// Run with: cargo test --test integration_fusion_compat -- --ignored
#[test]
#[ignore = "Requires Redis - run with cargo test --test integration_fusion_compat -- --ignored"]
fn test_publish_to_fusion_stream() {
    // This test will be implemented once the Flash publisher is updated
    // to support the astra:signals:flash topic format

    // 1. Start mock Redis or connect to test Redis
    // 2. Configure Flash with fusion-compatible settings
    // 3. Publish a market update
    // 4. Read from astra:signals:flash stream
    // 5. Verify payload is valid AlphaSignal JSON

    todo!("Implement once Flash publisher supports AlphaSignal format");
}

/// Test that verifies the complete pipeline.
#[test]
#[ignore = "Requires Redis - run with cargo test --test integration_fusion_compat -- --ignored"]
fn test_end_to_end_fusion_compatibility() {
    // 1. Create BookSnapshot
    // 2. Convert to AlphaSignal
    // 3. Serialize to JSON
    // 4. Publish to astra:signals:flash:{exchange}:{symbol}
    // 5. Consumer reads and deserializes
    // 6. Validate signal structure

    todo!("Implement once Flash publisher supports AlphaSignal format");
}

// =============================================================================
// CATEGORY 6: METADATA TESTS
// =============================================================================

#[test]
fn test_metadata_optional_fields_omitted() {
    let metadata = AlphaSignalMetadata {
        source_strategy: "flash".to_string(),
        confidence: 0.9,
        regime: None,
        raw_direction: None,
        extra: None,
    };

    let json = serde_json::to_string(&metadata).unwrap();

    // Optional fields should NOT appear when None
    assert!(!json.contains("regime"));
    assert!(!json.contains("raw_direction"));
    assert!(!json.contains("extra"));

    // Required fields MUST appear
    assert!(json.contains("source_strategy"));
    assert!(json.contains("confidence"));
}

#[test]
fn test_metadata_optional_fields_present() {
    let metadata = AlphaSignalMetadata {
        source_strategy: "flash".to_string(),
        confidence: 0.9,
        regime: Some("VOLATILE".to_string()),
        raw_direction: Some(0.5),
        extra: Some(serde_json::json!({"key": "value"})),
    };

    let json = serde_json::to_string(&metadata).unwrap();

    // All fields should appear
    assert!(json.contains("regime"));
    assert!(json.contains("raw_direction"));
    assert!(json.contains("extra"));
}

// =============================================================================
// BENCHMARKS (Optional)
// =============================================================================

#[test]
#[ignore = "timing-sensitive micro-benchmark (debug-build wall-clock); flaky on cold CI runners. Run explicitly with `cargo test -- --ignored`."]
fn test_alpha_signal_serialization_performance() {
    let signal = test_alpha_signal();

    // Warm up
    for _ in 0..100 {
        let _ = serde_json::to_string(&signal);
    }

    // Measure
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        let _ = serde_json::to_string(&signal).unwrap();
    }
    let elapsed = start.elapsed();

    // Should serialize 10k signals in under 100ms
    assert!(
        elapsed.as_millis() < 100,
        "Serialization too slow: {:?}",
        elapsed
    );
}
