//! Reconnection Logic for Flash.
//!
//! This module provides the [`ReconnectionManager`] for automatic WebSocket
//! reconnection with exponential backoff and subscription restoration.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         ReconnectionManager                              │
//! │  ┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐   │
//! │  │ ReconnectionConfig │  │ ReconnectionState │  │   EventEmitter    │   │
//! │  │ (backoff params,   │  │ (per-exchange     │  │   (reconnect      │   │
//! │  │  max retries)      │  │  tracking)        │  │   events)         │   │
//! │  └───────────────────┘  └───────────────────┘  └───────────────────┘   │
//! │                                    │                                     │
//! └────────────────────────────────────│─────────────────────────────────────┘
//!                                      ▼
//!                     [Connector] <──> [HeartbeatManager]
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::network::reconnect::{ReconnectionManager, ReconnectionConfig};
//! use astra_flash::network::connector::Connector;
//! use astra_flash::network::heartbeat::HeartbeatManager;
//! use std::sync::Arc;
//!
//! let (connector, _, _) = Connector::new(ws_config, metrics.clone());
//! let connector = Arc::new(connector);
//!
//! let (heartbeat, _) = HeartbeatManager::new(hb_config, Arc::clone(&connector), metrics.clone());
//! let heartbeat = Arc::new(heartbeat);
//!
//! let (reconnection, mut events) = ReconnectionManager::new(
//!     ReconnectionConfig::default(),
//!     connector,
//!     heartbeat,
//!     metrics,
//! );
//!
//! // Enable reconnection for an exchange
//! reconnection.enable(Exchange::Deribit, "wss://www.deribit.com/ws/api/v2");
//!
//! // Register subscriptions to restore after reconnect
//! reconnection.register_subscription(Exchange::Deribit, subscribe_msg);
//! ```

use crate::core::error::FlashError;
use crate::core::metrics::FlashMetrics;
use crate::core::types::Exchange;
use crate::network::connector::Connector;
use crate::network::heartbeat::HeartbeatManager;
use dashmap::DashMap;
use rand::Rng;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

// =============================================================================
// CHANNEL BUFFER SIZES
// =============================================================================

/// Buffer size for reconnection event channel.
const EVENT_CHANNEL_SIZE: usize = 1000;

// =============================================================================
// RECONNECTION STATUS
// =============================================================================

/// Status of reconnection for an exchange.
///
/// # State Machine
///
/// ```text
/// Disabled ──enable()──> Idle
///                          │
///                     disconnect
///                          │
///                          ▼
///                    Reconnecting ──success──> Idle
///                          │
///                       failure
///                          │
///                          ▼
///                       Waiting ──timeout──> Reconnecting
///                          │
///                     max retries
///                          │
///                          ▼
///                       Failed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReconnectionStatus {
    /// No reconnection in progress, connection is stable.
    #[default]
    Idle,
    /// Currently attempting to reconnect.
    Reconnecting,
    /// Waiting for backoff delay before next attempt.
    Waiting,
    /// Restoring subscriptions after successful reconnect.
    RestoringSubscriptions,
    /// All retries exhausted, manual intervention required.
    Failed,
    /// Reconnection disabled by user.
    Disabled,
}

impl ReconnectionStatus {
    /// Returns true if status is Idle.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Returns true if status is Reconnecting.
    #[must_use]
    pub const fn is_reconnecting(&self) -> bool {
        matches!(self, Self::Reconnecting)
    }

    /// Returns true if actively trying to reconnect.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Reconnecting | Self::Waiting | Self::RestoringSubscriptions
        )
    }

    /// Returns true if reconnection has failed.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Returns true if reconnection is disabled.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    /// Convert to numeric value for metrics.
    #[must_use]
    pub const fn as_u8(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Reconnecting => 1,
            Self::Waiting => 2,
            Self::RestoringSubscriptions => 3,
            Self::Failed => 4,
            Self::Disabled => 5,
        }
    }
}

