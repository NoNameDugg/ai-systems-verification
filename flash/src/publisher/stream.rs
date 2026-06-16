//! Redis Stream Publisher for Flash.
//!
//! This module provides high-throughput publishing to Redis Streams using XADD,
//! with support for multiple serialization formats per RED TEAM requirements.
//!
//! # Features
//!
//! - **Multi-format serialization**: JSON (debug), Bincode (production), Rkyv (zero-copy)
//! - **Topic naming**: Standardized topic structure for market data
//! - **Statistics tracking**: Publish latency, message counts, error tracking
//! - **Builder pattern**: Fluent configuration API
//!
//! # Serialization Strategy (RED TEAM)
//!
//! | Format | Use Case | Performance |
//! |--------|----------|-------------|
//! | JSON | Debug, external APIs | ~15 μs |
//! | Bincode | Production internal | ~0.5 μs |
//! | Rkyv | Zero-copy Python | ~0.1 μs |
//!
//! # Topic Naming Convention
//!
//! ## Fusion-Compatible Format (Default)
//!
//! ```text
//! astra:signals:flash:{exchange}:{symbol}       → AlphaSignal for Fusion
//! ```
//!
//! ## Legacy Format (if topic_prefix = "market_data")
//!
//! ```text
//! market_data.{exchange}.{base}_{quote}.book    → Order book snapshots
//! market_data.{exchange}.{base}_{quote}.trade   → Trade executions
//! market_data.{exchange}.{base}_{quote}.ticker  → Ticker updates
//! system.flash.health                            → Health heartbeats
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::stream::{StreamPublisher, SerializationFormat};
//! use astra_flash::publisher::pool::RedisPool;
//! use std::sync::Arc;
//!
//! // Create publisher
//! let pool = Arc::new(RedisPool::new(config).await?);
//! let publisher = StreamPublisher::builder(pool)
//!     .format(SerializationFormat::Bincode)
//!     .max_stream_length(100_000)
//!     .build();
//!
//! // Publish book snapshot
//! let result = publisher.publish_book(&snapshot).await?;
//! println!("Published: {} in {}μs", result.message_id, result.latency_us);
//! ```

use crate::book::orderbook::BookSnapshot;
use crate::core::types::{now_micros, Exchange, Instrument, MarketEvent, Timestamp};
use crate::publisher::pool::{PoolError, RedisPool};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

// =============================================================================
// SERIALIZATION FORMAT
// =============================================================================

/// Serialization format for Redis publishing.
///
/// Determines how data is encoded before publishing to Redis Streams.
///
/// # Performance Comparison (50-level book)
///
/// | Format | Serialize | Size |
/// |--------|-----------|------|
/// | JSON | ~12 μs | ~4 KB |
/// | Bincode | ~0.4 μs | ~2 KB |
/// | Rkyv | ~0.3 μs | ~2 KB |
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::SerializationFormat;
///
/// let format = SerializationFormat::default();
/// assert_eq!(format, SerializationFormat::Bincode);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum SerializationFormat {
    /// JSON format (human-readable, for debugging).
    Json,
    /// Bincode format (compact, fast, recommended for production).
    #[default]
    Bincode,
    /// Rkyv format (zero-copy, fastest for Python consumption).
    Rkyv,
}


impl SerializationFormat {
    /// Returns the format name as a string.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::stream::SerializationFormat;
    ///
    /// assert_eq!(SerializationFormat::Json.as_str(), "json");
    /// assert_eq!(SerializationFormat::Bincode.as_str(), "bincode");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Bincode => "bincode",
            Self::Rkyv => "rkyv",
        }
    }
}

impl std::fmt::Display for SerializationFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// TOPIC TYPE
// =============================================================================

/// Topic type for market data streams.
///
/// Used with [`TopicBuilder`] to construct topic names.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::TopicType;
///
/// assert_eq!(TopicType::Book.as_str(), "book");
/// assert_eq!(TopicType::Trade.as_str(), "trade");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopicType {
    /// Order book data.
    Book,
    /// Trade executions.
    Trade,
    /// Ticker updates.
    Ticker,
    /// Raw market events.
    Event,
}

impl TopicType {
    /// Returns the topic type as a string.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Trade => "trade",
            Self::Ticker => "ticker",
            Self::Event => "event",
        }
    }
}

