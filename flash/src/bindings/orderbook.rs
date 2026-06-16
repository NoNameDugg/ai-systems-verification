//! Python bindings for OrderBook (Phase 5.2).
//!
//! This module provides Python bindings for the Flash order book:
//!
//! - [`PyOrderBookConfig`] - Configuration for order book behavior
//! - [`PyOrderBookStats`] - Order book statistics
//! - [`PyOrderBook`] - The main order book class
//!
//! # Features
//!
//! - Thread-safe access via `parking_lot::RwLock`
//! - All OrderBook methods exposed to Python
//! - Efficient type conversions with minimal copying
//!
//! # Python Usage
//!
//! ```python
//! from astra_flash import OrderBook, OrderBookConfig, Exchange, Instrument, PriceLevel, Side
//!
//! # Create instrument
//! inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
//!
//! # Create order book with optional config
//! config = OrderBookConfig(max_depth=100, max_levels=200)
//! book = OrderBook(inst, config)
//!
//! # Apply snapshot
//! book.apply_snapshot(
//!     bids=[PriceLevel(50000.0, "1.5", 1234567890)],
//!     asks=[PriceLevel(50100.0, "2.0", 1234567890)],
//!     timestamp=1234567890
//! )
//!
//! # Query the book
//! print(book.best_bid)   # PyPriceLevel or None
//! print(book.mid_price)  # float or None
//! print(book.spread)     # float or None
//!
//! # Get snapshot
//! snapshot = book.snapshot(50)
//! ```

use parking_lot::RwLock;
use pyo3::prelude::*;
use std::sync::Arc;

use crate::bindings::types::{PyBookSnapshot, PyInstrument, PyPriceLevel, PySide};
use crate::book::{OrderBook, OrderBookStats};
use crate::core::config::{GapHandling, OrderBookConfig};
use crate::core::types::{Instrument, PriceLevel, Side};

// =============================================================================
// PYORDERBOOKCONFIG
// =============================================================================

/// Python wrapper for OrderBookConfig.
///
/// Configuration options for order book behavior.
///
/// # Properties
///
/// - `max_depth` - Maximum levels for serialization (default: 50)
/// - `max_levels` - Hard limit on levels per side (default: 100)
/// - `track_orders` - Whether to track L3 order data (default: false)
/// - `auto_prune` - Auto-remove zero-quantity levels (default: true)
///
/// # Example
///
/// ```python
/// from astra_flash import OrderBookConfig
///
/// # Default config
/// config = OrderBookConfig()
///
/// # Custom config
/// config = OrderBookConfig(max_depth=100, max_levels=200, auto_prune=True)
/// ```
#[pyclass(name = "OrderBookConfig", frozen)]
#[derive(Debug, Clone)]
pub struct PyOrderBookConfig {
    /// Maximum levels for serialization (soft limit).
    max_depth: usize,
    /// Maximum levels per side (hard limit for memory protection).
    max_levels: usize,
    /// Whether to track L3 order data.
    track_orders: bool,
    /// Auto-remove zero-quantity levels.
    auto_prune: bool,
}

#[pymethods]
impl PyOrderBookConfig {
    /// Creates a new order book configuration.
    ///
    /// # Arguments
    ///
    /// * `max_depth` - Maximum levels for serialization (default: 50)
    /// * `max_levels` - Hard limit on levels per side (default: 100)
    /// * `track_orders` - Whether to track L3 order data (default: false)
    /// * `auto_prune` - Auto-remove zero-quantity levels (default: true)
    #[new]
    #[pyo3(signature = (max_depth=50, max_levels=100, track_orders=false, auto_prune=true))]
    #[must_use]
    pub fn new(max_depth: usize, max_levels: usize, track_orders: bool, auto_prune: bool) -> Self {
        Self {
            max_depth,
            max_levels,
            track_orders,
            auto_prune,
        }
    }

    /// Returns the maximum depth for serialization.
    #[getter]
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Returns the maximum levels per side (hard limit).
    #[getter]
    #[must_use]
    pub fn max_levels(&self) -> usize {
        self.max_levels
    }

    /// Returns whether L3 order tracking is enabled.
    #[getter]
    #[must_use]
    pub fn track_orders(&self) -> bool {
        self.track_orders
    }

    /// Returns whether auto-pruning is enabled.
    #[getter]
    #[must_use]
    pub fn auto_prune(&self) -> bool {
        self.auto_prune
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderBookConfig(max_depth={}, max_levels={}, track_orders={}, auto_prune={})",
            self.max_depth, self.max_levels, self.track_orders, self.auto_prune
        )
    }

    fn __str__(&self) -> String {
        format!(
            "max_depth={}, max_levels={}",
            self.max_depth, self.max_levels
        )
    }
}

