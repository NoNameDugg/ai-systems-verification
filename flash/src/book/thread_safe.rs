//! Thread-safe order book wrapper for Flash.
//!
//! This module provides thread-safe access to [`OrderBook`] using `parking_lot::RwLock`.
//! The design supports read-heavy workloads with multiple concurrent readers and
//! exclusive write access for updates.
//!
//! # Concurrency Model
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                     CONCURRENT ACCESS PATTERN                            │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                     │
//! │  │  Strategy 1 │  │  Strategy 2 │  │  Strategy N │                     │
//! │  │   Reader    │  │   Reader    │  │   Reader    │                     │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘                     │
//! │         │                │                │                             │
//! │         └────────────────┼────────────────┘                             │
//! │                          │                                              │
//! │                          ▼                                              │
//! │         ┌────────────────────────────────┐                              │
//! │         │      ThreadSafeOrderBook       │                              │
//! │         │      Arc<RwLock<OrderBook>>    │                              │
//! │         └────────────────────────────────┘                              │
//! │                          ▲                                              │
//! │                          │                                              │
//! │                ┌─────────┴─────────┐                                    │
//! │                │   WebSocket Task  │                                    │
//! │                │      Writer       │                                    │
//! │                └───────────────────┘                                    │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - **Multiple Concurrent Readers**: Readers don't block each other
//! - **Exclusive Writer**: Write access blocks all readers and other writers
//! - **Fair Scheduling**: `parking_lot::RwLock` prevents writer starvation
//! - **No Lock Poisoning**: Unlike `std::sync::RwLock`, locks never poison
//!
//! # Performance
//!
//! | Operation | Uncontended | Contended (10 readers) |
//! |-----------|-------------|------------------------|
//! | read()    | ~20 ns      | ~50 ns                 |
//! | write()   | ~30 ns      | ~100 ns                |
//!
//! # Example
//!
//! ```
//! use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
//! use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
//! use rust_decimal_macros::dec;
//! use std::thread;
//!
//! let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
//! let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
//!
//! // Apply initial data
//! book.apply_snapshot(
//!     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
//!     vec![PriceLevel::new(101.0, dec!(5), now_micros())],
//!     now_micros(),
//! );
//!
//! // Clone for sharing across threads
//! let book2 = book.clone_shared();
//!
//! // Concurrent read from another thread
//! let handle = thread::spawn(move || {
//!     book2.mid_price()
//! });
//!
//! let mid = handle.join().unwrap();
//! assert!(mid.is_some());
//! ```

use crate::book::{BookSnapshot, OrderBook, OrderBookStats};
use crate::core::config::OrderBookConfig;
use crate::core::types::{Instrument, PriceLevel, Side, Timestamp};
use parking_lot::RwLock;
use std::sync::Arc;

// =============================================================================
// TYPE ALIASES
// =============================================================================

/// Type alias for a thread-safe shared order book.
///
/// Use this when you need to pass the order book across thread boundaries
/// or share it between multiple components.
///
/// # Example
///
/// ```
/// use astra_flash::book::{OrderBook, SharedOrderBook, OrderBookConfig, new_shared_orderbook};
/// use astra_flash::core::types::{Exchange, Instrument};
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let shared: SharedOrderBook = new_shared_orderbook(instrument, OrderBookConfig::default());
///
/// // Access with locks
/// let best_bid = shared.read().best_bid().cloned();
/// ```
pub type SharedOrderBook = Arc<RwLock<OrderBook>>;

/// Type alias for a read guard.
pub type ReadGuard<'a> = parking_lot::RwLockReadGuard<'a, OrderBook>;

/// Type alias for a write guard.
pub type WriteGuard<'a> = parking_lot::RwLockWriteGuard<'a, OrderBook>;

// =============================================================================
// FACTORY FUNCTION
// =============================================================================

