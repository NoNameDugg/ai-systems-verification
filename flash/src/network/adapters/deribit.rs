//! Deribit exchange adapter.
//!
//! This module implements the [`ExchangeAdapter`] trait for Deribit,
//! a cryptocurrency derivatives exchange.
//!
//! # WebSocket Protocol
//!
//! Deribit uses JSON-RPC 2.0 over WebSocket for all communication.
//!
//! # Channels
//!
//! - `book.{instrument}.{interval}` - Order book updates
//! - `trades.{instrument}.{interval}` - Trade data
//! - `ticker.{instrument}.{interval}` - Ticker data
//!
//! # Example
//!
//! ```ignore
//! use astra_flash::network::adapters::DeribitAdapter;
//!
//! let adapter = DeribitAdapter::default();
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

/// Order book update interval for Deribit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookInterval {
    /// Raw updates (every change).
    Raw,
    /// 100ms grouped updates.
    #[default]
    Ms100,
    /// Snapshot only (no updates).
    None,
}

impl BookInterval {
    /// Convert to Deribit channel suffix.
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Ms100 => "100ms",
            Self::None => "none",
        }
    }
}

// =============================================================================
// DERIBIT ADAPTER
// =============================================================================

/// WebSocket URL for Deribit.
const DERIBIT_WS_URL: &str = "wss://www.deribit.com/ws/api/v2";

/// Deribit exchange adapter.
///
/// Implements [`ExchangeAdapter`] for parsing Deribit WebSocket messages
/// and building subscription messages.
///
/// # Thread Safety
///
/// The adapter uses atomic counters for request IDs, making it safe to
/// use from multiple tasks.
///
/// # Example
///
/// ```
/// use astra_flash::network::adapters::DeribitAdapter;
///
/// let adapter = DeribitAdapter::default();
/// ```
#[derive(Debug)]
pub struct DeribitAdapter {
    /// Next request ID for JSON-RPC messages.
    next_id: Arc<AtomicU64>,
    /// Order book update interval.
    interval: BookInterval,
}

impl Default for DeribitAdapter {
    fn default() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            interval: BookInterval::default(),
        }
    }
}

impl Clone for DeribitAdapter {
    fn clone(&self) -> Self {
        Self {
            next_id: Arc::clone(&self.next_id),
            interval: self.interval,
        }
    }
}

