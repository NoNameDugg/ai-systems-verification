//! Batching and Backpressure for Flash Redis Publisher.
//!
//! This module provides intelligent message batching and backpressure handling
//! for high-throughput Redis Stream publishing.
//!
//! # Features
//!
//! - **Message batching**: Collects messages and flushes by size or timeout
//! - **Backpressure handling**: Monitors queue depth and takes action on overflow
//! - **Configurable thresholds**: Warning at 80%, critical at 95% (configurable)
//! - **Multiple overflow actions**: Block, DropOldest, DropNewest, WarnAndContinue
//!
//! # Batching Strategy
//!
//! Messages are flushed when either condition is met:
//! - Batch size reaches `max_batch_size` (default: 100)
//! - Batch age exceeds `max_batch_delay` (default: 10ms)
//!
//! # Backpressure Thresholds
//!
//! ```text
//! 0%          80% (warn)     95% (critical)     100%
//! |-------------|----------------|-----------------|
//!    Normal         Warning          Critical
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::batch::{Batcher, BatchConfig};
//! use std::time::Duration;
//!
//! // Create batcher with custom config
//! let config = BatchConfig::builder()
//!     .max_batch_size(50)
//!     .max_batch_delay(Duration::from_millis(5))
//!     .warn_threshold(0.75)
//!     .build()?;
//!
//! let batcher = Batcher::new(publisher, config)?;
//! batcher.start().await?;
//!
//! // Enqueue messages
//! batcher.enqueue(message).await?;
//!
//! // Check status
//! println!("Queue depth: {}", batcher.queue_depth());
//! println!("Backpressure: {:?}", batcher.backpressure_status());
//! ```

use crate::core::types::{now_micros, Timestamp};
use crate::publisher::stream::SerializationFormat;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

// =============================================================================
// OVERFLOW ACTION
// =============================================================================

/// Action to take when the message queue overflows.
///
/// # Variants
///
/// - **Block**: Default. Block producer until space is available.
/// - **DropOldest**: Remove oldest messages to make room.
/// - **DropNewest**: Reject incoming messages when full.
/// - **WarnAndContinue**: Log warning and attempt to continue.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::OverflowAction;
///
/// let action = OverflowAction::default();
/// assert_eq!(action, OverflowAction::Block);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OverflowAction {
    /// Block producer until space is available (default).
    #[default]
    Block,
    /// Remove oldest messages to make room for new ones.
    DropOldest,
    /// Reject incoming messages when queue is full.
    DropNewest,
    /// Log warning and attempt to continue.
    WarnAndContinue,
}

impl std::fmt::Display for OverflowAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block => write!(f, "Block"),
            Self::DropOldest => write!(f, "DropOldest"),
            Self::DropNewest => write!(f, "DropNewest"),
            Self::WarnAndContinue => write!(f, "WarnAndContinue"),
        }
    }
}

// =============================================================================
// BACKPRESSURE STATUS
// =============================================================================

/// Current backpressure status based on queue depth.
///
/// # Thresholds (default)
///
/// - **Normal**: Queue < 80% capacity
/// - **Warning**: Queue 80-95% capacity
/// - **Critical**: Queue > 95% capacity
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BackpressureStatus;
///
/// let status = BackpressureStatus::default();
/// assert_eq!(status, BackpressureStatus::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BackpressureStatus {
    /// Queue is under warning threshold.
    #[default]
    Normal,
    /// Queue is between warning and critical thresholds.
    Warning,
    /// Queue is above critical threshold.
    Critical,
}

impl std::fmt::Display for BackpressureStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Warning => write!(f, "Warning"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

// =============================================================================
// FLUSH TRIGGER
// =============================================================================

/// What triggered a batch flush.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::FlushTrigger;
///
/// let trigger = FlushTrigger::BatchFull;
/// assert_eq!(format!("{:?}", trigger), "BatchFull");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlushTrigger {
    /// Batch reached max_batch_size.
    BatchFull,
    /// Batch timeout (max_batch_delay) expired.
    Timeout,
    /// Manual flush was requested.
    Manual,
    /// Shutdown in progress, flushing remaining.
    Shutdown,
}