impl std::fmt::Display for ReconnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Reconnecting => write!(f, "reconnecting"),
            Self::Waiting => write!(f, "waiting"),
            Self::RestoringSubscriptions => write!(f, "restoring_subscriptions"),
            Self::Failed => write!(f, "failed"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

// =============================================================================
// RECONNECTION CONFIG
// =============================================================================

/// Configuration for reconnection behavior.
///
/// # Example
///
/// ```rust,ignore
/// let config = ReconnectionConfig {
///     initial_delay_ms: 1_000,
///     max_delay_ms: 30_000,
///     max_retries: 10,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Initial delay before first reconnection attempt (milliseconds).
    pub initial_delay_ms: u64,
    /// Maximum delay between reconnection attempts (milliseconds).
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// Jitter percentage (0.0 - 0.5) for delay randomization.
    pub jitter_percent: f64,
    /// Maximum number of reconnection attempts (0 = unlimited).
    pub max_retries: u32,
    /// Whether to automatically reconnect on disconnect.
    pub auto_reconnect: bool,
    /// Whether to restore subscriptions after reconnect.
    pub restore_subscriptions: bool,
    /// Delay after successful connect before restoring subscriptions (ms).
    pub restore_delay_ms: u64,
    /// Whether to request snapshot on reconnect (for sequence gap handling).
    pub request_snapshot_on_reconnect: bool,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1_000,
            max_delay_ms: 30_000,
            backoff_multiplier: 2.0,
            jitter_percent: 0.25,
            max_retries: 10,
            auto_reconnect: true,
            restore_subscriptions: true,
            restore_delay_ms: 500,
            request_snapshot_on_reconnect: true,
        }
    }
}

impl ReconnectionConfig {
    /// Validate this configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration values are invalid.
    pub fn validate(&self) -> Result<(), FlashError> {
        // jitter must be 0.0 to 0.5
        if !(0.0..=0.5).contains(&self.jitter_percent) {
            return Err(FlashError::ConfigError(
                "jitter_percent must be between 0.0 and 0.5".to_string(),
            ));
        }

        // multiplier must be >= 1.0
        if self.backoff_multiplier < 1.0 {
            return Err(FlashError::ConfigError(
                "backoff_multiplier must be >= 1.0".to_string(),
            ));
        }

        // max_delay must be >= initial_delay
        if self.max_delay_ms < self.initial_delay_ms {
            return Err(FlashError::ConfigError(
                "max_delay_ms must be >= initial_delay_ms".to_string(),
            ));
        }

        Ok(())
    }

    /// Get initial delay as Duration.
    #[must_use]
    pub const fn initial_delay(&self) -> Duration {
        Duration::from_millis(self.initial_delay_ms)
    }

    /// Get max delay as Duration.
    #[must_use]
    pub const fn max_delay(&self) -> Duration {
        Duration::from_millis(self.max_delay_ms)
    }

    /// Get restore delay as Duration.
    #[must_use]
    pub const fn restore_delay(&self) -> Duration {
        Duration::from_millis(self.restore_delay_ms)
    }

    /// Calculate delay for the given attempt number.
    ///
    /// Uses exponential backoff with jitter:
    /// `delay = min(initial * multiplier^(attempt-1) + jitter, max_delay)`
    ///
    /// # Arguments
    ///
    /// * `attempt` - The attempt number (1-based)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ReconnectionConfig::default();
    /// let delay1 = config.calculate_delay(1); // ~1s
    /// let delay2 = config.calculate_delay(2); // ~2s
    /// let delay3 = config.calculate_delay(3); // ~4s
    /// ```
    #[must_use]
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        // Treat 0 as 1
        let attempt = attempt.max(1);

        // Calculate base delay with exponential backoff
        let base_delay = self.initial_delay_ms as f64
            * self
                .backoff_multiplier
                .powi((attempt.saturating_sub(1)) as i32);

        // Cap at max delay
        let capped_delay = base_delay.min(self.max_delay_ms as f64);

