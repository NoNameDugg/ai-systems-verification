//! Core data types for Flash.
//!
//! This module defines the foundational types used throughout Flash:
//!
//! - **Type Aliases**: [`Price`], [`Quantity`], [`Timestamp`]
//! - **Enums**: [`Exchange`], [`Side`], [`MarketEventType`]
//! - **Structs**: [`Instrument`], [`PriceLevel`], [`MarketEvent`]
//! - **Data Payloads**: [`MarketData`]
//!
//! # Design Principles
//!
//! 1. **Zero-Copy Friendly**: Types are designed for minimal allocation in hot paths
//! 2. **Serde Compatible**: All types serialize/deserialize with serde
//! 3. **Thread-Safe**: Types are `Send + Sync` for concurrent use
//! 4. **Memory Efficient**: Optimized for size (see memory analysis in PLANNING.md)
//!
//! # Performance Targets
//!
//! | Type | Size | Clone Cost |
//! |------|------|------------|
//! | `PriceLevel` | ≤48 bytes | <100ns |
//! | `Exchange` | 1 byte | <1ns |
//! | `Instrument` | ~96 bytes | <50ns |
//!
//! # Example
//!
//! ```
//! use astra_flash::core::types::*;
//! use rust_decimal_macros::dec;
//!
//! // Create an instrument
//! let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
//!
//! // Create a price level
//! let level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);
//!
//! // Create a market event
//! let event = MarketEvent {
//!     event_type: MarketEventType::Snapshot,
//!     instrument,
//!     timestamp: 1234567890,
//!     local_timestamp: 1234567891,
//!     sequence: Some(1),
//!     data: MarketData::Book {
//!         bids: vec![level.clone()],
//!         asks: vec![level],
//!     },
//! };
//! ```

use chrono::{DateTime, TimeZone, Utc};
use ordered_float::OrderedFloat;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// =============================================================================
// TYPE ALIASES
// =============================================================================

/// Price type using [`OrderedFloat`] for BTreeMap key compatibility.
///
/// Wraps f64 with total ordering (NaN-safe). Use for order book price keys.
///
/// # Why OrderedFloat?
///
/// - `f64` doesn't implement `Ord` due to NaN handling
/// - `OrderedFloat` provides safe total ordering
/// - Zero-cost abstraction (same size as f64)
pub type Price = OrderedFloat<f64>;

/// Quantity type using [`Decimal`] for precise arithmetic.
///
/// Financial quantities require exact precision (0.1 + 0.2 must equal 0.3).
/// Decimal provides 96-bit precision suitable for all crypto/forex quantities.
pub type Quantity = Decimal;

/// Timestamp in microseconds since Unix epoch.
///
/// # Why i64?
///
/// - Compact (8 bytes vs 12+ for DateTime)
/// - Microsecond precision sufficient for HFT
/// - Easy arithmetic for latency calculations
/// - Convert to [`DateTime`] with [`timestamp_to_datetime`]
pub type Timestamp = i64;

// =============================================================================
// EXCHANGE ENUM
// =============================================================================

/// Supported exchange identifiers.
///
/// Each variant represents a specific exchange with its own WebSocket protocol,
/// authentication method, and message format.
///
/// # Size
///
/// 1 byte (discriminant only)
///
/// # Example
///
/// ```
/// use astra_flash::core::types::Exchange;
///
/// let exchange = Exchange::Deribit;
/// assert_eq!(exchange.as_str(), "deribit");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    /// Deribit crypto derivatives exchange.
    Deribit,
    /// Binance spot and futures exchange.
    Binance,
    /// OANDA forex broker.
    Oanda,
}

impl Exchange {
    /// Returns the lowercase string representation of the exchange.
    ///
    /// Used for logging, metrics labels, and topic naming.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::Exchange;
    ///
    /// assert_eq!(Exchange::Deribit.as_str(), "deribit");
    /// assert_eq!(Exchange::Binance.as_str(), "binance");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Deribit => "deribit",
            Self::Binance => "binance",
            Self::Oanda => "oanda",
        }
    }
}

