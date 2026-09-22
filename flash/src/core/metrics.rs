//! Prometheus metrics for Flash.
//!
//! This module provides a comprehensive metrics system for monitoring
//! the high-frequency market data adapter.
//!
//! # Metric Types
//!
//! - **Counters**: Track cumulative values (messages received, errors, etc.)
//! - **Gauges**: Track current state (book depth, connection status, etc.)
//! - **Histograms**: Track distributions (latencies, processing times, etc.)
//!
//! # Usage
//!
//! ```rust
//! use astra_flash::core::metrics::{FlashMetrics, MessageType, ProcessingStage};
//!
//! let metrics = FlashMetrics::new();
//!
//! // Record a message received
//! metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
//!
//! // Record processing duration using timing guard
//! {
//!     let _guard = metrics.time_processing("deribit", ProcessingStage::Parse);
//!     // ... do parsing work ...
//! } // Duration automatically recorded when guard is dropped
//!
//! // Set gauge values
//! metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);
//! ```
//!
//! # Thread Safety
//!
//! `FlashMetrics` is `Send + Sync` and can be safely shared across threads
//! using `Arc<FlashMetrics>`. All metric operations use atomic operations
//! for lock-free recording.
//!
//! # Prometheus Integration
//!
//! Start the metrics HTTP server to expose metrics at `/metrics`:
//!
//! ```rust,ignore
//! use astra_flash::core::metrics::FlashMetrics;
//!
//! // Start metrics server on port 9090
//! FlashMetrics::init_prometheus_exporter(9090)?;
//! // Server now running at http://localhost:9090/metrics
//! ```

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::time::Instant;
use thiserror::Error;

// =============================================================================
// METRIC NAME CONSTANTS
// =============================================================================

/// Metric name constants for type safety.
pub mod metric_names {
    // Counters
    /// Total messages received from exchanges.
    pub const MESSAGES_RECEIVED: &str = "flash_messages_received_total";
    /// Total messages published to Redis.
    pub const MESSAGES_PUBLISHED: &str = "flash_messages_published_total";
    /// Total errors by type and severity.
    pub const ERRORS: &str = "flash_errors_total";
    /// Total reconnection attempts.
    pub const RECONNECTIONS: &str = "flash_reconnections_total";

    // Histograms
    /// Processing duration in microseconds.
    pub const PROCESSING_DURATION: &str = "flash_processing_duration_microseconds";
    /// Order book update duration in microseconds.
    pub const ORDERBOOK_UPDATE_DURATION: &str = "flash_orderbook_update_duration_microseconds";
    /// Redis publish duration in microseconds.
    pub const REDIS_PUBLISH_DURATION: &str = "flash_redis_publish_duration_microseconds";

    // Gauges
    /// Current bid levels in order book.
    pub const ORDERBOOK_BID_LEVELS: &str = "flash_orderbook_bid_levels";
    /// Current ask levels in order book.
    pub const ORDERBOOK_ASK_LEVELS: &str = "flash_orderbook_ask_levels";
    /// Current order book spread.
    pub const ORDERBOOK_SPREAD: &str = "flash_orderbook_spread";
    /// WebSocket connection state (1 = connected, 0 = disconnected).
    pub const WEBSOCKET_CONNECTED: &str = "flash_websocket_connected";
    /// Current queue depth.
    pub const QUEUE_DEPTH: &str = "flash_queue_depth";
    /// Queue capacity.
    pub const QUEUE_CAPACITY: &str = "flash_queue_capacity";
    /// Uptime in seconds.
    pub const UPTIME_SECONDS: &str = "flash_uptime_seconds";
}

/// Total orderbook update messages processed
pub const FLASH_ORDERBOOK_UPDATES_TOTAL: &str = "flash_orderbook_updates_total";
/// Size of Redis publish batches in messages per batch
pub const FLASH_REDIS_BATCH_SIZE: &str = "flash_redis_batch_size";
/// Backpressure status: 0 = normal, 1 = active backpressure
pub const FLASH_BACKPRESSURE_STATUS: &str = "flash_backpressure_status";
/// Total messages dropped due to backpressure or buffer overflow
pub const FLASH_MESSAGES_DROPPED_TOTAL: &str = "flash_messages_dropped_total";