        // Add jitter (±jitter_percent)
        if self.jitter_percent > 0.0 {
            let jitter_range = capped_delay * self.jitter_percent;
            let jitter = rand::rng().random::<f64>().mul_add(2.0, -1.0) * jitter_range;
            let final_delay = (capped_delay + jitter).max(1.0) as u64;
            Duration::from_millis(final_delay)
        } else {
            Duration::from_millis(capped_delay as u64)
        }
    }
}

// =============================================================================
// RECONNECTION EVENT
// =============================================================================

/// Events emitted by the reconnection manager.
///
/// Subscribe to these events to monitor reconnection activity.
#[derive(Debug, Clone)]
pub enum ReconnectionEvent {
    /// Reconnection attempt started.
    AttemptStarted {
        /// The exchange attempting reconnection.
        exchange: Exchange,
        /// Which attempt this is (1-based).
        attempt: u32,
        /// Maximum configured retries.
        max_retries: u32,
        /// Delay before this attempt was started.
        delay_ms: u64,
    },

    /// Reconnection attempt succeeded.
    AttemptSucceeded {
        /// The exchange that reconnected.
        exchange: Exchange,
        /// Which attempt succeeded.
        attempt: u32,
        /// Total time from first attempt to success.
        duration_ms: u64,
    },

    /// Reconnection attempt failed.
    AttemptFailed {
        /// The exchange that failed.
        exchange: Exchange,
        /// Which attempt failed.
        attempt: u32,
        /// Error message.
        error: String,
        /// Delay until next attempt (if any).
        next_delay_ms: Option<u64>,
    },

    /// All retries exhausted.
    RetriesExhausted {
        /// The exchange that exhausted retries.
        exchange: Exchange,
        /// Total attempts made.
        total_attempts: u32,
        /// Total time spent trying.
        total_duration_ms: u64,
    },

    /// Subscription restoration started.
    RestoringSubscriptions {
        /// The exchange restoring subscriptions.
        exchange: Exchange,
        /// Number of subscriptions to restore.
        subscription_count: usize,
    },

    /// Subscription restored.
    SubscriptionRestored {
        /// The exchange.
        exchange: Exchange,
        /// The subscription that was restored.
        subscription: String,
    },

    /// All subscriptions restored.
    SubscriptionsRestored {
        /// The exchange.
        exchange: Exchange,
        /// Number of successfully restored subscriptions.
        restored_count: usize,
        /// Number of failed restorations.
        failed_count: usize,
    },

    /// Sequence gap detected.
    SequenceGap {
        /// The exchange with the gap.
        exchange: Exchange,
        /// Expected sequence number.
        expected: u64,
        /// Received sequence number.
        received: u64,
    },

    /// Snapshot requested due to gap.
    SnapshotRequested {
        /// The exchange.
        exchange: Exchange,
        /// Reason for snapshot request.
        reason: String,
    },

    /// Reconnection manager started for exchange.
    Started {
        /// The exchange.
        exchange: Exchange,
    },

    /// Reconnection manager stopped for exchange.
    Stopped {
        /// The exchange.
        exchange: Exchange,
        /// Reason for stopping.
        reason: String,
    },

    /// Backoff delay started.
    BackoffStarted {
        /// The exchange.
        exchange: Exchange,
        /// Delay duration in milliseconds.
        delay_ms: u64,
        /// Which attempt this is for.
        attempt: u32,
    },

    /// Manual reconnection triggered.
    ManualReconnect {
        /// The exchange.
        exchange: Exchange,
    },
}

// =============================================================================
// SEQUENCE GAP
// =============================================================================

/// Information about a sequence number gap.
#[derive(Debug, Clone)]
pub struct SequenceGap {
    /// Expected sequence number.
    pub expected: u64,
    /// Received sequence number.
    pub received: u64,
}

impl SequenceGap {
    /// Get the size of the gap (how many messages were missed).
    #[must_use]
    pub const fn gap_size(&self) -> u64 {
        self.received.saturating_sub(self.expected)
    }
}

