//! Order Book implementation for Flash.
//!
//! This module provides a high-performance L2 order book with:
//!
//! - **O(log n) updates** via BTreeMap
//! - **O(1) best bid/ask** via cached values
//! - **Memory protection** via max_levels enforcement (RED TEAM requirement)
//! - **Precise arithmetic** via rust_decimal
//!
//! # Architecture
//!
//! The order book uses `BTreeMap<OrderedFloat<f64>, PriceLevel>` for sorted price
//! level storage. This provides:
//!
//! - Sorted access for best bid (highest) and best ask (lowest)
//! - Efficient range queries for top N levels
//! - Logarithmic insert/update/delete operations
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Single level update | < 1 μs |
//! | Full snapshot (50 levels) | < 50 μs |
//! | Best bid/ask lookup | < 10 ns |
//! | Mid price calculation | < 20 ns |
//!
//! # Example
//!
//! ```
//! use astra_flash::book::OrderBook;
//! use astra_flash::core::config::OrderBookConfig;
//! use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
//! use rust_decimal_macros::dec;
//!
//! // Create order book
//! let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
//! let mut book = OrderBook::new(instrument, OrderBookConfig::default());
//!
//! // Apply delta update
//! let level = PriceLevel::new(50000.0, dec!(1.5), now_micros());
//! book.apply_delta(Side::Bid, vec![level], now_micros());
//!
//! // Query best bid
//! if let Some(best) = book.best_bid() {
//!     println!("Best bid: {} @ {}", best.quantity, best.price);
//! }
//! ```

use crate::core::config::OrderBookConfig;
use crate::core::types::{Instrument, PriceLevel, Side, Timestamp};
use ordered_float::OrderedFloat;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::metrics::FLASH_ORDERBOOK_UPDATES_TOTAL;
use metrics::counter;

// BlackBox tap integration (optional feature)
#[cfg(feature = "blackbox")]
use crate::blackbox::{event_types, Tap, TapExt};
#[cfg(feature = "blackbox")]
use std::sync::Arc;

// =============================================================================
// STATISTICS
// =============================================================================

/// Statistics for order book monitoring.
///
/// Tracks update counts, level counts, and quantities for observability.
///
/// # Example
///
/// ```
/// use astra_flash::book::{OrderBook, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument};
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let book = OrderBook::new(instrument, OrderBookConfig::default());
///
/// let stats = book.stats();
/// println!("Updates: {}, Snapshots: {}", stats.update_count, stats.snapshot_count);
/// ```
#[derive(Debug, Clone, Default)]
pub struct OrderBookStats {
    /// Number of delta updates applied.
    pub update_count: u64,

    /// Number of snapshots applied.
    pub snapshot_count: u64,

    /// Timestamp of last update (microseconds).
    pub last_update_timestamp: Timestamp,

    /// Number of bid levels.
    pub bid_levels: usize,

    /// Number of ask levels.
    pub ask_levels: usize,
}

// =============================================================================
// BOOK SNAPSHOT
// =============================================================================

/// Serializable order book snapshot.
///
/// Used for publishing to Redis or other consumers. Contains a point-in-time
/// view of the order book up to `max_depth` levels.
///
/// # Example
///
/// ```
/// use astra_flash::book::{OrderBook, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
/// use rust_decimal_macros::dec;
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
///
/// // Populate book...
/// book.apply_snapshot(
///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
///     vec![PriceLevel::new(101.0, dec!(5), now_micros())],
///     now_micros(),
/// );
///
/// // Create snapshot for publishing
/// let snapshot = book.to_snapshot(10);
/// println!("Bids: {}, Asks: {}", snapshot.bids.len(), snapshot.asks.len());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSnapshot {
    /// Instrument this snapshot represents.
    pub instrument: Instrument,

    /// Timestamp of the snapshot.
    pub timestamp: Timestamp,

    /// Bid levels (sorted by price descending).
    pub bids: Vec<PriceLevel>,

    /// Ask levels (sorted by price ascending).
    pub asks: Vec<PriceLevel>,
}

