//! OANDA exchange adapter.
//!
//! This module implements the [`ExchangeAdapter`] trait for OANDA,
//! a forex broker providing streaming prices.
//!
//! # WebSocket Protocol
//!
//! OANDA uses a streaming REST API that returns newline-delimited JSON.
//! Subscriptions are done via URL parameters.
//!
//! # Message Types
//!
//! - `PRICE` - Price update with bid/ask levels
//! - `HEARTBEAT` - Keep-alive message
//!
//! # Example
//!
//! ```ignore
//! use astra_flash::network::adapters::OandaAdapter;
//!
//! let adapter = OandaAdapter::new("account_id");
//! let events = adapter.parse_message(raw_json)?;
//! ```

use super::common::{contains_field, get_str, parse_iso_timestamp, parse_price};
use super::traits::{ExchangeAdapter, RateLimit};
use crate::core::error::{FlashError, FlashResult};
use crate::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel,
};
use rust_decimal::Decimal;
use serde::Deserialize;

// =============================================================================
// OANDA MESSAGE TYPES (Input Deserialization)
// =============================================================================

/// OANDA incoming price message format.
///
/// Represents a price update from OANDA's streaming API.
/// Prices are provided as string values for maximum precision.
///
/// # JSON Format
///
/// ```json
/// {
///   "type": "PRICE",
///   "time": "2023-12-29T00:00:00.000000Z",
///   "instrument": "EUR_USD",
///   "bids": [{"price": "1.08505", "liquidity": 1000000}],
///   "asks": [{"price": "1.08520", "liquidity": 1000000}]
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::network::adapters::oanda::OandaPrice;
///
/// let json = r#"{"type":"PRICE","time":"2023-12-29T00:00:00Z","instrument":"EUR_USD","bids":[],"asks":[]}"#;
/// let price: OandaPrice = serde_json::from_str(json).unwrap();
/// assert_eq!(price.instrument, "EUR_USD");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OandaPrice {
    /// Message type (always "PRICE" for price messages).
    #[serde(rename = "type")]
    pub msg_type: String,

    /// ISO-8601 timestamp from OANDA.
    pub time: String,

    /// Trading instrument (e.g., "EUR_USD").
    pub instrument: String,

    /// Bid price levels with liquidity.
    #[serde(default)]
    pub bids: Vec<OandaLevel>,

    /// Ask price levels with liquidity.
    #[serde(default)]
    pub asks: Vec<OandaLevel>,
}

impl OandaPrice {
    /// Check if this is a valid PRICE message.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.msg_type == "PRICE" && !self.instrument.is_empty()
    }

    /// Get the best bid price as f64.
    #[must_use]
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().and_then(|l| l.price.parse::<f64>().ok())
    }

    /// Get the best ask price as f64.
    #[must_use]
    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().and_then(|l| l.price.parse::<f64>().ok())
    }

    /// Get the mid price.
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }

    /// Get the spread.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Calculate total bid liquidity.
    #[must_use]
    pub fn total_bid_liquidity(&self) -> i64 {
        self.bids.iter().map(|l| l.liquidity).sum()
    }

    /// Calculate total ask liquidity.
    #[must_use]
    pub fn total_ask_liquidity(&self) -> i64 {
        self.asks.iter().map(|l| l.liquidity).sum()
    }
}

/// OANDA price level with liquidity.
///
/// Represents a single price level in OANDA's format.
/// Prices are strings for precision, liquidity is integer (units available).
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::network::adapters::oanda::OandaLevel;
///
/// let json = r#"{"price":"1.08505","liquidity":1000000}"#;
/// let level: OandaLevel = serde_json::from_str(json).unwrap();
/// assert_eq!(level.liquidity, 1000000);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OandaLevel {
    /// Price as a string (for precision).
    pub price: String,

    /// Liquidity available at this price (units).
    pub liquidity: i64,
}

impl OandaLevel {
    /// Create a new OANDA level.
    #[must_use]
    pub fn new(price: impl Into<String>, liquidity: i64) -> Self {
        Self {
            price: price.into(),
            liquidity,
        }
    }