impl std::fmt::Display for FlushTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BatchFull => write!(f, "BatchFull"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Manual => write!(f, "Manual"),
            Self::Shutdown => write!(f, "Shutdown"),
        }
    }
}

// =============================================================================
// DROP REASON
// =============================================================================

/// Reason a message was dropped.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::DropReason;
///
/// let reason = DropReason::QueueFull;
/// assert_eq!(format!("{:?}", reason), "QueueFull");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// Queue is at capacity and overflow action is DropNewest.
    QueueFull,
    /// Message was evicted by DropOldest policy.
    Evicted,
    /// Message exceeds maximum allowed size.
    MessageTooLarge,
    /// Message format is invalid.
    InvalidFormat,
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => write!(f, "QueueFull"),
            Self::Evicted => write!(f, "Evicted"),
            Self::MessageTooLarge => write!(f, "MessageTooLarge"),
            Self::InvalidFormat => write!(f, "InvalidFormat"),
        }
    }
}

// =============================================================================
// BATCH CONFIG
// =============================================================================

/// Configuration for the message batcher.
///
/// # Defaults
///
/// | Field | Default |
/// |-------|---------|
/// | max_batch_size | 100 |
/// | max_batch_delay | 10ms |
/// | channel_capacity | 10,000 |
/// | warn_threshold | 0.80 |
/// | critical_threshold | 0.95 |
/// | overflow_action | Block |
/// | auto_flush | true |
/// | min_batch_size | 1 |
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatchConfig;
/// use std::time::Duration;
///
/// let config = BatchConfig::default();
/// assert_eq!(config.max_batch_size, 100);
/// assert_eq!(config.max_batch_delay, Duration::from_millis(10));
/// ```
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum messages per batch before flush (default: 100).
    pub max_batch_size: usize,

    /// Maximum time to wait before flush (default: 10ms).
    pub max_batch_delay: Duration,

    /// Channel capacity for incoming messages (default: 10,000).
    pub channel_capacity: usize,

    /// Warning threshold (percentage of capacity, default: 0.80).
    pub warn_threshold: f64,

    /// Critical threshold (percentage of capacity, default: 0.95).
    pub critical_threshold: f64,

    /// Action when critical threshold exceeded.
    pub overflow_action: OverflowAction,

    /// Enable automatic flushing on timeout.
    pub auto_flush: bool,

    /// Minimum messages before considering flush worthwhile.
    pub min_batch_size: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            max_batch_delay: Duration::from_millis(10),
            channel_capacity: 10_000,
            warn_threshold: 0.80,
            critical_threshold: 0.95,
            overflow_action: OverflowAction::Block,
            auto_flush: true,
            min_batch_size: 1,
        }
    }
}

impl BatchConfig {
    /// Creates a new builder for `BatchConfig`.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::batch::BatchConfig;
    ///
    /// let config = BatchConfig::builder()
    ///     .max_batch_size(50)
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn builder() -> BatchConfigBuilder {
        BatchConfigBuilder::new()
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns `BatcherError::InvalidConfig` if:
    /// - `max_batch_size` is 0
    /// - `channel_capacity` is 0
    /// - `warn_threshold` >= `critical_threshold`
    /// - Thresholds are outside [0.0, 1.0]
    pub fn validate(&self) -> BatcherResult<()> {
        if self.max_batch_size == 0 {
            return Err(BatcherError::InvalidConfig {
                reason: "max_batch_size cannot be zero".to_string(),
            });
        }

        if self.channel_capacity == 0 {
            return Err(BatcherError::InvalidConfig {
                reason: "channel_capacity cannot be zero".to_string(),
            });
        }

        if self.warn_threshold < 0.0 || self.warn_threshold > 1.0 {
            return Err(BatcherError::InvalidConfig {
                reason: "warn_threshold must be between 0.0 and 1.0".to_string(),
            });
        }

        if self.critical_threshold < 0.0 || self.critical_threshold > 1.0 {
            return Err(BatcherError::InvalidConfig {
                reason: "critical_threshold must be between 0.0 and 1.0".to_string(),
            });
        }

        if self.warn_threshold >= self.critical_threshold {
            return Err(BatcherError::InvalidConfig {
                reason: "warn_threshold must be less than critical_threshold".to_string(),
            });
        }

        Ok(())
    }
}

// =============================================================================
// BATCH CONFIG BUILDER
// =============================================================================

/// Builder for `BatchConfig`.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatchConfigBuilder;
/// use std::time::Duration;
///
/// let config = BatchConfigBuilder::new()
///     .max_batch_size(50)
///     .max_batch_delay(Duration::from_millis(5))
///     .warn_threshold(0.75)
///     .critical_threshold(0.90)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct BatchConfigBuilder {
    max_batch_size: Option<usize>,
    max_batch_delay: Option<Duration>,
    channel_capacity: Option<usize>,
    warn_threshold: Option<f64>,
    critical_threshold: Option<f64>,
    overflow_action: Option<OverflowAction>,
    auto_flush: Option<bool>,
    min_batch_size: Option<usize>,
}

