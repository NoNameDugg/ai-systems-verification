//! Dual Output Publisher for Flash (Batch 3.2).
//!
//! This module implements dual output publishing to both:
//! - `market:orderbook:{symbol}` - Raw OrderBookSnapshot for Gateway UI
//! - `astra:signals:flash:{exchange}:{symbol}` - AlphaSignal for Fusion engine
//!
//! # Overview
//!
//! The dual output pattern is central to Flash's architecture:
//!
//! 1. **Gateway Output**: Raw orderbook data in Gateway-compatible format
//!    - Consumed by the gateway UI for visualization
//!    - Retains full price level detail
//!
//! 2. **Fusion Output**: Alpha signals derived from orderbook imbalance
//!    - Consumed by the decision-aggregation engine
//!    - Includes signal direction, strength, and metadata
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::{DualPublisher, DualPublisherConfig, RedisPool};
//! use std::sync::Arc;
//!
//! let pool = Arc::new(RedisPool::new(config).await?);
//! let publisher = DualPublisher::new(pool, DualPublisherConfig::default());
//!
//! // Publish to both streams
//! let result = publisher.publish_dual(&book_snapshot).await?;
//!
//! println!("Orderbook: {:?}", result.orderbook_key);
//! println!("Signal: {:?}", result.signal_key);
//! ```
//!
//! # Configuration
//!
//! Outputs can be individually enabled/disabled:
//!
//! ```rust,ignore
//! let config = DualPublisherConfig {
//!     orderbook_enabled: true,
//!     signal_enabled: true,  // Disable to skip Fusion output
//!     ttl_seconds: 60,
//!     ..Default::default()
//! };
//! ```

use crate::book::orderbook::BookSnapshot;
use crate::core::types::Instrument;
use crate::fusion::{AlphaSignal, FUSION_TOPIC_PREFIX};
use crate::gateway::OrderBookSnapshot as GatewaySnapshot;
use crate::publisher::pool::{PoolError, PooledConnection, RedisPool};
use crate::publisher::stream::SerializationFormat;
use parking_lot::RwLock;
use std::sync::Arc;
use thiserror::Error;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Default orderbook key prefix for Gateway consumption.
pub const DEFAULT_ORDERBOOK_PREFIX: &str = "market:orderbook:";

/// Default confidence level for AlphaSignal generation.
const DEFAULT_SIGNAL_CONFIDENCE: f64 = 0.8;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for the dual publisher.
///
/// Controls which outputs are enabled and their respective settings.
///
/// # Example
///
/// ```rust
/// use astra_flash::publisher::DualPublisherConfig;
///
/// let config = DualPublisherConfig {
///     orderbook_enabled: true,
///     signal_enabled: true,
///     ttl_seconds: 120,
///     ..DualPublisherConfig::default()
/// };
/// ```
///
/// # Shadow Mode (Batch 5.1)
///
/// When shadow mode is enabled, Redis keys include a namespace segment:
/// - Orderbook: `market:orderbook:rust:EUR_USD` (instead of `market:orderbook:EUR_USD`)
/// - Signal: `astra:signals:flash:rust:oanda:EUR_USD` (instead of `astra:signals:flash:oanda:EUR_USD`)
#[derive(Debug, Clone)]
pub struct DualPublisherConfig {
    /// Enable Gateway orderbook output.
    pub orderbook_enabled: bool,

    /// Enable Fusion signal output.
    pub signal_enabled: bool,

    /// Key prefix for orderbook stream (e.g., "market:orderbook:").
    pub orderbook_key_prefix: String,

    /// Key prefix for signal stream (e.g., "astra:signals:flash").
    pub signal_key_prefix: String,

    /// TTL in seconds for Redis keys (0 = no expiry).
    pub ttl_seconds: u64,

    /// Serialization format for publishing.
    pub format: SerializationFormat,

    /// Confidence level for generated AlphaSignals (0.0 to 1.0).
    pub signal_confidence: f64,

    /// Maximum stream length (MAXLEN option). 0 = unlimited.
    pub max_stream_length: u64,

    /// Enable Shadow Mode (Batch 5.1).
    ///
    /// When enabled, Redis keys include a namespace segment for parallel validation.
    pub shadow_mode_enabled: bool,

    /// Namespace to insert into Redis keys when shadow mode is enabled.
    ///
    /// Default: "rust"
    pub shadow_mode_namespace: String,
}

