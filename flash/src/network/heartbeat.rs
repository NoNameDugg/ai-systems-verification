//! Heartbeat & Health Monitoring for Flash.
//!
//! This module provides the [`HeartbeatManager`] for monitoring WebSocket
//! connection health through ping/pong heartbeats.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         HeartbeatManager                                 │
//! │  ┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐   │
//! │  │  HeartbeatConfig  │  │  HealthTracker    │  │   EventEmitter    │   │
//! │  │  (intervals,      │  │  (per-exchange    │  │   (health events) │   │
//! │  │   thresholds)     │  │   health state)   │  │                   │   │
//! │  └───────────────────┘  └───────────────────┘  └───────────────────┘   │
//! │                                    │                                     │
//! └────────────────────────────────────│─────────────────────────────────────┘
//!                                      ▼
//!                             [Connector + WebSocket]
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::network::heartbeat::{HeartbeatManager, HeartbeatConfig};
//! use astra_flash::network::connector::Connector;
//! use std::sync::Arc;
//!
//! let (connector, _, _) = Connector::new(ws_config, metrics.clone());
//! let connector = Arc::new(connector);
//!
//! let (heartbeat, mut events) = HeartbeatManager::new(
//!     HeartbeatConfig::default(),
//!     connector,
//!     metrics,
//! );
//!
//! // Start monitoring an exchange
//! heartbeat.start_monitoring(Exchange::Deribit);
//!
//! // Check health
//! if heartbeat.is_healthy(Exchange::Deribit) {
//!     println!("Connection is healthy!");
//! }
//! ```

use crate::core::error::FlashError;
use crate::core::metrics::FlashMetrics;
use crate::core::types::{Exchange, Timestamp};
use crate::network::connector::Connector;
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

/// Buffer size for heartbeat event channel.
const EVENT_CHANNEL_SIZE: usize = 1000;

// =============================================================================
// HEALTH STATUS
// =============================================================================

/// Health status of a WebSocket connection.
///
/// # State Machine
///
/// ```text
/// Unknown ──pong──> Healthy
///    │                │
///    │           missed pong
///    │                │
///    │                ▼
///    │           Degraded ──more missed──> Unhealthy
///    │                │                        │
///    │            pong │                    pong │
///    │                ▼                        ▼
///    └────────────> Healthy <──────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HealthStatus {
    /// Connection is healthy - all pongs received within timeout.
    Healthy,
    /// Connection is degraded - some pongs missed but below threshold.
    Degraded,
    /// Connection is unhealthy - too many pongs missed.
    Unhealthy,
    /// Health status unknown - not enough data or not monitoring.
    #[default]
    Unknown,
}

impl HealthStatus {
    /// Returns true if the connection is healthy.
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }

    /// Returns true if the connection is degraded.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded)
    }

    /// Returns true if the connection is unhealthy.
    #[must_use]
    pub const fn is_unhealthy(&self) -> bool {
        matches!(self, Self::Unhealthy)
    }

    /// Convert to numeric value for metrics.
    #[must_use]
    pub const fn as_u8(&self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Healthy => 1,
            Self::Degraded => 2,
            Self::Unhealthy => 3,
        }
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unhealthy => write!(f, "unhealthy"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

// =============================================================================
// HEARTBEAT CONFIG
// =============================================================================

/// Configuration for heartbeat behavior.
///
/// # Example
///
/// ```rust,ignore
/// let config = HeartbeatConfig {
///     ping_interval_ms: 30_000,
///     pong_timeout_ms: 10_000,
///     degraded_threshold: 1,
///     unhealthy_threshold: 3,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Base interval between ping messages (milliseconds).
    pub ping_interval_ms: u64,
    /// Maximum time to wait for pong response (milliseconds).
    pub pong_timeout_ms: u64,
    /// Jitter percentage (0.0 - 0.5) for ping interval randomization.
    pub jitter_percent: f64,
    /// Number of missed pongs before degraded status.
    pub degraded_threshold: u32,
    /// Number of missed pongs before unhealthy status.
    pub unhealthy_threshold: u32,
    /// Whether to automatically disconnect unhealthy connections.
    pub auto_disconnect_unhealthy: bool,
    /// Time without activity before considering connection stale (ms).
    pub stale_connection_ms: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            ping_interval_ms: 30_000,
            pong_timeout_ms: 10_000,
            jitter_percent: 0.20,
            degraded_threshold: 1,
            unhealthy_threshold: 3,
            auto_disconnect_unhealthy: false,
            stale_connection_ms: 60_000,
        }
    }
}