impl std::fmt::Display for Exchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// SIDE ENUM
// =============================================================================

/// Order book side (bid or ask).
///
/// # Size
///
/// 1 byte (discriminant only)
///
/// # Example
///
/// ```
/// use astra_flash::core::types::Side;
///
/// let bid = Side::Bid;
/// let ask = bid.opposite();
/// assert_eq!(ask, Side::Ask);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    /// Buy side (bids).
    Bid,
    /// Sell side (asks).
    Ask,
}

impl Side {
    /// Returns the opposite side.
    ///
    /// `Bid.opposite() == Ask` and `Ask.opposite() == Bid`
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::Side;
    ///
    /// assert_eq!(Side::Bid.opposite(), Side::Ask);
    /// assert_eq!(Side::Ask.opposite(), Side::Bid);
    /// ```
    #[must_use]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::Bid => Self::Ask,
            Self::Ask => Self::Bid,
        }
    }

    /// Returns the lowercase string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Bid => "bid",
            Self::Ask => "ask",
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// MARKET EVENT TYPE ENUM
// =============================================================================

/// Type of market event received from an exchange.
///
/// # Size
///
/// 1 byte (discriminant only)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketEventType {
    /// Full order book snapshot (replaces all data).
    Snapshot,
    /// Incremental update (delta changes).
    Delta,
    /// Trade execution.
    Trade,
    /// Connection heartbeat.
    Heartbeat,
}

impl MarketEventType {
    /// Returns the lowercase string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Delta => "delta",
            Self::Trade => "trade",
            Self::Heartbeat => "heartbeat",
        }
    }
}

impl std::fmt::Display for MarketEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// INSTRUMENT STRUCT
// =============================================================================

/// Trading instrument identifier.
///
/// Represents a tradable asset pair on a specific exchange. Provides a unified
/// representation across all supported exchanges.
///
/// # Fields
///
/// - `base`: Base currency (e.g., "BTC", "ETH")
/// - `quote`: Quote currency (e.g., "USD", "USDT")
/// - `exchange`: The exchange this instrument belongs to
/// - `raw_symbol`: Exchange-specific symbol (e.g., "BTC-PERPETUAL", "BTCUSDT")
///
/// # Example
///
/// ```
/// use astra_flash::core::types::{Instrument, Exchange};
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// assert_eq!(instrument.symbol(), "BTC/USD");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Instrument {
    /// Base currency (e.g., "BTC").
    pub base: String,
    /// Quote currency (e.g., "USD").
    pub quote: String,
    /// Exchange this instrument belongs to.
    pub exchange: Exchange,
    /// Exchange-specific raw symbol (e.g., "BTC-PERPETUAL").
    pub raw_symbol: String,
}

impl Instrument {
    /// Creates a new instrument.
    ///
    /// # Arguments
    ///
    /// * `base` - Base currency symbol
    /// * `quote` - Quote currency symbol
    /// * `exchange` - The exchange
    /// * `raw_symbol` - Exchange-specific symbol
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::{Instrument, Exchange};
    ///
    /// let btc = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// ```
    #[must_use]
    pub fn new(
        base: impl Into<String>,
        quote: impl Into<String>,
        exchange: Exchange,
        raw_symbol: impl Into<String>,
    ) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
            exchange,
            raw_symbol: raw_symbol.into(),
        }
    }

    /// Returns the unified symbol format "BASE/QUOTE".
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::{Instrument, Exchange};
    ///
    /// let instrument = Instrument::new("ETH", "USDT", Exchange::Binance, "ETHUSDT");
    /// assert_eq!(instrument.symbol(), "ETH/USDT");
    /// ```
    #[must_use]
    pub fn symbol(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    /// Returns the exchange this instrument belongs to.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::{Instrument, Exchange};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// assert_eq!(instrument.exchange(), Exchange::Deribit);
    /// ```
    #[must_use]
    pub const fn exchange(&self) -> Exchange {
        self.exchange
    }
}

impl std::fmt::Display for Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.exchange, self.raw_symbol)
    }
}

