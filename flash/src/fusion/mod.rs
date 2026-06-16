//! Signal-Output Integration Module
//!
//! This module provides the AlphaSignal format required for integration
//! with a downstream decision-aggregation engine.
//!
//! # Overview
//!
//! The downstream aggregator expects each upstream producer to publish signals
//! in a standardized format called `AlphaSignal`. This module provides:
//!
//! - [`AlphaSignal`] - The standardized signal format
//! - [`SignalDirection`] - Trading direction enum
//! - [`AlphaSignalMetadata`] - Source-specific metadata
//! - Conversion utilities from Flash-native types
//!
//! # Topic Naming Convention
//!
//! Flash publishes to: `astra:signals:flash:{exchange}:{symbol}`
//!
//! Examples:
//! - `astra:signals:flash:deribit:BTC_USD`
//! - `astra:signals:flash:binance:ETH_USD`
//! - `astra:signals:flash:oanda:EUR_USD`
//!
//! # Example
//!
//! ```rust
//! use astra_flash::fusion::{AlphaSignal, SignalDirection, AlphaSignalMetadata};
//! use astra_flash::book::orderbook::BookSnapshot;
//!
//! // Convert a book snapshot to AlphaSignal
//! fn convert_snapshot(snapshot: &BookSnapshot) -> AlphaSignal {
//!     AlphaSignal::from_book_snapshot(snapshot, 0.8)
//! }
//! ```

use crate::book::orderbook::BookSnapshot;
use crate::core::types::{Exchange, Instrument};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use uuid::Uuid;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Topic prefix for Fusion-compatible signals.
pub const FUSION_TOPIC_PREFIX: &str = "astra:signals:flash";

/// Source strategy identifier for Flash signals.
pub const SOURCE_STRATEGY: &str = "flash";

/// Default imbalance threshold for LONG signal.
pub const IMBALANCE_LONG_THRESHOLD: f64 = 0.1;

/// Default imbalance threshold for SHORT signal.
pub const IMBALANCE_SHORT_THRESHOLD: f64 = -0.1;

// =============================================================================
// SIGNAL DIRECTION
// =============================================================================

/// Trading signal direction.
///
/// Represents the directional bias of a trading signal.
/// Serializes to uppercase strings for Fusion compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SignalDirection {
    /// Bullish bias - expect price to rise.
    Long,
    /// Bearish bias - expect price to fall.
    Short,
    /// No directional bias.
    Neutral,
}

impl SignalDirection {
    /// Returns the direction as a string.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
            Self::Neutral => "NEUTRAL",
        }
    }

    /// Create direction from order imbalance.
    ///
    /// # Arguments
    ///
    /// * `imbalance` - Order imbalance ratio (-1.0 to 1.0)
    ///
    /// # Returns
    ///
    /// - `Long` if imbalance > 0.1 (more bid pressure)
    /// - `Short` if imbalance < -0.1 (more ask pressure)
    /// - `Neutral` otherwise
    #[must_use]
    pub fn from_imbalance(imbalance: f64) -> Self {
        if imbalance > IMBALANCE_LONG_THRESHOLD {
            Self::Long
        } else if imbalance < IMBALANCE_SHORT_THRESHOLD {
            Self::Short
        } else {
            Self::Neutral
        }
    }
}

impl std::fmt::Display for SignalDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// ALPHA SIGNAL METADATA
// =============================================================================