// =============================================================================
// RECONNECTION STATS
// =============================================================================

/// Statistics for reconnection activity.
#[derive(Debug, Clone)]
pub struct ReconnectionStats {
    /// Current reconnection status.
    pub status: ReconnectionStatus,
    /// Current attempt number (0 if idle).
    pub current_attempt: u32,
    /// Maximum configured retries.
    pub max_retries: u32,
    /// Time since reconnection started (if active).
    pub since_started: Option<Duration>,
    /// Time until next attempt (if waiting).
    pub until_next_attempt: Option<Duration>,
    /// Last error message.
    pub last_error: Option<String>,
    /// Total reconnection attempts (lifetime).
    pub total_attempts: u64,
    /// Successful reconnections (lifetime).
    pub successful_reconnects: u64,
    /// Success rate (0.0 - 1.0).
    pub success_rate: f64,
}

// =============================================================================
// INTERNAL STATE
// =============================================================================

/// Internal reconnection state per exchange.
struct ReconnectionState {
    /// Current reconnection status.
    status: ReconnectionStatus,
    /// Current attempt number (1-based, 0 = idle).
    attempt: AtomicU64,
    /// Time of last reconnection attempt (for future backoff tracking).
    #[allow(dead_code)]
    last_attempt: std::sync::RwLock<Option<Instant>>,
    /// Time when reconnection started.
    started_at: std::sync::RwLock<Option<Instant>>,
    /// URL to reconnect to.
    url: std::sync::RwLock<String>,
    /// Subscriptions to restore after reconnect.
    subscriptions: std::sync::RwLock<Vec<String>>,
    /// Last known sequence number (for gap detection).
    last_sequence: AtomicU64,
    /// Whether sequence has been initialized.
    sequence_initialized: AtomicBool,
    /// Whether reconnection is enabled for this exchange.
    enabled: AtomicBool,
    /// Total reconnection attempts (lifetime).
    total_attempts: AtomicU64,
    /// Successful reconnections (lifetime).
    successful_reconnects: AtomicU64,
    /// Last error message.
    last_error: std::sync::RwLock<Option<String>>,
    /// Task handle for reconnection task (for future task management).
    #[allow(dead_code)]
    task_handle: std::sync::RwLock<Option<JoinHandle<()>>>,
    /// Shutdown signal for this exchange's task.
    shutdown_tx: std::sync::RwLock<Option<broadcast::Sender<()>>>,
}

impl ReconnectionState {
    const fn new() -> Self {
        Self {
            status: ReconnectionStatus::Disabled,
            attempt: AtomicU64::new(0),
            last_attempt: std::sync::RwLock::new(None),
            started_at: std::sync::RwLock::new(None),
            url: std::sync::RwLock::new(String::new()),
            subscriptions: std::sync::RwLock::new(Vec::new()),
            last_sequence: AtomicU64::new(0),
            sequence_initialized: AtomicBool::new(false),
            enabled: AtomicBool::new(false),
            total_attempts: AtomicU64::new(0),
            successful_reconnects: AtomicU64::new(0),
            last_error: std::sync::RwLock::new(None),
            task_handle: std::sync::RwLock::new(None),
            shutdown_tx: std::sync::RwLock::new(None),
        }
    }
}

// =============================================================================
// TYPE ALIASES
// =============================================================================

/// Receiver for reconnection events.
pub type ReconnectionEventReceiver = mpsc::Receiver<ReconnectionEvent>;

// =============================================================================
// RECONNECTION MANAGER
// =============================================================================

