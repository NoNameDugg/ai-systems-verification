//! Delta processing for Flash Order Book.
//!
//! This module provides incremental order book update processing:
//!
//! - **DeltaUpdate** - Single incremental update
//! - **DeltaBatch** - Batch of updates
//! - **SequenceTracker** - Sequence gap detection
//! - **DeltaValidator** - Update validation
//! - **DeltaProcessor** - Orchestrates delta processing
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         DELTA MODULE                                 │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  DeltaProcessor     - Orchestrates delta operations                 │
//! │  ├── DeltaValidator - Validates prices, quantities                  │
//! │  └── SequenceTracker- Detects gaps, manages state                   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Single delta | < 1 μs |
//! | Batch (10 updates) | < 5 μs |
//! | Gap detection | O(1) |
//!
//! # Example
//!
//! ```
//! use astra_flash::book::{
//!     DeltaProcessor, DeltaProcessorConfig, DeltaUpdate, OrderBook, OrderBookConfig,
//! };
//! use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
//! use rust_decimal_macros::dec;
//!
//! // Create processor and book
//! let mut processor = DeltaProcessor::with_defaults();
//! let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
//! let mut book = OrderBook::new(instrument, OrderBookConfig::default());
//!
//! // Process delta
//! let update = DeltaUpdate::new(
//!     Side::Bid,
//!     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
//!     now_micros(),
//! );
//! processor.process_delta(&mut book, update).unwrap();
//! ```

use crate::book::orderbook::OrderBook;
use crate::core::types::{now_micros, PriceLevel, Side, Timestamp};
use crate::network::SequenceGap;
use rust_decimal::Decimal;
use std::time::Instant;
use thiserror::Error;

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during delta processing.
#[derive(Debug, Clone, Error)]
pub enum DeltaError {
    /// Sequence gap detected.
    #[error("Sequence gap: expected {expected}, got {actual}")]
    SequenceGap {
        /// Expected sequence number.
        expected: u64,
        /// Actual sequence number received.
        actual: u64,
    },

    /// Invalid price in delta.
    #[error("Invalid price {price}: {reason}")]
    InvalidPrice {
        /// The invalid price.
        price: f64,
        /// Reason for failure.
        reason: String,
    },

    /// Invalid quantity in delta.
    #[error("Invalid quantity: {reason}")]
    InvalidQuantity {
        /// Reason for failure.
        reason: String,
    },

    /// Invalid timestamp.
    #[error("Invalid timestamp {timestamp}: {reason}")]
    InvalidTimestamp {
        /// The invalid timestamp.
        timestamp: Timestamp,
        /// Reason for failure.
        reason: String,
    },

    /// Too many levels in single update.
    #[error("Too many levels: {count} > {max}")]
    TooManyLevels {
        /// Number of levels in update.
        count: usize,
        /// Maximum allowed.
        max: usize,
    },

    /// Processor not ready (e.g., recovering).
    #[error("Processor not ready: {reason}")]
    ProcessorNotReady {
        /// Reason for not being ready.
        reason: String,
    },
}

/// Result type for delta operations.
pub type DeltaResult<T> = Result<T, DeltaError>;

// =============================================================================
// DELTA UPDATE
// =============================================================================

/// A single incremental order book update.
///
/// Represents a change to one side of the order book. Each level in the update:
/// - `quantity > 0`: Insert or update the level
/// - `quantity = 0`: Delete the level
///
/// # Example
///
/// ```
/// use astra_flash::book::DeltaUpdate;
/// use astra_flash::core::types::{PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let update = DeltaUpdate::new(
///     Side::Bid,
///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
///     now_micros(),
/// );
///
/// assert!(update.is_bid());
/// assert_eq!(update.level_count(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct DeltaUpdate {
    /// Exchange sequence number (if provided).
    pub sequence: Option<u64>,

    /// Which side to update.
    pub side: Side,

    /// Price levels to update.
    pub levels: Vec<PriceLevel>,

    /// Exchange timestamp.
    pub timestamp: Timestamp,
}

impl DeltaUpdate {
    /// Creates a new delta update.
    #[must_use]
    pub const fn new(side: Side, levels: Vec<PriceLevel>, timestamp: Timestamp) -> Self {
        Self {
            sequence: None,
            side,
            levels,
            timestamp,
        }
    }