/// Label key constants.
pub mod labels {
    /// Exchange name label.
    pub const EXCHANGE: &str = "exchange";
    /// Instrument/symbol label.
    pub const INSTRUMENT: &str = "instrument";
    /// Message type label.
    pub const MESSAGE_TYPE: &str = "message_type";
    /// Topic name label.
    pub const TOPIC: &str = "topic";
    /// Error type label.
    pub const ERROR_TYPE: &str = "error_type";
    /// Error severity label.
    pub const SEVERITY: &str = "severity";
    /// Reconnection reason label.
    pub const REASON: &str = "reason";
    /// Processing stage label.
    pub const STAGE: &str = "stage";
    /// Update type label.
    pub const UPDATE_TYPE: &str = "update_type";
    /// Queue name label.
    pub const QUEUE_NAME: &str = "queue_name";
}

// =============================================================================
// ENUMS
// =============================================================================

/// Processing stages for latency tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessingStage {
    /// JSON/binary parsing stage.
    Parse,
    /// Message normalization stage.
    Normalize,
    /// Order book update stage.
    BookUpdate,
    /// Serialization stage (for Redis).
    Serialize,
    /// Redis publish stage.
    Publish,
}

impl ProcessingStage {
    /// Get the string representation for metrics labels.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Normalize => "normalize",
            Self::BookUpdate => "book_update",
            Self::Serialize => "serialize",
            Self::Publish => "publish",
        }
    }
}

/// Order book update types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateType {
    /// Full order book snapshot.
    Snapshot,
    /// Incremental delta update.
    Delta,
}

impl UpdateType {
    /// Get the string representation for metrics labels.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Delta => "delta",
        }
    }
}

/// Message types received from exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// Full order book snapshot.
    Snapshot,
    /// Incremental delta update.
    Delta,
    /// Trade execution.
    Trade,
    /// Heartbeat/ping message.
    Heartbeat,
    /// Unknown or unrecognized message.
    Unknown,
}

impl MessageType {
    /// Get the string representation for metrics labels.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Delta => "delta",
            Self::Trade => "trade",
            Self::Heartbeat => "heartbeat",
            Self::Unknown => "unknown",
        }
    }
}

/// Reasons for WebSocket reconnection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReconnectReason {
    /// Connection lost unexpectedly.
    ConnectionLost,
    /// Pong timeout (no response to ping).
    PongTimeout,
    /// Sequence gap detected, need resync.
    SequenceGap,
    /// Exchange initiated disconnect.
    ExchangeDisconnect,
    /// Manual reconnection request.
    Manual,
}

impl ReconnectReason {
    /// Get the string representation for metrics labels.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ConnectionLost => "connection_lost",
            Self::PongTimeout => "pong_timeout",
            Self::SequenceGap => "sequence_gap",
            Self::ExchangeDisconnect => "exchange_disconnect",
            Self::Manual => "manual",
        }
    }
}

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur in the metrics system.
#[derive(Debug, Error)]
pub enum MetricsError {
    /// Failed to initialize Prometheus exporter.
    #[error("Failed to initialize Prometheus exporter: {0}")]
    ExporterInitFailed(String),

    /// Port already in use.
    #[error("Metrics port {port} already in use")]
    PortInUse {
        /// The port that was already in use.
        port: u16,
    },

    /// Invalid metric name.
    #[error("Invalid metric name: {0}")]
    InvalidMetricName(String),

    /// Invalid label value.
    #[error("Invalid label value for {label}: {value}")]
    InvalidLabelValue {
        /// The label key.
        label: String,
        /// The invalid value.
        value: String,
    },
}

// =============================================================================
// FLASHMETRICS
// =============================================================================