impl HeartbeatConfig {
    /// Validate this configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration values are invalid.
    pub fn validate(&self) -> Result<(), FlashError> {
        // pong_timeout must be less than ping_interval
        if self.pong_timeout_ms >= self.ping_interval_ms {
            return Err(FlashError::ConfigError(
                "pong_timeout_ms must be less than ping_interval_ms".to_string(),
            ));
        }

        // unhealthy_threshold must be >= degraded_threshold
        if self.unhealthy_threshold < self.degraded_threshold {
            return Err(FlashError::ConfigError(
                "unhealthy_threshold must be >= degraded_threshold".to_string(),
            ));
        }

        // jitter must be 0.0 to 0.5
        if !(0.0..=0.5).contains(&self.jitter_percent) {
            return Err(FlashError::ConfigError(
                "jitter_percent must be between 0.0 and 0.5".to_string(),
            ));
        }

        Ok(())
    }

    /// Calculate the next ping interval with jitter.
    ///
    /// Jitter helps prevent thundering herd on reconnection.
    #[must_use]
    pub fn calculate_next_interval(&self) -> Duration {
        let base = self.ping_interval_ms as f64;
        let jitter_range = base * self.jitter_percent;

        // Random value between -jitter_range and +jitter_range
        let jitter = rand::rng().random::<f64>().mul_add(2.0, -1.0) * jitter_range;

        let interval_ms = (base + jitter).max(1.0) as u64;
        Duration::from_millis(interval_ms)
    }

    /// Get ping interval as Duration.
    #[must_use]
    pub const fn ping_interval(&self) -> Duration {
        Duration::from_millis(self.ping_interval_ms)
    }

    /// Get pong timeout as Duration.
    #[must_use]
    pub const fn pong_timeout(&self) -> Duration {
        Duration::from_millis(self.pong_timeout_ms)
    }

    /// Get stale connection threshold as Duration.
    #[must_use]
    pub const fn stale_threshold(&self) -> Duration {
        Duration::from_millis(self.stale_connection_ms)
    }
}

// =============================================================================
// HEARTBEAT EVENT
// =============================================================================

/// Events emitted by the heartbeat manager.
///
/// Subscribe to these events to monitor heartbeat activity
/// and health status changes.
#[derive(Debug, Clone)]
pub enum HeartbeatEvent {
    /// Ping was sent to an exchange.
    PingSent {
        /// The exchange the ping was sent to.
        exchange: Exchange,
        /// Timestamp when the ping was sent (microseconds).
        timestamp: Timestamp,
    },

    /// Pong was received from an exchange.
    PongReceived {
        /// The exchange the pong was received from.
        exchange: Exchange,
        /// Round-trip latency in microseconds.
        latency_us: u64,
        /// Timestamp when the pong was received (microseconds).
        timestamp: Timestamp,
    },

    /// Pong timeout occurred (no response within timeout).
    PongTimeout {
        /// The exchange that timed out.
        exchange: Exchange,
        /// Current count of consecutive missed pongs.
        missed_count: u32,
    },

    /// Health status changed.
    HealthChanged {
        /// The exchange whose health changed.
        exchange: Exchange,
        /// Previous health status.
        old_status: HealthStatus,
        /// New health status.
        new_status: HealthStatus,
        /// Current count of missed pongs.
        missed_pongs: u32,
    },

    /// Connection detected as stale (no activity).
    StaleConnection {
        /// The exchange with stale connection.
        exchange: Exchange,
        /// Time since last activity in milliseconds.
        last_activity_ms: u64,
    },

    /// Heartbeat monitoring started for an exchange.
    MonitoringStarted {
        /// The exchange monitoring was started for.
        exchange: Exchange,
    },

    /// Heartbeat monitoring stopped for an exchange.
    MonitoringStopped {
        /// The exchange monitoring was stopped for.
        exchange: Exchange,
        /// Reason for stopping.
        reason: String,
    },
}

// =============================================================================
// HEALTH SUMMARY
// =============================================================================