/// Metadata envelope for AlphaSignal.
///
/// Contains source-specific information about how the signal was generated.
///
/// # Zero-Copy Optimization (Batch 4.2)
///
/// The `source_strategy` field uses `Cow<'static, str>` to avoid heap
/// allocations when using the static `SOURCE_STRATEGY` constant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaSignalMetadata {
    /// Source strategy identifier (always "flash" for Flash signals).
    /// Uses `Cow<'static, str>` for zero-copy with static strings.
    pub source_strategy: Cow<'static, str>,

    /// Signal confidence level (0.0 to 1.0).
    ///
    /// Higher values indicate more reliable signals. For Flash:
    /// - Based on spread tightness
    /// - Book depth availability
    /// - Data freshness
    pub confidence: f64,

    /// Market regime classification.
    ///
    /// Optional. Possible values:
    /// - "TRENDING_UP", "TRENDING_DOWN"
    /// - "RANGING", "VOLATILE"
    /// - "UNKNOWN"
    ///
    /// Uses `Cow<'static, str>` for zero-copy with static strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regime: Option<Cow<'static, str>>,

    /// Raw directional value (-1.0 to 1.0).
    ///
    /// The unquantized signal strength before discretization to direction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_direction: Option<f64>,

    /// Additional source-specific data.
    ///
    /// For Flash signals, may include:
    /// - `bid_depth`: Total bid quantity
    /// - `ask_depth`: Total ask quantity
    /// - `best_bid`: Best bid price
    /// - `best_ask`: Best ask price
    /// - `spread`: Bid-ask spread
    /// - `imbalance`: Order flow imbalance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl AlphaSignalMetadata {
    /// Create new metadata with default Flash values (zero-copy).
    ///
    /// Uses `Cow::Borrowed` for `source_strategy` to avoid heap allocation.
    #[must_use]
    pub fn new(confidence: f64) -> Self {
        Self {
            source_strategy: Cow::Borrowed(SOURCE_STRATEGY),
            confidence: confidence.clamp(0.0, 1.0),
            regime: None,
            raw_direction: None,
            extra: None,
        }
    }

    /// Set the market regime with a static string (zero-copy).
    #[must_use]
    pub fn with_regime(mut self, regime: &'static str) -> Self {
        self.regime = Some(Cow::Borrowed(regime));
        self
    }

    /// Set the market regime with an owned string.
    #[must_use]
    pub fn with_regime_owned(mut self, regime: String) -> Self {
        self.regime = Some(Cow::Owned(regime));
        self
    }

    /// Set the raw directional value.
    #[must_use]
    pub fn with_raw_direction(mut self, direction: f64) -> Self {
        self.raw_direction = Some(direction.clamp(-1.0, 1.0));
        self
    }

    /// Set extra metadata.
    #[must_use]
    pub fn with_extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = Some(extra);
        self
    }
}

impl Default for AlphaSignalMetadata {
    fn default() -> Self {
        Self::new(0.5)
    }
}

// =============================================================================
// ALPHA SIGNAL
// =============================================================================

/// Standardized signal format for the downstream decision-aggregation engine.
///
/// This is the canonical format the aggregator expects for combining signals
/// from multiple upstream producers.
///
/// # Zero-Copy Optimization (Batch 4.2)
///
/// The `symbol` field uses `Cow<'static, str>` to enable zero-copy when
/// using static symbol strings (e.g., common trading pairs).
///
/// # Fields
///
/// All fields are required unless marked optional:
///
/// - `signal_id`: Unique identifier (UUID v4)
/// - `timestamp`: ISO-8601 UTC timestamp
/// - `symbol`: Trading pair (e.g., "EUR_USD", "BTC_USD")
/// - `signal`: Direction (LONG, SHORT, NEUTRAL)
/// - `strength`: Conviction level (0.0 to 1.0)
/// - `metadata`: Source-specific data
///
/// # Example
///
/// ```rust
/// use astra_flash::fusion::{AlphaSignal, SignalDirection, AlphaSignalMetadata};
///
/// let signal = AlphaSignal::new("BTC_USD", SignalDirection::Long, 0.75, 0.85);
/// assert!(signal.validate().is_ok());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaSignal {
    /// Unique signal identifier (UUID v4).
    pub signal_id: String,

    /// ISO-8601 UTC timestamp when signal was generated.
    pub timestamp: String,

    /// Trading symbol in format "BASE_QUOTE" (e.g., "BTC_USD").
    /// Uses `Cow<'static, str>` for zero-copy with static strings.
    pub symbol: Cow<'static, str>,

    /// Signal direction.
    pub signal: SignalDirection,

    /// Signal strength/conviction (0.0 to 1.0).
    ///
    /// Higher values indicate stronger signals.
    pub strength: f64,

    /// Source-specific metadata.
    pub metadata: AlphaSignalMetadata,
}