impl BatchConfigBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum batch size.
    #[must_use]
    pub const fn max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = Some(size);
        self
    }

    /// Sets the maximum batch delay.
    #[must_use]
    pub const fn max_batch_delay(mut self, delay: Duration) -> Self {
        self.max_batch_delay = Some(delay);
        self
    }

    /// Sets the channel capacity.
    #[must_use]
    pub const fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = Some(capacity);
        self
    }

    /// Sets the warning threshold (0.0 - 1.0).
    #[must_use]
    pub const fn warn_threshold(mut self, threshold: f64) -> Self {
        self.warn_threshold = Some(threshold);
        self
    }

    /// Sets the critical threshold (0.0 - 1.0).
    #[must_use]
    pub const fn critical_threshold(mut self, threshold: f64) -> Self {
        self.critical_threshold = Some(threshold);
        self
    }

    /// Sets the overflow action.
    #[must_use]
    pub const fn overflow_action(mut self, action: OverflowAction) -> Self {
        self.overflow_action = Some(action);
        self
    }

    /// Enables or disables auto-flush on timeout.
    #[must_use]
    pub const fn auto_flush(mut self, enabled: bool) -> Self {
        self.auto_flush = Some(enabled);
        self
    }

    /// Sets the minimum batch size before flush.
    #[must_use]
    pub const fn min_batch_size(mut self, size: usize) -> Self {
        self.min_batch_size = Some(size);
        self
    }

    /// Builds the configuration.
    ///
    /// # Errors
    ///
    /// Returns `BatcherError::InvalidConfig` if validation fails.
    pub fn build(self) -> BatcherResult<BatchConfig> {
        let defaults = BatchConfig::default();

        let config = BatchConfig {
            max_batch_size: self.max_batch_size.unwrap_or(defaults.max_batch_size),
            max_batch_delay: self.max_batch_delay.unwrap_or(defaults.max_batch_delay),
            channel_capacity: self.channel_capacity.unwrap_or(defaults.channel_capacity),
            warn_threshold: self.warn_threshold.unwrap_or(defaults.warn_threshold),
            critical_threshold: self
                .critical_threshold
                .unwrap_or(defaults.critical_threshold),
            overflow_action: self.overflow_action.unwrap_or(defaults.overflow_action),
            auto_flush: self.auto_flush.unwrap_or(defaults.auto_flush),
            min_batch_size: self.min_batch_size.unwrap_or(defaults.min_batch_size),
        };

        config.validate()?;
        Ok(config)
    }
}

// =============================================================================
// BATCH MESSAGE
// =============================================================================

/// A message queued for batching.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatchMessage;
/// use astra_flash::publisher::stream::SerializationFormat;
/// use astra_flash::core::types::now_micros;
///
/// let message = BatchMessage {
///     topic: "market_data.deribit.btc_usd.book".to_string(),
///     data: vec![1, 2, 3, 4],
///     format: SerializationFormat::Bincode,
///     enqueued_at: now_micros(),
///     priority: 0,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct BatchMessage {
    /// Topic to publish to.
    pub topic: String,
    /// Serialized message data.
    pub data: Vec<u8>,
    /// Serialization format used.
    pub format: SerializationFormat,
    /// Timestamp when message was enqueued (microseconds).
    pub enqueued_at: Timestamp,
    /// Message priority (higher = more important).
    pub priority: u8,
}