/// Central metrics registry for Flash.
///
/// This struct provides methods for recording all types of metrics:
/// counters, gauges, and histograms.
///
/// # Thread Safety
///
/// `FlashMetrics` is `Send + Sync` and can be safely shared across
/// async tasks and threads using `Arc<FlashMetrics>`. All recording
/// operations are lock-free using atomic operations.
///
/// # Example
///
/// ```rust
/// use astra_flash::core::metrics::{FlashMetrics, MessageType, ProcessingStage};
///
/// let metrics = FlashMetrics::new();
///
/// // Record counters
/// metrics.record_message_received("deribit", "BTC-PERPETUAL", MessageType::Delta);
/// metrics.record_error("parse_error", "warning");
///
/// // Record histograms
/// metrics.record_processing_duration("deribit", ProcessingStage::Parse, 2.5);
///
/// // Set gauges
/// metrics.set_orderbook_levels("BTC-PERPETUAL", 50, 50);
/// metrics.set_websocket_connected("deribit", true);
/// ```
#[derive(Debug, Clone)]
pub struct FlashMetrics {
    /// Start time for uptime calculation.
    start_time: Instant,
}

impl Default for FlashMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl FlashMetrics {
    /// Create a new metrics registry.
    ///
    /// This initializes the start time for uptime calculation.
    /// Call `init_prometheus_exporter` separately to start the HTTP endpoint.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }

    /// Initialize the Prometheus exporter and describe all metrics.
    ///
    /// This starts an HTTP server that exposes metrics at `/metrics`.
    /// The returned handle must be kept alive for the duration of the program.
    ///
    /// # Arguments
    ///
    /// * `port` - Port for the HTTP server (e.g., 9090)
    ///
    /// # Returns
    ///
    /// `Ok(())` on success. The HTTP listener is spawned as a background tokio task
    /// with internally-managed lifetime (registered globally via metrics-exporter-prometheus
    /// `.install()`); no handle return is needed for listener lifetime management.
    ///
    /// # Errors
    ///
    /// Returns `MetricsError::PortInUse` if the port is already bound.
    /// Returns `MetricsError::ExporterInitFailed` for other initialization errors.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// FlashMetrics::init_prometheus_exporter(9090)?;
    /// // Server now running at http://localhost:9090/metrics
    /// ```
    pub fn init_prometheus_exporter(port: u16) -> Result<(), MetricsError> {
        // Build the exporter with custom histogram buckets
        let builder = PrometheusBuilder::new();

        builder
            .with_http_listener(([0, 0, 0, 0], port))
            .install()
            .map_err(|e| {
                if e.to_string().contains("address already in use")
                    || e.to_string().contains("Address already in use")
                {
                    MetricsError::PortInUse { port }
                } else {
                    MetricsError::ExporterInitFailed(e.to_string())
                }
            })?;

        // Describe all metrics
        Self::describe_metrics();

        Ok(())
    }

    /// Describe all metrics for Prometheus.
    fn describe_metrics() {
        // Counters
        describe_counter!(
            metric_names::MESSAGES_RECEIVED,
            "Total messages received from exchanges"
        );
        describe_counter!(
            metric_names::MESSAGES_PUBLISHED,
            "Total messages published to Redis"
        );
        describe_counter!(metric_names::ERRORS, "Total errors by type and severity");
        describe_counter!(
            metric_names::RECONNECTIONS,
            "Total WebSocket reconnection attempts"
        );

        // Histograms
        describe_histogram!(
            metric_names::PROCESSING_DURATION,
            "Processing duration in microseconds"
        );
        describe_histogram!(
            metric_names::ORDERBOOK_UPDATE_DURATION,
            "Order book update duration in microseconds"
        );
        describe_histogram!(
            metric_names::REDIS_PUBLISH_DURATION,
            "Redis publish duration in microseconds"
        );

        // Gauges
        describe_gauge!(
            metric_names::ORDERBOOK_BID_LEVELS,
            "Current bid levels in order book"
        );
        describe_gauge!(
            metric_names::ORDERBOOK_ASK_LEVELS,
            "Current ask levels in order book"
        );
        describe_gauge!(
            metric_names::ORDERBOOK_SPREAD,
            "Current order book spread in price units"
        );
        describe_gauge!(
            metric_names::WEBSOCKET_CONNECTED,
            "WebSocket connection state (1=connected, 0=disconnected)"
        );
        describe_gauge!(metric_names::QUEUE_DEPTH, "Current queue depth");
        describe_gauge!(metric_names::QUEUE_CAPACITY, "Queue capacity");
        describe_gauge!(metric_names::UPTIME_SECONDS, "Process uptime in seconds");

        describe_counter!(
            FLASH_ORDERBOOK_UPDATES_TOTAL,
            "Total orderbook update messages processed"
        );
        describe_histogram!(
            FLASH_REDIS_BATCH_SIZE,
            "Size of Redis publish batches in messages per batch"
        );
        describe_gauge!(
            FLASH_BACKPRESSURE_STATUS,
            "Backpressure status: 0 = normal, 1 = active backpressure"
        );
        describe_counter!(
            FLASH_MESSAGES_DROPPED_TOTAL,
            "Total messages dropped due to backpressure or buffer overflow"
        );
    }