impl Default for DualPublisherConfig {
    fn default() -> Self {
        Self {
            orderbook_enabled: true,
            signal_enabled: true,
            orderbook_key_prefix: DEFAULT_ORDERBOOK_PREFIX.to_string(),
            signal_key_prefix: FUSION_TOPIC_PREFIX.to_string(),
            ttl_seconds: 60,
            format: SerializationFormat::Json,
            signal_confidence: DEFAULT_SIGNAL_CONFIDENCE,
            max_stream_length: 100_000,
            shadow_mode_enabled: false,
            shadow_mode_namespace: "rust".to_string(),
        }
    }
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Statistics for dual publishing operations.
///
/// Tracks publish counts and latency for monitoring.
#[derive(Debug, Clone, Default)]
pub struct DualStats {
    /// Number of orderbook snapshots published.
    pub orderbook_published: u64,

    /// Number of alpha signals published.
    pub signals_published: u64,

    /// Total latency in microseconds (sum).
    pub total_latency_us: u64,

    /// Total number of publish operations.
    pub publish_count: u64,

    /// Number of publish errors.
    pub publish_errors: u64,
}

impl DualStats {
    /// Calculate average latency in microseconds.
    #[must_use]
    pub fn avg_latency_us(&self) -> f64 {
        if self.publish_count == 0 {
            0.0
        } else {
            self.total_latency_us as f64 / self.publish_count as f64
        }
    }

    /// Reset all statistics to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// =============================================================================
// PUBLISH RESULT
// =============================================================================

/// Result of a dual publish operation.
///
/// Contains information about both published streams.
#[derive(Debug, Clone)]
pub struct DualPublishResult {
    /// Redis key for orderbook (None if disabled or failed).
    pub orderbook_key: Option<String>,

    /// Redis key for signal (None if disabled or failed).
    pub signal_key: Option<String>,

    /// Redis message ID for orderbook stream entry.
    pub orderbook_message_id: Option<String>,

    /// Redis message ID for signal stream entry.
    pub signal_message_id: Option<String>,

    /// Total publish latency in microseconds.
    pub latency_us: u64,
}

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during dual publishing.
#[derive(Debug, Error)]
pub enum DualPublishError {
    /// Pool error when getting connection.
    #[error("Pool error: {0}")]
    PoolError(#[from] PoolError),

    /// Redis command failed.
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    /// Serialization failed.
    #[error("Serialization failed: {0}")]
    SerializationError(String),

    /// Both outputs are disabled.
    #[error("Both orderbook and signal outputs are disabled")]
    NoOutputsEnabled,
}

/// Result type for dual publish operations.
pub type DualPublishResult_ = Result<DualPublishResult, DualPublishError>;

// =============================================================================
// BUILDER
// =============================================================================

/// Builder for creating [`DualPublisher`] with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// let publisher = DualPublisherBuilder::default()
///     .orderbook_enabled(true)
///     .signal_enabled(true)
///     .ttl_seconds(120)
///     .build(pool);
/// ```
#[derive(Debug, Clone, Default)]
pub struct DualPublisherBuilder {
    config: DualPublisherConfig,
}

impl DualPublisherBuilder {
    /// Enable/disable orderbook output.
    #[must_use]
    pub const fn orderbook_enabled(mut self, enabled: bool) -> Self {
        self.config.orderbook_enabled = enabled;
        self
    }

    /// Enable/disable signal output.
    #[must_use]
    pub const fn signal_enabled(mut self, enabled: bool) -> Self {
        self.config.signal_enabled = enabled;
        self
    }

    /// Set TTL in seconds.
    #[must_use]
    pub const fn ttl_seconds(mut self, ttl: u64) -> Self {
        self.config.ttl_seconds = ttl;
        self
    }

    /// Set orderbook key prefix.
    #[must_use]
    pub fn orderbook_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.orderbook_key_prefix = prefix.into();
        self
    }

    /// Set signal key prefix.
    #[must_use]
    pub fn signal_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.signal_key_prefix = prefix.into();
        self
    }