impl BatchMessage {
    /// Returns the size of the message data in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

// =============================================================================
// MESSAGE BATCH
// =============================================================================

/// A batch of messages ready for publishing.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::MessageBatch;
///
/// let mut batch = MessageBatch::new();
/// assert!(batch.messages.is_empty());
/// assert_eq!(batch.message_count, 0);
/// ```
#[derive(Debug, Clone)]
pub struct MessageBatch {
    /// Messages in this batch.
    pub messages: Vec<BatchMessage>,
    /// Batch creation timestamp.
    pub created_at: Timestamp,
    /// Total serialized size in bytes.
    pub total_bytes: usize,
    /// Number of messages.
    pub message_count: usize,
}

impl MessageBatch {
    /// Creates a new empty batch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            created_at: now_micros(),
            total_bytes: 0,
            message_count: 0,
        }
    }

    /// Adds a message to the batch.
    pub fn add(&mut self, message: BatchMessage) {
        self.total_bytes += message.data.len();
        self.message_count += 1;
        self.messages.push(message);
    }

    /// Clears the batch.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.total_bytes = 0;
        self.message_count = 0;
        self.created_at = now_micros();
    }

    /// Returns true if the batch is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns the number of messages in the batch.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

impl Default for MessageBatch {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// BATCH PUBLISH RESULT
// =============================================================================

/// Result of a batch publish operation.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatchPublishResult;
///
/// let result = BatchPublishResult {
///     messages_published: 100,
///     messages_failed: 0,
///     duration_us: 1000,
///     avg_message_time_us: 10.0,
///     message_ids: vec!["1234-0".to_string()],
/// };
/// ```
#[derive(Debug, Clone)]
pub struct BatchPublishResult {
    /// Number of messages successfully published.
    pub messages_published: usize,
    /// Number of messages that failed.
    pub messages_failed: usize,
    /// Total time for batch publish (microseconds).
    pub duration_us: u64,
    /// Average time per message (microseconds).
    pub avg_message_time_us: f64,
    /// Redis message IDs for successful publishes.
    pub message_ids: Vec<String>,
}

// =============================================================================
// BATCHER STATS
// =============================================================================

/// Statistics for the batcher.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatcherStats;
///
/// let stats = BatcherStats::default();
/// assert_eq!(stats.messages_enqueued, 0);
/// assert_eq!(stats.batches_published, 0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct BatcherStats {
    /// Total messages enqueued.
    pub messages_enqueued: u64,
    /// Total batches published.
    pub batches_published: u64,
    /// Total messages published.
    pub messages_published: u64,
    /// Messages dropped due to overflow.
    pub messages_dropped: u64,
    /// Current queue depth.
    pub current_queue_depth: usize,
    /// Peak queue depth.
    pub peak_queue_depth: usize,
    /// Total bytes processed.
    pub bytes_processed: u64,
    /// Average batch size.
    pub avg_batch_size: f64,
    /// Average batch latency (time from first message to flush).
    pub avg_batch_latency_us: f64,
    /// Number of timeout-triggered flushes.
    pub timeout_flushes: u64,
    /// Number of size-triggered flushes.
    pub size_flushes: u64,
    /// Current backpressure status.
    pub backpressure_status: BackpressureStatus,
    /// Time spent in backpressure (microseconds).
    pub backpressure_time_us: u64,
}

impl BatcherStats {
    /// Updates the peak queue depth if current depth is higher.
    pub fn update_peak(&mut self, current_depth: usize) {
        if current_depth > self.peak_queue_depth {
            self.peak_queue_depth = current_depth;
        }
    }

    /// Updates the average batch size with EMA (Exponential Moving Average).
    pub fn update_avg_batch_size(&mut self, batch_size: usize) {
        const ALPHA: f64 = 0.2;
        if self.batches_published == 0 {
            self.avg_batch_size = batch_size as f64;
        } else {
            self.avg_batch_size =
                ALPHA.mul_add(batch_size as f64, (1.0 - ALPHA) * self.avg_batch_size);
        }
    }