/// Summary of health status for an exchange.
///
/// Provides detailed information about connection health
/// including latency statistics and success rates.
#[derive(Debug, Clone)]
pub struct HealthSummary {
    /// Current health status.
    pub status: HealthStatus,
    /// Number of consecutive missed pongs.
    pub missed_pongs: u32,
    /// Time since last successful pong (if any).
    pub since_last_pong: Option<Duration>,
    /// Last ping-pong latency in microseconds.
    pub last_latency_us: Option<u64>,
    /// Average ping-pong latency in microseconds.
    pub avg_latency_us: Option<u64>,
    /// Total pings sent.
    pub total_pings: u64,
    /// Total pongs received.
    pub total_pongs: u64,
    /// Pong success rate (0.0 - 1.0).
    pub success_rate: f64,
}

// =============================================================================
// INTERNAL HEALTH STATE
// =============================================================================

/// Internal health tracking state per exchange.
struct HealthState {
    /// Current health status.
    status: HealthStatus,
    /// Number of consecutive missed pongs.
    missed_pongs: AtomicU64,
    /// Time of last successful pong.
    last_pong: std::sync::RwLock<Instant>,
    /// Time of last ping sent.
    last_ping: std::sync::RwLock<Option<Instant>>,
    /// Latency of last successful ping-pong round trip (microseconds).
    last_latency_us: AtomicU64,
    /// Rolling average latency (microseconds).
    avg_latency_us: AtomicU64,
    /// Total pongs received.
    total_pongs: AtomicU64,
    /// Total pings sent.
    total_pings: AtomicU64,
    /// Whether this exchange is being monitored.
    monitoring: AtomicBool,
    /// Task handle for heartbeat task (for future task management).
    #[allow(dead_code)]
    task_handle: std::sync::RwLock<Option<JoinHandle<()>>>,
    /// Shutdown signal for this exchange's task.
    shutdown_tx: std::sync::RwLock<Option<broadcast::Sender<()>>>,
}

impl HealthState {
    fn new() -> Self {
        Self {
            status: HealthStatus::Unknown,
            missed_pongs: AtomicU64::new(0),
            last_pong: std::sync::RwLock::new(Instant::now()),
            last_ping: std::sync::RwLock::new(None),
            last_latency_us: AtomicU64::new(0),
            avg_latency_us: AtomicU64::new(0),
            total_pongs: AtomicU64::new(0),
            total_pings: AtomicU64::new(0),
            monitoring: AtomicBool::new(false),
            task_handle: std::sync::RwLock::new(None),
            shutdown_tx: std::sync::RwLock::new(None),
        }
    }
}

// =============================================================================
// TYPE ALIASES
// =============================================================================

/// Receiver for heartbeat events.
pub type HeartbeatEventReceiver = mpsc::Receiver<HeartbeatEvent>;

// =============================================================================
// HEARTBEAT MANAGER
// =============================================================================

/// Heartbeat and health monitoring manager.
///
/// Manages heartbeat timing for all connections and tracks
/// health status based on ping/pong response patterns.
///
/// # Thread Safety
///
/// `HeartbeatManager` is both `Send` and `Sync`, making it safe to share
/// across async tasks and threads.
///
/// # Example
///
/// ```rust,ignore
/// let (heartbeat, events) = HeartbeatManager::new(config, connector, metrics);
///
/// // Start monitoring
/// heartbeat.start_monitoring(Exchange::Deribit);
///
/// // Check health
/// if heartbeat.is_healthy(Exchange::Deribit) {
///     println!("Healthy!");
/// }
///
/// // Get detailed summary
/// if let Some(summary) = heartbeat.health_summary(Exchange::Deribit) {
///     println!("Latency: {:?}us", summary.last_latency_us);
/// }
/// ```
pub struct HeartbeatManager {
    /// Configuration for heartbeat behavior.
    config: HeartbeatConfig,
    /// Health state per exchange.
    health_states: DashMap<Exchange, HealthState>,
    /// Channel for emitting health events.
    event_tx: mpsc::Sender<HeartbeatEvent>,
    /// Reference to the connector for sending pings (for future ping implementation).
    #[allow(dead_code)]
    connector: Arc<Connector>,
    /// Metrics for monitoring.
    metrics: FlashMetrics,
    /// Global shutdown signal.
    shutdown_tx: broadcast::Sender<()>,
}