impl DeribitAdapter {
    /// Create a new adapter with the specified book interval.
    pub fn with_interval(interval: BookInterval) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            interval,
        }
    }

    /// Get the current book interval.
    pub const fn book_interval(&self) -> BookInterval {
        self.interval
    }

    /// Get the next request ID.
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Parse a book snapshot or delta message.
    fn parse_book_message(
        &self,
        data: &serde_json::Value,
        channel: &str,
    ) -> FlashResult<MarketEvent> {
        // Extract instrument name from channel or data
        let instrument_name = data
            .get("instrument_name")
            .and_then(|v| v.as_str())
            .or_else(|| {
                // Extract from channel: "book.BTC-PERPETUAL.100ms"
                channel.split('.').nth(1)
            })
            .unwrap_or("UNKNOWN");

        // Parse timestamp
        let timestamp = parse_timestamp(
            data.get("timestamp").unwrap_or(&serde_json::Value::Null),
            true, // Deribit uses milliseconds
        )
        .unwrap_or_else(|_| crate::core::types::now_micros());

        // Parse bids and asks
        let bids = parse_levels_array(
            data.get("bids")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;
        let asks = parse_levels_array(
            data.get("asks")
                .unwrap_or(&serde_json::Value::Array(vec![])),
            timestamp,
        )?;

        // Determine event type
        let event_type = match get_str(data, "type") {
            "snapshot" => MarketEventType::Snapshot,
            _ => MarketEventType::Delta,
        };

        // Extract sequence number
        let sequence = data.get("change_id").and_then(serde_json::Value::as_u64);

        // Build instrument
        let instrument = self.parse_instrument(instrument_name);

        Ok(MarketEvent {
            event_type,
            instrument,
            timestamp,
            local_timestamp: crate::core::types::now_micros(),
            sequence,
            data: MarketData::Book { bids, asks },
        })
    }

    /// Parse a trade message.
    fn parse_trade_message(
        &self,
        data: &serde_json::Value,
        channel: &str,
    ) -> FlashResult<Vec<MarketEvent>> {
        // Trades come as an array
        let trades = data
            .as_array()
            .ok_or_else(|| FlashError::UnknownMessageFormat("Expected trades array".to_string()))?;

        let mut events = Vec::with_capacity(trades.len());

        for trade in trades {
            let instrument_name = get_str(trade, "instrument_name");
            let instrument = if instrument_name.is_empty() {
                // Extract from channel
                let name = channel.split('.').nth(1).unwrap_or("UNKNOWN");
                self.parse_instrument(name)
            } else {
                self.parse_instrument(instrument_name)
            };

            let timestamp = parse_timestamp(
                trade.get("timestamp").unwrap_or(&serde_json::Value::Null),
                true,
            )
            .unwrap_or_else(|_| crate::core::types::now_micros());

            let price = trade
                .get("price")
                .map_or(0.0, |v| parse_price(v).unwrap_or(0.0));

            let quantity = trade
                .get("amount")
                .map(|v| parse_quantity(v).unwrap_or_default())
                .unwrap_or_default();

            let side = match get_str(trade, "direction") {
                "buy" => Side::Bid,
                _ => Side::Ask,
            };

            let trade_id = trade.get("trade_id").and_then(|v| {
                v.as_str()
                    .map(ToString::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });

            events.push(MarketEvent {
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
            });
        }

        Ok(events)
    }

    /// Parse instrument from Deribit symbol.
    fn parse_instrument(&self, symbol: &str) -> Instrument {
        // BTC-PERPETUAL -> BTC/USD
        // ETH-29DEC23 -> ETH/USD
        let base = symbol.split('-').next().unwrap_or(symbol);
        Instrument::new(base, "USD", Exchange::Deribit, symbol)
    }

    /// Build channel name for an instrument.
    fn build_channel(&self, instrument: &Instrument) -> String {
        format!("book.{}.{}", instrument.raw_symbol, self.interval.as_str())
    }
}

impl ExchangeAdapter for DeribitAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::Deribit
    }

    fn parse_message(&self, raw: &str) -> FlashResult<Vec<MarketEvent>> {
        // Quick checks before full parse
        if contains_field(raw, "\"method\":\"heartbeat\"") {
            return Ok(vec![]);
        }

        // Check for error first
        if contains_field(raw, "\"error\"") {
            if let Some(err) = self.parse_error(raw) {
                return Err(err);
            }
        }

        // Parse JSON
        let json: serde_json::Value = serde_json::from_str(raw).map_err(FlashError::ParseError)?;

        // Check for subscription method
        let method = get_str(&json, "method");
        if method != "subscription" {
            // Not a data message (might be a response)
            return Ok(vec![]);
        }

        // Get params
        let params = json
            .get("params")
            .ok_or_else(|| FlashError::UnknownMessageFormat("Missing params".to_string()))?;

        let channel = get_str(params, "channel");
        let data = params.get("data").ok_or_else(|| {
            FlashError::UnknownMessageFormat("Missing data in params".to_string())
        })?;

        // Route based on channel type
        if channel.starts_with("book.") {
            let event = self.parse_book_message(data, channel)?;
            Ok(vec![event])
        } else if channel.starts_with("trades.") {
            self.parse_trade_message(data, channel)
        } else {
            // Unknown channel - return empty
            Ok(vec![])
        }
    }

    fn build_subscribe(&self, instruments: &[Instrument]) -> Vec<String> {
        if instruments.is_empty() {
            return vec![];
        }

        let channels: Vec<String> = instruments.iter().map(|i| self.build_channel(i)).collect();

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "public/subscribe",
            "params": {
                "channels": channels
            }
        });

        vec![msg.to_string()]
    }

    fn build_unsubscribe(&self, instruments: &[Instrument]) -> Vec<String> {
        if instruments.is_empty() {
            return vec![];
        }

        let channels: Vec<String> = instruments.iter().map(|i| self.build_channel(i)).collect();

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "public/unsubscribe",
            "params": {
                "channels": channels
            }
        });

        vec![msg.to_string()]
    }

    fn handle_ping(&self, payload: &[u8]) -> Option<Vec<u8>> {
        // Check if this is a Deribit heartbeat request
        let payload_str = std::str::from_utf8(payload).ok()?;

        if contains_field(payload_str, "\"method\":\"heartbeat\"")
            || contains_field(payload_str, "test_request")
        {
            // Respond with test message
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": self.next_request_id(),
                "method": "public/test",
                "params": {}
            });
            Some(response.to_string().into_bytes())
        } else {
            None
        }
    }

    fn parse_error(&self, raw: &str) -> Option<FlashError> {
        let json: serde_json::Value = serde_json::from_str(raw).ok()?;
        let error = json.get("error")?;

        let code = error.get("code")?.as_i64()?;
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        match code {
            10020 => Some(FlashError::RateLimited {
                exchange: "deribit".to_string(),
                retry_after_ms: 1000,
            }),
            13009 => Some(FlashError::AuthenticationFailed {
                exchange: "deribit".to_string(),
                reason: message,
            }),
            _ => Some(FlashError::ExchangeError {
                exchange: "deribit".to_string(),
                code: code as i32,
                message,
            }),
        }
    }

    fn is_heartbeat(&self, raw: &str) -> bool {
        contains_field(raw, "\"method\":\"heartbeat\"")
    }

    fn is_subscription_response(&self, raw: &str) -> bool {
        contains_field(raw, "\"result\"") && !contains_field(raw, "\"method\"")
    }

    fn build_auth(&self, api_key: &str, api_secret: &str, timestamp: i64) -> Option<String> {
        // Deribit uses client_credentials grant
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "public/auth",
            "params": {
                "grant_type": "client_credentials",
                "client_id": api_key,
                "client_secret": api_secret,
                "timestamp": timestamp
            }
        });
        Some(msg.to_string())
    }

    fn websocket_url(&self) -> &str {
        DERIBIT_WS_URL
    }

    fn rate_limit(&self) -> RateLimit {
        RateLimit::DERIBIT
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
        let adapter = DeribitAdapter::default();
        assert_eq!(adapter.exchange(), Exchange::Deribit);
        assert_eq!(adapter.interval, BookInterval::Ms100);
    }

    #[test]
    fn test_with_interval() {
        let adapter = DeribitAdapter::with_interval(BookInterval::Raw);
        assert_eq!(adapter.interval, BookInterval::Raw);
    }

    #[test]
    fn test_build_channel() {
        let adapter = DeribitAdapter::default();
        let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
        let channel = adapter.build_channel(&instrument);
        assert_eq!(channel, "book.BTC-PERPETUAL.100ms");
    }

    #[test]
    fn test_parse_instrument() {
        let adapter = DeribitAdapter::default();
        let instrument = adapter.parse_instrument("BTC-PERPETUAL");
        assert_eq!(instrument.base, "BTC");
        assert_eq!(instrument.quote, "USD");
        assert_eq!(instrument.raw_symbol, "BTC-PERPETUAL");
    }

    #[test]
    fn test_request_id_increment() {
        let adapter = DeribitAdapter::default();
        let id1 = adapter.next_request_id();
        let id2 = adapter.next_request_id();
        assert_eq!(id2, id1 + 1);
    }

    #[test]
    fn test_is_heartbeat() {
        let adapter = DeribitAdapter::default();
        assert!(adapter.is_heartbeat(r#"{"jsonrpc":"2.0","method":"heartbeat"}"#));
        assert!(!adapter.is_heartbeat(r#"{"jsonrpc":"2.0","method":"subscription"}"#));
    }

    #[test]
    fn test_is_subscription_response() {
        let adapter = DeribitAdapter::default();
        assert!(adapter.is_subscription_response(r#"{"jsonrpc":"2.0","id":1,"result":[]}"#));
        assert!(!adapter.is_subscription_response(r#"{"jsonrpc":"2.0","method":"subscription"}"#));
    }
}