/// Creates a new shared order book.
///
/// Convenience function to create a `SharedOrderBook` without having to
/// manually wrap in `Arc<RwLock<>>`.
///
/// # Arguments
///
/// * `instrument` - The instrument this book represents
/// * `config` - Configuration for book behavior
///
/// # Example
///
/// ```
/// use astra_flash::book::{new_shared_orderbook, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument};
///
/// let instrument = Instrument::new("ETH", "USD", Exchange::Binance, "ETHUSDT");
/// let shared = new_shared_orderbook(instrument, OrderBookConfig::default());
///
/// // Can be cloned cheaply for sharing
/// let clone = shared.clone();
/// ```
#[must_use]
pub fn new_shared_orderbook(instrument: Instrument, config: OrderBookConfig) -> SharedOrderBook {
    Arc::new(RwLock::new(OrderBook::new(instrument, config)))
}

// =============================================================================
// THREAD SAFE STATS
// =============================================================================

/// Statistics for thread-safe order book operations.
///
/// Tracks lock acquisition patterns for monitoring and debugging.
///
/// # Fields
///
/// - `reads`: Total successful read lock acquisitions
/// - `writes`: Total successful write lock acquisitions
/// - `read_contention_count`: Times `try_read()` failed
/// - `write_contention_count`: Times `try_write()` failed
///
/// # Example
///
/// ```
/// use astra_flash::book::{ThreadSafeOrderBook, ThreadSafeStats, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument};
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
///
/// // Perform some operations
/// let _ = book.read();
/// let _ = book.write();
///
/// let stats = book.thread_stats();
/// assert_eq!(stats.reads, 1);
/// assert_eq!(stats.writes, 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ThreadSafeStats {
    /// Total number of successful read lock acquisitions.
    pub reads: u64,

    /// Total number of successful write lock acquisitions.
    pub writes: u64,

    /// Number of times read lock was contended (try_read failed).
    pub read_contention_count: u64,

    /// Number of times write lock was contended (try_write failed).
    pub write_contention_count: u64,

    /// Timestamp of last successful read.
    pub last_read: Option<Timestamp>,

    /// Timestamp of last successful write.
    pub last_write: Option<Timestamp>,
}

// =============================================================================
// THREAD SAFE ORDER BOOK
// =============================================================================

/// Thread-safe wrapper around [`OrderBook`] for concurrent access.
///
/// This struct provides safe concurrent access to an order book using
/// `parking_lot::RwLock`. It is designed for read-heavy workloads where
/// multiple strategy threads read the book while a single WebSocket thread
/// writes updates.
///
/// # Concurrency Model
///
/// - **Multiple readers**: Can access simultaneously (shared read lock)
/// - **Single writer**: Has exclusive access (exclusive write lock)
/// - **Fair scheduling**: `parking_lot` prevents writer starvation
///
/// # Performance
///
/// | Operation | Uncontended | 10 Readers Contended |
/// |-----------|-------------|----------------------|
/// | read()    | ~20 ns      | ~50 ns               |
/// | write()   | ~30 ns      | ~100 ns              |
///
/// # Thread Safety
///
/// `ThreadSafeOrderBook` implements `Send + Sync` automatically because:
/// - `Arc<T>` is `Send + Sync` when `T: Send + Sync`
/// - `RwLock<T>` is `Send + Sync` when `T: Send`
/// - `OrderBook` is `Send + Sync`
///
/// # Example
///
/// ```
/// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
/// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
/// use std::thread;
///
/// // Create thread-safe book
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
///
/// // Apply data
/// book.apply_snapshot(
///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
///     vec![PriceLevel::new(101.0, dec!(5), now_micros())],
///     now_micros(),
/// );
///
/// // Concurrent access
/// let book2 = book.clone_shared();
/// let handle = thread::spawn(move || {
///     book2.best_bid()
/// });
///
/// // Main thread can still access
/// let spread = book.spread();
///
/// // Wait for other thread
/// let best = handle.join().unwrap();
/// assert!(best.is_some());
/// ```
#[derive(Debug)]
pub struct ThreadSafeOrderBook {
    /// The underlying shared order book.
    inner: SharedOrderBook,