    /// Creates a delta update with sequence number.
    #[must_use]
    pub const fn with_sequence(
        sequence: u64,
        side: Side,
        levels: Vec<PriceLevel>,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            sequence: Some(sequence),
            side,
            levels,
            timestamp,
        }
    }

    /// Gets the number of levels in this update.
    #[inline]
    #[must_use]
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Returns true if this is a bid-side update.
    #[inline]
    #[must_use]
    pub const fn is_bid(&self) -> bool {
        matches!(self.side, Side::Bid)
    }

    /// Returns true if this is an ask-side update.
    #[inline]
    #[must_use]
    pub const fn is_ask(&self) -> bool {
        matches!(self.side, Side::Ask)
    }
}

// DeltaUpdate is automatically Send + Sync because all its fields are:
// - sequence: Option<u64>
// - side: Side (enum)
// - levels: Vec<PriceLevel> (f64, Decimal, Option<u32>, i64)
// - timestamp: i64

// =============================================================================
// DELTA BATCH
// =============================================================================

/// A batch of delta updates.
///
/// Used when multiple updates arrive in the same exchange message.
///
/// # Example
///
/// ```
/// use astra_flash::book::{DeltaBatch, DeltaUpdate};
/// use astra_flash::core::types::{PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let updates = vec![
///     DeltaUpdate::new(Side::Bid, vec![PriceLevel::new(100.0, dec!(10), now_micros())], now_micros()),
///     DeltaUpdate::new(Side::Ask, vec![PriceLevel::new(101.0, dec!(5), now_micros())], now_micros()),
/// ];
/// let batch = DeltaBatch::new(updates, now_micros());
///
/// assert_eq!(batch.update_count(), 2);
/// assert_eq!(batch.total_levels(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct DeltaBatch {
    /// Exchange sequence number for the batch.
    pub sequence: Option<u64>,

    /// Individual updates in this batch.
    pub updates: Vec<DeltaUpdate>,

    /// Timestamp for the batch.
    pub timestamp: Timestamp,
}

impl DeltaBatch {
    /// Creates a new batch.
    #[must_use]
    pub const fn new(updates: Vec<DeltaUpdate>, timestamp: Timestamp) -> Self {
        Self {
            sequence: None,
            updates,
            timestamp,
        }
    }

    /// Creates a batch with sequence number.
    #[must_use]
    pub const fn with_sequence(
        sequence: u64,
        updates: Vec<DeltaUpdate>,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            sequence: Some(sequence),
            updates,
            timestamp,
        }
    }

    /// Gets the total number of levels across all updates.
    #[must_use]
    pub fn total_levels(&self) -> usize {
        self.updates.iter().map(DeltaUpdate::level_count).sum()
    }

    /// Gets the number of updates in the batch.
    #[inline]
    #[must_use]
    pub fn update_count(&self) -> usize {
        self.updates.len()
    }

    /// Returns true if the batch is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }
}

// DeltaBatch is Send + Sync because all fields are Send + Sync:
// - updates: Vec<DeltaUpdate> where DeltaUpdate contains PriceLevel (f64, Decimal, Option<u32>, i64)
// - sequence: Option<u64>
// - timestamp: i64
// The compiler auto-derives Send + Sync for these types.

// =============================================================================
// SEQUENCE STATE
// =============================================================================

/// State machine for sequence tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceState {
    /// No sequence seen yet.
    Uninitialized,
    /// Sequences in order.
    Normal,
    /// Gap detected, awaiting recovery.
    GapDetected,
    /// Snapshot requested, waiting for response.
    Recovering,
}

// =============================================================================
// SEQUENCE CHECK RESULT
// =============================================================================

/// Result of checking a sequence number.
#[derive(Debug, Clone)]
pub enum SequenceCheckResult {
    /// Sequence is in order.
    Ok,
    /// Sequence gap detected.
    Gap(SequenceGap),
    /// Out of order but within window (tolerable).
    OutOfOrder,
}

// =============================================================================
// SEQUENCE CONFIG
// =============================================================================

/// Configuration for sequence tracking.
#[derive(Debug, Clone)]
pub struct SequenceConfig {
    /// Allow out-of-order sequences within this window.
    pub reorder_window: usize,

    /// Maximum gaps to track before forcing recovery.
    pub max_gaps: usize,

