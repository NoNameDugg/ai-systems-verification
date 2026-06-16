//! Backpressure handling for Flash (Batch 3.3).
//!
//! This module implements non-blocking sends with DROP policy for market data.
//!
//! # Design Principle
//!
//! > **Stale market data is toxic.** If Redis Writer falls behind, DROP incoming
//! > frames rather than blocking the WebSocket reader. The `dropped_frames` metric
//! > alerts operators to capacity issues.
//!
//! # Core Components
//!
//! - [`BackpressureSender`]: Non-blocking sender that uses `try_send`
//! - [`BackpressureMetrics`]: Atomic counters for dropped/sent frames
//! - [`send_with_backpressure`]: Standalone function for existing channels
//!
//! # Example
//!
//! ```rust
//! use astra_flash::publisher::backpressure::{BackpressureSender, BackpressureConfig};
//!
//! // Create sender with default config (capacity=1000, policy=DropNewest)
//! let (sender, mut receiver) = BackpressureSender::<u64>::with_capacity(100);
//!
//! // Non-blocking send - returns immediately
//! match sender.try_send(42) {
//!     Ok(()) => println!("Sent!"),
//!     Err(e) => println!("Dropped: {:?}", e),
//! }
//!
//! // Check metrics
//! let metrics = sender.metrics();
//! println!("Sent: {}, Dropped: {}", metrics.sent_frames(), metrics.dropped_frames());
//! ```
//!
//! # Flood Handling
//!
//! ```rust,ignore
//! // Under flood conditions (producer faster than consumer):
//! for i in 0..100_000 {
//!     let _ = sender.try_send(i);  // Never blocks!
//! }
//!
//! // Check drop rate after flood
//! if sender.metrics().exceeds_alert_threshold(0.01) {
//!     tracing::warn!("Drop rate exceeded 1% threshold!");
//! }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, warn};

// =============================================================================
// BACKPRESSURE POLICY
// =============================================================================

/// Policy for handling backpressure when channel is full.
///
/// # Variants
///
/// - **DropNewest** (default): Drop incoming message, never block. Best for HFT.
/// - **WarnAndContinue**: Log warning and drop message.
///
/// # Example
///
/// ```rust
/// use astra_flash::publisher::backpressure::BackpressurePolicy;
///
/// let policy = BackpressurePolicy::default();
/// assert_eq!(policy, BackpressurePolicy::DropNewest);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BackpressurePolicy {
    /// Drop incoming message when channel is full (default for HFT).
    #[default]
    DropNewest,
    /// Log warning and drop message.
    WarnAndContinue,
}

impl std::fmt::Display for BackpressurePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DropNewest => write!(f, "DropNewest"),
            Self::WarnAndContinue => write!(f, "WarnAndContinue"),
        }
    }
}

// =============================================================================
// BACKPRESSURE CONFIG
// =============================================================================

/// Configuration for backpressure handling.
///
/// # Defaults
///
/// | Field | Default | Description |
/// |-------|---------|-------------|
/// | channel_capacity | 1000 | Bounded channel size |
/// | policy | DropNewest | Drop incoming when full |
/// | warn_on_drop | true | Log warning when frames dropped |
/// | drop_rate_alert_threshold | 0.01 | Alert when >1% dropped |
///
/// # Example
///
/// ```rust
/// use astra_flash::publisher::backpressure::BackpressureConfig;
///
/// let config = BackpressureConfig::builder()
///     .channel_capacity(500)
///     .drop_rate_alert_threshold(0.05)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Channel buffer capacity.
    pub channel_capacity: usize,
    /// Policy when channel is full.
    pub policy: BackpressurePolicy,
    /// Whether to log warnings when frames are dropped.
    pub warn_on_drop: bool,
    /// Threshold for alerting on drop rate (0.0-1.0).
    pub drop_rate_alert_threshold: f64,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1000,
            policy: BackpressurePolicy::DropNewest,
            warn_on_drop: true,
            drop_rate_alert_threshold: 0.01, // 1% threshold
        }
    }
}