// =============================================================================
// ORDER BOOK
// =============================================================================

/// L2 Order Book with O(log n) updates and O(1) best bid/ask.
///
/// # Design
///
/// Uses `BTreeMap<OrderedFloat<f64>, PriceLevel>` for sorted price levels:
///
/// - **Bids**: Stored in natural BTreeMap order, accessed via `iter().rev()` for descending
/// - **Asks**: Stored in natural BTreeMap order (ascending)
/// - **Best prices**: Cached in separate fields for O(1) access
///
/// # Thread Safety
///
/// This struct is NOT internally synchronized. For concurrent access,
/// wrap in `Arc<RwLock<OrderBook>>` (provided in Part 3.4).
///
/// # Memory Protection (RED TEAM)
///
/// The `max_levels` configuration enforces a hard limit on levels per side.
/// This prevents memory exhaustion from adversarial order placement.
///
/// # Example
///
/// ```
/// use astra_flash::book::{OrderBook, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
///
/// // Apply update
/// let level = PriceLevel::new(50000.0, dec!(1.5), now_micros());
/// book.apply_delta(Side::Bid, vec![level], now_micros());
///
/// // Query
/// if let Some(bid) = book.best_bid() {
///     println!("Best bid: {} @ {}", bid.quantity, bid.price);
/// }
/// ```
pub struct OrderBook {
    /// Instrument this book represents.
    instrument: Instrument,

    /// Configuration.
    config: OrderBookConfig,

    /// Bid levels: price -> level (BTreeMap sorts ascending, we access descending).
    bids: BTreeMap<OrderedFloat<f64>, PriceLevel>,

    /// Ask levels: price -> level (BTreeMap sorts ascending).
    asks: BTreeMap<OrderedFloat<f64>, PriceLevel>,

    /// Cached best bid for O(1) access.
    best_bid: Option<PriceLevel>,

    /// Cached best ask for O(1) access.
    best_ask: Option<PriceLevel>,

    /// Statistics.
    stats: OrderBookStats,

    /// Last sequence number for gap detection.
    last_sequence: Option<u64>,

    /// Cached total bid quantity.
    total_bid_qty: Decimal,

    /// Cached total ask quantity.
    total_ask_qty: Decimal,

    /// BlackBox tap for recording state changes (optional feature).
    #[cfg(feature = "blackbox")]
    tap: Option<Arc<dyn Tap>>,
}

impl std::fmt::Debug for OrderBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("OrderBook");
        debug_struct
            .field("instrument", &self.instrument)
            .field("config", &self.config)
            .field("bids", &self.bids)
            .field("asks", &self.asks)
            .field("best_bid", &self.best_bid)
            .field("best_ask", &self.best_ask)
            .field("stats", &self.stats)
            .field("last_sequence", &self.last_sequence)
            .field("total_bid_qty", &self.total_bid_qty)
            .field("total_ask_qty", &self.total_ask_qty);

        #[cfg(feature = "blackbox")]
        debug_struct.field("tap", &self.tap.as_ref().map(|_| "<Tap>"));

        debug_struct.finish()
    }
}

impl OrderBook {
    // =========================================================================
    // CONSTRUCTORS
    // =========================================================================

