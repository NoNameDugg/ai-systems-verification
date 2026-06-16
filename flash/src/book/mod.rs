//! Order Book module - L2/L3 order book management for Flash.
//!
//! This module provides high-performance order book management with:
//!
//! - **O(log n) updates** via BTreeMap sorted storage
//! - **O(1) best bid/ask** via cached values
//! - **Memory protection** via max_levels enforcement (RED TEAM)
//! - **Precise arithmetic** via rust_decimal
//! - **Snapshot processing** via validation, diffing, serialization
//!
//! # Components
//!
//! - [`OrderBook`] - Core order book with BTreeMap implementation
//! - [`OrderBookConfig`] - Configuration for book behavior
//! - [`OrderBookStats`] - Statistics for monitoring
//! - [`BookSnapshot`] - Serializable snapshot for publishing
//! - [`SnapshotProcessor`] - Advanced snapshot processing operations
//! - [`SnapshotValidator`] - Validates snapshots before applying
//! - [`SnapshotDiff`] - Represents changes between snapshots
//! - [`ThreadSafeOrderBook`] - Thread-safe wrapper with RwLock
//! - [`SharedOrderBook`] - Type alias for Arc<RwLock<OrderBook>>
//!
//! # Phase 3 Implementation
//!
//! This module is implemented in Phase 3 of the TDI roadmap:
//!
//! - Part 3.1: OrderBook Core (BTreeMap implementation) ✓
//! - Part 3.2: Snapshot Processing ✓
//! - Part 3.3: Delta/Incremental Updates ✓
//! - Part 3.4: Thread-Safe Wrapper (RwLock) ✓
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Single level update | < 1 μs |
//! | Full snapshot (50 levels) | < 50 μs |
//! | Best bid/ask lookup | < 10 ns |
//! | Snapshot validation | < 10 μs |
//! | Snapshot diff | < 100 μs |
//!
//! # Architecture
//!
//! The order book uses `BTreeMap<OrderedFloat<f64>, PriceLevel>` for sorted
//! price levels, with cached best bid/ask for O(1) access.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        OrderBook                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │  bids: BTreeMap<OrderedFloat, PriceLevel>  [sorted asc]     │
//! │  asks: BTreeMap<OrderedFloat, PriceLevel>  [sorted asc]     │
//! │  best_bid: Option<PriceLevel>              [cached]         │
//! │  best_ask: Option<PriceLevel>              [cached]         │
//! │  stats: OrderBookStats                                       │
//! └─────────────────────────────────────────────────────────────┘
//!                           │
//!                           ▼
//!              ┌────────────────────────┐
//!              │  Methods               │
//!              ├────────────────────────┤
//!              │  apply_snapshot()      │ O(n log n)
//!              │  apply_delta()         │ O(k log n)
//!              │  best_bid()            │ O(1)
//!              │  best_ask()            │ O(1)
//!              │  mid_price()           │ O(1)
//!              │  spread()              │ O(1)
//!              │  top_bids(n)           │ O(n)
//!              │  top_asks(n)           │ O(n)
//!              └────────────────────────┘
//!                           │
//!                           ▼
//!              ┌────────────────────────┐
//!              │  SnapshotProcessor     │
//!              ├────────────────────────┤
//!              │  validate()            │ Integrity check
//!              │  diff()                │ Change detection
//!              │  serialize_*()         │ JSON/Bincode
//!              └────────────────────────┘
//! ```
//!
//! # Memory Protection (RED TEAM)
//!
//! The `max_levels` configuration prevents memory exhaustion attacks:
//!
//! ```text
//! THREAT: Dust Attack
//! ────────────────────
//! Adversary sends millions of tiny orders at extreme prices
//! to exhaust memory.
//!
//! MITIGATION: max_levels Hard Limit
//! ──────────────────────────────────
//! When levels exceed max_levels, worst-priced levels are dropped:
//! - Bids: lowest prices dropped (worst bids)
//! - Asks: highest prices dropped (worst asks)
//! ```
//!
//! # Example
//!
//! ```
//! use astra_flash::book::{OrderBook, OrderBookConfig, SnapshotProcessor, SnapshotProcessorConfig};
//! use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
//! use rust_decimal_macros::dec;
//!
//! // Create order book
//! let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
//! let config = OrderBookConfig::default();
//! let mut book = OrderBook::new(instrument, config);
//!
//! // Apply snapshot with validation
//! let processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());
//! processor.apply_with_validation(
//!     &mut book,
//!     vec![
//!         PriceLevel::new(100.0, dec!(10), now_micros()),
//!         PriceLevel::new(99.0, dec!(5), now_micros()),
//!     ],
//!     vec![
//!         PriceLevel::new(101.0, dec!(8), now_micros()),
//!         PriceLevel::new(102.0, dec!(4), now_micros()),
//!     ],
//!     now_micros(),
//! ).unwrap();
//!
//! // Apply delta update
//! book.apply_delta(
//!     Side::Bid,
//!     vec![PriceLevel::new(100.0, dec!(15), now_micros())],
//!     now_micros(),
//! );
//!
//! // Query
//! if let Some(bid) = book.best_bid() {
//!     println!("Best bid: {} @ {}", bid.quantity, bid.price);
//! }
//!
//! println!("Mid price: {:?}", book.mid_price());
//! println!("Spread: {:?}", book.spread());
//! ```

// =============================================================================
// SUBMODULES
// =============================================================================

pub mod delta;
pub mod orderbook;
pub mod snapshot;
pub mod thread_safe;

// =============================================================================
// RE-EXPORTS
// =============================================================================

pub use delta::{
    BatchApplyResult, DeltaApplyResult, DeltaBatch, DeltaError, DeltaProcessor,
    DeltaProcessorConfig, DeltaProcessorStats, DeltaResult, DeltaUpdate, DeltaValidationConfig,
    DeltaValidator, SequenceCheckResult, SequenceConfig, SequenceState, SequenceTracker,
    SnapshotRequest, SnapshotRequestReason,
};
pub use orderbook::{BookSnapshot, OrderBook, OrderBookStats};
pub use snapshot::{
    LevelChange, SnapshotDiff, SnapshotError, SnapshotMetadata, SnapshotProcessor,
    SnapshotProcessorConfig, SnapshotProcessorStats, SnapshotResult, SnapshotValidator,
    ValidationConfig,
};
pub use thread_safe::{
    new_shared_orderbook, OrderBookWriteHandle, ReadGuard, SharedOrderBook, ThreadSafeOrderBook,
    ThreadSafeStats, WriteGuard,
};

// Re-export OrderBookConfig from core::config for convenience
pub use crate::core::config::OrderBookConfig;