impl BackpressureConfig {
    /// Create a new config builder.
    #[must_use]
    pub fn builder() -> BackpressureConfigBuilder {
        BackpressureConfigBuilder::default()
    }
}

// =============================================================================
// BACKPRESSURE CONFIG BUILDER
// =============================================================================

/// Builder for [`BackpressureConfig`].
#[derive(Debug, Default)]
pub struct BackpressureConfigBuilder {
    channel_capacity: Option<usize>,
    policy: Option<BackpressurePolicy>,
    warn_on_drop: Option<bool>,
    drop_rate_alert_threshold: Option<f64>,
}

impl BackpressureConfigBuilder {
    /// Set channel capacity.
    #[must_use]
    pub const fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = Some(capacity);
        self
    }

    /// Set backpressure policy.
    #[must_use]
    pub const fn policy(mut self, policy: BackpressurePolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Set whether to warn on drop.
    #[must_use]
    pub const fn warn_on_drop(mut self, warn: bool) -> Self {
        self.warn_on_drop = Some(warn);
        self
    }

    /// Set drop rate alert threshold.
    #[must_use]
    pub const fn drop_rate_alert_threshold(mut self, threshold: f64) -> Self {
        self.drop_rate_alert_threshold = Some(threshold);
        self
    }

    /// Build the config.
    #[must_use]
    pub fn build(self) -> BackpressureConfig {
        let defaults = BackpressureConfig::default();
        BackpressureConfig {
            channel_capacity: self.channel_capacity.unwrap_or(defaults.channel_capacity),
            policy: self.policy.unwrap_or(defaults.policy),
            warn_on_drop: self.warn_on_drop.unwrap_or(defaults.warn_on_drop),
            drop_rate_alert_threshold: self
                .drop_rate_alert_threshold
                .unwrap_or(defaults.drop_rate_alert_threshold),
        }
    }
}

// =============================================================================
// BACKPRESSURE METRICS
// =============================================================================

/// Atomic metrics for backpressure tracking.
///
/// Thread-safe counters for monitoring frame drops and send rates.
///
/// # Example
///
/// ```rust
/// use astra_flash::publisher::backpressure::BackpressureMetrics;
///
/// let metrics = BackpressureMetrics::default();
///
/// metrics.increment_sent();
/// metrics.increment_sent();
/// metrics.increment_dropped();
///
/// assert_eq!(metrics.sent_frames(), 2);
/// assert_eq!(metrics.dropped_frames(), 1);
/// assert!((metrics.drop_rate() - 0.333).abs() < 0.01);
/// ```
#[derive(Debug, Default)]
pub struct BackpressureMetrics {
    /// Number of frames dropped due to backpressure.
    dropped_frames: AtomicU64,
    /// Number of frames successfully sent.
    sent_frames: AtomicU64,
}

impl BackpressureMetrics {
    /// Create new metrics with all counters at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment dropped frames counter.
    pub fn increment_dropped(&self) {
        self.dropped_frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment sent frames counter.
    pub fn increment_sent(&self) {
        self.sent_frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the number of dropped frames.
    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Get the number of sent frames.
    #[must_use]
    pub fn sent_frames(&self) -> u64 {
        self.sent_frames.load(Ordering::Relaxed)
    }

    /// Get total frames (sent + dropped).
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.sent_frames() + self.dropped_frames()
    }

    /// Calculate drop rate (0.0 to 1.0).
    ///
    /// Returns 0.0 if no frames have been processed.
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        let total = self.total_frames();
        if total == 0 {
            0.0
        } else {
            self.dropped_frames() as f64 / total as f64
        }
    }

    /// Check if drop rate exceeds the given threshold.
    #[must_use]
    pub fn exceeds_alert_threshold(&self, threshold: f64) -> bool {
        self.drop_rate() > threshold
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.dropped_frames.store(0, Ordering::Relaxed);
        self.sent_frames.store(0, Ordering::Relaxed);
    }
}

// =============================================================================
// BACKPRESSURE SEND ERROR
// =============================================================================