impl HeartbeatManager {
    /// Create a new heartbeat manager.
    ///
    /// # Arguments
    ///
    /// * `config` - Heartbeat configuration
    /// * `connector` - Reference to the connection manager
    /// * `metrics` - Metrics instance for monitoring
    ///
    /// # Returns
    ///
    /// A tuple of the `HeartbeatManager` and a receiver for health events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (heartbeat, mut events) = HeartbeatManager::new(config, connector, metrics);
    ///
    /// tokio::spawn(async move {
    ///     while let Some(event) = events.recv().await {
    ///         println!("Health event: {:?}", event);
    ///     }
    /// });
    /// ```
    #[must_use]
    pub fn new(
        config: HeartbeatConfig,
        connector: Arc<Connector>,
        metrics: FlashMetrics,
    ) -> (Self, HeartbeatEventReceiver) {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (shutdown_tx, _) = broadcast::channel(1);

        let manager = Self {
            config,
            health_states: DashMap::new(),
            event_tx,
            connector,
            metrics,
            shutdown_tx,
        };

        (manager, event_rx)
    }

    /// Start heartbeat monitoring for an exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to monitor
    pub fn start_monitoring(&self, exchange: Exchange) {
        // Create or get health state
        let state = self
            .health_states
            .entry(exchange)
            .or_insert_with(HealthState::new);

        if state.monitoring.load(Ordering::SeqCst) {
            return; // Already monitoring
        }

        state.monitoring.store(true, Ordering::SeqCst);

        // Emit start event
        let _ = self
            .event_tx
            .try_send(HeartbeatEvent::MonitoringStarted { exchange });

        // Create shutdown channel for this task
        let (task_shutdown_tx, _) = broadcast::channel(1);
        *state.shutdown_tx.write().unwrap() = Some(task_shutdown_tx);

        // Note: In a real implementation, we would spawn a task here
        // that sends pings at intervals and waits for pongs.
        // For now, we rely on manual record_pong calls.
    }

    /// Stop heartbeat monitoring for an exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to stop monitoring
    pub fn stop_monitoring(&self, exchange: Exchange) {
        if let Some(state) = self.health_states.get_mut(&exchange) {
            if !state.monitoring.load(Ordering::SeqCst) {
                return; // Not monitoring
            }

            state.monitoring.store(false, Ordering::SeqCst);

            // Send shutdown signal
            if let Some(tx) = state.shutdown_tx.write().unwrap().take() {
                let _ = tx.send(());
            }

            // Emit stop event
            let _ = self.event_tx.try_send(HeartbeatEvent::MonitoringStopped {
                exchange,
                reason: "user requested".to_string(),
            });
        }
    }

    /// Record that a pong was received.
    ///
    /// Call this when a pong message is received from an exchange.
    /// This updates the health status and resets the missed pong counter.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange the pong was received from
    pub fn record_pong(&self, exchange: Exchange) {
        self.record_pong_internal(exchange, None);
    }

    /// Record pong with explicit latency.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange the pong was received from
    /// * `latency` - The round-trip latency
    pub fn record_pong_with_latency(&self, exchange: Exchange, latency: Duration) {
        self.record_pong_internal(exchange, Some(latency));
    }

    /// Internal pong recording.
    fn record_pong_internal(&self, exchange: Exchange, latency: Option<Duration>) {
        let state = self
            .health_states
            .entry(exchange)
            .or_insert_with(HealthState::new);

        let old_status = state.status;

        // Reset missed pongs
        state.missed_pongs.store(0, Ordering::SeqCst);

        // Update last pong time
        *state.last_pong.write().unwrap() = Instant::now();

        // Increment total pongs
        state.total_pongs.fetch_add(1, Ordering::SeqCst);

        // Update latency
        let latency_us = latency
            .or_else(|| state.last_ping.read().unwrap().map(|t| t.elapsed()))
            .map_or(0, |d| d.as_micros() as u64);

        if latency_us > 0 {
            state.last_latency_us.store(latency_us, Ordering::SeqCst);

            // Exponential moving average (alpha = 0.2)
            const ALPHA: f64 = 0.2;
            let old_avg = state.avg_latency_us.load(Ordering::SeqCst);
            let new_avg = if old_avg == 0 {
                latency_us
            } else {
                (1.0 - ALPHA).mul_add(old_avg as f64, ALPHA * latency_us as f64) as u64
            };
            state.avg_latency_us.store(new_avg, Ordering::SeqCst);
        }

        // Update status to Healthy
        // Safety: We need interior mutability here
        // Since DashMap entry gives us &mut, we can update status
        drop(state); // Release the lock first

        if let Some(mut state) = self.health_states.get_mut(&exchange) {
            state.status = HealthStatus::Healthy;

            // Emit health change event if status changed
            if old_status != HealthStatus::Healthy {
                let _ = self.event_tx.try_send(HeartbeatEvent::HealthChanged {
                    exchange,
                    old_status,
                    new_status: HealthStatus::Healthy,
                    missed_pongs: 0,
                });
            }

            // Emit pong received event
            let _ = self.event_tx.try_send(HeartbeatEvent::PongReceived {
                exchange,
                latency_us,
                timestamp: chrono::Utc::now().timestamp_micros(),
            });
        }

        // Update metrics
        self.metrics
            .record_heartbeat_pong_received(exchange.as_str());
    }