    // =========================================================================
    // COUNTER METHODS
    // =========================================================================

    /// Record a message received from an exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name (e.g., "deribit", "binance")
    /// * `instrument` - Instrument symbol (e.g., "BTC-PERPETUAL")
    /// * `msg_type` - Type of message received
    pub fn record_message_received(&self, exchange: &str, instrument: &str, msg_type: MessageType) {
        counter!(
            metric_names::MESSAGES_RECEIVED,
            labels::EXCHANGE => exchange.to_string(),
            labels::INSTRUMENT => instrument.to_string(),
            labels::MESSAGE_TYPE => msg_type.as_str().to_string()
        )
        .increment(1);
    }

    /// Record a message published to Redis.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `instrument` - Instrument symbol
    /// * `topic` - Redis topic/stream name
    pub fn record_message_published(&self, exchange: &str, instrument: &str, topic: &str) {
        counter!(
            metric_names::MESSAGES_PUBLISHED,
            labels::EXCHANGE => exchange.to_string(),
            labels::INSTRUMENT => instrument.to_string(),
            labels::TOPIC => topic.to_string()
        )
        .increment(1);
    }

    /// Record an error occurrence.
    ///
    /// # Arguments
    ///
    /// * `error_type` - Type of error (e.g., "parse_error", "connection_failed")
    /// * `severity` - Error severity (e.g., "warning", "error", "critical")
    pub fn record_error(&self, error_type: &str, severity: &str) {
        counter!(
            metric_names::ERRORS,
            labels::ERROR_TYPE => error_type.to_string(),
            labels::SEVERITY => severity.to_string()
        )
        .increment(1);
    }

    /// Record a WebSocket reconnection attempt.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `reason` - Reason for reconnection
    pub fn record_reconnection(&self, exchange: &str, reason: ReconnectReason) {
        counter!(
            metric_names::RECONNECTIONS,
            labels::EXCHANGE => exchange.to_string(),
            labels::REASON => reason.as_str().to_string()
        )
        .increment(1);
    }

    // =========================================================================
    // HISTOGRAM METHODS
    // =========================================================================

    /// Record processing duration for a stage.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `stage` - Processing stage
    /// * `microseconds` - Duration in microseconds
    pub fn record_processing_duration(
        &self,
        exchange: &str,
        stage: ProcessingStage,
        microseconds: f64,
    ) {
        histogram!(
            metric_names::PROCESSING_DURATION,
            labels::EXCHANGE => exchange.to_string(),
            labels::STAGE => stage.as_str().to_string()
        )
        .record(microseconds);
    }

    /// Record order book update duration.
    ///
    /// # Arguments
    ///
    /// * `instrument` - Instrument symbol
    /// * `update_type` - Type of update (snapshot or delta)
    /// * `microseconds` - Duration in microseconds
    pub fn record_orderbook_update_duration(
        &self,
        instrument: &str,
        update_type: UpdateType,
        microseconds: f64,
    ) {
        histogram!(
            metric_names::ORDERBOOK_UPDATE_DURATION,
            labels::INSTRUMENT => instrument.to_string(),
            labels::UPDATE_TYPE => update_type.as_str().to_string()
        )
        .record(microseconds);
    }

    /// Record Redis publish duration.
    ///
    /// # Arguments
    ///
    /// * `topic` - Redis topic/stream name
    /// * `microseconds` - Duration in microseconds
    pub fn record_redis_publish_duration(&self, topic: &str, microseconds: f64) {
        histogram!(
            metric_names::REDIS_PUBLISH_DURATION,
            labels::TOPIC => topic.to_string()
        )
        .record(microseconds);
    }

    // =========================================================================
    // GAUGE METHODS
    // =========================================================================