/// Error returned when a backpressure send fails.
///
/// Contains the dropped value so it can be logged or processed.
#[derive(Debug, Error)]
pub enum BackpressureSendError<T> {
    /// Channel is full, message was dropped.
    #[error("Channel full: frame dropped (backpressure)")]
    ChannelFull {
        /// The value that was dropped.
        dropped_value: T,
    },

    /// Channel was closed, message was dropped.
    #[error("Channel closed unexpectedly")]
    ChannelClosed {
        /// The value that was dropped.
        dropped_value: T,
    },
}

/// Result type for backpressure send operations.
pub type BackpressureResult<T> = Result<T, BackpressureSendError<T>>;

// =============================================================================
// BACKPRESSURE SENDER
// =============================================================================

/// Non-blocking sender with backpressure handling.
///
/// Wraps `tokio::sync::mpsc::Sender` and provides:
/// - `try_send()` instead of blocking `send()`
/// - Automatic dropped frames metric tracking
/// - Alert threshold monitoring
///
/// # Design
///
/// Uses `try_send()` internally which returns immediately:
/// - On success: increments `sent_frames`
/// - On `Full`: increments `dropped_frames`, never blocks
/// - On `Closed`: increments `dropped_frames`, logs error
///
/// # Example
///
/// ```rust
/// use astra_flash::publisher::backpressure::BackpressureSender;
///
/// # tokio_test::block_on(async {
/// let (sender, mut receiver) = BackpressureSender::<String>::with_capacity(10);
///
/// // Send message (non-blocking)
/// let result = sender.try_send("hello".to_string());
/// assert!(result.is_ok());
///
/// // Check metrics
/// assert_eq!(sender.metrics().sent_frames(), 1);
///
/// // Receive
/// let msg = receiver.recv().await.unwrap();
/// assert_eq!(msg, "hello");
/// # });
/// ```
#[derive(Debug)]
pub struct BackpressureSender<T> {
    /// Inner sender.
    tx: mpsc::Sender<T>,
    /// Shared metrics.
    metrics: Arc<BackpressureMetrics>,
    /// Configuration.
    config: BackpressureConfig,
}

impl<T> Clone for BackpressureSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            metrics: Arc::clone(&self.metrics),
            config: self.config.clone(),
        }
    }
}

impl<T: std::fmt::Debug> BackpressureSender<T> {
    /// Create a new sender with the given config.
    ///
    /// Returns both the sender and the receiver.
    #[must_use]
    pub fn new(config: BackpressureConfig) -> (Self, mpsc::Receiver<T>) {
        let (tx, rx) = mpsc::channel(config.channel_capacity);
        let sender = Self {
            tx,
            metrics: Arc::new(BackpressureMetrics::new()),
            config,
        };
        (sender, rx)
    }

    /// Create a sender with just a capacity (uses default config).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> (Self, mpsc::Receiver<T>) {
        let config = BackpressureConfig::builder()
            .channel_capacity(capacity)
            .build();
        Self::new(config)
    }

    /// Try to send a message without blocking.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if message was sent
    /// - `Err(ChannelFull)` if channel is full (message dropped)
    /// - `Err(ChannelClosed)` if receiver was dropped
    ///
    /// # Example
    ///
    /// ```rust
    /// use astra_flash::publisher::backpressure::BackpressureSender;
    ///
    /// let (sender, _rx) = BackpressureSender::<u64>::with_capacity(10);
    ///
    /// match sender.try_send(42) {
    ///     Ok(()) => println!("Sent!"),
    ///     Err(e) => println!("Dropped: {:?}", e),
    /// }
    /// ```
    pub fn try_send(&self, value: T) -> Result<(), BackpressureSendError<T>> {
        match self.tx.try_send(value) {
            Ok(()) => {
                self.metrics.increment_sent();
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(dropped_value)) => {
                self.metrics.increment_dropped();
                if self.config.warn_on_drop {
                    warn!(
                        dropped_frames = self.metrics.dropped_frames(),
                        drop_rate = %format!("{:.2}%", self.metrics.drop_rate() * 100.0),
                        "Backpressure: dropped frame (channel full)"
                    );
                }
                Err(BackpressureSendError::ChannelFull { dropped_value })
            }
            Err(mpsc::error::TrySendError::Closed(dropped_value)) => {
                self.metrics.increment_dropped();
                error!("Channel closed unexpectedly");
                Err(BackpressureSendError::ChannelClosed { dropped_value })
            }
        }
    }

    /// Get shared metrics reference.
    #[must_use]
    pub fn metrics(&self) -> Arc<BackpressureMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Get the channel capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.config.channel_capacity
    }

    /// Get available capacity in the channel.
    #[must_use]
    pub fn available_capacity(&self) -> usize {
        self.tx.capacity()
    }

    /// Check if channel is full.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.tx.capacity() == 0
    }

    /// Check if channel is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tx.capacity() == self.config.channel_capacity
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &BackpressureConfig {
        &self.config
    }
}