    /// Updates the average batch latency with EMA.
    pub fn update_avg_latency(&mut self, latency_us: u64) {
        const ALPHA: f64 = 0.2;
        if self.batches_published == 0 {
            self.avg_batch_latency_us = latency_us as f64;
        } else {
            self.avg_batch_latency_us =
                ALPHA.mul_add(latency_us as f64, (1.0 - ALPHA) * self.avg_batch_latency_us);
        }
    }

    /// Resets all statistics.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// =============================================================================
// BATCHER EVENT
// =============================================================================

/// Event emitted by the batcher.
///
/// Subscribe to these events for monitoring and observability.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::{BatcherEvent, FlushTrigger};
///
/// let event = BatcherEvent::BatchFlushed {
///     message_count: 50,
///     bytes: 10240,
///     trigger: FlushTrigger::BatchFull,
/// };
/// ```
#[derive(Debug, Clone)]
pub enum BatcherEvent {
    /// Message was enqueued.
    MessageEnqueued {
        /// Topic of the message.
        topic: String,
        /// Size in bytes.
        size: usize,
    },
    /// Batch was flushed.
    BatchFlushed {
        /// Number of messages in batch.
        message_count: usize,
        /// Total bytes.
        bytes: usize,
        /// What triggered the flush.
        trigger: FlushTrigger,
    },
    /// Backpressure status changed.
    BackpressureChanged {
        /// Previous status.
        old_status: BackpressureStatus,
        /// New status.
        new_status: BackpressureStatus,
        /// Current queue depth.
        queue_depth: usize,
    },
    /// Message was dropped.
    MessageDropped {
        /// Topic of dropped message.
        topic: String,
        /// Reason for dropping.
        reason: DropReason,
    },
    /// Error occurred.
    Error {
        /// The error that occurred.
        error: BatcherError,
    },
}

// =============================================================================
// BATCHER ERROR
// =============================================================================

/// Errors that can occur in the batcher.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::BatcherError;
///
/// let error = BatcherError::InvalidConfig {
///     reason: "max_batch_size cannot be zero".to_string(),
/// };
/// assert!(error.to_string().contains("Invalid configuration"));
/// ```
#[derive(Debug, Clone, Error)]
pub enum BatcherError {
    /// Configuration is invalid.
    #[error("Invalid configuration: {reason}")]
    InvalidConfig {
        /// Reason the configuration is invalid.
        reason: String,
    },

    /// Channel is full and overflow action is Block.
    #[error("Channel full, blocking producer")]
    ChannelFull,

    /// Message was dropped due to overflow.
    #[error("Message dropped: {reason:?}")]
    MessageDropped {
        /// Reason the message was dropped.
        reason: DropReason,
    },

    /// Batch publish failed.
    #[error("Batch publish failed: {reason}")]
    PublishFailed {
        /// Reason the publish failed.
        reason: String,
    },

    /// Batcher is not running.
    #[error("Batcher not running")]
    NotRunning,

    /// Batcher is already running.
    #[error("Batcher already running")]
    AlreadyRunning,

    /// Shutdown timed out.
    #[error("Shutdown timed out after {timeout_ms}ms")]
    ShutdownTimeout {
        /// Timeout in milliseconds.
        timeout_ms: u64,
    },