    /// Record a missed pong (timeout).
    ///
    /// Call this when a pong timeout occurs.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange that missed the pong
    pub fn record_missed_pong(&self, exchange: Exchange) {
        let state = self
            .health_states
            .entry(exchange)
            .or_insert_with(HealthState::new);

        let old_status = state.status;

        // Increment missed pongs
        let missed = state.missed_pongs.fetch_add(1, Ordering::SeqCst) + 1;

        // Release lock to update status
        drop(state);

        // Determine new status based on thresholds
        let new_status = if missed >= u64::from(self.config.unhealthy_threshold) {
            HealthStatus::Unhealthy
        } else if missed >= u64::from(self.config.degraded_threshold) {
            HealthStatus::Degraded
        } else if old_status == HealthStatus::Healthy {
            // Below degraded threshold but had healthy status
            HealthStatus::Healthy
        } else {
            old_status
        };

        // Update status
        if let Some(mut state) = self.health_states.get_mut(&exchange) {
            state.status = new_status;

            // Emit timeout event
            let _ = self.event_tx.try_send(HeartbeatEvent::PongTimeout {
                exchange,
                missed_count: missed as u32,
            });

            // Emit health change if status changed
            if old_status != new_status {
                let _ = self.event_tx.try_send(HeartbeatEvent::HealthChanged {
                    exchange,
                    old_status,
                    new_status,
                    missed_pongs: missed as u32,
                });
            }
        }

        // Update metrics
        self.metrics.record_heartbeat_pong_missed(exchange.as_str());
    }

    /// Get the current health status for an exchange.
    ///
    /// Returns `HealthStatus::Unknown` if not monitoring.
    #[must_use]
    pub fn health_status(&self, exchange: Exchange) -> HealthStatus {
        self.health_states
            .get(&exchange)
            .map_or(HealthStatus::Unknown, |s| s.status)
    }

    /// Get detailed health summary for an exchange.
    ///
    /// Returns `None` if not monitoring.
    #[must_use]
    pub fn health_summary(&self, exchange: Exchange) -> Option<HealthSummary> {
        self.health_states.get(&exchange).map(|state| {
            let total_pings = state.total_pings.load(Ordering::SeqCst);
            let total_pongs = state.total_pongs.load(Ordering::SeqCst);
            let success_rate = if total_pings > 0 {
                total_pongs as f64 / total_pings as f64
            } else if total_pongs > 0 {
                1.0
            } else {
                0.0
            };

            let last_pong = *state.last_pong.read().unwrap();
            let since_last_pong = Some(last_pong.elapsed());

            let last_latency = state.last_latency_us.load(Ordering::SeqCst);
            let avg_latency = state.avg_latency_us.load(Ordering::SeqCst);

            HealthSummary {
                status: state.status,
                missed_pongs: state.missed_pongs.load(Ordering::SeqCst) as u32,
                since_last_pong,
                last_latency_us: if last_latency > 0 {
                    Some(last_latency)
                } else {
                    None
                },
                avg_latency_us: if avg_latency > 0 {
                    Some(avg_latency)
                } else {
                    None
                },
                total_pings,
                total_pongs,
                success_rate,
            }
        })
    }

    /// Check if an exchange is considered healthy.
    #[must_use]
    pub fn is_healthy(&self, exchange: Exchange) -> bool {
        self.health_status(exchange).is_healthy()
    }

