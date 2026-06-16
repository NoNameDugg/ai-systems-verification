//! Exchange adapter trait definition.
//!
//! This module defines the [`ExchangeAdapter`] trait that all exchange-specific
//! adapters must implement. The trait provides a unified interface for:
//!
//! - Parsing exchange messages into normalized [`MarketEvent`]s
//! - Building subscription/unsubscription messages
//! - Handling exchange-specific ping/pong protocols
//! - Parsing error responses
//!
//! # Example
//!
//! ```ignore
//! use astra_flash::network::adapters::{ExchangeAdapter, create_adapter};
//! use astra_flash::core::types::Exchange;
//!
//! let adapter = create_adapter(Exchange::Deribit);
//! let events = adapter.parse_message(raw_json)?;
//! ```

use crate::core::error::FlashResult;
use crate::core::types::{Exchange, Instrument, MarketEvent};
use std::fmt::Debug;

// =============================================================================
// RATE LIMIT CONFIGURATION
// =============================================================================

/// Rate limiting configuration for an exchange.
///
/// Each exchange has different rate limits for WebSocket messages and
/// subscriptions. This struct captures those limits.
///
/// # Example
///
/// ```
/// use astra_flash::network::adapters::RateLimit;
///
/// let limit = RateLimit::DERIBIT;
/// assert!(limit.ws_messages_per_second > 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// Maximum WebSocket messages per second.
    pub ws_messages_per_second: u32,
    /// Minimum interval between messages in milliseconds.
    pub min_interval_ms: u64,
    /// Maximum subscriptions per connection.
    pub max_subscriptions: u32,
}

impl RateLimit {
    /// Deribit rate limits.
    ///
    /// Deribit allows up to 100 messages per second on WebSocket connections.
    pub const DERIBIT: Self = Self {
        ws_messages_per_second: 100,
        min_interval_ms: 10,
        max_subscriptions: 200,
    };

    /// Binance rate limits.
    ///
    /// Binance allows 5 outgoing messages per second on WebSocket connections.
    pub const BINANCE: Self = Self {
        ws_messages_per_second: 5,
        min_interval_ms: 200,
        max_subscriptions: 1024,
    };

    /// OANDA rate limits.
    ///
    /// OANDA allows up to 120 requests per second.
    pub const OANDA: Self = Self {
        ws_messages_per_second: 120,
        min_interval_ms: 8,
        max_subscriptions: 100,
    };
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            ws_messages_per_second: 10,
            min_interval_ms: 100,
            max_subscriptions: 100,
        }
    }
}

// =============================================================================
// EXCHANGE ADAPTER TRAIT
// =============================================================================

/// Trait for exchange-specific message parsing and building.
///
/// Each exchange has its own WebSocket protocol, message format, and
/// subscription mechanism. This trait abstracts those differences to provide
/// a unified interface for the rest of the system.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` for use across async tasks.
/// All methods take `&self` to allow shared access.
///
/// # Performance
///
/// Implementations should minimize allocations in [`parse_message`] as
/// it's called for every incoming message in the hot path.
///
/// # Example
///
/// ```ignore
/// use astra_flash::network::adapters::ExchangeAdapter;
///
/// fn process_message<A: ExchangeAdapter>(adapter: &A, raw: &str) {
///     match adapter.parse_message(raw) {
///         Ok(events) => {
///             for event in events {
///                 println!("Received: {:?}", event);
///             }
///         }
///         Err(e) => eprintln!("Parse error: {}", e),
///     }
/// }
/// ```
pub trait ExchangeAdapter: Send + Sync + Debug {
    /// Returns the exchange this adapter handles.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let adapter = DeribitAdapter::default();
    /// assert_eq!(adapter.exchange(), Exchange::Deribit);
    /// ```
    fn exchange(&self) -> Exchange;