// =============================================================================
// PRICE LEVEL STRUCT
// =============================================================================

/// Single price level in an order book.
///
/// Represents a bid or ask at a specific price with the total quantity available.
///
/// # Size
///
/// Approximately 40 bytes (see memory analysis in PLANNING.md):
/// - `price`: 8 bytes (f64)
/// - `quantity`: 16 bytes (Decimal)
/// - `order_count`: 8 bytes (Option<u32> with padding)
/// - `timestamp`: 8 bytes (i64)
///
/// # Example
///
/// ```
/// use astra_flash::core::types::PriceLevel;
/// use rust_decimal_macros::dec;
///
/// let level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);
/// assert_eq!(level.price, 50000.0);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceLevel {
    /// Price of this level.
    pub price: f64,
    /// Total quantity at this price.
    pub quantity: Quantity,
    /// Number of orders at this level (L3 data, if available).
    pub order_count: Option<u32>,
    /// Exchange timestamp when this level was updated.
    pub timestamp: Timestamp,
}

impl PriceLevel {
    /// Creates a new price level.
    ///
    /// # Arguments
    ///
    /// * `price` - The price of this level
    /// * `quantity` - Total quantity at this price
    /// * `timestamp` - Exchange timestamp in microseconds
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::types::PriceLevel;
    /// use rust_decimal_macros::dec;
    ///
    /// let level = PriceLevel::new(42000.50, dec!(2.5), 1703808000000000);
    /// ```
    #[must_use]
    pub const fn new(price: f64, quantity: Quantity, timestamp: Timestamp) -> Self {
        Self {
            price,
            quantity,
            order_count: None,
            timestamp,
        }
    }

    /// Creates a new price level with order count (L3 data).
    ///
    /// # Arguments
    ///
    /// * `price` - The price of this level
    /// * `quantity` - Total quantity at this price
    /// * `order_count` - Number of orders at this level
    /// * `timestamp` - Exchange timestamp in microseconds
    #[must_use]
    pub const fn with_order_count(
        price: f64,
        quantity: Quantity,
        order_count: u32,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            price,
            quantity,
            order_count: Some(order_count),
            timestamp,
        }
    }

    /// Returns true if the quantity is zero (level should be removed).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.quantity.is_zero()
    }
}

// =============================================================================
// MARKET DATA ENUM
// =============================================================================

/// Market data payload in a [`MarketEvent`].
///
/// Represents the actual data content of a market event, which can be:
/// - Order book data (snapshot or delta)
/// - Trade execution
/// - Heartbeat from exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketData {
    /// Order book data (bids and asks).
    Book {
        /// Bid levels (buy orders), sorted by price descending.
        bids: Vec<PriceLevel>,
        /// Ask levels (sell orders), sorted by price ascending.
        asks: Vec<PriceLevel>,
    },
    /// Trade execution data.
    Trade {
        /// Trade price.
        price: f64,
        /// Trade quantity.
        quantity: Quantity,
        /// Trade side (taker side).
        side: Side,
        /// Exchange trade ID (if available).
        trade_id: Option<String>,
    },
    /// Exchange heartbeat.
    Heartbeat {
        /// Exchange timestamp.
        exchange_time: Timestamp,
    },
}

impl MarketData {
    /// Returns true if this is book data.
    #[must_use]
    pub const fn is_book(&self) -> bool {
        matches!(self, Self::Book { .. })
    }

    /// Returns true if this is trade data.
    #[must_use]
    pub const fn is_trade(&self) -> bool {
        matches!(self, Self::Trade { .. })
    }

    /// Returns true if this is heartbeat data.
    #[must_use]
    pub const fn is_heartbeat(&self) -> bool {
        matches!(self, Self::Heartbeat { .. })
    }
}

// =============================================================================
// MARKET EVENT STRUCT
// =============================================================================