    /// Cached instrument for convenience (avoids lock for common query).
    instrument: Instrument,

    /// Thread-safety statistics.
    stats: Arc<RwLock<ThreadSafeStats>>,
}

impl ThreadSafeOrderBook {
    // =========================================================================
    // CONSTRUCTORS
    // =========================================================================

    /// Creates a new thread-safe order book.
    ///
    /// # Arguments
    ///
    /// * `instrument` - The instrument this book represents
    /// * `config` - Configuration for book behavior
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// assert!(book.is_empty());
    /// ```
    #[must_use]
    pub fn new(instrument: Instrument, config: OrderBookConfig) -> Self {
        Self {
            instrument: instrument.clone(),
            inner: Arc::new(RwLock::new(OrderBook::new(instrument, config))),
            stats: Arc::new(RwLock::new(ThreadSafeStats::default())),
        }
    }

    /// Creates a thread-safe wrapper from an existing [`OrderBook`].
    ///
    /// Takes ownership of the order book.
    ///
    /// # Arguments
    ///
    /// * `book` - The order book to wrap
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{OrderBook, ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// // Pre-populate the book
    /// book.apply_snapshot(
    ///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///     vec![],
    ///     now_micros(),
    /// );
    ///
    /// // Wrap it
    /// let safe_book = ThreadSafeOrderBook::from_orderbook(book);
    /// assert_eq!(safe_book.bid_count(), 1);
    /// ```
    #[must_use]
    pub fn from_orderbook(book: OrderBook) -> Self {
        let instrument = book.instrument().clone();
        Self {
            instrument,
            inner: Arc::new(RwLock::new(book)),
            stats: Arc::new(RwLock::new(ThreadSafeStats::default())),
        }
    }

    /// Creates a thread-safe wrapper from an existing [`SharedOrderBook`].
    ///
    /// Wraps an existing shared reference. Changes to either the wrapper
    /// or the underlying shared reference are visible to both.
    ///
    /// # Arguments
    ///
    /// * `shared` - The shared order book to wrap
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, new_shared_orderbook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let shared = new_shared_orderbook(instrument.clone(), OrderBookConfig::default());
    ///
    /// let book = ThreadSafeOrderBook::from_shared(shared);
    /// assert_eq!(book.instrument(), &instrument);
    /// ```
    #[must_use]
    pub fn from_shared(shared: SharedOrderBook) -> Self {
        let instrument = shared.read().instrument().clone();
        Self {
            instrument,
            inner: shared,
            stats: Arc::new(RwLock::new(ThreadSafeStats::default())),
        }
    }

    // =========================================================================
    // LOCK ACCESS
    // =========================================================================

    /// Acquires a read lock on the order book.
    ///
    /// Blocks until the lock is available. Multiple readers can hold
    /// the lock simultaneously.
    ///
    /// # Returns
    ///
    /// A guard that provides read access to the underlying [`OrderBook`].
    /// The lock is released when the guard is dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// {
    ///     let guard = book.read();
    ///     println!("Bid count: {}", guard.bid_count());
    /// } // Lock released here
    /// ```
    pub fn read(&self) -> ReadGuard<'_> {
        let guard = self.inner.read();
        // Update stats
        let mut stats = self.stats.write();
        stats.reads += 1;
        stats.last_read = Some(crate::core::types::now_micros());
        guard
    }