    /// Timeout for gap recovery (microseconds).
    pub recovery_timeout_us: u64,
}

impl Default for SequenceConfig {
    fn default() -> Self {
        Self {
            reorder_window: 10,
            max_gaps: 5,
            recovery_timeout_us: 30_000_000, // 30 seconds
        }
    }
}

// =============================================================================
// SEQUENCE TRACKER
// =============================================================================

/// Tracks sequence numbers and detects gaps.
///
/// # Example
///
/// ```
/// use astra_flash::book::{SequenceTracker, SequenceConfig, SequenceState, SequenceCheckResult};
///
/// let mut tracker = SequenceTracker::new(SequenceConfig::default());
///
/// // First sequence
/// tracker.record(1);
/// assert_eq!(tracker.state(), SequenceState::Normal);
///
/// // Sequential
/// tracker.record(2);
/// assert_eq!(tracker.last_sequence(), Some(2));
///
/// // Gap!
/// let result = tracker.record(10);
/// assert!(matches!(result, SequenceCheckResult::Gap(_)));
/// ```
#[derive(Debug)]
pub struct SequenceTracker {
    config: SequenceConfig,
    last_sequence: Option<u64>,
    expected_next: Option<u64>,
    gaps: Vec<SequenceGap>,
    state: SequenceState,
    last_update: Timestamp,
}

impl SequenceTracker {
    /// Creates a new sequence tracker.
    #[must_use]
    pub const fn new(config: SequenceConfig) -> Self {
        Self {
            config,
            last_sequence: None,
            expected_next: None,
            gaps: Vec::new(),
            state: SequenceState::Uninitialized,
            last_update: 0,
        }
    }

    /// Records a sequence number and checks for gaps.
    pub fn record(&mut self, sequence: u64) -> SequenceCheckResult {
        self.last_update = now_micros();

        // First sequence
        if self.state == SequenceState::Uninitialized {
            self.last_sequence = Some(sequence);
            self.expected_next = Some(sequence + 1);
            self.state = SequenceState::Normal;
            return SequenceCheckResult::Ok;
        }

        let expected = self.expected_next.unwrap_or(sequence);

        if sequence == expected {
            // In order
            self.last_sequence = Some(sequence);
            self.expected_next = Some(sequence + 1);
            SequenceCheckResult::Ok
        } else if sequence > expected {
            // Gap detected
            let gap = SequenceGap {
                expected,
                received: sequence,
            };
            self.gaps.push(gap.clone());
            self.last_sequence = Some(sequence);
            self.expected_next = Some(sequence + 1);
            self.state = SequenceState::GapDetected;

            // Trim gaps if too many
            while self.gaps.len() > self.config.max_gaps {
                self.gaps.remove(0);
            }

            SequenceCheckResult::Gap(gap)
        } else {
            // Out of order (old message)
            SequenceCheckResult::OutOfOrder
        }
    }

    /// Gets the current state.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> SequenceState {
        self.state
    }

    /// Gets pending gaps.
    #[must_use]
    pub fn gaps(&self) -> &[SequenceGap] {
        &self.gaps
    }

    /// Gets the last sequence number.
    #[inline]
    #[must_use]
    pub const fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// Resets the tracker to uninitialized state.
    pub fn reset(&mut self) {
        self.last_sequence = None;
        self.expected_next = None;
        self.gaps.clear();
        self.state = SequenceState::Uninitialized;
        self.last_update = 0;
    }

    /// Resets the tracker with a known sequence.
    pub fn reset_with_sequence(&mut self, sequence: u64) {
        self.gaps.clear();
        self.last_sequence = Some(sequence);
        self.expected_next = Some(sequence + 1);
        self.state = SequenceState::Normal;
        self.last_update = now_micros();
    }

    /// Returns true if recovery is needed.
    #[must_use]
    pub fn needs_recovery(&self) -> bool {
        self.state == SequenceState::GapDetected
    }

    /// Marks recovery as started.
    pub fn start_recovery(&mut self) {
        if self.state == SequenceState::GapDetected {
            self.state = SequenceState::Recovering;
        }
    }

    /// Marks recovery as complete.
    pub fn complete_recovery(&mut self, new_sequence: u64) {
        self.gaps.clear();
        self.last_sequence = Some(new_sequence);
        self.expected_next = Some(new_sequence + 1);
        self.state = SequenceState::Normal;
        self.last_update = now_micros();
    }
}