impl AlphaSignal {
    /// Create a new AlphaSignal with current timestamp (zero-copy for static symbols).
    ///
    /// # Arguments
    ///
    /// * `symbol` - Static trading symbol for zero-copy
    /// * `signal` - Signal direction
    /// * `strength` - Signal strength (0.0 to 1.0)
    /// * `confidence` - Confidence level (0.0 to 1.0)
    #[must_use]
    pub fn new(
        symbol: &'static str,
        signal: SignalDirection,
        strength: f64,
        confidence: f64,
    ) -> Self {
        Self {
            signal_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            symbol: Cow::Borrowed(symbol),
            signal,
            strength: strength.clamp(0.0, 1.0),
            metadata: AlphaSignalMetadata::new(confidence),
        }
    }

    /// Create a new AlphaSignal with an owned symbol string.
    ///
    /// Use this when the symbol is dynamically constructed.
    /// For static symbols, prefer `new()` which is zero-copy.
    #[must_use]
    pub fn new_owned(
        symbol: String,
        signal: SignalDirection,
        strength: f64,
        confidence: f64,
    ) -> Self {
        Self {
            signal_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            symbol: Cow::Owned(symbol),
            signal,
            strength: strength.clamp(0.0, 1.0),
            metadata: AlphaSignalMetadata::new(confidence),
        }
    }

    /// Create an AlphaSignal from a BookSnapshot.
    ///
    /// Calculates order imbalance from bid/ask quantities and derives
    /// signal direction and strength accordingly.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The order book snapshot to analyze
    /// * `confidence` - Signal confidence (0.0 to 1.0)
    ///
    /// # Signal Derivation
    ///
    /// 1. Calculate order imbalance: (bid_qty - ask_qty) / (bid_qty + ask_qty)
    /// 2. Direction: LONG if imbalance > 0.1, SHORT if < -0.1, else NEUTRAL
    /// 3. Strength: |imbalance| clamped to [0, 1]
    #[must_use]
    pub fn from_book_snapshot(snapshot: &BookSnapshot, confidence: f64) -> Self {
        // Calculate total bid and ask quantities
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

        // Calculate imbalance ratio
        let imbalance = if total_bid_qty + total_ask_qty > 0.0 {
            (total_bid_qty - total_ask_qty) / (total_bid_qty + total_ask_qty)
        } else {
            0.0
        };

        // Derive direction and strength
        let direction = SignalDirection::from_imbalance(imbalance);
        let strength = imbalance.abs().min(1.0);

        // Get best bid/ask prices
        let best_bid = snapshot.bids.first().map(|l| l.price);
        let best_ask = snapshot.asks.first().map(|l| l.price);
        let spread = match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        };

        // Build symbol (owned since constructed dynamically)
        let symbol = format!("{}_{}", snapshot.instrument.base, snapshot.instrument.quote);

        // Build extra metadata
        let extra = serde_json::json!({
            "bid_depth": total_bid_qty,
            "ask_depth": total_ask_qty,
            "best_bid": best_bid,
            "best_ask": best_ask,
            "spread": spread,
            "imbalance": imbalance,
            "bid_levels": snapshot.bids.len(),
            "ask_levels": snapshot.asks.len(),
            "exchange": snapshot.instrument.exchange.as_str(),
        });