    /// Set the current order book levels.
    ///
    /// # Arguments
    ///
    /// * `instrument` - Instrument symbol
    /// * `bid_levels` - Number of bid levels
    /// * `ask_levels` - Number of ask levels
    pub fn set_orderbook_levels(&self, instrument: &str, bid_levels: usize, ask_levels: usize) {
        gauge!(
            metric_names::ORDERBOOK_BID_LEVELS,
            labels::INSTRUMENT => instrument.to_string()
        )
        .set(bid_levels as f64);

        gauge!(
            metric_names::ORDERBOOK_ASK_LEVELS,
            labels::INSTRUMENT => instrument.to_string()
        )
        .set(ask_levels as f64);
    }

    /// Set the current order book spread.
    ///
    /// # Arguments
    ///
    /// * `instrument` - Instrument symbol
    /// * `spread` - Spread value in price units
    pub fn set_orderbook_spread(&self, instrument: &str, spread: f64) {
        gauge!(
            metric_names::ORDERBOOK_SPREAD,
            labels::INSTRUMENT => instrument.to_string()
        )
        .set(spread);
    }

    /// Set WebSocket connection state.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `connected` - Whether connected (true) or disconnected (false)
    pub fn set_websocket_connected(&self, exchange: &str, connected: bool) {
        let value = if connected { 1.0 } else { 0.0 };
        gauge!(
            metric_names::WEBSOCKET_CONNECTED,
            labels::EXCHANGE => exchange.to_string()
        )
        .set(value);
    }

    /// Set queue depth and capacity.
    ///
    /// # Arguments
    ///
    /// * `queue_name` - Name of the queue
    /// * `depth` - Current queue depth
    /// * `capacity` - Queue capacity
    pub fn set_queue_depth(&self, queue_name: &str, depth: usize, capacity: usize) {
        gauge!(
            metric_names::QUEUE_DEPTH,
            labels::QUEUE_NAME => queue_name.to_string()
        )
        .set(depth as f64);

        gauge!(
            metric_names::QUEUE_CAPACITY,
            labels::QUEUE_NAME => queue_name.to_string()
        )
        .set(capacity as f64);
    }

    // =========================================================================
    // HEARTBEAT METRICS METHODS
    // =========================================================================

    /// Record a heartbeat pong received.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    pub fn record_heartbeat_pong_received(&self, exchange: &str) {
        counter!(
            "flash_heartbeat_pongs_received_total",
            labels::EXCHANGE => exchange.to_string()
        )
        .increment(1);
    }

    /// Record a heartbeat pong missed (timeout).
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    pub fn record_heartbeat_pong_missed(&self, exchange: &str) {
        counter!(
            "flash_heartbeat_pongs_missed_total",
            labels::EXCHANGE => exchange.to_string()
        )
        .increment(1);
    }

    /// Record heartbeat latency.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `latency_us` - Latency in microseconds
    pub fn record_heartbeat_latency(&self, exchange: &str, latency_us: f64) {
        histogram!(
            "flash_heartbeat_latency_microseconds",
            labels::EXCHANGE => exchange.to_string()
        )
        .record(latency_us);
    }

    /// Set heartbeat health status gauge.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `status` - Numeric status (0=Unknown, 1=Healthy, 2=Degraded, 3=Unhealthy)
    pub fn set_heartbeat_health_status(&self, exchange: &str, status: u8) {
        gauge!(
            "flash_heartbeat_health_status",
            labels::EXCHANGE => exchange.to_string()
        )
        .set(f64::from(status));
    }

    // =========================================================================
    // UTILITY METHODS
    // =========================================================================

    /// Get the uptime in seconds since this metrics instance was created.
    ///
    /// # Returns
    ///
    /// Uptime in seconds as a floating-point value.
    #[must_use]
    pub fn uptime_seconds(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Create a timing guard for automatic duration recording.
    ///
    /// The guard will automatically record the elapsed time when dropped.
    ///
    /// # Arguments
    ///
    /// * `exchange` - Exchange name
    /// * `stage` - Processing stage
    ///
    /// # Returns
    ///
    /// A `TimingGuard` that records duration on drop.
    ///
    /// # Example
    ///
    /// ```rust
    /// use astra_flash::core::metrics::{FlashMetrics, ProcessingStage};
    ///
    /// let metrics = FlashMetrics::new();
    /// {
    ///     let _guard = metrics.time_processing("deribit", ProcessingStage::Parse);
    ///     // ... do parsing work ...
    /// } // Duration automatically recorded here
    /// ```
    #[must_use]
    pub fn time_processing(&self, exchange: &str, stage: ProcessingStage) -> TimingGuard<'_> {
        TimingGuard {
            metrics: self,
            exchange: exchange.to_string(),
            stage,
            start: Instant::now(),
        }
    }
}