// =============================================================================
// DELTA VALIDATION CONFIG
// =============================================================================

/// Configuration for delta validation.
#[derive(Debug, Clone)]
pub struct DeltaValidationConfig {
    /// Whether to validate prices.
    pub validate_prices: bool,

    /// Whether to validate quantities.
    pub validate_quantities: bool,

    /// Whether to validate timestamps.
    pub validate_timestamps: bool,

    /// Maximum price.
    pub max_price: f64,

    /// Minimum price.
    pub min_price: f64,

    /// Maximum levels per single update.
    pub max_levels_per_update: usize,
}

impl Default for DeltaValidationConfig {
    fn default() -> Self {
        Self {
            validate_prices: true,
            validate_quantities: true,
            validate_timestamps: false,
            max_price: 1e12,
            min_price: 0.0,
            max_levels_per_update: 1000,
        }
    }
}

// =============================================================================
// DELTA VALIDATOR
// =============================================================================

/// Validates delta updates before applying.
///
/// # Example
///
/// ```
/// use astra_flash::book::{DeltaValidator, DeltaValidationConfig, DeltaUpdate};
/// use astra_flash::core::types::{PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let validator = DeltaValidator::new(DeltaValidationConfig::default());
/// let update = DeltaUpdate::new(
///     Side::Bid,
///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
///     now_micros(),
/// );
///
/// assert!(validator.validate(&update).is_ok());
/// ```
#[derive(Debug, Clone)]
pub struct DeltaValidator {
    config: DeltaValidationConfig,
}

impl DeltaValidator {
    /// Creates a new validator.
    #[must_use]
    pub const fn new(config: DeltaValidationConfig) -> Self {
        Self { config }
    }

    /// Creates a validator with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DeltaValidationConfig::default())
    }

    /// Validates a delta update.
    pub fn validate(&self, delta: &DeltaUpdate) -> DeltaResult<()> {
        // Check level count
        if delta.levels.len() > self.config.max_levels_per_update {
            return Err(DeltaError::TooManyLevels {
                count: delta.levels.len(),
                max: self.config.max_levels_per_update,
            });
        }

        // Validate each level
        if self.config.validate_prices || self.config.validate_quantities {
            for level in &delta.levels {
                if self.config.validate_prices {
                    self.validate_price(level.price)?;
                }
                if self.config.validate_quantities {
                    self.validate_quantity(level.quantity)?;
                }
            }
        }

        Ok(())
    }

    /// Validates a batch of deltas.
    pub fn validate_batch(&self, batch: &DeltaBatch) -> DeltaResult<()> {
        for update in &batch.updates {
            self.validate(update)?;
        }
        Ok(())
    }

    /// Gets the configuration.
    #[must_use]
    pub const fn config(&self) -> &DeltaValidationConfig {
        &self.config
    }

    /// Validates a price.
    fn validate_price(&self, price: f64) -> DeltaResult<()> {
        if price.is_nan() || price.is_infinite() {
            return Err(DeltaError::InvalidPrice {
                price,
                reason: "Price is NaN or Infinite".to_string(),
            });
        }

        if price < self.config.min_price {
            return Err(DeltaError::InvalidPrice {
                price,
                reason: format!("Price below minimum {}", self.config.min_price),
            });
        }

        if price > self.config.max_price {
            return Err(DeltaError::InvalidPrice {
                price,
                reason: format!("Price above maximum {}", self.config.max_price),
            });
        }

        Ok(())
    }

    /// Validates a quantity.
    fn validate_quantity(&self, quantity: Decimal) -> DeltaResult<()> {
        // Note: Zero quantity is valid (means delete)
        // Only reject negative quantities
        if quantity < Decimal::ZERO {
            return Err(DeltaError::InvalidQuantity {
                reason: "Negative quantity".to_string(),
            });
        }
        Ok(())
    }
}

// =============================================================================
// APPLY RESULTS
// =============================================================================

/// Result of applying a single delta.
#[derive(Debug, Clone, Default)]
pub struct DeltaApplyResult {
    /// Number of levels added.
    pub levels_added: usize,

    /// Number of levels modified.
    pub levels_modified: usize,

    /// Number of levels removed.
    pub levels_removed: usize,

    /// Whether best bid changed.
    pub best_bid_changed: bool,