    /// Parse price as f64.
    #[must_use]
    pub fn price_f64(&self) -> Option<f64> {
        self.price.parse::<f64>().ok()
    }
}

/// OANDA heartbeat message format.
///
/// Sent periodically to keep the connection alive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OandaHeartbeat {
    /// Message type (always "HEARTBEAT").
    #[serde(rename = "type")]
    pub msg_type: String,

    /// ISO-8601 timestamp.
    pub time: String,
}

// =============================================================================
// OANDA ADAPTER
// =============================================================================

/// Base WebSocket URL for OANDA (practice/demo).
const OANDA_WS_URL_PRACTICE: &str = "wss://stream-fxpractice.oanda.com/v3/accounts";

/// Base WebSocket URL for OANDA (live).
const OANDA_WS_URL_LIVE: &str = "wss://stream-fxtrade.oanda.com/v3/accounts";

/// OANDA exchange adapter.
///
/// Implements [`ExchangeAdapter`] for parsing OANDA streaming prices
/// and building subscription URLs.
///
/// # Authentication
///
/// OANDA requires an access token in the Authorization header.
/// The adapter stores the token for building auth messages.
///
/// # Thread Safety
///
/// The adapter is thread-safe and can be cloned.
#[derive(Debug, Clone)]
pub struct OandaAdapter {
    /// Account ID for streaming (used for URL construction in future).
    #[allow(dead_code)]
    account_id: String,
    /// Access token for authentication.
    access_token: Option<String>,
    /// Use live environment (vs practice).
    use_live: bool,
}

impl OandaAdapter {
    /// Create a new adapter with account ID.
    ///
    /// # Arguments
    ///
    /// * `account_id` - OANDA account ID
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::network::adapters::OandaAdapter;
    ///
    /// let adapter = OandaAdapter::new("<YOUR_OANDA_ACCOUNT_ID>");
    /// ```
    pub fn new(account_id: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            access_token: None,
            use_live: false,
        }
    }

    /// Create a new adapter with account ID and access token.
    ///
    /// # Arguments
    ///
    /// * `account_id` - OANDA account ID
    /// * `access_token` - OANDA API access token
    pub fn with_token(account_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            access_token: Some(access_token.into()),
            use_live: false,
        }
    }

    /// Set whether to use live environment.
    pub const fn with_live(mut self, use_live: bool) -> Self {
        self.use_live = use_live;
        self
    }

    /// Parse a PRICE message.
    fn parse_price_message(&self, json: &serde_json::Value) -> FlashResult<MarketEvent> {
        let instrument_str = get_str(json, "instrument");
        let instrument = self.parse_instrument(instrument_str);

        let timestamp = json
            .get("time")
            .and_then(|v| v.as_str())
            .map_or_else(crate::core::types::now_micros, |s| {
                parse_iso_timestamp(s).unwrap_or_else(|_| crate::core::types::now_micros())
            });

        // Parse bids
        let bids = self.parse_liquidity_levels(
            json.get("bids")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;

        // Parse asks
        let asks = self.parse_liquidity_levels(
            json.get("asks")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;

        Ok(MarketEvent {
            event_type: MarketEventType::Snapshot,
            instrument,
            timestamp,
            local_timestamp: crate::core::types::now_micros(),
            sequence: None,
            data: MarketData::Book { bids, asks },
        })
    }

    /// Parse OANDA's liquidity levels format.
    fn parse_liquidity_levels(
        &self,
        arr: &serde_json::Value,
        timestamp: crate::core::types::Timestamp,
    ) -> FlashResult<Vec<PriceLevel>> {
        let arr = match arr.as_array() {
            Some(a) => a,
            None => return Ok(vec![]),
        };

        let mut levels = Vec::with_capacity(arr.len());

        for item in arr {
            let price = item
                .get("price")
                .map_or(0.0, |v| parse_price(v).unwrap_or(0.0));

            // OANDA provides liquidity instead of quantity
            // We convert to a quantity representation
            let liquidity = item
                .get("liquidity")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);

            // Convert liquidity to decimal (represents units available)
            let quantity = Decimal::from(liquidity);

            levels.push(PriceLevel::new(price, quantity, timestamp));
        }

        Ok(levels)
    }

    /// Parse a HEARTBEAT message (for future heartbeat handling).
    #[allow(dead_code)]
    fn parse_heartbeat_message(&self, json: &serde_json::Value) -> FlashResult<MarketEvent> {
        let timestamp = json
            .get("time")
            .and_then(|v| v.as_str())
            .map_or_else(crate::core::types::now_micros, |s| {
                parse_iso_timestamp(s).unwrap_or_else(|_| crate::core::types::now_micros())
            });

        Ok(MarketEvent {
            event_type: MarketEventType::Heartbeat,
            instrument: Instrument::new("", "", Exchange::Oanda, ""),
            timestamp,
            local_timestamp: crate::core::types::now_micros(),
            sequence: None,
            data: MarketData::Heartbeat {
                exchange_time: timestamp,
            },
        })
    }

    /// Parse instrument from OANDA format.
    fn parse_instrument(&self, symbol: &str) -> Instrument {
        // EUR_USD -> EUR/USD
        let parts: Vec<&str> = symbol.split('_').collect();
        if parts.len() == 2 {
            Instrument::new(parts[0], parts[1], Exchange::Oanda, symbol)
        } else {
            Instrument::new(symbol, "", Exchange::Oanda, symbol)
        }
    }

    /// Build the streaming URL with instruments (for future streaming support).
    #[allow(dead_code)]
    fn build_streaming_url(&self, instruments: &[Instrument]) -> String {
        let base = if self.use_live {
            OANDA_WS_URL_LIVE
        } else {
            OANDA_WS_URL_PRACTICE
        };

        let symbols: Vec<&str> = instruments.iter().map(|i| i.raw_symbol.as_str()).collect();

        format!(
            "{}/{}/pricing/stream?instruments={}",
            base,
            self.account_id,
            symbols.join(",")
        )
    }
}