/// Unified market event from any exchange.
///
/// Represents a normalized market data event that can be processed uniformly
/// regardless of which exchange it originated from.
///
/// # Example
///
/// ```
/// use astra_flash::core::types::*;
/// use rust_decimal_macros::dec;
///
/// let event = MarketEvent {
///     event_type: MarketEventType::Trade,
///     instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
///     timestamp: 1234567890,
///     local_timestamp: 1234567891,
///     sequence: Some(1),
///     data: MarketData::Trade {
///         price: 50000.0,
///         quantity: dec!(0.1),
///         side: Side::Bid,
///         trade_id: Some("trade_123".into()),
///     },
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEvent {
    /// Type of market event.
    pub event_type: MarketEventType,
    /// Instrument this event is for.
    pub instrument: Instrument,
    /// Exchange timestamp in microseconds.
    pub timestamp: Timestamp,
    /// Local receipt timestamp in microseconds.
    pub local_timestamp: Timestamp,
    /// Exchange sequence number (for gap detection).
    pub sequence: Option<u64>,
    /// Event data payload.
    pub data: MarketData,
}

impl MarketEvent {
    /// Returns the latency between exchange time and local time in microseconds.
    ///
    /// Negative values indicate clock skew (local behind exchange).
    #[must_use]
    pub const fn latency_micros(&self) -> i64 {
        self.local_timestamp - self.timestamp
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Converts a microsecond timestamp to a chrono [`DateTime<Utc>`].
///
/// Returns `None` if the timestamp is invalid (e.g., out of range).
///
/// # Example
///
/// ```
/// use astra_flash::core::types::timestamp_to_datetime;
///
/// let dt = timestamp_to_datetime(1703808000000000);
/// assert!(dt.is_some());
/// ```
#[must_use]
pub fn timestamp_to_datetime(timestamp: Timestamp) -> Option<DateTime<Utc>> {
    let secs = timestamp / 1_000_000;
    let nsecs = ((timestamp % 1_000_000) * 1000) as u32;
    Utc.timestamp_opt(secs, nsecs).single()
}

/// Converts a chrono [`DateTime<Utc>`] to a microsecond timestamp.
///
/// # Example
///
/// ```
/// use chrono::Utc;
/// use astra_flash::core::types::datetime_to_timestamp;
///
/// let now = Utc::now();
/// let timestamp = datetime_to_timestamp(&now);
/// ```
#[must_use]
pub const fn datetime_to_timestamp(dt: &DateTime<Utc>) -> Timestamp {
    dt.timestamp_micros()
}

/// Returns the current UTC timestamp in microseconds.
///
/// # Example
///
/// ```
/// use astra_flash::core::types::now_micros;
///
/// let timestamp = now_micros();
/// assert!(timestamp > 0);
/// ```
#[must_use]
pub fn now_micros() -> Timestamp {
    Utc::now().timestamp_micros()
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::mem::size_of;

    #[test]
    fn test_exchange_size() {
        assert_eq!(size_of::<Exchange>(), 1);
    }

    #[test]
    fn test_side_size() {
        assert_eq!(size_of::<Side>(), 1);
    }

    #[test]
    fn test_price_level_size() {
        // PriceLevel should be ≤ 48 bytes
        let size = size_of::<PriceLevel>();
        assert!(size <= 48, "PriceLevel size is {} bytes", size);
    }

    #[test]
    fn test_price_level_new() {
        let level = PriceLevel::new(100.0, dec!(10), 1234567890);
        assert!((level.price - 100.0).abs() < f64::EPSILON);
        assert_eq!(level.quantity, dec!(10));
        assert_eq!(level.timestamp, 1234567890);
        assert!(level.order_count.is_none());
    }

    #[test]
    fn test_instrument_symbol() {
        let inst = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
        assert_eq!(inst.symbol(), "BTC/USD");
    }

    #[test]
    fn test_side_opposite() {
        assert_eq!(Side::Bid.opposite(), Side::Ask);
        assert_eq!(Side::Ask.opposite(), Side::Bid);
    }

    #[test]
    fn test_timestamp_conversion() {
        let ts: Timestamp = 1703808000000000; // 2023-12-29 00:00:00 UTC
        let dt = timestamp_to_datetime(ts);
        assert!(dt.is_some());
    }
}