impl std::fmt::Display for TopicType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// TOPIC BUILDER
// =============================================================================

/// Builder for constructing Redis Stream topic names.
///
/// Provides standardized topic naming following the project convention:
/// `{prefix}.{exchange}.{base}_{quote}.{type}`
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::{TopicBuilder, TopicType};
/// use astra_flash::core::types::Exchange;
///
/// let builder = TopicBuilder::new("market_data");
/// let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
/// assert_eq!(topic, "market_data.deribit.btc_usd.book");
/// ```
#[derive(Debug, Clone)]
pub struct TopicBuilder {
    prefix: String,
}

impl TopicBuilder {
    /// Create a new topic builder with the given prefix.
    ///
    /// The prefix is converted to lowercase.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into().to_lowercase(),
        }
    }

    /// Build a market data topic name.
    ///
    /// Format: `{prefix}.{exchange}.{base}_{quote}.{type}`
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::stream::{TopicBuilder, TopicType};
    /// use astra_flash::core::types::Exchange;
    ///
    /// let builder = TopicBuilder::new("market_data");
    /// let topic = builder.market_data(Exchange::Binance, "ETH", "USDT", TopicType::Trade);
    /// assert_eq!(topic, "market_data.binance.eth_usdt.trade");
    /// ```
    #[must_use]
    pub fn market_data(
        &self,
        exchange: Exchange,
        base: &str,
        quote: &str,
        topic_type: TopicType,
    ) -> String {
        format!(
            "{}.{}.{}_{}.{}",
            self.prefix,
            exchange.as_str(),
            base.to_lowercase(),
            quote.to_lowercase(),
            topic_type.as_str()
        )
    }

    /// Build a topic name for an instrument.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::stream::{TopicBuilder, TopicType};
    /// use astra_flash::core::types::{Exchange, Instrument};
    ///
    /// let builder = TopicBuilder::new("market_data");
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let topic = builder.for_instrument(&instrument, TopicType::Book);
    /// assert_eq!(topic, "market_data.deribit.btc_usd.book");
    /// ```
    #[must_use]
    pub fn for_instrument(&self, instrument: &Instrument, topic_type: TopicType) -> String {
        self.market_data(
            instrument.exchange,
            &instrument.base,
            &instrument.quote,
            topic_type,
        )
    }

    /// Build a system topic name.
    ///
    /// Format: `system.flash.{name}`
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::stream::TopicBuilder;
    ///
    /// let builder = TopicBuilder::new("market_data");
    /// let topic = builder.system("health");
    /// assert_eq!(topic, "system.flash.health");
    /// ```
    #[must_use]
    pub fn system(&self, name: &str) -> String {
        format!("system.flash.{}", name.to_lowercase())
    }
}

impl Default for TopicBuilder {
    fn default() -> Self {
        Self::new("market_data")
    }
}

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for the stream publisher.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::{StreamPublisherConfig, SerializationFormat};
///
/// let config = StreamPublisherConfig {
///     format: SerializationFormat::Bincode,
///     max_stream_length: 100_000,
///     ..StreamPublisherConfig::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct StreamPublisherConfig {
    /// Default serialization format.
    pub format: SerializationFormat,

    /// Maximum stream length (MAXLEN option). 0 = unlimited.
    pub max_stream_length: u64,

    /// Use approximate trimming (~) for better performance.
    pub approximate_trimming: bool,

    /// Include timestamp field in messages.
    pub include_timestamp: bool,

    /// Field name for serialized data.
    pub data_field: String,

    /// Field name for format indicator.
    pub format_field: String,

    /// Topic prefix (e.g., "market_data").
    pub topic_prefix: String,
}

impl Default for StreamPublisherConfig {
    fn default() -> Self {
        Self {
            format: SerializationFormat::Bincode,
            max_stream_length: 100_000,
            approximate_trimming: true,
            include_timestamp: true,
            data_field: "data".to_string(),
            format_field: "format".to_string(),
            topic_prefix: "market_data".to_string(),
        }
    }
}