    /// Creates a new empty order book.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `config` - Configuration for book behavior
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{OrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = OrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// assert!(book.is_empty());
    /// ```
    #[must_use]
    pub fn new(instrument: Instrument, config: OrderBookConfig) -> Self {
        Self {
            instrument,
            config,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            best_bid: None,
            best_ask: None,
            stats: OrderBookStats::default(),
            last_sequence: None,
            total_bid_qty: Decimal::ZERO,
            total_ask_qty: Decimal::ZERO,
            #[cfg(feature = "blackbox")]
            tap: None,
        }
    }

    /// Creates a new order book with BlackBox tap for recording state changes.
    ///
    /// This constructor is only available when the `blackbox` feature is enabled.
    /// The tap will record all state changes (snapshots and deltas) to the
    /// configured journal for later replay.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `config` - Configuration for book behavior
    /// * `tap` - BlackBox tap for recording events
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use astra_flash::blackbox::{JournalTap, Tap};
    /// use astra_flash::book::OrderBook;
    ///
    /// let journal_tap = JournalTap::new(writer);
    /// let book = OrderBook::with_tap(instrument, config, journal_tap);
    /// ```
    #[cfg(feature = "blackbox")]
    #[must_use]
    pub fn with_tap<T: Tap + 'static>(
        instrument: Instrument,
        config: OrderBookConfig,
        tap: T,
    ) -> Self {
        Self {
            instrument,
            config,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            best_bid: None,
            best_ask: None,
            stats: OrderBookStats::default(),
            last_sequence: None,
            total_bid_qty: Decimal::ZERO,
            total_ask_qty: Decimal::ZERO,
            tap: Some(Arc::new(tap)),
        }
    }

    /// Creates a new order book with default configuration.
    ///
    /// Convenience constructor using `OrderBookConfig::default()`.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::OrderBook;
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("ETH", "USD", Exchange::Binance, "ETHUSDT");
    /// let book = OrderBook::with_instrument(instrument);
    ///
    /// assert_eq!(book.config().max_depth, 50);
    /// ```
    #[must_use]
    pub fn with_instrument(instrument: Instrument) -> Self {
        Self::new(instrument, OrderBookConfig::default())
    }

