//! Binance exchange adapter.
//!
//! This module implements the [`ExchangeAdapter`] trait for Binance,
//! a cryptocurrency spot and futures exchange.
//!
//! # WebSocket Protocol
//!
//! Binance uses JSON messages over WebSocket. Subscription is done via
//! SUBSCRIBE/UNSUBSCRIBE methods.
//!
//! # Streams
//!
//! - `<symbol>@depth` - Full depth updates (1000ms)
//! - `<symbol>@depth@100ms` - 100ms depth updates
//! - `<symbol>@trade` - Trade updates
//!
//! # Example
//!
//! ```ignore
//! use astra_flash::network::adapters::BinanceAdapter;
//!
//! let adapter = BinanceAdapter::default();
//! let events = adapter.parse_message(raw_json)?;
//! ```

use super::common::{
    contains_field, get_str, parse_levels_array, parse_price, parse_quantity, parse_timestamp,
};
use super::traits::{ExchangeAdapter, RateLimit};
use crate::core::error::{FlashError, FlashResult};
use crate::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType, Side};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Depth update interval for Binance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateInterval {
    /// 1000ms updates (default).
    #[default]
    Ms1000,
    /// 100ms updates.
    Ms100,
}

impl UpdateInterval {
    /// Convert to Binance stream suffix.
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Ms1000 => "",
            Self::Ms100 => "@100ms",
        }
    }
}

// =============================================================================
// BINANCE ADAPTER
// =============================================================================

/// WebSocket URL for Binance.
const BINANCE_WS_URL: &str = "wss://stream.binance.com:9443/ws";

/// Combined streams URL for Binance (for future multi-stream support).
#[allow(dead_code)]
const BINANCE_COMBINED_URL: &str = "wss://stream.binance.com:9443/stream";

/// Binance exchange adapter.
///
/// Implements [`ExchangeAdapter`] for parsing Binance WebSocket messages
/// and building subscription messages.
///
/// # Thread Safety
///
/// The adapter uses atomic counters for request IDs, making it safe to
/// use from multiple tasks.
#[derive(Debug)]
pub struct BinanceAdapter {
    /// Next request ID for subscriptions.
    next_id: Arc<AtomicU64>,
    /// Update interval for depth.
    interval: UpdateInterval,
}

impl Default for BinanceAdapter {
    fn default() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            interval: UpdateInterval::default(),
        }
    }
}

impl Clone for BinanceAdapter {
    fn clone(&self) -> Self {
        Self {
            next_id: Arc::clone(&self.next_id),
            interval: self.interval,
        }
    }
}