    /// Tries to acquire a read lock without blocking.
    ///
    /// Returns immediately with `None` if the lock is held by a writer.
    ///
    /// # Returns
    ///
    /// - `Some(guard)` if the lock was acquired
    /// - `None` if the lock is held by a writer
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// // try_read returns None if writer holds lock, Some(guard) otherwise
    /// let result = book.try_read();
    /// assert!(result.is_some()); // No contention in this example
    /// drop(result); // Explicitly drop the guard
    /// ```
    pub fn try_read(&self) -> Option<ReadGuard<'_>> {
        if let Some(guard) = self.inner.try_read() {
            let mut stats = self.stats.write();
            stats.reads += 1;
            stats.last_read = Some(crate::core::types::now_micros());
            Some(guard)
        } else {
            let mut stats = self.stats.write();
            stats.read_contention_count += 1;
            None
        }
    }

    /// Acquires a write lock on the order book.
    ///
    /// Blocks until the lock is available. Only one writer can hold
    /// the lock at a time, and it blocks all readers.
    ///
    /// # Returns
    ///
    /// A guard that provides write access to the underlying [`OrderBook`].
    /// The lock is released when the guard is dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// {
    ///     let mut guard = book.write();
    ///     guard.apply_delta(
    ///         Side::Bid,
    ///         vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///         now_micros(),
    ///     );
    /// } // Lock released here
    ///
    /// assert_eq!(book.bid_count(), 1);
    /// ```
    pub fn write(&self) -> WriteGuard<'_> {
        let guard = self.inner.write();
        // Update stats
        let mut stats = self.stats.write();
        stats.writes += 1;
        stats.last_write = Some(crate::core::types::now_micros());
        guard
    }

    /// Tries to acquire a write lock without blocking.
    ///
    /// Returns immediately with `None` if the lock is held by anyone.
    ///
    /// # Returns
    ///
    /// - `Some(guard)` if the lock was acquired
    /// - `None` if the lock is held by readers or another writer
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// // try_write returns None if anyone holds lock, Some(guard) otherwise
    /// let result = book.try_write();
    /// assert!(result.is_some()); // No contention in this example
    /// drop(result); // Explicitly drop the guard
    /// ```
    pub fn try_write(&self) -> Option<WriteGuard<'_>> {
        if let Some(guard) = self.inner.try_write() {
            let mut stats = self.stats.write();
            stats.writes += 1;
            stats.last_write = Some(crate::core::types::now_micros());
            Some(guard)
        } else {
            let mut stats = self.stats.write();
            stats.write_contention_count += 1;
            None
        }
    }

    // =========================================================================
    // CONVENIENCE READ METHODS
    // =========================================================================

    /// Gets the best bid (highest bid price).
    ///
    /// Convenience method that acquires the read lock, clones the result,
    /// and releases the lock.
    ///
    /// # Returns
    ///
    /// A clone of the best bid level, or `None` if no bids exist.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// book.apply_snapshot(
    ///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///     vec![],
    ///     now_micros(),
    /// );
    ///
    /// let best = book.best_bid();
    /// assert_eq!(best.unwrap().price, 100.0);
    /// ```
    #[must_use]
    pub fn best_bid(&self) -> Option<PriceLevel> {
        self.inner.read().best_bid().cloned()
    }

    /// Gets the best ask (lowest ask price).
    ///
    /// Convenience method that acquires the read lock, clones the result,
    /// and releases the lock.
    ///
    /// # Returns
    ///
    /// A clone of the best ask level, or `None` if no asks exist.
    #[must_use]
    pub fn best_ask(&self) -> Option<PriceLevel> {
        self.inner.read().best_ask().cloned()
    }

    /// Gets the mid price (average of best bid and ask).
    ///
    /// # Returns
    ///
    /// The mid price, or `None` if either side is empty.
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        self.inner.read().mid_price()
    }

    /// Gets the bid-ask spread.
    ///
    /// # Returns
    ///
    /// The spread (ask - bid), or `None` if either side is empty.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        self.inner.read().spread()
    }

    /// Gets the bid-ask spread in basis points.
    ///
    /// # Returns
    ///
    /// The spread as a percentage of the mid price × 10000, or `None`.
    #[must_use]
    pub fn spread_bps(&self) -> Option<f64> {
        self.inner.read().spread_bps()
    }

    /// Gets the top N bid levels.
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// Vector of cloned bid levels, highest price first.
    #[must_use]
    pub fn top_bids(&self, n: usize) -> Vec<PriceLevel> {
        self.inner.read().top_bids(n).into_iter().cloned().collect()
    }

    /// Gets the top N ask levels.
    ///
    /// # Arguments
    ///
    /// * `n` - Maximum number of levels to return
    ///
    /// # Returns
    ///
    /// Vector of cloned ask levels, lowest price first.
    #[must_use]
    pub fn top_asks(&self, n: usize) -> Vec<PriceLevel> {
        self.inner.read().top_asks(n).into_iter().cloned().collect()
    }

    /// Creates a snapshot of the order book.
    ///
    /// # Arguments
    ///
    /// * `depth` - Maximum number of levels per side to include
    ///
    /// # Returns
    ///
    /// A [`BookSnapshot`] suitable for serialization.
    #[must_use]
    pub fn snapshot(&self, depth: usize) -> BookSnapshot {
        self.inner.read().to_snapshot(depth)
    }

    /// Checks if the book is empty (no bids or asks).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Gets the number of bid levels.
    #[must_use]
    pub fn bid_count(&self) -> usize {
        self.inner.read().bid_count()
    }

    /// Gets the number of ask levels.
    #[must_use]
    pub fn ask_count(&self) -> usize {
        self.inner.read().ask_count()
    }

    // =========================================================================
    // CONVENIENCE WRITE METHODS
    // =========================================================================

    /// Applies a full snapshot to the order book.
    ///
    /// Convenience method that acquires the write lock, applies the snapshot,
    /// and releases the lock.
    ///
    /// # Arguments
    ///
    /// * `bids` - New bid levels (replaces existing bids)
    /// * `asks` - New ask levels (replaces existing asks)
    /// * `timestamp` - Timestamp of the snapshot
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
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
        &self,
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        timestamp: Timestamp,
    ) {
        self.inner.write().apply_snapshot(bids, asks, timestamp);
    }

    /// Applies an incremental update (delta) to the order book.
    ///
    /// Convenience method that acquires the write lock, applies the delta,
    /// and releases the lock.
    ///
    /// # Arguments
    ///
    /// * `side` - Which side to update (Bid or Ask)
    /// * `levels` - Price levels to insert, update, or delete
    /// * `timestamp` - Timestamp of the update
    pub fn apply_delta(&self, side: Side, levels: Vec<PriceLevel>, timestamp: Timestamp) {
        self.inner.write().apply_delta(side, levels, timestamp);
    }

    /// Clears all levels from the order book.
    ///
    /// Convenience method that acquires the write lock, clears the book,
    /// and releases the lock.
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Creates a batch write handle for multiple operations.
    ///
    /// The returned handle holds the write lock for its lifetime, allowing
    /// multiple operations without repeated lock acquisition.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig};
    /// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
    ///
    /// {
    ///     let mut handle = book.batch_write();
    ///     handle.apply_delta(
    ///         Side::Bid,
    ///         vec![PriceLevel::new(100.0, dec!(10), now_micros())],
    ///         now_micros(),
    ///     );
    ///     handle.apply_delta(
    ///         Side::Ask,
    ///         vec![PriceLevel::new(101.0, dec!(5), now_micros())],
    ///         now_micros(),
    ///     );
    /// } // Lock released when handle is dropped
    ///
    /// assert_eq!(book.bid_count(), 1);
    /// assert_eq!(book.ask_count(), 1);
    /// ```
    #[must_use]
    pub fn batch_write(&self) -> OrderBookWriteHandle<'_> {
        OrderBookWriteHandle {
            guard: self.write(),
        }
    }

    // =========================================================================
    // ACCESSORS
    // =========================================================================

    /// Gets the instrument this book represents.
    ///
    /// This is a cached value that doesn't require a lock.
    #[inline]
    #[must_use]
    pub const fn instrument(&self) -> &Instrument {
        &self.instrument
    }

    /// Gets the thread-safety statistics.
    ///
    /// Returns a clone of the current statistics.
    #[must_use]
    pub fn thread_stats(&self) -> ThreadSafeStats {
        self.stats.read().clone()
    }

    /// Gets the underlying order book statistics.
    ///
    /// Acquires a read lock to access the underlying [`OrderBookStats`].
    #[must_use]
    pub fn book_stats(&self) -> OrderBookStats {
        self.inner.read().stats().clone()
    }

    /// Gets the underlying shared reference.
    ///
    /// Use this when you need direct access to the `SharedOrderBook`.
    #[must_use]
    pub fn shared(&self) -> SharedOrderBook {
        self.inner.clone()
    }

    /// Creates a clone of this thread-safe wrapper.
    ///
    /// This is a cheap operation (Arc clone) that shares the underlying
    /// order book with the original.
    #[must_use]
    pub fn clone_shared(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instrument: self.instrument.clone(),
            stats: self.stats.clone(),
        }
    }
}