        Self {
            signal_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            symbol: Cow::Owned(symbol),
            signal: direction,
            strength,
            metadata: AlphaSignalMetadata::new(confidence)
                .with_raw_direction(imbalance)
                .with_extra(extra),
        }
    }

    /// Validate that this signal has all required fields.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if signal is valid
    /// - `Err(message)` describing the validation failure
    pub fn validate(&self) -> Result<(), String> {
        // Validate signal_id is a valid UUID
        if Uuid::parse_str(&self.signal_id).is_err() {
            return Err("signal_id must be a valid UUID".to_string());
        }

        // Validate timestamp is ISO-8601
        if DateTime::parse_from_rfc3339(&self.timestamp).is_err() {
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

    /// Build the Fusion-compatible topic for this signal.
    ///
    /// Format: `astra:signals:flash:{exchange}:{symbol}`
    #[must_use]
    pub fn topic(&self, exchange: &Exchange) -> String {
        format!(
            "{}:{}:{}",
            FUSION_TOPIC_PREFIX,
            exchange.as_str(),
            self.symbol
        )
    }

    /// Serialize to JSON for Redis publishing.
    ///
    /// # Returns
    ///
    /// JSON string representation suitable for Redis XADD.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize to JSON bytes for Redis publishing.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

// =============================================================================
// TOPIC BUILDER EXTENSION
// =============================================================================

/// Build a Fusion-compatible topic from instrument.
#[must_use]
pub fn build_fusion_topic(instrument: &Instrument) -> String {
    format!(
        "{}:{}:{}_{}",
        FUSION_TOPIC_PREFIX,
        instrument.exchange.as_str(),
        instrument.base,
        instrument.quote
    )
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::PriceLevel;
    use rust_decimal_macros::dec;

    fn test_instrument() -> Instrument {
        Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
    }

    fn test_snapshot() -> BookSnapshot {
        BookSnapshot {
            instrument: test_instrument(),
            timestamp: crate::core::types::now_micros(),
            bids: vec![PriceLevel {
                price: 50000.0,
                quantity: dec!(10.0),
                order_count: Some(5),
                timestamp: crate::core::types::now_micros(),
            }],
            asks: vec![PriceLevel {
                price: 50010.0,
                quantity: dec!(8.0),
                order_count: Some(4),
                timestamp: crate::core::types::now_micros(),
            }],
        }
    }

    #[test]
    fn test_alpha_signal_new() {
        let signal = AlphaSignal::new("BTC_USD", SignalDirection::Long, 0.75, 0.85);

        assert!(signal.validate().is_ok());
        assert_eq!(&*signal.symbol, "BTC_USD");
        assert_eq!(signal.signal, SignalDirection::Long);
        assert!((signal.strength - 0.75).abs() < f64::EPSILON);
        assert_eq!(&*signal.metadata.source_strategy, "flash");
    }

    #[test]
    fn test_alpha_signal_from_snapshot() {
        let snapshot = test_snapshot();
        let signal = AlphaSignal::from_book_snapshot(&snapshot, 0.8);

        assert!(signal.validate().is_ok());
        assert_eq!(&*signal.symbol, "BTC_USD");
        assert_eq!(&*signal.metadata.source_strategy, "flash");

        // Check extra metadata
        let extra = signal.metadata.extra.as_ref().unwrap();
        assert!(extra.get("bid_depth").is_some());
        assert!(extra.get("ask_depth").is_some());
    }

    #[test]
    fn test_signal_direction_from_imbalance() {
        assert_eq!(SignalDirection::from_imbalance(0.5), SignalDirection::Long);
        assert_eq!(
            SignalDirection::from_imbalance(-0.5),
            SignalDirection::Short
        );
        assert_eq!(
            SignalDirection::from_imbalance(0.05),
            SignalDirection::Neutral
        );
    }

    #[test]
    fn test_fusion_topic() {
        let signal = AlphaSignal::new("BTC_USD", SignalDirection::Long, 0.5, 0.5);
        let topic = signal.topic(&Exchange::Deribit);

        assert_eq!(topic, "astra:signals:flash:deribit:BTC_USD");
    }

    #[test]
    fn test_build_fusion_topic() {
        let instrument = test_instrument();
        let topic = build_fusion_topic(&instrument);

        assert_eq!(topic, "astra:signals:flash:deribit:BTC_USD");
    }

    #[test]
    fn test_alpha_signal_serialization() {
        let signal = AlphaSignal::new("EUR_USD", SignalDirection::Short, 0.6, 0.9);
        let json = signal.to_json().expect("Should serialize");

        assert!(json.contains("signal_id"));
        assert!(json.contains("EUR_USD"));
        assert!(json.contains("SHORT"));
        assert!(json.contains("flash"));
    }

    #[test]
    fn test_metadata_builder() {
        let metadata = AlphaSignalMetadata::new(0.9)
            .with_regime("TRENDING_UP")
            .with_raw_direction(0.7)
            .with_extra(serde_json::json!({"key": "value"}));

        assert_eq!(metadata.regime.as_deref(), Some("TRENDING_UP"));
        assert_eq!(metadata.raw_direction, Some(0.7));
        assert!(metadata.extra.is_some());
    }
}