impl Default for PyOrderBookConfig {
    fn default() -> Self {
        Self::new(50, 100, false, true)
    }
}

impl From<OrderBookConfig> for PyOrderBookConfig {
    fn from(c: OrderBookConfig) -> Self {
        Self {
            max_depth: c.max_depth,
            max_levels: c.max_levels,
            track_orders: c.track_orders,
            auto_prune: c.auto_prune,
        }
    }
}

impl From<PyOrderBookConfig> for OrderBookConfig {
    fn from(c: PyOrderBookConfig) -> Self {
        Self {
            max_depth: c.max_depth,
            max_levels: c.max_levels,
            track_orders: c.track_orders,
            auto_prune: c.auto_prune,
            gap_handling: GapHandling::default(),
        }
    }
}

// =============================================================================
// PYORDERBOOKSTATS
// =============================================================================

/// Python wrapper for OrderBookStats.
///
/// Statistics for order book monitoring.
///
/// # Properties
///
/// - `update_count` - Number of delta updates applied
/// - `snapshot_count` - Number of snapshots applied
/// - `last_update_timestamp` - Timestamp of last update (microseconds)
/// - `bid_levels` - Current number of bid levels
/// - `ask_levels` - Current number of ask levels
///
/// # Example
///
/// ```python
/// from astra_flash import OrderBook
///
/// book = OrderBook(inst)
/// stats = book.stats
/// print(f"Updates: {stats.update_count}, Snapshots: {stats.snapshot_count}")
/// ```
#[pyclass(name = "OrderBookStats", frozen)]
#[derive(Debug, Clone)]
pub struct PyOrderBookStats {
    /// Number of delta updates applied.
    update_count: u64,
    /// Number of snapshots applied.
    snapshot_count: u64,
    /// Timestamp of last update (microseconds).
    last_update_timestamp: i64,
    /// Current number of bid levels.
    bid_levels: usize,
    /// Current number of ask levels.
    ask_levels: usize,
}

#[pymethods]
impl PyOrderBookStats {
    /// Returns the number of delta updates applied.
    #[getter]
    #[must_use]
    pub fn update_count(&self) -> u64 {
        self.update_count
    }

    /// Returns the number of snapshots applied.
    #[getter]
    #[must_use]
    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count
    }

    /// Returns the timestamp of last update (microseconds).
    #[getter]
    #[must_use]
    pub fn last_update_timestamp(&self) -> i64 {
        self.last_update_timestamp
    }

    /// Returns the current number of bid levels.
    #[getter]
    #[must_use]
    pub fn bid_levels(&self) -> usize {
        self.bid_levels
    }

    /// Returns the current number of ask levels.
    #[getter]
    #[must_use]
    pub fn ask_levels(&self) -> usize {
        self.ask_levels
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderBookStats(updates={}, snapshots={}, bids={}, asks={})",
            self.update_count, self.snapshot_count, self.bid_levels, self.ask_levels
        )
    }
}

impl From<OrderBookStats> for PyOrderBookStats {
    fn from(s: OrderBookStats) -> Self {
        Self {
            update_count: s.update_count,
            snapshot_count: s.snapshot_count,
            last_update_timestamp: s.last_update_timestamp,
            bid_levels: s.bid_levels,
            ask_levels: s.ask_levels,
        }
    }
}

// =============================================================================
// PYORDERBOOK
// =============================================================================

/// Python wrapper for OrderBook.
///
/// L2 Order Book with O(log n) updates and O(1) best bid/ask access.
///
/// # Thread Safety
///
/// This class is thread-safe. Multiple Python threads can safely access
/// the same order book instance.
///
/// # Example
///
/// ```python
/// from astra_flash import OrderBook, Instrument, Exchange, PriceLevel, Side
///
/// # Create instrument
/// inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
///
/// # Create order book
/// book = OrderBook(inst)
///
/// # Apply snapshot
/// book.apply_snapshot(
///     bids=[PriceLevel(50000.0, "1.5", 1234567890)],
///     asks=[PriceLevel(50100.0, "2.0", 1234567890)],
///     timestamp=1234567890
/// )
///
/// # Query
/// print(book.best_bid)   # PyPriceLevel
/// print(book.mid_price)  # 50050.0
/// print(book.spread)     # 100.0
///
/// # Get depth
/// bids = book.top_bids(10)
/// asks = book.top_asks(10)
///
/// # Create snapshot for export
/// snapshot = book.snapshot(50)
/// ```
#[pyclass(name = "OrderBook")]
pub struct PyOrderBook {
    /// Thread-safe inner order book.
    inner: Arc<RwLock<OrderBook>>,
    /// Cached instrument for lock-free access.
    instrument_cache: PyInstrument,
}