    /// Redis stream error.
    #[error("Stream error: {reason}")]
    StreamError {
        /// Description of the stream error.
        reason: String,
    },
}

/// Result type for batcher operations.
pub type BatcherResult<T> = Result<T, BatcherError>;

// =============================================================================
// BACKPRESSURE MONITOR
// =============================================================================

/// Monitor for tracking and updating backpressure status.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::{BackpressureMonitor, BackpressureStatus};
///
/// let monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);
/// let status = monitor.check(5000);
/// assert_eq!(status, BackpressureStatus::Normal);
/// ```
#[derive(Debug, Clone)]
pub struct BackpressureMonitor {
    /// Warning threshold (0.0 - 1.0).
    warn_threshold: f64,
    /// Critical threshold (0.0 - 1.0).
    critical_threshold: f64,
    /// Channel capacity.
    capacity: usize,
    /// Current status.
    current_status: BackpressureStatus,
}

impl BackpressureMonitor {
    /// Creates a new backpressure monitor.
    #[must_use]
    pub const fn new(warn_threshold: f64, critical_threshold: f64, capacity: usize) -> Self {
        Self {
            warn_threshold,
            critical_threshold,
            capacity,
            current_status: BackpressureStatus::Normal,
        }
    }

    /// Creates a monitor from a `BatchConfig`.
    #[must_use]
    pub fn from_config(config: &BatchConfig) -> Self {
        Self::new(
            config.warn_threshold,
            config.critical_threshold,
            config.channel_capacity,
        )
    }

    /// Checks the queue depth and returns the new status.
    #[must_use]
    pub fn check(&self, queue_depth: usize) -> BackpressureStatus {
        let utilization = queue_depth as f64 / self.capacity as f64;

        if utilization >= self.critical_threshold {
            BackpressureStatus::Critical
        } else if utilization >= self.warn_threshold {
            BackpressureStatus::Warning
        } else {
            BackpressureStatus::Normal
        }
    }

    /// Updates the monitor and returns the new status, indicating if it changed.
    pub fn update(&mut self, queue_depth: usize) -> (BackpressureStatus, bool) {
        let new_status = self.check(queue_depth);
        let changed = new_status != self.current_status;
        self.current_status = new_status;
        (new_status, changed)
    }

    /// Returns the current status.
    #[must_use]
    pub const fn status(&self) -> BackpressureStatus {
        self.current_status
    }