impl BinanceAdapter {
    /// Create a new adapter with the specified update interval.
    pub fn with_interval(interval: UpdateInterval) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            interval,
        }
    }

    /// Get the current update interval.
    pub const fn update_interval(&self) -> UpdateInterval {
        self.interval
    }

    /// Get the next request ID.
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Parse a depth update message.
    fn parse_depth_update(&self, json: &serde_json::Value) -> FlashResult<MarketEvent> {
        let symbol = get_str(json, "s");
        let instrument = self.parse_instrument(symbol);

        let timestamp = parse_timestamp(
            json.get("E").unwrap_or(&serde_json::Value::Null),
            true, // Binance uses milliseconds
        )
        .unwrap_or_else(|_| crate::core::types::now_micros());

        let bids = parse_levels_array(
            json.get("b").unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;
        let asks = parse_levels_array(
            json.get("a").unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;

        // Get sequence numbers for gap detection
        let sequence = json.get("u").and_then(serde_json::Value::as_u64);

        Ok(MarketEvent {
            event_type: MarketEventType::Delta,
            instrument,
            timestamp,
            local_timestamp: crate::core::types::now_micros(),
            sequence,
            data: MarketData::Book { bids, asks },
        })
    }

    /// Parse a partial depth snapshot message.
    fn parse_partial_depth(&self, json: &serde_json::Value) -> FlashResult<MarketEvent> {
        // Partial depth doesn't have symbol in the message
        // It comes from the stream name
        let instrument = Instrument::new("UNKNOWN", "USDT", Exchange::Binance, "UNKNOWN");

        let timestamp = crate::core::types::now_micros();

        let bids = parse_levels_array(
            json.get("bids")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;
        let asks = parse_levels_array(
            json.get("asks")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;

        let sequence = json.get("lastUpdateId").and_then(serde_json::Value::as_u64);

        Ok(MarketEvent {
            event_type: MarketEventType::Snapshot,
            instrument,
            timestamp,
            local_timestamp: timestamp,
            sequence,
            data: MarketData::Book { bids, asks },
        })
    }

    /// Parse a trade message.
    fn parse_trade_message(&self, json: &serde_json::Value) -> FlashResult<MarketEvent> {
        let symbol = get_str(json, "s");
        let instrument = self.parse_instrument(symbol);

        let timestamp = parse_timestamp(json.get("E").unwrap_or(&serde_json::Value::Null), true)
            .unwrap_or_else(|_| crate::core::types::now_micros());

        let price = json.get("p").map_or(0.0, |v| parse_price(v).unwrap_or(0.0));

        let quantity = json
            .get("q")
            .map(|v| parse_quantity(v).unwrap_or_default())
            .unwrap_or_default();

        // m = true means buyer is the market maker, so taker is seller (Ask)
        let is_buyer_maker = json
            .get("m")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let side = if is_buyer_maker { Side::Ask } else { Side::Bid };

        let trade_id = json
            .get("t")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n.to_string());

        Ok(MarketEvent {
            event_type: MarketEventType::Trade,
            instrument,
            timestamp,
            local_timestamp: crate::core::types::now_micros(),
            sequence: None,
            data: MarketData::Trade {
                price,
                quantity,
                side,
                trade_id,
            },
        })
    }

    /// Parse instrument from Binance symbol.
    fn parse_instrument(&self, symbol: &str) -> Instrument {
        // BTCUSDT -> BTC/USDT
        // Try common quote currencies
        let quote_currencies = ["USDT", "BUSD", "BTC", "ETH", "BNB", "USD"];

        for quote in quote_currencies {
            if let Some(base) = symbol.strip_suffix(quote) {
                return Instrument::new(base, quote, Exchange::Binance, symbol);
            }
        }

        // Fallback: assume last 4 chars are quote
        if symbol.len() > 4 {
            let (base, quote) = symbol.split_at(symbol.len() - 4);
            Instrument::new(base, quote, Exchange::Binance, symbol)
        } else {
            Instrument::new(symbol, "USDT", Exchange::Binance, symbol)
        }
    }

    /// Build stream name for an instrument.
    fn build_stream(&self, instrument: &Instrument) -> String {
        let symbol = instrument.raw_symbol.to_lowercase();
        format!("{}@depth{}", symbol, self.interval.as_str())
    }
}

impl ExchangeAdapter for BinanceAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn parse_message(&self, raw: &str) -> FlashResult<Vec<MarketEvent>> {
        // Check for error first
        if contains_field(raw, "\"error\"") {
            if let Some(err) = self.parse_error(raw) {
                return Err(err);
            }
        }

        // Parse JSON
        let json: serde_json::Value = serde_json::from_str(raw).map_err(FlashError::ParseError)?;

        // Check for combined stream format
        if let Some(data) = json.get("data") {
            // Combined stream: {"stream": "...", "data": {...}}
            return self.parse_message(&data.to_string());
        }

        // Check event type
        let event_type = get_str(&json, "e");

        match event_type {
            "depthUpdate" => {
                let event = self.parse_depth_update(&json)?;
                Ok(vec![event])
            }
            "trade" => {
                let event = self.parse_trade_message(&json)?;
                Ok(vec![event])
            }
            "" => {
                // Might be partial depth snapshot
                if json.get("bids").is_some() && json.get("asks").is_some() {
                    let event = self.parse_partial_depth(&json)?;
                    Ok(vec![event])
                } else {
                    // Unknown format
                    Ok(vec![])
                }
            }
            _ => {
                // Unknown event type
                Ok(vec![])
            }
        }
    }

    fn build_subscribe(&self, instruments: &[Instrument]) -> Vec<String> {
        if instruments.is_empty() {
            return vec![];
        }

        let streams: Vec<String> = instruments.iter().map(|i| self.build_stream(i)).collect();

        let msg = serde_json::json!({
            "method": "SUBSCRIBE",
            "params": streams,
            "id": self.next_request_id()
        });

        vec![msg.to_string()]
    }

    fn build_unsubscribe(&self, instruments: &[Instrument]) -> Vec<String> {
        if instruments.is_empty() {
            return vec![];
        }

        let streams: Vec<String> = instruments.iter().map(|i| self.build_stream(i)).collect();

        let msg = serde_json::json!({
            "method": "UNSUBSCRIBE",
            "params": streams,
            "id": self.next_request_id()
        });

        vec![msg.to_string()]
    }

    fn handle_ping(&self, _payload: &[u8]) -> Option<Vec<u8>> {
        // Binance uses WebSocket protocol ping/pong, not application level
        None
    }

    fn parse_error(&self, raw: &str) -> Option<FlashError> {
        let json: serde_json::Value = serde_json::from_str(raw).ok()?;
        let error = json.get("error")?;

        let code = error.get("code")?.as_i64()?;
        let message = error
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        match code {
            -1003 => Some(FlashError::RateLimited {
                exchange: "binance".to_string(),
                retry_after_ms: 1000,
            }),
            -1121 => Some(FlashError::ExchangeError {
                exchange: "binance".to_string(),
                code: code as i32,
                message,
            }),
            _ => Some(FlashError::ExchangeError {
                exchange: "binance".to_string(),
                code: code as i32,
                message,
            }),
        }
    }

    fn is_heartbeat(&self, _raw: &str) -> bool {
        // Binance doesn't send application-level heartbeats
        false
    }

    fn is_subscription_response(&self, raw: &str) -> bool {
        contains_field(raw, "\"result\"") && contains_field(raw, "\"id\"")
    }

    fn build_auth(&self, _api_key: &str, _api_secret: &str, _timestamp: i64) -> Option<String> {
        // Binance public streams don't require authentication
        None
    }

    fn websocket_url(&self) -> &str {
        BINANCE_WS_URL
    }

    fn rate_limit(&self) -> RateLimit {
        RateLimit::BINANCE
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let adapter = BinanceAdapter::default();
        assert_eq!(adapter.exchange(), Exchange::Binance);
        assert_eq!(adapter.interval, UpdateInterval::Ms1000);
    }

    #[test]
    fn test_with_interval() {
        let adapter = BinanceAdapter::with_interval(UpdateInterval::Ms100);
        assert_eq!(adapter.interval, UpdateInterval::Ms100);
    }

    #[test]
    fn test_build_stream() {
        let adapter = BinanceAdapter::default();
        let instrument = Instrument::new("BTC", "USDT", Exchange::Binance, "BTCUSDT");
        let stream = adapter.build_stream(&instrument);
        assert_eq!(stream, "btcusdt@depth");
    }

    #[test]
    fn test_build_stream_100ms() {
        let adapter = BinanceAdapter::with_interval(UpdateInterval::Ms100);
        let instrument = Instrument::new("BTC", "USDT", Exchange::Binance, "BTCUSDT");
        let stream = adapter.build_stream(&instrument);
        assert_eq!(stream, "btcusdt@depth@100ms");
    }

    #[test]
    fn test_parse_instrument() {
        let adapter = BinanceAdapter::default();

        let inst = adapter.parse_instrument("BTCUSDT");
        assert_eq!(inst.base, "BTC");
        assert_eq!(inst.quote, "USDT");

        let inst = adapter.parse_instrument("ETHBTC");
        assert_eq!(inst.base, "ETH");
        assert_eq!(inst.quote, "BTC");
    }

    #[test]
    fn test_request_id_increment() {
        let adapter = BinanceAdapter::default();
        let id1 = adapter.next_request_id();
        let id2 = adapter.next_request_id();
        assert_eq!(id2, id1 + 1);
    }

    #[test]
    fn test_is_subscription_response() {
        let adapter = BinanceAdapter::default();
        assert!(adapter.is_subscription_response(r#"{"result":null,"id":1}"#));
        assert!(!adapter.is_subscription_response(r#"{"e":"depthUpdate"}"#));
    }

    #[test]
    fn test_handle_ping_returns_none() {
        let adapter = BinanceAdapter::default();
        assert!(adapter.handle_ping(b"ping").is_none());
    }
}