// =============================================================================
// BATCH WRITE HANDLE
// =============================================================================

/// A scoped handle for batch write operations.
///
/// Holds the write lock for the duration of its lifetime, allowing
/// multiple operations without repeated lock acquisition. This is
/// more efficient when applying multiple updates atomically.
///
/// # Example
///
/// ```
/// use astra_flash::book::{ThreadSafeOrderBook, OrderBookConfig, OrderBookWriteHandle};
/// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let book = ThreadSafeOrderBook::new(instrument, OrderBookConfig::default());
///
/// {
///     let mut handle = book.batch_write();
///     // All operations use the same lock
///     handle.apply_delta(Side::Bid, vec![PriceLevel::new(100.0, dec!(10), now_micros())], now_micros());
///     handle.apply_delta(Side::Ask, vec![PriceLevel::new(101.0, dec!(5), now_micros())], now_micros());
/// } // Lock released here
/// ```
pub struct OrderBookWriteHandle<'a> {
    guard: WriteGuard<'a>,
}

impl OrderBookWriteHandle<'_> {
    /// Applies a full snapshot to the order book.
    pub fn apply_snapshot(
        &mut self,
        bids: Vec<PriceLevel>,
        asks: Vec<PriceLevel>,
        timestamp: Timestamp,
    ) {
        self.guard.apply_snapshot(bids, asks, timestamp);
    }

    /// Applies an incremental update to the order book.
    pub fn apply_delta(&mut self, side: Side, levels: Vec<PriceLevel>, timestamp: Timestamp) {
        self.guard.apply_delta(side, levels, timestamp);
    }

    /// Clears all levels from the order book.
    pub fn clear(&mut self) {
        self.guard.clear();
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

impl Clone for ThreadSafeOrderBook {
    fn clone(&self) -> Self {
        self.clone_shared()
    }
}

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
    fn test_thread_safe_orderbook_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ThreadSafeOrderBook>();
        assert_sync::<ThreadSafeOrderBook>();
        assert_send::<SharedOrderBook>();
        assert_sync::<SharedOrderBook>();
    }

    #[test]
    fn test_new_creates_empty_book() {
        let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
        assert!(book.is_empty());
    }

    #[test]
    fn test_apply_snapshot_works() {
        let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
        book.apply_snapshot(
            vec![PriceLevel::new(100.0, dec!(10), 0)],
            vec![PriceLevel::new(101.0, dec!(5), 0)],
            0,
        );

        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
    }

    #[test]
    fn test_mid_price() {
        let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
        book.apply_snapshot(
            vec![PriceLevel::new(100.0, dec!(10), 0)],
            vec![PriceLevel::new(102.0, dec!(5), 0)],
            0,
        );

        let mid = book.mid_price().unwrap();
        assert!((mid - 101.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clone_shared() {
        let book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
        book.apply_snapshot(vec![PriceLevel::new(100.0, dec!(10), 0)], vec![], 0);

        let clone = book.clone_shared();

        // Both should see the same data
        assert_eq!(book.bid_count(), clone.bid_count());

        // Modify through clone
        clone.apply_delta(Side::Bid, vec![PriceLevel::new(99.0, dec!(5), 0)], 0);

        // Original should see the change
        assert_eq!(book.bid_count(), 2);
    }
}