#[pymethods]
impl PyOrderBook {
    // =========================================================================
    // CONSTRUCTORS
    // =========================================================================

    /// Creates a new order book.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `config` - Optional configuration (defaults to standard config)
    ///
    /// # Example
    ///
    /// ```python
    /// book = OrderBook(inst)  # Default config
    /// book = OrderBook(inst, config)  # Custom config
    /// ```
    #[new]
    #[pyo3(signature = (instrument, config=None))]
    #[must_use]
    pub fn new(instrument: PyInstrument, config: Option<PyOrderBookConfig>) -> Self {
        let rust_config = config.map(Into::into).unwrap_or_default();
        let rust_instrument: Instrument = instrument.clone().into();
        let book = OrderBook::new(rust_instrument, rust_config);

        Self {
            inner: Arc::new(RwLock::new(book)),
            instrument_cache: instrument,
        }
    }

    /// Creates an order book from a snapshot.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `bids` - Initial bid levels
    /// * `asks` - Initial ask levels
    /// * `timestamp` - Timestamp of the snapshot
    /// * `config` - Optional configuration
    ///
    /// # Example
    ///
    /// ```python
    /// book = OrderBook.from_snapshot(
    ///     inst,
    ///     bids=[PriceLevel(100.0, "10", ts)],
    ///     asks=[PriceLevel(101.0, "5", ts)],
    ///     timestamp=ts
    /// )
    /// ```
    #[staticmethod]
    #[pyo3(signature = (instrument, bids, asks, timestamp, config=None))]
    #[must_use]
    pub fn from_snapshot(
        instrument: PyInstrument,
        bids: Vec<PyPriceLevel>,
        asks: Vec<PyPriceLevel>,
        timestamp: i64,
        config: Option<PyOrderBookConfig>,
    ) -> Self {
        let book = Self::new(instrument, config);
        book.apply_snapshot(bids, asks, timestamp);
        book
    }

    // =========================================================================
    // CORE OPERATIONS
    // =========================================================================

    /// Applies a full snapshot to the order book.
    ///
    /// Replaces all existing data with the new snapshot.
    ///
    /// # Arguments
    ///
    /// * `bids` - New bid levels
    /// * `asks` - New ask levels
    /// * `timestamp` - Timestamp of the snapshot
    ///
    /// # Example
    ///
    /// ```python
    /// book.apply_snapshot(
    ///     bids=[PriceLevel(100.0, "10", ts)],
    ///     asks=[PriceLevel(101.0, "5", ts)],
    ///     timestamp=ts
    /// )
    /// ```
    pub fn apply_snapshot(&self, bids: Vec<PyPriceLevel>, asks: Vec<PyPriceLevel>, timestamp: i64) {
        let rust_bids: Vec<PriceLevel> = bids.into_iter().map(Into::into).collect();
        let rust_asks: Vec<PriceLevel> = asks.into_iter().map(Into::into).collect();
        self.inner
            .write()
            .apply_snapshot(rust_bids, rust_asks, timestamp);
    }

    /// Applies an incremental update (delta) to the order book.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to update (Bid or Ask)
    /// * `levels` - Price levels to insert, update, or delete
    /// * `timestamp` - Timestamp of the update
    ///
    /// # Behavior
    ///
    /// - `quantity > 0`: Insert or update the level
    /// - `quantity = 0`: Remove the level (if auto_prune enabled)
    ///
    /// # Example
    ///
    /// ```python
    /// # Insert/update
    /// book.apply_delta(Side.Bid, [PriceLevel(99.0, "5.0", ts)], ts)
    ///
    /// # Remove (zero quantity)
    /// book.apply_delta(Side.Bid, [PriceLevel(99.0, "0", ts)], ts)
    /// ```
    pub fn apply_delta(&self, side: PySide, levels: Vec<PyPriceLevel>, timestamp: i64) {
        let rust_side: Side = side.into();
        let rust_levels: Vec<PriceLevel> = levels.into_iter().map(Into::into).collect();
        self.inner
            .write()
            .apply_delta(rust_side, rust_levels, timestamp);
    }