    /// Creates an order book from a snapshot.
    ///
    /// Convenience constructor that immediately applies a snapshot.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `config` - Configuration for book behavior
    /// * `bids` - Initial bid levels
    /// * `asks` - Initial ask levels
    /// * `timestamp` - Timestamp of the snapshot
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{OrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = OrderBook::from_snapshot(
    ///     instrument,
    ///     OrderBookConfig::default(),
    ///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///     vec![PriceLevel::new(101.0, dec!(5), now_micros())],
    ///     now_micros(),
    /// );
    ///
    /// assert!(!book.is_empty());
    /// ```
    #[must_use]
    pub fn from_snapshot(
        instrument: Instrument,
        config: OrderBookConfig,
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        timestamp: Timestamp,
    ) -> Self {
        let mut book = Self::new(instrument, config);
        book.apply_snapshot(bids, asks, timestamp);
        book
    }

    // =========================================================================
    // CORE OPERATIONS
    // =========================================================================

    /// Applies a full snapshot, replacing all existing data.
    ///
    /// # Arguments
    ///
    /// * `bids` - New bid levels (replaces existing bids)
    /// * `asks` - New ask levels (replaces existing asks)
    /// * `timestamp` - Timestamp of the snapshot
    ///
    /// # Performance
    ///
    /// O(n log n) where n = total number of levels.
    ///
    /// # Behavior
    ///
    /// - Clears all existing levels
    /// - Inserts new levels (respecting max_levels)
    /// - Auto-prunes zero-quantity levels if enabled
    /// - Updates caches and statistics
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{OrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// book.apply_snapshot(
    ///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///     vec![PriceLevel::new(101.0, dec!(5), now_micros())],
    ///     now_micros(),
    /// );
    ///
    /// assert_eq!(book.bid_count(), 1);
    /// assert_eq!(book.ask_count(), 1);
    /// ```
    pub fn apply_snapshot(
        &mut self,
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        timestamp: Timestamp,
    ) {
        // Clear existing data
        self.bids.clear();
        self.asks.clear();
        self.total_bid_qty = Decimal::ZERO;
        self.total_ask_qty = Decimal::ZERO;

        // Insert bids (enforce max_levels HARD LIMIT)
        // RED TEAM: This prevents dust attack via snapshot
        for level in bids.into_iter().take(self.config.max_levels) {
            if level.quantity > Decimal::ZERO || !self.config.auto_prune {
                self.total_bid_qty += level.quantity;
                self.bids.insert(OrderedFloat(level.price), level);
            }
        }

        // Insert asks (enforce max_levels HARD LIMIT)
        for level in asks.into_iter().take(self.config.max_levels) {
            if level.quantity > Decimal::ZERO || !self.config.auto_prune {
                self.total_ask_qty += level.quantity;
                self.asks.insert(OrderedFloat(level.price), level);
            }
        }

        // Update caches
        self.update_best_bid_cache();
        self.update_best_ask_cache();

        // Update stats
        self.stats.snapshot_count += 1;
        self.stats.last_update_timestamp = timestamp;
        self.stats.bid_levels = self.bids.len();
        self.stats.ask_levels = self.asks.len();

        // TAP-2 (Internal): Record snapshot to BlackBox journal
        #[cfg(feature = "blackbox")]
        if let Some(ref tap) = self.tap {
            if tap.is_active() {
                // Serialize current state for replay
                let snapshot = self.to_snapshot(self.config.max_depth);
                if let Ok(payload) = bincode::serialize(&snapshot) {
                    tap.record_flash_internal(event_types::BOOK_SNAPSHOT, &payload, timestamp);
                }
            }
        }
    }

    /// Applies an incremental update (delta) to one side of the book.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to update (Bid or Ask)
    /// * `levels` - Price levels to insert, update, or delete
    /// * `timestamp` - Timestamp of the update
    ///
    /// # Performance
    ///
    /// O(k log n) where k = number of levels in update, n = book depth.
    ///
    /// # Behavior
    ///
    /// - `quantity > 0`: Insert or update the level
    /// - `quantity = 0`: Remove the level (if auto_prune enabled)
    /// - Enforces max_levels by dropping worst-priced levels
    ///
    /// # RED TEAM: max_levels Enforcement
    ///
    /// After applying updates, if the book exceeds max_levels, worst-priced
    /// levels are dropped:
    /// - For bids: lowest prices dropped (worst bids)
    /// - For asks: highest prices dropped (worst asks)
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{OrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// // Insert new level
    /// book.apply_delta(
    ///     Side::Bid,
    ///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///     now_micros(),
    /// );
    ///
    /// // Remove level (zero quantity)
    /// book.apply_delta(
    ///     Side::Bid,
    ///     vec![PriceLevel::new(100.0, dec!(0), now_micros())],
    ///     now_micros(),
    /// );
    /// ```
    pub fn apply_delta(&mut self, side: Side, levels: Vec<PriceLevel>, timestamp: Timestamp) {
        let (book_side, total_qty) = match side {
            Side::Bid => (&mut self.bids, &mut self.total_bid_qty),
            Side::Ask => (&mut self.asks, &mut self.total_ask_qty),
        };

        for level in levels {
            let price_key = OrderedFloat(level.price);

            if level.quantity > Decimal::ZERO {
                // Insert or update: adjust total quantity
                if let Some(existing) = book_side.get(&price_key) {
                    *total_qty -= existing.quantity;
                }
                *total_qty += level.quantity;
                book_side.insert(price_key, level);
            } else if self.config.auto_prune {
                // Remove level with zero quantity
                if let Some(existing) = book_side.remove(&price_key) {
                    *total_qty -= existing.quantity;
                }
            } else {
                // auto_prune disabled: update with zero quantity
                if let Some(existing) = book_side.get(&price_key) {
                    *total_qty -= existing.quantity;
                }
                book_side.insert(price_key, level);
            }
        }

        // Enforce max_levels HARD LIMIT (remove worst levels)
        // RED TEAM: This prevents "dust attacks"
        while book_side.len() > self.config.max_levels {
            match side {
                Side::Bid => {
                    // Remove lowest bid (worst)
                    if let Some((_, removed)) = book_side.pop_first() {
                        *total_qty -= removed.quantity;
                    }
                },
                Side::Ask => {
                    // Remove highest ask (worst)
                    if let Some((_, removed)) = book_side.pop_last() {
                        *total_qty -= removed.quantity;
                    }
                },
            }
        }

        // Update caches
        match side {
            Side::Bid => self.update_best_bid_cache(),
            Side::Ask => self.update_best_ask_cache(),
        }

        // Update stats
        self.stats.update_count += 1;
        counter!(FLASH_ORDERBOOK_UPDATES_TOTAL).increment(1);
        self.stats.last_update_timestamp = timestamp;
        self.stats.bid_levels = self.bids.len();
        self.stats.ask_levels = self.asks.len();

        // TAP-2 (Internal): Record delta to BlackBox journal
        // Note: Records final state after delta applied (same as snapshot format)
        #[cfg(feature = "blackbox")]
        if let Some(ref tap) = self.tap {
            if tap.is_active() {
                // Serialize current state for replay
                let snapshot = self.to_snapshot(self.config.max_depth);
                if let Ok(payload) = bincode::serialize(&snapshot) {
                    tap.record_flash_internal(event_types::BOOK_DELTA, &payload, timestamp);
                }
            }
        }
    }

    /// Updates a single price level.
    ///
    /// Convenience method for single-level updates.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to update
    /// * `level` - The price level to insert/update
    ///
    /// # Performance
    ///
    /// O(log n).
    pub fn update_level(&mut self, side: Side, level: PriceLevel) {
        let ts = level.timestamp;
        self.apply_delta(side, vec![level], ts);
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
    /// The removed level if it existed, `None` otherwise.
    ///
    /// # Performance
    ///
    /// O(log n).
    pub fn remove_level(&mut self, side: Side, price: f64) -> Option<PriceLevel> {
        let price_key = OrderedFloat(price);

        let (book_side, total_qty) = match side {
            Side::Bid => (&mut self.bids, &mut self.total_bid_qty),
            Side::Ask => (&mut self.asks, &mut self.total_ask_qty),
        };

        let removed = book_side.remove(&price_key);

        if let Some(ref level) = removed {
            *total_qty -= level.quantity;
        }

        // Update caches
        match side {
            Side::Bid => self.update_best_bid_cache(),
            Side::Ask => self.update_best_ask_cache(),
        }

        // Update stats
        self.stats.bid_levels = self.bids.len();
        self.stats.ask_levels = self.asks.len();

        removed
    }

    /// Clears all levels from the book.
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.best_bid = None;
        self.best_ask = None;
        self.total_bid_qty = Decimal::ZERO;
        self.total_ask_qty = Decimal::ZERO;
        self.stats.bid_levels = 0;
        self.stats.ask_levels = 0;
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Gets the best bid (highest bid price).
    ///
    /// # Returns
    ///
    /// Reference to the best bid level, or `None` if no bids exist.
    ///
    /// # Performance
    ///
    /// O(1) - uses cached value.
    #[inline]
    #[must_use]
    pub const fn best_bid(&self) -> Option<&PriceLevel> {
        self.best_bid.as_ref()
    }

    /// Gets the best ask (lowest ask price).
    ///
    /// # Returns
    ///
    /// Reference to the best ask level, or `None` if no asks exist.
    ///
    /// # Performance
    ///
    /// O(1) - uses cached value.
    #[inline]
    #[must_use]
    pub const fn best_ask(&self) -> Option<&PriceLevel> {
        self.best_ask.as_ref()
    }

    /// Gets the mid price (average of best bid and ask).
    ///
    /// # Returns
    ///
    /// The mid price, or `None` if either side is empty.
    ///
    /// # Performance
    ///
    /// O(1).
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        match (&self.best_bid, &self.best_ask) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2.0),
            _ => None,
        }
    }

    /// Gets the bid-ask spread.
    ///
    /// # Returns
    ///
    /// The spread (ask - bid), or `None` if either side is empty.
    ///
    /// # Performance
    ///
    /// O(1).
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (&self.best_bid, &self.best_ask) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    /// Gets the bid-ask spread in basis points.
    ///
    /// # Returns
    ///
    /// The spread as a percentage of the mid price × 10000, or `None`.
    ///
    /// # Performance
    ///
    /// O(1).
    #[must_use]
    pub fn spread_bps(&self) -> Option<f64> {
        match (self.spread(), self.mid_price()) {
            (Some(spread), Some(mid)) if mid > 0.0 => Some((spread / mid) * 10000.0),
            _ => None,
        }
    }

    /// Gets the top N bid levels (sorted by price descending).
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// Vector of references to bid levels, highest price first.
    ///
    /// # Performance
    ///
    /// O(N) where N = min(n, bid_count).
    #[must_use]
    pub fn top_bids(&self, n: usize) -> Vec<&PriceLevel> {
        self.bids.values().rev().take(n).collect()
    }

    /// Gets the top N ask levels (sorted by price ascending).
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// Vector of references to ask levels, lowest price first.
    ///
    /// # Performance
    ///
    /// O(N) where N = min(n, ask_count).
    #[must_use]
    pub fn top_asks(&self, n: usize) -> Vec<&PriceLevel> {
        self.asks.values().take(n).collect()
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
    /// Reference to the level if found, `None` otherwise.
    ///
    /// # Performance
    ///
    /// O(log n).
    #[must_use]
    pub fn get_level(&self, side: Side, price: f64) -> Option<&PriceLevel> {
        let price_key = OrderedFloat(price);
        match side {
            Side::Bid => self.bids.get(&price_key),
            Side::Ask => self.asks.get(&price_key),
        }
    }

    /// Checks if the book has any bids.
    #[inline]
    #[must_use]
    pub fn has_bids(&self) -> bool {
        !self.bids.is_empty()
    }

    /// Checks if the book has any asks.
    #[inline]
    #[must_use]
    pub fn has_asks(&self) -> bool {
        !self.asks.is_empty()
    }

    /// Checks if the book is empty (no bids or asks).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    /// Gets the total bid quantity.
    ///
    /// # Performance
    ///
    /// O(1) - uses cached value.
    #[inline]
    #[must_use]
    pub const fn total_bid_quantity(&self) -> Decimal {
        self.total_bid_qty
    }

    /// Gets the total ask quantity.
    ///
    /// # Performance
    ///
    /// O(1) - uses cached value.
    #[inline]
    #[must_use]
    pub const fn total_ask_quantity(&self) -> Decimal {
        self.total_ask_qty
    }

    /// Gets the imbalance ratio: (bid_qty - ask_qty) / (bid_qty + ask_qty).
    ///
    /// # Returns
    ///
    /// - Positive values indicate bid-heavy book
    /// - Negative values indicate ask-heavy book
    /// - Zero indicates balanced book
    /// - `None` if book is empty
    ///
    /// # Performance
    ///
    /// O(1).
    #[must_use]
    pub fn imbalance(&self) -> Option<f64> {
        let total = self.total_bid_qty + self.total_ask_qty;
        if total.is_zero() {
            return None;
        }

        let diff = self.total_bid_qty - self.total_ask_qty;
        // Convert Decimal to f64 for the ratio
        let diff_f64 = diff.to_string().parse::<f64>().unwrap_or(0.0);
        let total_f64 = total.to_string().parse::<f64>().unwrap_or(1.0);

        Some(diff_f64 / total_f64)
    }

    // =========================================================================
    // ACCESSORS
    // =========================================================================

    /// Gets the instrument.
    #[inline]
    #[must_use]
    pub const fn instrument(&self) -> &Instrument {
        &self.instrument
    }

    /// Gets the configuration.
    #[inline]
    #[must_use]
    pub const fn config(&self) -> &OrderBookConfig {
        &self.config
    }

    /// Gets the statistics.
    #[inline]
    #[must_use]
    pub const fn stats(&self) -> &OrderBookStats {
        &self.stats
    }

    /// Gets the last sequence number.
    #[inline]
    #[must_use]
    pub const fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// Sets the last sequence number.
    #[inline]
    pub fn set_last_sequence(&mut self, sequence: u64) {
        self.last_sequence = Some(sequence);
    }

    /// Gets the number of bid levels.
    #[inline]
    #[must_use]
    pub fn bid_count(&self) -> usize {
        self.bids.len()
    }

    /// Gets the number of ask levels.
    #[inline]
    #[must_use]
    pub fn ask_count(&self) -> usize {
        self.asks.len()
    }

    // =========================================================================
    // SERIALIZATION
    // =========================================================================

    /// Creates a snapshot for serialization.
    ///
    /// # Arguments
    ///
    /// * `depth` - Maximum number of levels per side to include
    ///
    /// # Returns
    ///
    /// A `BookSnapshot` suitable for publishing to Redis.
    #[must_use]
    pub fn to_snapshot(&self, depth: usize) -> BookSnapshot {
        BookSnapshot {
            instrument: self.instrument.clone(),
            timestamp: self.stats.last_update_timestamp,
            bids: self.top_bids(depth).into_iter().cloned().collect(),
            asks: self.top_asks(depth).into_iter().cloned().collect(),
        }
    }

    /// Gets all bids as a vector (sorted by price descending).
    #[must_use]
    pub fn all_bids(&self) -> Vec<&PriceLevel> {
        self.bids.values().rev().collect()
    }

    /// Gets all asks as a vector (sorted by price ascending).
    #[must_use]
    pub fn all_asks(&self) -> Vec<&PriceLevel> {
        self.asks.values().collect()
    }

    // =========================================================================
    // PRIVATE METHODS
    // =========================================================================

    /// Updates the cached best bid.
    fn update_best_bid_cache(&mut self) {
        // Best bid is the highest price (last in BTreeMap order)
        self.best_bid = self.bids.values().next_back().cloned();
    }

    /// Updates the cached best ask.
    fn update_best_ask_cache(&mut self) {
        // Best ask is the lowest price (first in BTreeMap order)
        self.best_ask = self.asks.values().next().cloned();
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

// OrderBook is Send + Sync because all its fields are Send + Sync.
// This is verified by compile-time tests in orderbook_test.rs.

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::Exchange;
    use rust_decimal_macros::dec;

    fn test_instrument() -> Instrument {
        Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
    }

    #[test]
    fn test_orderbook_config_default() {
        let config = OrderBookConfig::default();
        assert_eq!(config.max_depth, 50);
        assert_eq!(config.max_levels, 100);
        assert!(!config.track_orders);
        assert!(config.auto_prune);
    }

    #[test]
    fn test_orderbook_new() {
        let book = OrderBook::new(test_instrument(), OrderBookConfig::default());
        assert!(book.is_empty());
        assert_eq!(book.bid_count(), 0);
        assert_eq!(book.ask_count(), 0);
    }

    #[test]
    fn test_orderbook_apply_delta() {
        let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());

        let level = PriceLevel::new(100.0, dec!(10), 1234567890);
        book.apply_delta(Side::Bid, vec![level], 1234567890);

        assert_eq!(book.bid_count(), 1);
        assert!((book.best_bid().unwrap().price - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_orderbook_mid_price() {
        let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
        book.apply_delta(Side::Bid, vec![PriceLevel::new(100.0, dec!(10), 0)], 0);
        book.apply_delta(Side::Ask, vec![PriceLevel::new(102.0, dec!(5), 0)], 0);

        let mid = book.mid_price().unwrap();
        assert!((mid - 101.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_orderbook_spread() {
        let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
        book.apply_delta(Side::Bid, vec![PriceLevel::new(100.0, dec!(10), 0)], 0);
        book.apply_delta(Side::Ask, vec![PriceLevel::new(102.0, dec!(5), 0)], 0);

        let spread = book.spread().unwrap();
        assert!((spread - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_orderbook_max_levels_enforcement() {
        let config = OrderBookConfig {
            max_levels: 3,
            ..OrderBookConfig::default()
        };
        let mut book = OrderBook::new(test_instrument(), config);

        // Insert 5 levels
        for i in 0..5 {
            book.apply_delta(
                Side::Bid,
                vec![PriceLevel::new(100.0 - (i as f64), dec!(1), 0)],
                0,
            );
        }

        // Should be capped at 3
        assert_eq!(book.bid_count(), 3);
    }
}