    /// Set serialization format.
    #[must_use]
    pub const fn format(mut self, format: SerializationFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set signal confidence level.
    #[must_use]
    pub fn signal_confidence(mut self, confidence: f64) -> Self {
        self.config.signal_confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Set maximum stream length.
    #[must_use]
    pub const fn max_stream_length(mut self, len: u64) -> Self {
        self.config.max_stream_length = len;
        self
    }

    /// Enable/disable shadow mode (Batch 5.1).
    ///
    /// When enabled, Redis keys include a namespace segment for parallel validation.
    #[must_use]
    pub const fn shadow_mode_enabled(mut self, enabled: bool) -> Self {
        self.config.shadow_mode_enabled = enabled;
        self
    }

    /// Set shadow mode namespace.
    ///
    /// Default: "rust"
    #[must_use]
    pub fn shadow_mode_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.shadow_mode_namespace = namespace.into();
        self
    }

    /// Get the current configuration.
    #[must_use]
    pub const fn config(&self) -> &DualPublisherConfig {
        &self.config
    }

    /// Build the dual publisher.
    #[must_use]
    pub fn build(self, pool: Arc<RedisPool>) -> DualPublisher {
        DualPublisher::new(pool, self.config)
    }
}

// =============================================================================
// DUAL PUBLISHER
// =============================================================================

/// Dual output publisher for Gateway and Fusion streams.
///
/// Publishes a single [`BookSnapshot`] to both output streams:
/// 1. Gateway: `market:orderbook:{symbol}` with [`GatewaySnapshot`]
/// 2. Fusion: `astra:signals:flash:{exchange}:{symbol}` with [`AlphaSignal`]
///
/// # Thread Safety
///
/// `DualPublisher` is `Send + Sync` and can be safely shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::publisher::{DualPublisher, DualPublisherConfig, RedisPool};
/// use std::sync::Arc;
///
/// let pool = Arc::new(RedisPool::new(config).await?);
/// let publisher = DualPublisher::new(pool, DualPublisherConfig::default());
///
/// let result = publisher.publish_dual(&snapshot).await?;
/// println!("Published to {} and {}",
///     result.orderbook_key.unwrap_or_default(),
///     result.signal_key.unwrap_or_default()
/// );
/// ```
pub struct DualPublisher {
    pool: Arc<RedisPool>,
    config: DualPublisherConfig,
    stats: Arc<RwLock<DualStats>>,
}

impl DualPublisher {
    /// Create a new dual publisher.
    #[must_use]
    pub fn new(pool: Arc<RedisPool>, config: DualPublisherConfig) -> Self {
        Self {
            pool,
            config,
            stats: Arc::new(RwLock::new(DualStats::default())),
        }
    }

    /// Create a publisher with default configuration.
    #[must_use]
    pub fn with_defaults(pool: Arc<RedisPool>) -> Self {
        Self::new(pool, DualPublisherConfig::default())
    }

    /// Create a builder for constructing a DualPublisher.
    #[must_use]
    pub fn builder() -> DualPublisherBuilder {
        DualPublisherBuilder::default()
    }

    /// Generate the orderbook key for an instrument.
    ///
    /// # Format
    ///
    /// - Production: `{prefix}{base}_{quote}` (e.g., `market:orderbook:EUR_USD`)
    /// - Shadow Mode: `{prefix}{namespace}:{base}_{quote}` (e.g., `market:orderbook:rust:EUR_USD`)
    #[must_use]
    pub fn orderbook_key(instrument: &Instrument, config: &DualPublisherConfig) -> String {
        if config.shadow_mode_enabled {
            format!(
                "{}{}:{}_{}",
                config.orderbook_key_prefix,
                config.shadow_mode_namespace,
                instrument.base,
                instrument.quote
            )
        } else {
            format!(
                "{}{}_{}",
                config.orderbook_key_prefix, instrument.base, instrument.quote
            )
        }
    }

    /// Generate the signal key for an instrument.
    ///
    /// # Format
    ///
    /// - Production: `{prefix}:{exchange}:{base}_{quote}` (e.g., `astra:signals:flash:oanda:EUR_USD`)
    /// - Shadow Mode: `{prefix}:{namespace}:{exchange}:{base}_{quote}` (e.g., `astra:signals:flash:rust:oanda:EUR_USD`)
    #[must_use]
    pub fn signal_key(instrument: &Instrument, config: &DualPublisherConfig) -> String {
        if config.shadow_mode_enabled {
            format!(
                "{}:{}:{}:{}_{}",
                config.signal_key_prefix,
                config.shadow_mode_namespace,
                instrument.exchange.as_str(),
                instrument.base,
                instrument.quote
            )
        } else {
            format!(
                "{}:{}:{}_{}",
                config.signal_key_prefix,
                instrument.exchange.as_str(),
                instrument.base,
                instrument.quote
            )
        }
    }