    /// Updates a single price level.
    ///
    /// Convenience method for single-level updates.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to update
    /// * `level` - The price level to insert/update
    pub fn update_level(&self, side: PySide, level: PyPriceLevel) {
        let rust_side: Side = side.into();
        let rust_level: PriceLevel = level.into();
        self.inner.write().update_level(rust_side, rust_level);
    }

    /// Removes a level by price.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to remove from
    /// * `price` - The price to remove
    ///
    /// # Returns
    ///
    /// The removed level if it existed, None otherwise.
    #[must_use]
    pub fn remove_level(&self, side: PySide, price: f64) -> Option<PyPriceLevel> {
        let rust_side: Side = side.into();
        self.inner
            .write()
            .remove_level(rust_side, price)
            .map(Into::into)
    }

    /// Clears all levels from the order book.
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    // =========================================================================
    // QUERIES (Properties)
    // =========================================================================

    /// Returns the best bid (highest bid price).
    ///
    /// # Returns
    ///
    /// The best bid PyPriceLevel, or None if no bids.
    #[getter]
    #[must_use]
    pub fn best_bid(&self) -> Option<PyPriceLevel> {
        self.inner.read().best_bid().cloned().map(Into::into)
    }

    /// Returns the best ask (lowest ask price).
    ///
    /// # Returns
    ///
    /// The best ask PyPriceLevel, or None if no asks.
    #[getter]
    #[must_use]
    pub fn best_ask(&self) -> Option<PyPriceLevel> {
        self.inner.read().best_ask().cloned().map(Into::into)
    }

    /// Returns the mid price ((best_bid + best_ask) / 2).
    ///
    /// # Returns
    ///
    /// The mid price, or None if either side is empty.
    #[getter]
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        self.inner.read().mid_price()
    }

    /// Returns the bid-ask spread (best_ask - best_bid).
    ///
    /// # Returns
    ///
    /// The spread, or None if either side is empty.
    #[getter]
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        self.inner.read().spread()
    }

    /// Returns the spread in basis points.
    ///
    /// # Returns
    ///
    /// The spread as (spread / mid_price) * 10000, or None.
    #[getter]
    #[must_use]
    pub fn spread_bps(&self) -> Option<f64> {
        self.inner.read().spread_bps()
    }

    /// Returns the number of bid levels.
    #[getter]
    #[must_use]
    pub fn bid_count(&self) -> usize {
        self.inner.read().bid_count()
    }

    /// Returns the number of ask levels.
    #[getter]
    #[must_use]
    pub fn ask_count(&self) -> usize {
        self.inner.read().ask_count()
    }

    /// Returns true if the book is empty (no bids or asks).
    #[getter]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Returns the instrument this book represents.
    #[getter]
    #[must_use]
    pub fn instrument(&self) -> PyInstrument {
        self.instrument_cache.clone()
    }

    /// Returns the order book statistics.
    #[getter]
    #[must_use]
    pub fn stats(&self) -> PyOrderBookStats {
        self.inner.read().stats().clone().into()
    }

    // =========================================================================
    // QUERY METHODS
    // =========================================================================

    /// Returns the top N bid levels (sorted by price descending).
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// List of PyPriceLevel, highest price first.
    #[must_use]
    pub fn top_bids(&self, n: usize) -> Vec<PyPriceLevel> {
        self.inner
            .read()
            .top_bids(n)
            .into_iter()
            .cloned()
            .map(Into::into)
            .collect()
    }

    /// Returns the top N ask levels (sorted by price ascending).
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// List of PyPriceLevel, lowest price first.
    #[must_use]
    pub fn top_asks(&self, n: usize) -> Vec<PyPriceLevel> {
        self.inner
            .read()
            .top_asks(n)
            .into_iter()
            .cloned()
            .map(Into::into)
            .collect()
    }

    /// Gets a specific level by price.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to search
    /// * `price` - The price to look up
    ///
    /// # Returns
    ///
    /// The PyPriceLevel if found, None otherwise.
    #[must_use]
    pub fn get_level(&self, side: PySide, price: f64) -> Option<PyPriceLevel> {
        let rust_side: Side = side.into();
        self.inner
            .read()
            .get_level(rust_side, price)
            .cloned()
            .map(Into::into)
    }

    /// Creates a snapshot of the order book.
    ///
    /// # Arguments
    ///
    /// * `depth` - Maximum number of levels per side
    ///
    /// # Returns
    ///
    /// A PyBookSnapshot suitable for serialization.
    #[must_use]
    pub fn snapshot(&self, depth: usize) -> PyBookSnapshot {
        self.inner.read().to_snapshot(depth).into()
    }

    /// Returns the total bid quantity as a string (preserves precision).
    #[must_use]
    pub fn total_bid_quantity(&self) -> String {
        self.inner.read().total_bid_quantity().to_string()
    }

    /// Returns the total ask quantity as a string (preserves precision).
    #[must_use]
    pub fn total_ask_quantity(&self) -> String {
        self.inner.read().total_ask_quantity().to_string()
    }

    /// Returns the imbalance ratio: (bid_qty - ask_qty) / (bid_qty + ask_qty).
    ///
    /// # Returns
    ///
    /// - Positive values indicate bid-heavy book
    /// - Negative values indicate ask-heavy book
    /// - Zero indicates balanced book
    /// - None if book is empty
    #[must_use]
    pub fn imbalance(&self) -> Option<f64> {
        self.inner.read().imbalance()
    }

    /// Returns true if the book has any bids.
    #[must_use]
    pub fn has_bids(&self) -> bool {
        self.inner.read().has_bids()
    }

    /// Returns true if the book has any asks.
    #[must_use]
    pub fn has_asks(&self) -> bool {
        self.inner.read().has_asks()
    }

    // =========================================================================
    // PYTHON MAGIC METHODS
    // =========================================================================

    fn __repr__(&self) -> String {
        let guard = self.inner.read();
        format!(
            "OrderBook({}, bids={}, asks={})",
            self.instrument_cache.raw_symbol(),
            guard.bid_count(),
            guard.ask_count()
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}:{}",
            self.instrument_cache.exchange().as_str(),
            self.instrument_cache.raw_symbol()
        )
    }

    /// Returns the total number of levels (bids + asks).
    fn __len__(&self) -> usize {
        let guard = self.inner.read();
        guard.bid_count() + guard.ask_count()
    }

    /// Returns True if the book is not empty.
    fn __bool__(&self) -> bool {
        !self.inner.read().is_empty()
    }
}