    /// Returns the channel capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the utilization percentage for a given queue depth.
    #[must_use]
    pub fn utilization(&self, queue_depth: usize) -> f64 {
        queue_depth as f64 / self.capacity as f64
    }
}

// =============================================================================
// BATCH ACCUMULATOR
// =============================================================================

/// Accumulator for collecting messages into batches.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::batch::{BatchAccumulator, BatchMessage};
/// use astra_flash::publisher::stream::SerializationFormat;
/// use astra_flash::core::types::now_micros;
///
/// let mut accumulator = BatchAccumulator::new(100);
///
/// let msg = BatchMessage {
///     topic: "test".to_string(),
///     data: vec![1, 2, 3],
///     format: SerializationFormat::Bincode,
///     enqueued_at: now_micros(),
///     priority: 0,
/// };
///
/// accumulator.add(msg);
/// assert!(!accumulator.is_ready());
/// ```
#[derive(Debug)]
pub struct BatchAccumulator {
    /// Current batch being accumulated.
    batch: MessageBatch,
    /// Maximum batch size.
    max_size: usize,
    /// Minimum batch size for timeout flush.
    min_size: usize,
    /// First message timestamp for latency tracking.
    first_message_at: Option<Timestamp>,
}

impl BatchAccumulator {
    /// Creates a new accumulator with the given max size.
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            batch: MessageBatch::new(),
            max_size,
            min_size: 1,
            first_message_at: None,
        }
    }

    /// Creates an accumulator from a `BatchConfig`.
    #[must_use]
    pub fn from_config(config: &BatchConfig) -> Self {
        Self {
            batch: MessageBatch::new(),
            max_size: config.max_batch_size,
            min_size: config.min_batch_size,
            first_message_at: None,
        }
    }

    /// Adds a message to the batch.
    pub fn add(&mut self, message: BatchMessage) {
        if self.first_message_at.is_none() {
            self.first_message_at = Some(message.enqueued_at);
        }
        self.batch.add(message);
    }

    /// Returns true if the batch is ready (at max size).
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.batch.len() >= self.max_size
    }

    /// Returns true if the batch meets minimum size for timeout flush.
    #[must_use]
    pub fn meets_minimum(&self) -> bool {
        self.batch.len() >= self.min_size
    }

    /// Returns true if the batch is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Returns the number of messages in the batch.
    #[must_use]
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Takes the current batch and resets the accumulator.
    pub fn take(&mut self) -> MessageBatch {
        let batch = std::mem::take(&mut self.batch);
        self.first_message_at = None;
        batch
    }

    /// Returns the latency from first message (microseconds).
    #[must_use]
    pub fn latency_us(&self) -> Option<u64> {
        self.first_message_at.map(|first| {
            let now = now_micros();
            if now > first {
                (now - first) as u64
            } else {
                0
            }
        })
    }

    /// Returns the total bytes in the current batch.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.batch.total_bytes
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overflow_action_default() {
        assert_eq!(OverflowAction::default(), OverflowAction::Block);
    }

    #[test]
    fn test_backpressure_status_default() {
        assert_eq!(BackpressureStatus::default(), BackpressureStatus::Normal);
    }

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_batch_size, 100);
        assert_eq!(config.channel_capacity, 10_000);
    }

    #[test]
    fn test_batch_config_validation_zero_size() {
        let mut config = BatchConfig::default();
        config.max_batch_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_batch_config_validation_inverted_thresholds() {
        let mut config = BatchConfig::default();
        config.warn_threshold = 0.95;
        config.critical_threshold = 0.80;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_message_batch_add() {
        let mut batch = MessageBatch::new();
        let msg = BatchMessage {
            topic: "test".to_string(),
            data: vec![1, 2, 3],
            format: SerializationFormat::Bincode,
            enqueued_at: now_micros(),
            priority: 0,
        };
        batch.add(msg);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.total_bytes, 3);
    }

    #[test]
    fn test_message_batch_clear() {
        let mut batch = MessageBatch::new();
        let msg = BatchMessage {
            topic: "test".to_string(),
            data: vec![1, 2, 3],
            format: SerializationFormat::Bincode,
            enqueued_at: now_micros(),
            priority: 0,
        };
        batch.add(msg);
        batch.clear();
        assert!(batch.is_empty());
        assert_eq!(batch.total_bytes, 0);
    }

    #[test]
    fn test_backpressure_monitor_check() {
        let monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);

        assert_eq!(monitor.check(5000), BackpressureStatus::Normal);
        assert_eq!(monitor.check(8500), BackpressureStatus::Warning);
        assert_eq!(monitor.check(9600), BackpressureStatus::Critical);
    }

    #[test]
    fn test_batch_accumulator_ready() {
        let mut acc = BatchAccumulator::new(3);

        for i in 0..3 {
            acc.add(BatchMessage {
                topic: "test".to_string(),
                data: vec![i as u8],
                format: SerializationFormat::Bincode,
                enqueued_at: now_micros(),
                priority: 0,
            });
        }

        assert!(acc.is_ready());
    }

    #[test]
    fn test_batch_accumulator_take() {
        let mut acc = BatchAccumulator::new(10);

        acc.add(BatchMessage {
            topic: "test".to_string(),
            data: vec![1, 2, 3],
            format: SerializationFormat::Bincode,
            enqueued_at: now_micros(),
            priority: 0,
        });

        let batch = acc.take();
        assert_eq!(batch.len(), 1);
        assert!(acc.is_empty());
    }

    #[test]
    fn test_batcher_stats_update_peak() {
        let mut stats = BatcherStats::default();
        stats.update_peak(100);
        assert_eq!(stats.peak_queue_depth, 100);
        stats.update_peak(50);
        assert_eq!(stats.peak_queue_depth, 100); // Should not decrease
    }

    #[test]
    fn test_batcher_stats_update_avg() {
        let mut stats = BatcherStats::default();
        stats.update_avg_batch_size(100);
        assert!((stats.avg_batch_size - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_flush_trigger_display() {
        assert_eq!(FlushTrigger::BatchFull.to_string(), "BatchFull");
        assert_eq!(FlushTrigger::Timeout.to_string(), "Timeout");
    }

    #[test]
    fn test_drop_reason_display() {
        assert_eq!(DropReason::QueueFull.to_string(), "QueueFull");
        assert_eq!(DropReason::Evicted.to_string(), "Evicted");
    }
}