/// Reconnection manager for automatic WebSocket recovery.
///
/// Manages reconnection attempts for all exchanges with
/// exponential backoff and subscription restoration.
///
/// # Thread Safety
///
/// `ReconnectionManager` is both `Send` and `Sync`, making it safe to share
/// across async tasks and threads.
///
/// # Example
///
/// ```rust,ignore
/// let (reconnection, events) = ReconnectionManager::new(config, connector, heartbeat, metrics);
///
/// // Enable reconnection
/// reconnection.enable(Exchange::Deribit, "wss://www.deribit.com/ws/api/v2");
///
/// // Register subscriptions
/// reconnection.register_subscription(Exchange::Deribit, subscribe_msg);
///
/// // Check status
/// if reconnection.is_reconnecting(Exchange::Deribit) {
///     println!("Reconnecting...");
/// }
/// ```
pub struct ReconnectionManager {
    /// Configuration for reconnection behavior.
    config: ReconnectionConfig,
    /// Reconnection state per exchange.
    states: DashMap<Exchange, ReconnectionState>,
    /// Channel for emitting reconnection events.
    event_tx: mpsc::Sender<ReconnectionEvent>,
    /// Reference to the connector for reconnection (for future auto-reconnect).
    #[allow(dead_code)]
    connector: Arc<Connector>,
    /// Reference to heartbeat manager for health monitoring (for future health checks).
    #[allow(dead_code)]
    heartbeat: Arc<HeartbeatManager>,
    /// Metrics for monitoring (for future metrics integration).
    #[allow(dead_code)]
    metrics: FlashMetrics,
    /// Global shutdown signal.
    shutdown_tx: broadcast::Sender<()>,
}

impl ReconnectionManager {
    /// Create a new reconnection manager.
    ///
    /// # Arguments
    ///
    /// * `config` - Reconnection configuration
    /// * `connector` - Reference to the connection manager
    /// * `heartbeat` - Reference to the heartbeat manager
    /// * `metrics` - Metrics instance for monitoring
    ///
    /// # Returns
    ///
    /// A tuple of the `ReconnectionManager` and a receiver for events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (reconnection, mut events) = ReconnectionManager::new(
    ///     config, connector, heartbeat, metrics
    /// );
    ///
    /// tokio::spawn(async move {
    ///     while let Some(event) = events.recv().await {
    ///         println!("Reconnection event: {:?}", event);
    ///     }
    /// });
    /// ```
    #[must_use]
    pub fn new(
        config: ReconnectionConfig,
        connector: Arc<Connector>,
        heartbeat: Arc<HeartbeatManager>,
        metrics: FlashMetrics,
    ) -> (Self, ReconnectionEventReceiver) {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (shutdown_tx, _) = broadcast::channel(1);

        let manager = Self {
            config,
            states: DashMap::new(),
            event_tx,
            connector,
            heartbeat,
            metrics,
            shutdown_tx,
        };

        (manager, event_rx)
    }

    /// Enable automatic reconnection for an exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to enable reconnection for
    /// * `url` - WebSocket URL to reconnect to
    pub fn enable(&self, exchange: Exchange, url: &str) {
        let state = self
            .states
            .entry(exchange)
            .or_insert_with(ReconnectionState::new);

        state.enabled.store(true, Ordering::SeqCst);
        *state.url.write().unwrap() = url.to_string();

        // Update status to Idle if was Disabled
        drop(state);

        if let Some(mut state) = self.states.get_mut(&exchange) {
            if state.status == ReconnectionStatus::Disabled {
                state.status = ReconnectionStatus::Idle;
            }
        }

        // Emit started event
        let _ = self
            .event_tx
            .try_send(ReconnectionEvent::Started { exchange });
    }

    /// Disable automatic reconnection for an exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to disable reconnection for
    pub fn disable(&self, exchange: Exchange) {
        if let Some(mut state) = self.states.get_mut(&exchange) {
            state.enabled.store(false, Ordering::SeqCst);
            state.status = ReconnectionStatus::Disabled;

            // Send shutdown signal if active
            if let Some(tx) = state.shutdown_tx.write().unwrap().take() {
                let _ = tx.send(());
            }
        }

        // Emit stopped event
        let _ = self.event_tx.try_send(ReconnectionEvent::Stopped {
            exchange,
            reason: "user disabled".to_string(),
        });
    }