// =============================================================================
// STANDALONE FUNCTION
// =============================================================================

/// Send with backpressure using an existing channel.
///
/// This is the canonical backpressure function from the standard design:
///
/// ```rust
/// use astra_flash::publisher::backpressure::{send_with_backpressure, BackpressureMetrics};
/// use std::sync::Arc;
/// use tokio::sync::mpsc;
///
/// # tokio_test::block_on(async {
/// let (tx, mut rx) = mpsc::channel::<u64>(10);
/// let metrics = Arc::new(BackpressureMetrics::default());
///
/// // Non-blocking send
/// send_with_backpressure(&tx, 42, &metrics);
///
/// assert_eq!(metrics.sent_frames(), 1);
/// # });
/// ```
///
/// # Behavior
///
/// - `Ok` → message sent, increment `sent_frames`
/// - `Full` → DROP message, increment `dropped_frames`, warn
/// - `Closed` → DROP message, increment `dropped_frames`, error
pub fn send_with_backpressure<T: std::fmt::Debug>(
    tx: &mpsc::Sender<T>,
    msg: T,
    metrics: &BackpressureMetrics,
) {
    match tx.try_send(msg) {
        Ok(()) => {
            metrics.increment_sent();
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            metrics.increment_dropped();
            warn!(
                dropped_frames = metrics.dropped_frames(),
                drop_rate = %format!("{:.2}%", metrics.drop_rate() * 100.0),
                "Backpressure: dropped frame (channel full)"
            );
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            metrics.increment_dropped();
            error!("Channel closed unexpectedly");
        }
    }
}

// =============================================================================
// SEND + SYNC VERIFICATION
// =============================================================================

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BackpressurePolicy>();
    assert_send_sync::<BackpressureConfig>();
    assert_send_sync::<BackpressureMetrics>();
    // BackpressureSender is Send+Sync when T is Send
};

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_default() {
        assert_eq!(BackpressurePolicy::default(), BackpressurePolicy::DropNewest);
    }

    #[test]
    fn test_config_default() {
        let config = BackpressureConfig::default();
        assert_eq!(config.channel_capacity, 1000);
        assert_eq!(config.policy, BackpressurePolicy::DropNewest);
    }

    #[test]
    fn test_metrics_basic() {
        let metrics = BackpressureMetrics::new();
        metrics.increment_sent();
        metrics.increment_dropped();
        assert_eq!(metrics.total_frames(), 2);
    }

    #[test]
    fn test_metrics_drop_rate() {
        let metrics = BackpressureMetrics::new();
        for _ in 0..9 {
            metrics.increment_sent();
        }
        metrics.increment_dropped();
        assert!((metrics.drop_rate() - 0.1).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_sender_try_send_success() {
        let (sender, mut rx) = BackpressureSender::<u64>::with_capacity(10);
        assert!(sender.try_send(42).is_ok());
        assert_eq!(rx.recv().await.unwrap(), 42);
    }

    #[test]
    fn test_sender_try_send_full() {
        let (sender, _rx) = BackpressureSender::<u64>::with_capacity(1);
        assert!(sender.try_send(1).is_ok());
        assert!(sender.try_send(2).is_err());
        assert_eq!(sender.metrics().dropped_frames(), 1);
    }
}
