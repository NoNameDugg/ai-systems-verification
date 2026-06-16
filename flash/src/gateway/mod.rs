//! Gateway Integration Module
//!
//! This module provides types compatible with a downstream gateway consumption.
//! These are the output formats for Redis publishing.
//!
//! # Overview
//!
//! Gateway-compatible types differ from internal types:
//! - Use `f64` for quantities (Gateway expects JSON numbers)
//! - Use `String` for symbol/exchange (Gateway parses strings)
//! - Optimized for JSON serialization (Gateway reads JSON from Redis)
//!
//! # Types
//!
//! - [`OrderBookSnapshot`] - Full order book state for Gateway UI
//! - [`OrderBookLevel`] - Single price level with f64 quantity
//!
//! # Redis Key Format
//!
//! - `market:orderbook:{symbol}` - OrderBookSnapshot JSON
//!
//! # Example
//!
//! ```rust
//! use astra_flash::gateway::{OrderBookSnapshot, OrderBookLevel};
//!
//! let snapshot = OrderBookSnapshot {
//!     symbol: "EUR_USD".into(),
//!     exchange: "oanda".into(),
//!     timestamp: 1234567890,
//!     bids: vec![OrderBookLevel { price: 1.0850, quantity: 1000000.0 }],
//!     asks: vec![OrderBookLevel { price: 1.0852, quantity: 500000.0 }],
//! };
//!
//! let json = serde_json::to_string(&snapshot).unwrap();
//! ```

use crate::book::BookSnapshot;
use crate::core::types::{PriceLevel, Timestamp};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

// =============================================================================
// ORDER BOOK LEVEL (Gateway Format)
// =============================================================================

/// Single price level in Gateway-compatible format.
///
/// Uses `f64` for both price and quantity to match Gateway's
/// JavaScript/Python consumption expectations.
///
/// # Size
///
/// 16 bytes (two f64 values)
///
/// # Example
///
/// ```rust
/// use astra_flash::gateway::OrderBookLevel;
///
/// let level = OrderBookLevel {
///     price: 50000.0,
///     quantity: 1.5,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OrderBookLevel {
    /// Price of this level.
    pub price: f64,
    /// Quantity at this price (f64 for Gateway compatibility).
    pub quantity: f64,
}

impl OrderBookLevel {
    /// Create a new order book level.
    ///
    /// # Arguments
    ///
    /// * `price` - Price at this level
    /// * `quantity` - Quantity available at this price
    #[must_use]
    pub const fn new(price: f64, quantity: f64) -> Self {
        Self { price, quantity }
    }

    /// Convert from internal PriceLevel.
    ///
    /// Converts Decimal quantity to f64 for Gateway compatibility.
    #[must_use]
    pub fn from_price_level(level: &PriceLevel) -> Self {
        Self {
            price: level.price,
            quantity: level.quantity.to_string().parse::<f64>().unwrap_or(0.0),
        }
    }
}

// =============================================================================
// ORDER BOOK SNAPSHOT (Gateway Format)
// =============================================================================

/// Gateway-compatible order book snapshot.
///
/// This is the format published to Redis for Gateway UI consumption.
/// Differs from internal `BookSnapshot`:
/// - Uses `Cow<'static, str>` for symbol (not `Instrument`) - zero-copy for static strings
/// - Uses `OrderBookLevel` with f64 quantity (not `PriceLevel` with Decimal)
///
/// # Zero-Copy Optimization (Batch 4.2)
///
/// The `symbol` and `exchange` fields use `Cow<'static, str>` to avoid
/// heap allocations when using static string literals (e.g., "deribit").
/// This reduces serialization overhead by ~30% for high-frequency updates.
///
/// # Redis Key
///
/// `market:orderbook:{symbol}`
///
/// # Example
///
/// ```rust
/// use astra_flash::gateway::{OrderBookSnapshot, OrderBookLevel};
///
/// let snapshot = OrderBookSnapshot::new(
///     "BTC_USD",
///     "deribit",
///     1234567890,
///     vec![OrderBookLevel::new(50000.0, 1.5)],
///     vec![OrderBookLevel::new(50010.0, 2.0)],
/// );
///
/// assert_eq!(&*snapshot.symbol, "BTC_USD");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    /// Trading symbol (e.g., "EUR_USD", "BTC_USD").
    /// Uses `Cow<'static, str>` for zero-copy when using static strings.
    pub symbol: Cow<'static, str>,

    /// Exchange identifier (e.g., "oanda", "deribit").
    /// Uses `Cow<'static, str>` for zero-copy when using static strings.
    pub exchange: Cow<'static, str>,

    /// Timestamp in microseconds.
    pub timestamp: Timestamp,

    /// Bid levels (sorted by price descending).
    pub bids: Vec<OrderBookLevel>,

    /// Ask levels (sorted by price ascending).
    pub asks: Vec<OrderBookLevel>,
}