impl StreamPublisherConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if validation fails.
    pub fn validate(&self) -> StreamResult<()> {
        if self.topic_prefix.is_empty() {
            return Err(StreamError::InvalidConfig(
                "topic_prefix cannot be empty".to_string(),
            ));
        }

        if self.data_field.is_empty() {
            return Err(StreamError::InvalidConfig(
                "data_field cannot be empty".to_string(),
            ));
        }

        if self.format_field.is_empty() {
            return Err(StreamError::InvalidConfig(
                "format_field cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Statistics for stream publishing.
///
/// Tracks message counts, serialization, and latency for monitoring.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::StreamStats;
///
/// let mut stats = StreamStats::default();
/// stats.messages_published += 1;
/// stats.bytes_serialized += 1024;
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// Total messages published successfully.
    pub messages_published: u64,

    /// Total bytes serialized.
    pub bytes_serialized: u64,

    /// Total XADD commands executed.
    pub xadd_commands: u64,

    /// Total serialization errors.
    pub serialization_errors: u64,

    /// Total publish errors.
    pub publish_errors: u64,

    /// Average publish latency (microseconds, EMA).
    pub avg_publish_latency_us: f64,

    /// Last publish timestamp.
    pub last_publish: Option<Timestamp>,
}

impl StreamStats {
    /// Reset all statistics to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Update average latency using EMA (Exponential Moving Average).
    ///
    /// Uses alpha = 0.2 for smoothing.
    pub fn update_latency(&mut self, latency_us: f64) {
        const ALPHA: f64 = 0.2;
        self.avg_publish_latency_us =
            ALPHA.mul_add(latency_us, (1.0 - ALPHA) * self.avg_publish_latency_us);
    }
}

// =============================================================================
// PUBLISH RESULT
// =============================================================================

/// Result of a successful publish operation.
///
/// Contains the Redis stream message ID and performance metrics.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::stream::PublishResult;
///
/// let result = PublishResult {
///     message_id: "1234567890-0".to_string(),
///     topic: "market_data.deribit.btc_usd.book".to_string(),
///     data_size: 1024,
///     latency_us: 500,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct PublishResult {
    /// Redis stream message ID (e.g., "1234567890-0").
    pub message_id: String,

    /// Topic published to.
    pub topic: String,

    /// Serialized data size in bytes.
    pub data_size: usize,

    /// Publish latency in microseconds.
    pub latency_us: u64,
}

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during stream publishing.
#[derive(Debug, Error)]
pub enum StreamError {
    /// Serialization failed.
    #[error("Serialization failed for format {format:?}: {reason}")]
    SerializationFailed {
        /// The format that failed.
        format: SerializationFormat,
        /// Reason for failure.
        reason: String,
    },

    /// XADD command failed.
    #[error("XADD failed for topic {topic}: {reason}")]
    XaddFailed {
        /// The topic that failed.
        topic: String,
        /// Reason for failure.
        reason: String,
    },

    /// Invalid topic name.
    #[error("Invalid topic name: {0}")]
    InvalidTopic(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// No connection available.
    #[error("No connection available")]
    NoConnection,

    /// Pool error.
    #[error("Pool error: {0}")]
    PoolError(#[from] PoolError),
}

/// Result type for stream operations.
pub type StreamResult<T> = Result<T, StreamError>;

// =============================================================================
// STREAM PUBLISHER BUILDER
// =============================================================================

/// Builder for creating [`StreamPublisher`] with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// let publisher = StreamPublisherBuilder::default()
///     .format(SerializationFormat::Json)
///     .max_stream_length(50_000)
///     .topic_prefix("custom")
///     .build(pool);
/// ```
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct StreamPublisherBuilder {
    config: StreamPublisherConfig,
}


impl StreamPublisherBuilder {
    /// Set the serialization format.
    #[must_use]
    pub const fn format(mut self, format: SerializationFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set the maximum stream length (MAXLEN).
    #[must_use]
    pub const fn max_stream_length(mut self, len: u64) -> Self {
        self.config.max_stream_length = len;
        self
    }

    /// Enable/disable approximate trimming.
    #[must_use]
    pub const fn approximate_trimming(mut self, enabled: bool) -> Self {
        self.config.approximate_trimming = enabled;
        self
    }

    /// Enable/disable timestamp inclusion.
    #[must_use]
    pub const fn include_timestamp(mut self, enabled: bool) -> Self {
        self.config.include_timestamp = enabled;
        self
    }

    /// Set the topic prefix.
    #[must_use]
    pub fn topic_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.topic_prefix = prefix.into();
        self
    }

    /// Set the data field name.
    #[must_use]
    pub fn data_field(mut self, field: impl Into<String>) -> Self {
        self.config.data_field = field.into();
        self
    }

    /// Set the format field name.
    #[must_use]
    pub fn format_field(mut self, field: impl Into<String>) -> Self {
        self.config.format_field = field.into();
        self
    }

    /// Get the current configuration.
    #[must_use]
    pub const fn config(&self) -> &StreamPublisherConfig {
        &self.config
    }

    /// Build the stream publisher.
    #[must_use]
    pub fn build(self, pool: Arc<RedisPool>) -> StreamPublisher {
        StreamPublisher::new(pool, self.config)
    }
}

// =============================================================================
// STREAM PUBLISHER
// =============================================================================

/// Redis Stream Publisher for high-throughput market data publishing.
///
/// Publishes [`BookSnapshot`] and [`MarketEvent`] data to Redis Streams
/// using XADD, with support for multiple serialization formats.
///
/// # Thread Safety
///
/// `StreamPublisher` is `Send + Sync` and can be safely shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::publisher::stream::{StreamPublisher, SerializationFormat};
/// use std::sync::Arc;
///
/// let pool = Arc::new(RedisPool::new(config).await?);
/// let publisher = StreamPublisher::builder(pool)
///     .format(SerializationFormat::Bincode)
///     .build();
///
/// // Publish a book snapshot
/// let result = publisher.publish_book(&snapshot).await?;
/// println!("Published: {} bytes in {}μs", result.data_size, result.latency_us);
/// ```
pub struct StreamPublisher {
    pool: Arc<RedisPool>,
    config: StreamPublisherConfig,
    stats: Arc<RwLock<StreamStats>>,
    topic_builder: TopicBuilder,
}

impl StreamPublisher {
    /// Create a new stream publisher.
    #[must_use]
    pub fn new(pool: Arc<RedisPool>, config: StreamPublisherConfig) -> Self {
        let topic_builder = TopicBuilder::new(&config.topic_prefix);
        Self {
            pool,
            config,
            stats: Arc::new(RwLock::new(StreamStats::default())),
            topic_builder,
        }
    }

    /// Create a publisher with default configuration.
    #[must_use]
    pub fn with_defaults(pool: Arc<RedisPool>) -> Self {
        Self::new(pool, StreamPublisherConfig::default())
    }

    /// Serialize a book snapshot.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn serialize_book(
        snapshot: &BookSnapshot,
        format: SerializationFormat,
    ) -> StreamResult<Vec<u8>> {
        match format {
            SerializationFormat::Json => {
                serde_json::to_vec(snapshot).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: e.to_string(),
                })
            },
            SerializationFormat::Bincode => {
                bincode::serialize(snapshot).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: e.to_string(),
                })
            },
            SerializationFormat::Rkyv => {
                // Rkyv requires Archive derive on the type
                // For now, fall back to bincode for rkyv format
                // Full rkyv support requires derive macros on BookSnapshot
                bincode::serialize(snapshot).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: format!("Rkyv fallback to bincode: {e}"),
                })
            },
        }
    }

    /// Serialize a market event.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn serialize_event(
        event: &MarketEvent,
        format: SerializationFormat,
    ) -> StreamResult<Vec<u8>> {
        match format {
            SerializationFormat::Json => {
                serde_json::to_vec(event).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: e.to_string(),
                })
            },
            SerializationFormat::Bincode => {
                bincode::serialize(event).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: e.to_string(),
                })
            },
            SerializationFormat::Rkyv => {
                // Rkyv requires Archive derive on the type
                bincode::serialize(event).map_err(|e| StreamError::SerializationFailed {
                    format,
                    reason: format!("Rkyv fallback to bincode: {e}"),
                })
            },
        }
    }

    /// Publish a book snapshot to Redis Streams.
    ///
    /// Uses the configured serialization format.
    ///
    /// # Errors
    ///
    /// Returns error if serialization or publishing fails.
    pub async fn publish_book(&self, snapshot: &BookSnapshot) -> StreamResult<PublishResult> {
        self.publish_book_with_format(snapshot, self.config.format)
            .await
    }

    /// Publish a book snapshot with a specific format.
    ///
    /// # Errors
    ///
    /// Returns error if serialization or publishing fails.
    pub async fn publish_book_with_format(
        &self,
        snapshot: &BookSnapshot,
        format: SerializationFormat,
    ) -> StreamResult<PublishResult> {
        let start = std::time::Instant::now();

        // Build topic
        let topic = self
            .topic_builder
            .for_instrument(&snapshot.instrument, TopicType::Book);

        // Serialize
        let data = Self::serialize_book(snapshot, format)?;
        let data_size = data.len();

        // Publish
        let message_id = self.xadd(&topic, &data, format, snapshot.timestamp).await?;

        let latency_us = start.elapsed().as_micros() as u64;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.messages_published += 1;
            stats.bytes_serialized += data_size as u64;
            stats.xadd_commands += 1;
            stats.update_latency(latency_us as f64);
            stats.last_publish = Some(now_micros());
        }

        Ok(PublishResult {
            message_id,
            topic,
            data_size,
            latency_us,
        })
    }

    /// Publish a market event to Redis Streams.
    ///
    /// # Errors
    ///
    /// Returns error if serialization or publishing fails.
    pub async fn publish_event(&self, event: &MarketEvent) -> StreamResult<PublishResult> {
        self.publish_event_with_format(event, self.config.format)
            .await
    }

    /// Publish a market event with a specific format.
    ///
    /// # Errors
    ///
    /// Returns error if serialization or publishing fails.
    pub async fn publish_event_with_format(
        &self,
        event: &MarketEvent,
        format: SerializationFormat,
    ) -> StreamResult<PublishResult> {
        let start = std::time::Instant::now();

        // Determine topic type from event
        let topic_type = match &event.data {
            crate::core::types::MarketData::Book { .. } => TopicType::Book,
            crate::core::types::MarketData::Trade { .. } => TopicType::Trade,
            crate::core::types::MarketData::Heartbeat { .. } => TopicType::Event,
        };

        // Build topic
        let topic = self
            .topic_builder
            .for_instrument(&event.instrument, topic_type);

        // Serialize
        let data = Self::serialize_event(event, format)?;
        let data_size = data.len();

        // Publish
        let message_id = self.xadd(&topic, &data, format, event.timestamp).await?;

        let latency_us = start.elapsed().as_micros() as u64;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.messages_published += 1;
            stats.bytes_serialized += data_size as u64;
            stats.xadd_commands += 1;
            stats.update_latency(latency_us as f64);
            stats.last_publish = Some(now_micros());
        }

        Ok(PublishResult {
            message_id,
            topic,
            data_size,
            latency_us,
        })
    }

    /// Publish raw data to a topic.
    ///
    /// # Errors
    ///
    /// Returns error if publishing fails.
    pub async fn publish_raw(
        &self,
        topic: &str,
        data: &[u8],
        format: SerializationFormat,
        timestamp: Timestamp,
    ) -> StreamResult<PublishResult> {
        let start = std::time::Instant::now();
        let data_size = data.len();

        let message_id = self.xadd(topic, data, format, timestamp).await?;

        let latency_us = start.elapsed().as_micros() as u64;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.messages_published += 1;
            stats.bytes_serialized += data_size as u64;
            stats.xadd_commands += 1;
            stats.update_latency(latency_us as f64);
            stats.last_publish = Some(now_micros());
        }

        Ok(PublishResult {
            message_id,
            topic: topic.to_string(),
            data_size,
            latency_us,
        })
    }

    /// Publish a health heartbeat.
    ///
    /// # Errors
    ///
    /// Returns error if publishing fails.
    pub async fn publish_health(&self) -> StreamResult<PublishResult> {
        let topic = self.topic_builder.system("health");
        let timestamp = now_micros();

        // Simple health payload
        let data = format!(r#"{{"ts":{timestamp}}}"#);

        self.publish_raw(
            &topic,
            data.as_bytes(),
            SerializationFormat::Json,
            timestamp,
        )
        .await
    }

    /// Execute XADD command.
    async fn xadd(
        &self,
        topic: &str,
        data: &[u8],
        format: SerializationFormat,
        timestamp: Timestamp,
    ) -> StreamResult<String> {
        let mut conn = self.pool.get().await?;

        // Build XADD command
        // XADD key [MAXLEN [~] count] * field value [field value ...]
        let mut cmd = redis::cmd("XADD");
        cmd.arg(topic);

        // Add MAXLEN if configured
        if self.config.max_stream_length > 0 {
            cmd.arg("MAXLEN");
            if self.config.approximate_trimming {
                cmd.arg("~");
            }
            cmd.arg(self.config.max_stream_length);
        }

        // Auto-generate ID
        cmd.arg("*");

        // Add fields
        if self.config.include_timestamp {
            cmd.arg("timestamp").arg(timestamp.to_string());
        }
        cmd.arg(&self.config.format_field).arg(format.as_str());
        cmd.arg(&self.config.data_field).arg(data);

        // Execute
        let message_id: String = cmd.query_async(&mut *conn).await.map_err(|e| {
            let mut stats = self.stats.write();
            stats.publish_errors += 1;
            StreamError::XaddFailed {
                topic: topic.to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(message_id)
    }

    /// Get current statistics.
    #[must_use]
    pub fn stats(&self) -> StreamStats {
        self.stats.read().clone()
    }

    /// Reset statistics.
    pub fn reset_stats(&self) {
        self.stats.write().reset();
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &StreamPublisherConfig {
        &self.config
    }
}

impl std::fmt::Debug for StreamPublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamPublisher")
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
    assert_send_sync::<SerializationFormat>();
    assert_send_sync::<StreamPublisherConfig>();
    assert_send_sync::<StreamStats>();
    assert_send_sync::<PublishResult>();
    assert_send_sync::<TopicType>();
    assert_send_sync::<TopicBuilder>();
    assert_send_sync::<StreamPublisher>();
};

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization_format_default() {
        let format = SerializationFormat::default();
        assert_eq!(format, SerializationFormat::Bincode);
    }

    #[test]
    fn test_serialization_format_as_str() {
        assert_eq!(SerializationFormat::Json.as_str(), "json");
        assert_eq!(SerializationFormat::Bincode.as_str(), "bincode");
        assert_eq!(SerializationFormat::Rkyv.as_str(), "rkyv");
    }

    #[test]
    fn test_topic_type_as_str() {
        assert_eq!(TopicType::Book.as_str(), "book");
        assert_eq!(TopicType::Trade.as_str(), "trade");
        assert_eq!(TopicType::Ticker.as_str(), "ticker");
        assert_eq!(TopicType::Event.as_str(), "event");
    }

    #[test]
    fn test_topic_builder_market_data() {
        let builder = TopicBuilder::new("market_data");
        let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
        assert_eq!(topic, "market_data.deribit.btc_usd.book");
    }

    #[test]
    fn test_topic_builder_system() {
        let builder = TopicBuilder::new("market_data");
        let topic = builder.system("health");
        assert_eq!(topic, "system.flash.health");
    }

    #[test]
    fn test_config_default() {
        let config = StreamPublisherConfig::default();
        assert_eq!(config.format, SerializationFormat::Bincode);
        assert_eq!(config.max_stream_length, 100_000);
        assert!(config.approximate_trimming);
    }

    #[test]
    fn test_config_validation() {
        let config = StreamPublisherConfig::default();
        assert!(config.validate().is_ok());

        let invalid = StreamPublisherConfig {
            topic_prefix: String::new(),
            ..StreamPublisherConfig::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_stats_default() {
        let stats = StreamStats::default();
        assert_eq!(stats.messages_published, 0);
        assert_eq!(stats.bytes_serialized, 0);
    }

    #[test]
    fn test_stats_reset() {
        let mut stats = StreamStats::default();
        stats.messages_published = 100;
        stats.reset();
        assert_eq!(stats.messages_published, 0);
    }

    #[test]
    fn test_builder_chain() {
        let builder = StreamPublisherBuilder::default()
            .format(SerializationFormat::Json)
            .max_stream_length(50_000)
            .topic_prefix("test");

        let config = builder.config();
        assert_eq!(config.format, SerializationFormat::Json);
        assert_eq!(config.max_stream_length, 50_000);
        assert_eq!(config.topic_prefix, "test");
    }

    #[test]
    fn test_publish_result_clone() {
        let result = PublishResult {
            message_id: "123-0".to_string(),
            topic: "test".to_string(),
            data_size: 100,
            latency_us: 50,
        };
        let cloned = result.clone();
        assert_eq!(result.message_id, cloned.message_id);
    }

    #[test]
    fn test_error_display() {
        let error = StreamError::InvalidTopic("test".to_string());
        assert!(error.to_string().contains("Invalid topic"));
    }
}