    /// Whether best ask changed.
    pub best_ask_changed: bool,

    /// Processing time in nanoseconds.
    pub processing_time_ns: u64,
}

/// Result of applying a batch.
#[derive(Debug, Clone, Default)]
pub struct BatchApplyResult {
    /// Number of updates applied.
    pub updates_applied: usize,

    /// Total levels affected.
    pub total_levels: usize,

    /// Whether best bid changed.
    pub best_bid_changed: bool,

    /// Whether best ask changed.
    pub best_ask_changed: bool,

    /// Processing time in nanoseconds.
    pub processing_time_ns: u64,
}

// =============================================================================
// SNAPSHOT REQUEST
// =============================================================================

/// Reason for requesting a snapshot.
#[derive(Debug, Clone)]
pub enum SnapshotRequestReason {
    /// Sequence gap detected.
    SequenceGap,
    /// Book corruption detected.
    BookCorruption,
    /// Initial subscription.
    InitialSubscription,
    /// Manual request.
    ManualRequest,
}

/// Request for snapshot recovery.
#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    /// Reason for the request.
    pub reason: SnapshotRequestReason,

    /// Gap information if applicable.
    pub gap: Option<SequenceGap>,

    /// When the request was created.
    pub requested_at: Timestamp,
}

impl SnapshotRequest {
    /// Creates a request for sequence gap recovery.
    #[must_use]
    pub fn for_sequence_gap(gap: SequenceGap) -> Self {
        Self {
            reason: SnapshotRequestReason::SequenceGap,
            gap: Some(gap),
            requested_at: now_micros(),
        }
    }

    /// Creates a request for initial subscription.
    #[must_use]
    pub fn for_initial_subscription() -> Self {
        Self {
            reason: SnapshotRequestReason::InitialSubscription,
            gap: None,
            requested_at: now_micros(),
        }
    }

    /// Creates a manual request.
    #[must_use]
    pub fn manual() -> Self {
        Self {
            reason: SnapshotRequestReason::ManualRequest,
            gap: None,
            requested_at: now_micros(),
        }
    }
}

// =============================================================================
// DELTA PROCESSOR STATS
// =============================================================================

/// Statistics for delta processing.
#[derive(Debug, Clone, Default)]
pub struct DeltaProcessorStats {
    /// Number of deltas processed.
    pub deltas_processed: u64,

    /// Number of deltas successfully applied.
    pub deltas_applied: u64,

    /// Number of deltas rejected.
    pub deltas_rejected: u64,

    /// Number of sequence gaps detected.
    pub sequence_gaps_detected: u64,

    /// Number of snapshots requested.
    pub snapshots_requested: u64,

    /// Last processing time in microseconds.
    pub last_processing_time_us: u64,
}

// =============================================================================
// DELTA PROCESSOR CONFIG
// =============================================================================

/// Configuration for delta processor.
#[derive(Debug, Clone)]
pub struct DeltaProcessorConfig {
    /// Whether to validate deltas before applying.
    pub validate_before_apply: bool,

    /// Whether to track sequence numbers.
    pub track_sequences: bool,

    /// Whether to auto-request snapshots on gaps.
    pub auto_request_snapshot: bool,

    /// Maximum pending gaps before forcing recovery.
    pub max_pending_gaps: usize,

    /// Delta validation settings.
    pub validation: DeltaValidationConfig,

    /// Sequence tracking settings.
    pub sequence: SequenceConfig,
}

impl Default for DeltaProcessorConfig {
    fn default() -> Self {
        Self {
            validate_before_apply: true,
            track_sequences: true,
            auto_request_snapshot: true,
            max_pending_gaps: 5,
            validation: DeltaValidationConfig::default(),
            sequence: SequenceConfig::default(),
        }
    }
}

// =============================================================================
// DELTA PROCESSOR
// =============================================================================