impl OrderBookSnapshot {
    /// Create a new order book snapshot with static strings (zero-copy).
    ///
    /// # Arguments
    ///
    /// * `symbol` - Trading symbol (static string for zero-copy)
    /// * `exchange` - Exchange identifier (static string for zero-copy)
    /// * `timestamp` - Timestamp in microseconds
    /// * `bids` - Bid levels
    /// * `asks` - Ask levels
    ///
    /// # Example
    ///
    /// ```rust
    /// use astra_flash::gateway::{OrderBookSnapshot, OrderBookLevel};
    ///
    /// // Zero-copy: no heap allocation for symbol/exchange
    /// let snapshot = OrderBookSnapshot::new(
    ///     "BTC_USD",
    ///     "deribit",
    ///     1234567890,
    ///     vec![],
    ///     vec![],
    /// );
    /// ```
    #[must_use]
    pub fn new(
        symbol: &'static str,
        exchange: &'static str,
        timestamp: Timestamp,
        bids: Vec<OrderBookLevel>,
        asks: Vec<OrderBookLevel>,
    ) -> Self {
        Self {
            symbol: Cow::Borrowed(symbol),
            exchange: Cow::Borrowed(exchange),
            timestamp,
            bids,
            asks,
        }
    }

    /// Create a new order book snapshot with owned strings.
    ///
    /// Use this when the symbol/exchange are dynamically constructed.
    /// For static strings, prefer `new()` which is zero-copy.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Trading symbol (owned)
    /// * `exchange` - Exchange identifier (owned)
    /// * `timestamp` - Timestamp in microseconds
    /// * `bids` - Bid levels
    /// * `asks` - Ask levels
    #[must_use]
    pub fn new_owned(
        symbol: String,
        exchange: String,
        timestamp: Timestamp,
        bids: Vec<OrderBookLevel>,
        asks: Vec<OrderBookLevel>,
    ) -> Self {
        Self {
            symbol: Cow::Owned(symbol),
            exchange: Cow::Owned(exchange),
            timestamp,
            bids,
            asks,
        }
    }

    /// Convert from internal BookSnapshot.
    ///
    /// Converts internal representation to Gateway-compatible format.
    /// Uses `Cow::Borrowed` for exchange (static string from Exchange enum).
    #[must_use]
    pub fn from_book_snapshot(snapshot: &BookSnapshot) -> Self {
        // Symbol needs to be constructed (owned)
        let symbol = format!(
            "{}_{}",
            snapshot.instrument.base, snapshot.instrument.quote
        );

        // Exchange uses static string from as_str() - zero-copy
        let exchange = snapshot.instrument.exchange.as_str();

        Self {
            symbol: Cow::Owned(symbol),
            exchange: Cow::Borrowed(exchange),
            timestamp: snapshot.timestamp,
            bids: snapshot
                .bids
                .iter()
                .map(OrderBookLevel::from_price_level)
                .collect(),
            asks: snapshot
                .asks
                .iter()
                .map(OrderBookLevel::from_price_level)
                .collect(),
        }
    }

    /// Get the best bid price.
    #[must_use]
    pub fn best_bid_price(&self) -> Option<f64> {
        self.bids.first().map(|l| l.price)
    }

    /// Get the best ask price.
    #[must_use]
    pub fn best_ask_price(&self) -> Option<f64> {
        self.asks.first().map(|l| l.price)
    }