impl ExchangeAdapter for OandaAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::Oanda
    }

    fn parse_message(&self, raw: &str) -> FlashResult<Vec<MarketEvent>> {
        // Check for error first
        if contains_field(raw, "\"errorMessage\"") {
            if let Some(err) = self.parse_error(raw) {
                return Err(err);
            }
        }

        // Parse JSON
        let json: serde_json::Value = serde_json::from_str(raw).map_err(FlashError::ParseError)?;

        let msg_type = get_str(&json, "type");

        match msg_type {
            "PRICE" => {
                let event = self.parse_price_message(&json)?;
                Ok(vec![event])
            }
            "HEARTBEAT" => {
                // Return empty for heartbeats (handled separately)
                Ok(vec![])
            }
            _ => {
                // Unknown message type
                Ok(vec![])
            }
        }
    }

    fn build_subscribe(&self, _instruments: &[Instrument]) -> Vec<String> {
        // OANDA uses URL parameters for subscription
        // Return empty as subscription is handled via URL
        vec![]
    }

    fn build_unsubscribe(&self, _instruments: &[Instrument]) -> Vec<String> {
        // OANDA doesn't support unsubscribe - close connection instead
        vec![]
    }

    fn handle_ping(&self, _payload: &[u8]) -> Option<Vec<u8>> {
        // OANDA handles ping/pong at the protocol level
        None
    }

    fn parse_error(&self, raw: &str) -> Option<FlashError> {
        let json: serde_json::Value = serde_json::from_str(raw).ok()?;

        let message = json
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        Some(FlashError::ExchangeError {
            exchange: "oanda".to_string(),
            code: 0,
            message,
        })
    }

    fn is_heartbeat(&self, raw: &str) -> bool {
        contains_field(raw, "\"type\":\"HEARTBEAT\"")
    }

    fn is_subscription_response(&self, _raw: &str) -> bool {
        // OANDA doesn't send subscription responses
        false
    }

    fn build_auth(&self, _api_key: &str, _api_secret: &str, _timestamp: i64) -> Option<String> {
        // OANDA uses Authorization header, not a message
        // Return a placeholder that includes the token
        self.access_token
            .as_ref()
            .map(|token| format!("Authorization: Bearer {token}"))
    }

    fn websocket_url(&self) -> &str {
        if self.use_live {
            OANDA_WS_URL_LIVE
        } else {
            OANDA_WS_URL_PRACTICE
        }
    }

    fn rate_limit(&self) -> RateLimit {
        RateLimit::OANDA
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let adapter = OandaAdapter::new("test_account");
        assert_eq!(adapter.exchange(), Exchange::Oanda);
        assert_eq!(adapter.account_id, "test_account");
        assert!(adapter.access_token.is_none());
    }

    #[test]
    fn test_with_token() {
        let adapter = OandaAdapter::with_token("test_account", "test_token");
        assert!(adapter.access_token.is_some());
        assert_eq!(adapter.access_token.as_ref().unwrap(), "test_token");
    }

    #[test]
    fn test_with_live() {
        let adapter = OandaAdapter::new("test").with_live(true);
        assert!(adapter.use_live);
        assert!(adapter.websocket_url().contains("fxtrade"));
    }

    #[test]
    fn test_parse_instrument() {
        let adapter = OandaAdapter::new("test");

        let inst = adapter.parse_instrument("EUR_USD");
        assert_eq!(inst.base, "EUR");
        assert_eq!(inst.quote, "USD");
        assert_eq!(inst.raw_symbol, "EUR_USD");
    }

    #[test]
    fn test_build_streaming_url() {
        let adapter = OandaAdapter::new("test_account");
        let instruments = vec![
            Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD"),
            Instrument::new("GBP", "USD", Exchange::Oanda, "GBP_USD"),
        ];

        let url = adapter.build_streaming_url(&instruments);
        assert!(url.contains("test_account"));
        assert!(url.contains("EUR_USD"));
        assert!(url.contains("GBP_USD"));
    }

    #[test]
    fn test_is_heartbeat() {
        let adapter = OandaAdapter::new("test");
        assert!(adapter.is_heartbeat(r#"{"type":"HEARTBEAT","time":"2023-12-29T00:00:00Z"}"#));
        assert!(!adapter.is_heartbeat(r#"{"type":"PRICE"}"#));
    }

    #[test]
    fn test_build_subscribe_empty() {
        let adapter = OandaAdapter::new("test");
        let instruments = vec![Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD")];
        let messages = adapter.build_subscribe(&instruments);
        // OANDA uses URL for subscription
        assert!(messages.is_empty());
    }

    #[test]
    fn test_build_auth() {
        let adapter = OandaAdapter::with_token("test", "my_token");
        let auth = adapter.build_auth("", "", 0);
        assert!(auth.is_some());
        assert!(auth.unwrap().contains("Bearer my_token"));
    }

    // =========================================================================
    // OandaLevel Tests (5 tests)
    // =========================================================================

    #[test]
    fn test_oanda_level_new() {
        let level = OandaLevel::new("1.08505", 1000000);
        assert_eq!(level.price, "1.08505");
        assert_eq!(level.liquidity, 1000000);
    }

    #[test]
    fn test_oanda_level_price_f64() {
        let level = OandaLevel::new("1.08505", 1000000);
        let price = level.price_f64().unwrap();
        assert!((price - 1.08505).abs() < f64::EPSILON);
    }

    #[test]
    fn test_oanda_level_price_f64_invalid() {
        let level = OandaLevel::new("invalid", 1000000);
        assert!(level.price_f64().is_none());
    }

    #[test]
    fn test_oanda_level_deserialize() {
        let json = r#"{"price":"1.08505","liquidity":1000000}"#;
        let level: OandaLevel = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(level.price, "1.08505");
        assert_eq!(level.liquidity, 1000000);
    }

    #[test]
    fn test_oanda_level_deserialize_negative_liquidity() {
        // Edge case: negative liquidity (shouldn't happen but test parsing)
        let json = r#"{"price":"1.08505","liquidity":-100}"#;
        let level: OandaLevel = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(level.liquidity, -100);
    }

    // =========================================================================
    // OandaPrice Tests (10 tests)
    // =========================================================================

    #[test]
    fn test_oanda_price_deserialize_full() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00.000000Z",
            "instrument": "EUR_USD",
            "bids": [{"price": "1.08505", "liquidity": 1000000}],
            "asks": [{"price": "1.08520", "liquidity": 500000}]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");

        assert_eq!(price.msg_type, "PRICE");
        assert_eq!(price.instrument, "EUR_USD");
        assert_eq!(price.bids.len(), 1);
        assert_eq!(price.asks.len(), 1);
        assert_eq!(price.bids[0].liquidity, 1000000);
    }

    #[test]
    fn test_oanda_price_deserialize_empty_levels() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD"
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");

        assert!(price.bids.is_empty());
        assert!(price.asks.is_empty());
    }

    #[test]
    fn test_oanda_price_is_valid() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [],
            "asks": []
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        assert!(price.is_valid());
    }

    #[test]
    fn test_oanda_price_is_valid_wrong_type() {
        let json = r#"{
            "type": "HEARTBEAT",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [],
            "asks": []
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        assert!(!price.is_valid());
    }

    #[test]
    fn test_oanda_price_best_bid() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [{"price": "1.08505", "liquidity": 1000000}],
            "asks": []
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        let bid = price.best_bid().unwrap();
        assert!((bid - 1.08505).abs() < f64::EPSILON);
    }

    #[test]
    fn test_oanda_price_best_ask() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [],
            "asks": [{"price": "1.08520", "liquidity": 500000}]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        let ask = price.best_ask().unwrap();
        assert!((ask - 1.08520).abs() < f64::EPSILON);
    }

    #[test]
    fn test_oanda_price_mid_price() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [{"price": "1.08500", "liquidity": 1000000}],
            "asks": [{"price": "1.08520", "liquidity": 500000}]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        let mid = price.mid_price().unwrap();
        assert!((mid - 1.08510).abs() < f64::EPSILON);
    }

    #[test]
    fn test_oanda_price_spread() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [{"price": "1.08500", "liquidity": 1000000}],
            "asks": [{"price": "1.08520", "liquidity": 500000}]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        let spread = price.spread().unwrap();
        assert!((spread - 0.00020).abs() < f64::EPSILON);
    }

    #[test]
    fn test_oanda_price_total_liquidity() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "EUR_USD",
            "bids": [
                {"price": "1.08505", "liquidity": 1000000},
                {"price": "1.08500", "liquidity": 500000}
            ],
            "asks": [
                {"price": "1.08520", "liquidity": 750000},
                {"price": "1.08525", "liquidity": 250000}
            ]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(price.total_bid_liquidity(), 1500000);
        assert_eq!(price.total_ask_liquidity(), 1000000);
    }

    #[test]
    fn test_oanda_price_multiple_levels() {
        let json = r#"{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00Z",
            "instrument": "GBP_USD",
            "bids": [
                {"price": "1.27000", "liquidity": 2000000},
                {"price": "1.26990", "liquidity": 1500000},
                {"price": "1.26980", "liquidity": 1000000}
            ],
            "asks": [
                {"price": "1.27010", "liquidity": 1800000},
                {"price": "1.27020", "liquidity": 1200000}
            ]
        }"#;

        let price: OandaPrice = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(price.bids.len(), 3);
        assert_eq!(price.asks.len(), 2);
        assert_eq!(price.instrument, "GBP_USD");
    }

    // =========================================================================
    // OandaHeartbeat Tests (2 tests)
    // =========================================================================

    #[test]
    fn test_oanda_heartbeat_deserialize() {
        let json = r#"{
            "type": "HEARTBEAT",
            "time": "2023-12-29T00:00:00.000000Z"
        }"#;

        let heartbeat: OandaHeartbeat = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(heartbeat.msg_type, "HEARTBEAT");
        assert!(heartbeat.time.contains("2023-12-29"));
    }

    #[test]
    fn test_oanda_heartbeat_time_format() {
        let json = r#"{
            "type": "HEARTBEAT",
            "time": "2023-12-29T12:30:45.123456Z"
        }"#;

        let heartbeat: OandaHeartbeat = serde_json::from_str(json).expect("Should deserialize");
        assert!(heartbeat.time.contains("12:30:45"));
    }
}