    /// Parse a raw WebSocket message into market events.
    ///
    /// # Arguments
    ///
    /// * `raw` - The raw JSON message from the WebSocket
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<MarketEvent>)` - Successfully parsed events (may be empty for
    ///   messages that don't produce events, like heartbeats)
    /// - `Err(FlashError)` - Parsing failed
    ///
    /// # Performance
    ///
    /// Target: < 1 μs per message
    ///
    /// # Example
    ///
    /// ```ignore
    /// let events = adapter.parse_message(r#"{"data": {...}}"#)?;
    /// for event in events {
    ///     process_event(event);
    /// }
    /// ```
    fn parse_message(&self, raw: &str) -> FlashResult<Vec<MarketEvent>>;

    /// Build subscription message(s) for the given instruments.
    ///
    /// Some exchanges allow subscribing to multiple instruments in one
    /// message, others require separate messages.
    ///
    /// # Arguments
    ///
    /// * `instruments` - Instruments to subscribe to
    ///
    /// # Returns
    ///
    /// Vector of JSON messages to send to the exchange.
    /// May be empty if the exchange uses URL parameters for subscription.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let instruments = vec![Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")];
    /// let messages = adapter.build_subscribe(&instruments);
    /// for msg in messages {
    ///     ws.send(msg).await?;
    /// }
    /// ```
    fn build_subscribe(&self, instruments: &[Instrument]) -> Vec<String>;

    /// Build unsubscription message(s) for the given instruments.
    ///
    /// # Arguments
    ///
    /// * `instruments` - Instruments to unsubscribe from
    ///
    /// # Returns
    ///
    /// Vector of JSON messages to send to the exchange.
    fn build_unsubscribe(&self, instruments: &[Instrument]) -> Vec<String>;

    /// Handle an exchange-specific ping and return the pong response.
    ///
    /// Some exchanges use application-level ping/pong in addition to
    /// WebSocket protocol ping/pong frames.
    ///
    /// # Arguments
    ///
    /// * `payload` - The ping payload bytes
    ///
    /// # Returns
    ///
    /// - `Some(bytes)` - Pong response to send back
    /// - `None` - No response needed (use WebSocket pong frame instead)
    ///
    /// # Example
    ///
    /// ```ignore
    /// if let Some(pong) = adapter.handle_ping(ping_data) {
    ///     ws.send_text(String::from_utf8(pong)?).await?;
    /// }
    /// ```
    fn handle_ping(&self, payload: &[u8]) -> Option<Vec<u8>>;

    /// Check if a message indicates an error from the exchange.
    ///
    /// # Arguments
    ///
    /// * `raw` - The raw JSON message
    ///
    /// # Returns
    ///
    /// - `Some(FlashError)` - Error detected and parsed
    /// - `None` - Not an error message
    ///
    /// # Example
    ///
    /// ```ignore
    /// if let Some(err) = adapter.parse_error(raw) {
    ///     handle_exchange_error(err);
    ///     return;
    /// }
    /// ```
    fn parse_error(&self, raw: &str) -> Option<crate::core::error::FlashError>;

    /// Check if this message is a heartbeat.
    ///
    /// # Arguments
    ///
    /// * `raw` - The raw JSON message
    ///
    /// # Returns
    ///
    /// `true` if this is a heartbeat message, `false` otherwise.
    fn is_heartbeat(&self, raw: &str) -> bool;

    /// Check if this message is a subscription confirmation.
    ///
    /// # Arguments
    ///
    /// * `raw` - The raw JSON message
    ///
    /// # Returns
    ///
    /// `true` if this is a subscription response, `false` otherwise.
    fn is_subscription_response(&self, raw: &str) -> bool;

    /// Build authentication message if required.
    ///
    /// # Arguments
    ///
    /// * `api_key` - API key
    /// * `api_secret` - API secret
    /// * `timestamp` - Current timestamp for signing
    ///
    /// # Returns
    ///
    /// - `Some(message)` - Authentication message to send
    /// - `None` - No authentication required for public streams
    fn build_auth(&self, api_key: &str, api_secret: &str, timestamp: i64) -> Option<String>;

    /// Get the WebSocket URL for this exchange.
    ///
    /// # Returns
    ///
    /// The WebSocket URL to connect to.
    fn websocket_url(&self) -> &str;

    /// Get the rate limit for this exchange.
    ///
    /// # Returns
    ///
    /// Rate limiting configuration.
    fn rate_limit(&self) -> RateLimit;
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_deribit() {
        let limit = RateLimit::DERIBIT;
        assert_eq!(limit.ws_messages_per_second, 100);
        assert_eq!(limit.min_interval_ms, 10);
        assert_eq!(limit.max_subscriptions, 200);
    }

    #[test]
    fn test_rate_limit_binance() {
        let limit = RateLimit::BINANCE;
        assert_eq!(limit.ws_messages_per_second, 5);
        assert_eq!(limit.min_interval_ms, 200);
        assert_eq!(limit.max_subscriptions, 1024);
    }

    #[test]
    fn test_rate_limit_oanda() {
        let limit = RateLimit::OANDA;
        assert_eq!(limit.ws_messages_per_second, 120);
        assert_eq!(limit.min_interval_ms, 8);
        assert_eq!(limit.max_subscriptions, 100);
    }

    #[test]
    fn test_rate_limit_default() {
        let limit = RateLimit::default();
        assert!(limit.ws_messages_per_second > 0);
        assert!(limit.max_subscriptions > 0);
    }
}