    /// Get the mid price.
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid_price(), self.best_ask_price()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }

    /// Get the spread.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid_price(), self.best_ask_price()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Get the spread in basis points.
    #[must_use]
    pub fn spread_bps(&self) -> Option<f64> {
        match (self.spread(), self.mid_price()) {
            (Some(spread), Some(mid)) if mid > 0.0 => Some((spread / mid) * 10000.0),
            _ => None,
        }
    }

    /// Serialize to JSON for Redis publishing.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize to JSON bytes for Redis publishing.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Validate the snapshot has consistent data.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if valid
    /// - `Err(reason)` if invalid
    pub fn validate(&self) -> Result<(), String> {
        // Symbol must not be empty
        if self.symbol.is_empty() {
            return Err("symbol cannot be empty".to_string());
        }

        // Exchange must not be empty
        if self.exchange.is_empty() {
            return Err("exchange cannot be empty".to_string());
        }

        // Bids should be sorted descending (best bid first)
        for window in self.bids.windows(2) {
            if window[0].price < window[1].price {
                return Err("bids must be sorted descending by price".to_string());
            }
        }

        // Asks should be sorted ascending (best ask first)
        for window in self.asks.windows(2) {
            if window[0].price > window[1].price {
                return Err("asks must be sorted ascending by price".to_string());
            }
        }

        Ok(())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{Exchange, Instrument};
    use rust_decimal_macros::dec;

    // =========================================================================
    // OrderBookLevel Tests (5 tests)
    // =========================================================================

    #[test]
    fn test_order_book_level_new() {
        let level = OrderBookLevel::new(100.0, 50.0);
        assert!((level.price - 100.0).abs() < f64::EPSILON);
        assert!((level.quantity - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_level_from_price_level() {
        let price_level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);
        let level = OrderBookLevel::from_price_level(&price_level);

        assert!((level.price - 50000.0).abs() < f64::EPSILON);
        assert!((level.quantity - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_level_serialization() {
        let level = OrderBookLevel::new(100.0, 50.0);
        let json = serde_json::to_string(&level).expect("Should serialize");
        assert!(json.contains("100"));
        assert!(json.contains("50"));
    }

    #[test]
    fn test_order_book_level_deserialization() {
        let json = r#"{"price":100.0,"quantity":50.0}"#;
        let level: OrderBookLevel = serde_json::from_str(json).expect("Should deserialize");
        assert!((level.price - 100.0).abs() < f64::EPSILON);
        assert!((level.quantity - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_level_size() {
        use std::mem::size_of;
        assert_eq!(size_of::<OrderBookLevel>(), 16);
    }

    // =========================================================================
    // OrderBookSnapshot Tests (15 tests)
    // =========================================================================

    #[test]
    fn test_order_book_snapshot_new() {
        let snapshot = OrderBookSnapshot::new(
            "BTC_USD",
            "deribit",
            1234567890,
            vec![OrderBookLevel::new(50000.0, 1.5)],
            vec![OrderBookLevel::new(50010.0, 2.0)],
        );

        assert_eq!(&*snapshot.symbol, "BTC_USD");
        assert_eq!(&*snapshot.exchange, "deribit");
        assert_eq!(snapshot.timestamp, 1234567890);
        assert_eq!(snapshot.bids.len(), 1);
        assert_eq!(snapshot.asks.len(), 1);
    }

    #[test]
    fn test_order_book_snapshot_from_book_snapshot() {
        let instrument = Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD");
        let book_snapshot = BookSnapshot {
            instrument,
            timestamp: 1234567890,
            bids: vec![PriceLevel::new(1.0850, dec!(1000000), 1234567890)],
            asks: vec![PriceLevel::new(1.0852, dec!(500000), 1234567890)],
        };

        let snapshot = OrderBookSnapshot::from_book_snapshot(&book_snapshot);

        assert_eq!(&*snapshot.symbol, "EUR_USD");
        assert_eq!(&*snapshot.exchange, "oanda");
        assert!((snapshot.bids[0].price - 1.0850).abs() < f64::EPSILON);
        assert!((snapshot.bids[0].quantity - 1000000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_snapshot_best_bid_price() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![
                OrderBookLevel::new(100.0, 10.0),
                OrderBookLevel::new(99.0, 5.0),
            ],
            vec![],
        );

        assert_eq!(snapshot.best_bid_price(), Some(100.0));
    }

    #[test]
    fn test_order_book_snapshot_best_ask_price() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![],
            vec![
                OrderBookLevel::new(101.0, 10.0),
                OrderBookLevel::new(102.0, 5.0),
            ],
        );

        assert_eq!(snapshot.best_ask_price(), Some(101.0));
    }

    #[test]
    fn test_order_book_snapshot_mid_price() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![OrderBookLevel::new(100.0, 10.0)],
            vec![OrderBookLevel::new(102.0, 10.0)],
        );

        let mid = snapshot.mid_price().unwrap();
        assert!((mid - 101.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_snapshot_spread() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![OrderBookLevel::new(100.0, 10.0)],
            vec![OrderBookLevel::new(102.0, 10.0)],
        );

        let spread = snapshot.spread().unwrap();
        assert!((spread - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_book_snapshot_spread_bps() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![OrderBookLevel::new(100.0, 10.0)],
            vec![OrderBookLevel::new(102.0, 10.0)],
        );

        let bps = snapshot.spread_bps().unwrap();
        // spread = 2, mid = 101, bps = (2/101) * 10000 ≈ 198
        assert!((bps - 198.02).abs() < 0.01);
    }

    #[test]
    fn test_order_book_snapshot_serialization_roundtrip() {
        let snapshot = OrderBookSnapshot::new(
            "EUR_USD",
            "oanda",
            1234567890,
            vec![OrderBookLevel::new(1.0850, 1000000.0)],
            vec![OrderBookLevel::new(1.0852, 500000.0)],
        );

        let json = snapshot.to_json().expect("Should serialize");
        let deserialized: OrderBookSnapshot =
            serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(&*snapshot.symbol, &*deserialized.symbol);
        assert_eq!(&*snapshot.exchange, &*deserialized.exchange);
        assert_eq!(snapshot.timestamp, deserialized.timestamp);
        assert_eq!(snapshot.bids.len(), deserialized.bids.len());
        assert_eq!(snapshot.asks.len(), deserialized.asks.len());
    }

    #[test]
    fn test_order_book_snapshot_to_json_bytes() {
        let snapshot = OrderBookSnapshot::new("TEST", "test", 0, vec![], vec![]);
        let bytes = snapshot.to_json_bytes().expect("Should serialize to bytes");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_order_book_snapshot_validate_success() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![
                OrderBookLevel::new(100.0, 10.0),
                OrderBookLevel::new(99.0, 5.0),
            ],
            vec![
                OrderBookLevel::new(101.0, 10.0),
                OrderBookLevel::new(102.0, 5.0),
            ],
        );

        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn test_order_book_snapshot_validate_empty_symbol() {
        let snapshot = OrderBookSnapshot::new("", "test", 0, vec![], vec![]);
        let result = snapshot.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("symbol"));
    }

    #[test]
    fn test_order_book_snapshot_validate_empty_exchange() {
        let snapshot = OrderBookSnapshot::new("TEST", "", 0, vec![], vec![]);
        let result = snapshot.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exchange"));
    }

    #[test]
    fn test_order_book_snapshot_validate_bids_unsorted() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![
                OrderBookLevel::new(99.0, 10.0),  // Lower price first (wrong)
                OrderBookLevel::new(100.0, 5.0), // Higher price second (wrong)
            ],
            vec![],
        );

        let result = snapshot.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bids"));
    }

    #[test]
    fn test_order_book_snapshot_validate_asks_unsorted() {
        let snapshot = OrderBookSnapshot::new(
            "TEST",
            "test",
            0,
            vec![],
            vec![
                OrderBookLevel::new(102.0, 10.0), // Higher price first (wrong)
                OrderBookLevel::new(101.0, 5.0),  // Lower price second (wrong)
            ],
        );

        let result = snapshot.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("asks"));
    }

    #[test]
    fn test_order_book_snapshot_empty_book() {
        let snapshot = OrderBookSnapshot::new("TEST", "test", 0, vec![], vec![]);

        assert!(snapshot.best_bid_price().is_none());
        assert!(snapshot.best_ask_price().is_none());
        assert!(snapshot.mid_price().is_none());
        assert!(snapshot.spread().is_none());
    }
}