/// Orchestrates delta processing operations.
///
/// Wraps OrderBook operations with:
/// - Validation before apply
/// - Sequence tracking and gap detection
/// - Statistics collection
///
/// # Example
///
/// ```
/// use astra_flash::book::{
///     DeltaProcessor, DeltaProcessorConfig, DeltaUpdate, OrderBook, OrderBookConfig,
/// };
/// use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side, now_micros};
/// use rust_decimal_macros::dec;
///
/// let mut processor = DeltaProcessor::with_defaults();
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let mut book = OrderBook::new(instrument, OrderBookConfig::default());
///
/// // Process delta with sequence tracking
/// let update = DeltaUpdate::with_sequence(
///     1,
///     Side::Bid,
///     vec![PriceLevel::new(100.0, dec!(10), now_micros())],
///     now_micros(),
/// );
/// processor.process_delta(&mut book, update).unwrap();
///
/// assert_eq!(book.bid_count(), 1);
/// assert_eq!(processor.stats().deltas_applied, 1);
/// ```
#[derive(Debug)]
pub struct DeltaProcessor {
    config: DeltaProcessorConfig,
    sequence_tracker: SequenceTracker,
    validator: DeltaValidator,
    stats: DeltaProcessorStats,
}

impl DeltaProcessor {
    /// Creates a new delta processor.
    #[must_use]
    pub fn new(config: DeltaProcessorConfig) -> Self {
        let sequence_tracker = SequenceTracker::new(config.sequence.clone());
        let validator = DeltaValidator::new(config.validation.clone());

        Self {
            config,
            sequence_tracker,
            validator,
            stats: DeltaProcessorStats::default(),
        }
    }

    /// Creates a processor with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DeltaProcessorConfig::default())
    }

    /// Processes and applies a single delta update.
    ///
    /// # Arguments
    ///
    /// * `book` - Order book to apply to
    /// * `delta` - Delta update to process
    ///
    /// # Returns
    ///
    /// `Ok(DeltaApplyResult)` if successful, `Err(DeltaError)` otherwise.
    pub fn process_delta(
        &mut self,
        book: &mut OrderBook,
        delta: DeltaUpdate,
    ) -> DeltaResult<DeltaApplyResult> {
        let start = Instant::now();
        self.stats.deltas_processed += 1;

        // 1. Check sequence
        if self.config.track_sequences {
            if let Some(seq) = delta.sequence {
                match self.sequence_tracker.record(seq) {
                    SequenceCheckResult::Gap(gap) => {
                        self.stats.sequence_gaps_detected += 1;
                        self.stats.deltas_rejected += 1;
                        return Err(DeltaError::SequenceGap {
                            expected: gap.expected,
                            actual: gap.received,
                        });
                    }
                    SequenceCheckResult::Ok | SequenceCheckResult::OutOfOrder => {}
                }
            }
        }

        // 2. Validate
        if self.config.validate_before_apply {
            if let Err(e) = self.validator.validate(&delta) {
                self.stats.deltas_rejected += 1;
                return Err(e);
            }
        }

        // 3. Capture before state
        let before_bid = book.best_bid().cloned();
        let before_ask = book.best_ask().cloned();

        // 4. Apply to book
        book.apply_delta(delta.side, delta.levels, delta.timestamp);

        // 5. Capture after state
        let after_bid = book.best_bid().cloned();
        let after_ask = book.best_ask().cloned();

        // 6. Build result
        let elapsed = start.elapsed();
        self.stats.deltas_applied += 1;
        self.stats.last_processing_time_us = elapsed.as_micros() as u64;

        Ok(DeltaApplyResult {
            levels_added: 0, // Would need deeper tracking
            levels_modified: 0,
            levels_removed: 0,
            best_bid_changed: before_bid != after_bid,
            best_ask_changed: before_ask != after_ask,
            processing_time_ns: elapsed.as_nanos() as u64,
        })
    }

    /// Processes and applies a batch of deltas.
    pub fn process_batch(
        &mut self,
        book: &mut OrderBook,
        batch: DeltaBatch,
    ) -> DeltaResult<BatchApplyResult> {
        let start = Instant::now();

        // Check batch sequence
        if self.config.track_sequences {
            if let Some(seq) = batch.sequence {
                match self.sequence_tracker.record(seq) {
                    SequenceCheckResult::Gap(gap) => {
                        self.stats.sequence_gaps_detected += 1;
                        return Err(DeltaError::SequenceGap {
                            expected: gap.expected,
                            actual: gap.received,
                        });
                    }
                    SequenceCheckResult::Ok | SequenceCheckResult::OutOfOrder => {}
                }
            }
        }

        // Validate batch
        if self.config.validate_before_apply {
            self.validator.validate_batch(&batch)?;
        }

        // Capture before state
        let before_bid = book.best_bid().cloned();
        let before_ask = book.best_ask().cloned();

        // Apply all updates
        let mut total_levels = 0;
        for update in batch.updates {
            total_levels += update.level_count();
            book.apply_delta(update.side, update.levels, update.timestamp);
            self.stats.deltas_processed += 1;
            self.stats.deltas_applied += 1;
        }

        // Capture after state
        let after_bid = book.best_bid().cloned();
        let after_ask = book.best_ask().cloned();

        let elapsed = start.elapsed();
        self.stats.last_processing_time_us = elapsed.as_micros() as u64;

        Ok(BatchApplyResult {
            updates_applied: total_levels,
            total_levels,
            best_bid_changed: before_bid != after_bid,
            best_ask_changed: before_ask != after_ask,
            processing_time_ns: elapsed.as_nanos() as u64,
        })
    }

    /// Resets the processor state.
    pub fn reset(&mut self) {
        self.sequence_tracker.reset();
    }

    /// Resets with a known sequence number.
    pub fn reset_with_sequence(&mut self, sequence: u64) {
        self.sequence_tracker.reset_with_sequence(sequence);
    }

    /// Gets the current sequence state.
    #[must_use]
    pub fn sequence_state(&self) -> SequenceState {
        self.sequence_tracker.state()
    }

    /// Gets pending gaps.
    #[must_use]
    pub fn pending_gaps(&self) -> &[SequenceGap] {
        self.sequence_tracker.gaps()
    }

    /// Returns true if a snapshot is needed.
    #[must_use]
    pub fn needs_snapshot(&self) -> bool {
        self.sequence_tracker.needs_recovery()
    }

    /// Creates a snapshot request if needed.
    #[must_use]
    pub fn create_snapshot_request(&self) -> Option<SnapshotRequest> {
        if !self.needs_snapshot() {
            return None;
        }

        let gap = self.sequence_tracker.gaps().last().cloned();
        Some(SnapshotRequest {
            reason: SnapshotRequestReason::SequenceGap,
            gap,
            requested_at: now_micros(),
        })
    }

    /// Gets processing statistics.
    #[must_use]
    pub const fn stats(&self) -> &DeltaProcessorStats {
        &self.stats
    }

    /// Gets configuration.
    #[must_use]
    pub const fn config(&self) -> &DeltaProcessorConfig {
        &self.config
    }
}