    /// Trigger manual reconnection attempt.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to reconnect
    ///
    /// # Errors
    ///
    /// Returns error if reconnection is disabled for this exchange.
    #[allow(clippy::unused_async)] // Async for API consistency; will await actual reconnect in future
    pub async fn reconnect_now(&self, exchange: Exchange) -> Result<(), FlashError> {
        let state = self.states.get(&exchange);

        if state.is_none() || !state.as_ref().unwrap().enabled.load(Ordering::SeqCst) {
            return Err(FlashError::ConfigError(format!(
                "Reconnection disabled for {exchange}"
            )));
        }

        // Emit manual reconnect event
        let _ = self
            .event_tx
            .try_send(ReconnectionEvent::ManualReconnect { exchange });

        // In a full implementation, this would trigger reconnection
        Ok(())
    }

    /// Check if reconnection is in progress.
    #[must_use]
    pub fn is_reconnecting(&self, exchange: Exchange) -> bool {
        self.status(exchange).is_active()
    }

    /// Get current reconnection status.
    #[must_use]
    pub fn status(&self, exchange: Exchange) -> ReconnectionStatus {
        self.states
            .get(&exchange)
            .map_or(ReconnectionStatus::Disabled, |s| s.status)
    }

    /// Get reconnection statistics.
    ///
    /// Returns `None` if reconnection is not enabled for this exchange.
    #[must_use]
    pub fn stats(&self, exchange: Exchange) -> Option<ReconnectionStats> {
        self.states.get(&exchange).and_then(|state| {
            if !state.enabled.load(Ordering::SeqCst) {
                return None;
            }

            let total_attempts = state.total_attempts.load(Ordering::SeqCst);
            let successful = state.successful_reconnects.load(Ordering::SeqCst);
            let success_rate = if total_attempts > 0 {
                successful as f64 / total_attempts as f64
            } else {
                0.0
            };

            let since_started = state.started_at.read().unwrap().map(|t| t.elapsed());

            Some(ReconnectionStats {
                status: state.status,
                current_attempt: state.attempt.load(Ordering::SeqCst) as u32,
                max_retries: self.config.max_retries,
                since_started,
                until_next_attempt: None, // Would require tracking next attempt time
                last_error: state.last_error.read().unwrap().clone(),
                total_attempts,
                successful_reconnects: successful,
                success_rate,
            })
        })
    }

    /// Register subscription for restoration.
    ///
    /// Subscriptions are restored after successful reconnection.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange
    /// * `subscription` - Subscription message to restore
    pub fn register_subscription(&self, exchange: Exchange, subscription: String) {
        let state = self
            .states
            .entry(exchange)
            .or_insert_with(ReconnectionState::new);
        state.subscriptions.write().unwrap().push(subscription);
    }

    /// Clear all registered subscriptions.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to clear subscriptions for
    pub fn clear_subscriptions(&self, exchange: Exchange) {
        if let Some(state) = self.states.get(&exchange) {
            state.subscriptions.write().unwrap().clear();
        }
    }

    /// Record sequence number for gap detection.
    ///
    /// Returns `Some(SequenceGap)` if a gap is detected.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange
    /// * `sequence` - Received sequence number
    #[must_use]
    pub fn record_sequence(&self, exchange: Exchange, sequence: u64) -> Option<SequenceGap> {
        let state = self
            .states
            .entry(exchange)
            .or_insert_with(ReconnectionState::new);

        if !state.sequence_initialized.load(Ordering::SeqCst) {
            // First sequence, just initialize
            state.last_sequence.store(sequence, Ordering::SeqCst);
            state.sequence_initialized.store(true, Ordering::SeqCst);
            return None;
        }

        let last = state.last_sequence.load(Ordering::SeqCst);

        // Check for gap (expected is last + 1, but allow same sequence for duplicates)
        if sequence != last + 1 && sequence != last && sequence > last {
            let gap = SequenceGap {
                expected: last + 1,
                received: sequence,
            };

            // Emit gap event
            let _ = self.event_tx.try_send(ReconnectionEvent::SequenceGap {
                exchange,
                expected: gap.expected,
                received: gap.received,
            });

            // Update sequence
            state.last_sequence.store(sequence, Ordering::SeqCst);

            return Some(gap);
        }

        // Update sequence
        state.last_sequence.store(sequence, Ordering::SeqCst);
        None
    }