    /// Publish a book snapshot to both output streams.
    ///
    /// Converts the snapshot to Gateway and Fusion formats, then publishes
    /// to both Redis streams in parallel.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Both outputs are disabled
    /// - Redis connection fails
    /// - Serialization fails
    pub async fn publish_dual(&self, snapshot: &BookSnapshot) -> DualPublishResult_ {
        if !self.config.orderbook_enabled && !self.config.signal_enabled {
            return Err(DualPublishError::NoOutputsEnabled);
        }

        let start = std::time::Instant::now();

        let mut result = DualPublishResult {
            orderbook_key: None,
            signal_key: None,
            orderbook_message_id: None,
            signal_message_id: None,
            latency_us: 0,
        };

        // Get connection
        let mut conn = self.pool.get().await?;

        // Publish orderbook if enabled
        if self.config.orderbook_enabled {
            let gateway = GatewaySnapshot::from_book_snapshot(snapshot);
            let key = Self::orderbook_key(&snapshot.instrument, &self.config);
            let data = gateway
                .to_json()
                .map_err(|e| DualPublishError::SerializationError(e.to_string()))?;

            let message_id = self.xadd(&mut conn, &key, &data).await?;
            result.orderbook_key = Some(key);
            result.orderbook_message_id = Some(message_id);
        }

        // Publish signal if enabled
        if self.config.signal_enabled {
            let signal = AlphaSignal::from_book_snapshot(snapshot, self.config.signal_confidence);
            let key = Self::signal_key(&snapshot.instrument, &self.config);
            let data = signal
                .to_json()
                .map_err(|e| DualPublishError::SerializationError(e.to_string()))?;

            let message_id = self.xadd(&mut conn, &key, &data).await?;
            result.signal_key = Some(key);
            result.signal_message_id = Some(message_id);
        }

        result.latency_us = start.elapsed().as_micros() as u64;

        // Update statistics
        {
            let mut stats = self.stats.write();
            if result.orderbook_key.is_some() {
                stats.orderbook_published += 1;
            }
            if result.signal_key.is_some() {
                stats.signals_published += 1;
            }
            stats.publish_count += 1;
            stats.total_latency_us += result.latency_us;
        }

        Ok(result)
    }

    /// Execute XADD command to publish to a stream.
    async fn xadd(
        &self,
        conn: &mut PooledConnection,
        key: &str,
        data: &str,
    ) -> Result<String, DualPublishError> {
        let mut cmd = redis::cmd("XADD");
        cmd.arg(key);

        // Add MAXLEN if configured
        if self.config.max_stream_length > 0 {
            cmd.arg("MAXLEN");
            cmd.arg("~"); // Approximate trimming
            cmd.arg(self.config.max_stream_length);
        }

        // Auto-generate ID
        cmd.arg("*");

        // Add fields
        cmd.arg("data").arg(data);
        cmd.arg("format").arg(self.config.format.as_str());

        let message_id: String = cmd.query_async(&mut **conn).await?;

        Ok(message_id)
    }

    /// Get current statistics.
    #[must_use]
    pub fn stats(&self) -> DualStats {
        self.stats.read().clone()
    }

    /// Reset statistics.
    pub fn reset_stats(&self) {
        self.stats.write().reset();
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &DualPublisherConfig {
        &self.config
    }
}

impl std::fmt::Debug for DualPublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DualPublisher")
            .field("config", &self.config)
            .field("stats", &*self.stats.read())
            .finish()
    }
}

// =============================================================================
// SEND + SYNC VERIFICATION
// =============================================================================

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DualPublisherConfig>();
    assert_send_sync::<DualStats>();
    assert_send_sync::<DualPublishResult>();
    assert_send_sync::<DualPublisher>();
};

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = DualPublisherConfig::default();
        assert!(config.orderbook_enabled);
        assert!(config.signal_enabled);
        assert_eq!(config.orderbook_key_prefix, DEFAULT_ORDERBOOK_PREFIX);
        assert_eq!(config.signal_key_prefix, FUSION_TOPIC_PREFIX);
    }

    #[test]
    fn test_stats_default() {
        let stats = DualStats::default();
        assert_eq!(stats.orderbook_published, 0);
        assert_eq!(stats.signals_published, 0);
    }

    #[test]
    fn test_stats_avg_latency() {
        let stats = DualStats {
            total_latency_us: 1000,
            publish_count: 4,
            ..Default::default()
        };
        assert!((stats.avg_latency_us() - 250.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_avg_latency_zero_count() {
        let stats = DualStats::default();
        assert!((stats.avg_latency_us() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_builder_chain() {
        let builder = DualPublisherBuilder::default()
            .orderbook_enabled(true)
            .signal_enabled(false)
            .ttl_seconds(120);

        let config = builder.config();
        assert!(config.orderbook_enabled);
        assert!(!config.signal_enabled);
        assert_eq!(config.ttl_seconds, 120);
    }
}