// DeltaProcessor is Send + Sync because all fields are Send + Sync:
// - config: DeltaProcessorConfig (POD types)
// - sequence_tracker: SequenceTracker (POD types)
// - validator: DeltaValidator (POD types)
// - stats: DeltaProcessorStats (POD types)
// The compiler auto-derives Send + Sync for these types.

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_level(price: f64, qty: Decimal) -> PriceLevel {
        PriceLevel::new(price, qty, now_micros())
    }

    #[test]
    fn test_delta_update_basic() {
        let update = DeltaUpdate::new(Side::Bid, vec![test_level(100.0, dec!(10))], now_micros());

        assert!(update.is_bid());
        assert!(!update.is_ask());
        assert_eq!(update.level_count(), 1);
        assert!(update.sequence.is_none());
    }

    #[test]
    fn test_sequence_tracker_basic() {
        let mut tracker = SequenceTracker::new(SequenceConfig::default());

        // First message
        let result = tracker.record(1);
        assert!(matches!(result, SequenceCheckResult::Ok));
        assert_eq!(tracker.state(), SequenceState::Normal);

        // Sequential
        let result = tracker.record(2);
        assert!(matches!(result, SequenceCheckResult::Ok));

        // Gap
        let result = tracker.record(10);
        assert!(matches!(result, SequenceCheckResult::Gap(_)));
        assert_eq!(tracker.state(), SequenceState::GapDetected);
    }

    #[test]
    fn test_validator_basic() {
        let validator = DeltaValidator::with_defaults();
        let update = DeltaUpdate::new(Side::Bid, vec![test_level(100.0, dec!(10))], now_micros());

        assert!(validator.validate(&update).is_ok());
    }

    #[test]
    fn test_validator_rejects_nan() {
        let validator = DeltaValidator::with_defaults();
        let update = DeltaUpdate::new(
            Side::Bid,
            vec![test_level(f64::NAN, dec!(10))],
            now_micros(),
        );

        assert!(validator.validate(&update).is_err());
    }
}