    /// Calculate next backoff delay.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The attempt number (1-based)
    #[must_use]
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        self.config.calculate_delay(attempt)
    }

    /// Reset reconnection state (after successful connection).
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to reset
    pub fn reset(&self, exchange: Exchange) {
        if let Some(mut state) = self.states.get_mut(&exchange) {
            state.status = ReconnectionStatus::Idle;
            state.attempt.store(0, Ordering::SeqCst);
            *state.started_at.write().unwrap() = None;
            *state.last_error.write().unwrap() = None;

            // Reset sequence tracking
            state.sequence_initialized.store(false, Ordering::SeqCst);
            state.last_sequence.store(0, Ordering::SeqCst);
        }
    }

    /// Gracefully shutdown all reconnection tasks.
    #[allow(clippy::unused_async)] // Async for graceful shutdown pattern consistency
    pub async fn shutdown(&self) {
        // Send global shutdown signal
        let _ = self.shutdown_tx.send(());

        // Disable all exchanges
        let exchanges: Vec<Exchange> = self.states.iter().map(|e| *e.key()).collect();
        for exchange in exchanges {
            self.disable(exchange);
        }
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

// ReconnectionManager is Send + Sync via DashMap and Arc internals

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnection_status_display() {
        assert_eq!(ReconnectionStatus::Idle.to_string(), "idle");
        assert_eq!(ReconnectionStatus::Reconnecting.to_string(), "reconnecting");
        assert_eq!(ReconnectionStatus::Waiting.to_string(), "waiting");
        assert_eq!(
            ReconnectionStatus::RestoringSubscriptions.to_string(),
            "restoring_subscriptions"
        );
        assert_eq!(ReconnectionStatus::Failed.to_string(), "failed");
        assert_eq!(ReconnectionStatus::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_reconnection_status_methods() {
        assert!(ReconnectionStatus::Idle.is_idle());
        assert!(ReconnectionStatus::Reconnecting.is_reconnecting());
        assert!(ReconnectionStatus::Reconnecting.is_active());
        assert!(ReconnectionStatus::Failed.is_failed());
        assert!(ReconnectionStatus::Disabled.is_disabled());
    }

    #[test]
    fn test_config_default() {
        let config = ReconnectionConfig::default();
        assert_eq!(config.initial_delay_ms, 1_000);
        assert_eq!(config.max_delay_ms, 30_000);
        assert!((config.backoff_multiplier - 2.0).abs() < 0.001);
        assert!((config.jitter_percent - 0.25).abs() < 0.001);
        assert_eq!(config.max_retries, 10);
    }

    #[test]
    fn test_config_validation() {
        let config = ReconnectionConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid = ReconnectionConfig::default();
        invalid.jitter_percent = 0.6;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_backoff_calculation() {
        let config = ReconnectionConfig {
            initial_delay_ms: 1000,
            max_delay_ms: 30_000,
            backoff_multiplier: 2.0,
            jitter_percent: 0.0,
            ..ReconnectionConfig::default()
        };

        assert_eq!(config.calculate_delay(1), Duration::from_millis(1000));
        assert_eq!(config.calculate_delay(2), Duration::from_millis(2000));
        assert_eq!(config.calculate_delay(3), Duration::from_millis(4000));
    }

    #[test]
    fn test_sequence_gap() {
        let gap = SequenceGap {
            expected: 100,
            received: 110,
        };
        assert_eq!(gap.gap_size(), 10);
    }
}