// =============================================================================
// TIMING GUARD
// =============================================================================

/// RAII guard for automatic duration recording.
///
/// When this guard is dropped, it automatically records the elapsed time
/// since creation as a processing duration metric.
///
/// # Example
///
/// ```rust
/// use astra_flash::core::metrics::{FlashMetrics, ProcessingStage};
///
/// let metrics = FlashMetrics::new();
///
/// {
///     let _guard = metrics.time_processing("deribit", ProcessingStage::Parse);
///     // ... parsing code ...
/// } // Duration recorded automatically
/// ```
#[derive(Debug)]
pub struct TimingGuard<'a> {
    metrics: &'a FlashMetrics,
    exchange: String,
    stage: ProcessingStage,
    start: Instant,
}

impl Drop for TimingGuard<'_> {
    fn drop(&mut self) {
        let elapsed_us = self.start.elapsed().as_micros() as f64;
        self.metrics
            .record_processing_duration(&self.exchange, self.stage, elapsed_us);
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_stage_as_str() {
        assert_eq!(ProcessingStage::Parse.as_str(), "parse");
        assert_eq!(ProcessingStage::Normalize.as_str(), "normalize");
        assert_eq!(ProcessingStage::BookUpdate.as_str(), "book_update");
        assert_eq!(ProcessingStage::Serialize.as_str(), "serialize");
        assert_eq!(ProcessingStage::Publish.as_str(), "publish");
    }

    #[test]
    fn test_update_type_as_str() {
        assert_eq!(UpdateType::Snapshot.as_str(), "snapshot");
        assert_eq!(UpdateType::Delta.as_str(), "delta");
    }

    #[test]
    fn test_message_type_as_str() {
        assert_eq!(MessageType::Snapshot.as_str(), "snapshot");
        assert_eq!(MessageType::Delta.as_str(), "delta");
        assert_eq!(MessageType::Trade.as_str(), "trade");
        assert_eq!(MessageType::Heartbeat.as_str(), "heartbeat");
        assert_eq!(MessageType::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_reconnect_reason_as_str() {
        assert_eq!(ReconnectReason::ConnectionLost.as_str(), "connection_lost");
        assert_eq!(ReconnectReason::PongTimeout.as_str(), "pong_timeout");
        assert_eq!(ReconnectReason::SequenceGap.as_str(), "sequence_gap");
        assert_eq!(
            ReconnectReason::ExchangeDisconnect.as_str(),
            "exchange_disconnect"
        );
        assert_eq!(ReconnectReason::Manual.as_str(), "manual");
    }

    #[test]
    fn test_metrics_error_display() {
        let error = MetricsError::PortInUse { port: 9090 };
        assert!(error.to_string().contains("9090"));

        let error = MetricsError::ExporterInitFailed("test".to_string());
        assert!(error.to_string().contains("test"));

        let error = MetricsError::InvalidMetricName("bad_name".to_string());
        assert!(error.to_string().contains("bad_name"));

        let error = MetricsError::InvalidLabelValue {
            label: "exchange".to_string(),
            value: "bad".to_string(),
        };
        assert!(error.to_string().contains("exchange"));
        assert!(error.to_string().contains("bad"));
    }

    #[test]
    fn test_flash_metrics_default() {
        let metrics = FlashMetrics::default();
        assert!(metrics.uptime_seconds() >= 0.0);
    }

    #[test]
    fn test_flash_metrics_uptime() {
        let metrics = FlashMetrics::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let uptime = metrics.uptime_seconds();
        assert!(uptime >= 0.01);
    }

    #[test]
    fn test_timing_guard_creation() {
        let metrics = FlashMetrics::new();
        let _guard = metrics.time_processing("test", ProcessingStage::Parse);
        // Guard should be created without panic
    }
}