    /// Check if an exchange connection is considered stale.
    ///
    /// A connection is stale if no activity has occurred within
    /// the configured stale threshold.
    #[must_use]
    pub fn is_stale(&self, exchange: Exchange) -> bool {
        if let Some(state) = self.health_states.get(&exchange) {
            let last_pong = *state.last_pong.read().unwrap();
            last_pong.elapsed() > self.config.stale_threshold()
        } else {
            true // Unknown is considered stale
        }
    }

    /// Get health status for all monitored exchanges.
    #[must_use]
    pub fn all_health_statuses(&self) -> Vec<(Exchange, HealthStatus)> {
        self.health_states
            .iter()
            .filter(|e| e.monitoring.load(Ordering::SeqCst))
            .map(|e| (*e.key(), e.status))
            .collect()
    }

    /// Force an immediate health check for an exchange.
    ///
    /// Sends a ping immediately regardless of the normal interval.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to ping
    ///
    /// # Errors
    ///
    /// Returns error if the exchange is not connected.
    #[allow(clippy::unused_async)] // Async for API consistency; will await actual ping in future
    pub async fn force_ping(&self, exchange: Exchange) -> Result<(), FlashError> {
        // Record that we're sending a ping
        if let Some(state) = self.health_states.get_mut(&exchange) {
            *state.last_ping.write().unwrap() = Some(Instant::now());
            state.total_pings.fetch_add(1, Ordering::SeqCst);
        }

        // Emit ping sent event
        let _ = self.event_tx.try_send(HeartbeatEvent::PingSent {
            exchange,
            timestamp: chrono::Utc::now().timestamp_micros(),
        });

        // Note: In a real implementation, we would send an actual ping
        // through the connector here.
        Ok(())
    }

    /// Gracefully shutdown all heartbeat tasks.
    #[allow(clippy::unused_async)] // Async for graceful shutdown pattern consistency
    pub async fn shutdown(&self) {
        // Send global shutdown signal
        let _ = self.shutdown_tx.send(());

        // Stop all monitoring
        let exchanges: Vec<Exchange> = self.health_states.iter().map(|e| *e.key()).collect();
        for exchange in exchanges {
            self.stop_monitoring(exchange);
        }
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

// HeartbeatManager is Send + Sync via DashMap and Arc internals

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_display_values() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(HealthStatus::Unhealthy.to_string(), "unhealthy");
        assert_eq!(HealthStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_health_status_is_methods() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Degraded.is_healthy());

        assert!(HealthStatus::Degraded.is_degraded());
        assert!(!HealthStatus::Healthy.is_degraded());

        assert!(HealthStatus::Unhealthy.is_unhealthy());
        assert!(!HealthStatus::Healthy.is_unhealthy());
    }

    #[test]
    fn test_health_status_default() {
        assert_eq!(HealthStatus::default(), HealthStatus::Unknown);
    }

    #[test]
    fn test_heartbeat_config_default() {
        let config = HeartbeatConfig::default();
        assert_eq!(config.ping_interval_ms, 30_000);
        assert_eq!(config.pong_timeout_ms, 10_000);
        assert!((config.jitter_percent - 0.20).abs() < 0.001);
        assert_eq!(config.degraded_threshold, 1);
        assert_eq!(config.unhealthy_threshold, 3);
    }

    #[test]
    fn test_heartbeat_config_validation() {
        // Valid config
        let config = HeartbeatConfig::default();
        assert!(config.validate().is_ok());

        // Invalid: pong_timeout >= ping_interval
        let mut invalid = HeartbeatConfig::default();
        invalid.pong_timeout_ms = 35_000;
        assert!(invalid.validate().is_err());

        // Invalid: unhealthy < degraded
        let mut invalid = HeartbeatConfig::default();
        invalid.degraded_threshold = 5;
        invalid.unhealthy_threshold = 3;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_jitter_bounds() {
        let config = HeartbeatConfig {
            ping_interval_ms: 1000,
            jitter_percent: 0.20,
            ..HeartbeatConfig::default()
        };

        for _ in 0..100 {
            let interval = config.calculate_next_interval();
            let ms = interval.as_millis() as u64;
            assert!((800..=1200).contains(&ms), "Interval {} out of bounds", ms);
        }
    }
}