// =============================================================================
// CONVERSION METHODS (For Rust interop)
// =============================================================================

impl PyOrderBook {
    /// Creates a PyOrderBook from a Rust OrderBook.
    #[must_use]
    pub fn from_rust_orderbook(book: OrderBook) -> Self {
        let instrument: PyInstrument = book.instrument().clone().into();
        Self {
            inner: Arc::new(RwLock::new(book)),
            instrument_cache: instrument,
        }
    }

    /// Converts to a Rust OrderBook (clones the inner book).
    #[must_use]
    pub fn to_rust_orderbook(&self) -> OrderBook {
        // Clone the inner book to get an owned OrderBook
        let guard = self.inner.read();
        let snapshot = guard.to_snapshot(guard.config().max_levels);

        OrderBook::from_snapshot(
            guard.instrument().clone(),
            guard.config().clone(),
            snapshot.bids,
            snapshot.asks,
            snapshot.timestamp,
        )
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

// PyOrderBook is Send + Sync due to Arc<RwLock<>>
// This is verified by compile-time tests

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::types::PyExchange;

    fn test_instrument() -> PyInstrument {
        PyInstrument::new(
            "BTC".to_string(),
            "USD".to_string(),
            PyExchange::Deribit,
            "BTC-PERPETUAL".to_string(),
        )
    }

    fn test_price_level(price: f64, qty: &str) -> PyPriceLevel {
        PyPriceLevel::new(price, qty.to_string(), 1000, None)
    }

    #[test]
    fn test_pyorderbook_config_default() {
        let config = PyOrderBookConfig::default();
        assert_eq!(config.max_depth(), 50);
        assert_eq!(config.max_levels(), 100);
        assert!(!config.track_orders());
        assert!(config.auto_prune());
    }

    #[test]
    fn test_pyorderbook_new() {
        let book = PyOrderBook::new(test_instrument(), None);
        assert!(book.is_empty());
    }

    #[test]
    fn test_pyorderbook_apply_snapshot() {
        let book = PyOrderBook::new(test_instrument(), None);
        book.apply_snapshot(
            vec![test_price_level(100.0, "10")],
            vec![test_price_level(101.0, "5")],
            1000,
        );

        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
        assert!(!book.is_empty());
    }

    #[test]
    fn test_pyorderbook_mid_price() {
        let book = PyOrderBook::new(test_instrument(), None);
        book.apply_snapshot(
            vec![test_price_level(100.0, "10")],
            vec![test_price_level(102.0, "5")],
            1000,
        );

        let mid = book.mid_price().unwrap();
        assert!((mid - 101.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pyorderbook_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<PyOrderBook>();
        assert_sync::<PyOrderBook>();
    }
}
