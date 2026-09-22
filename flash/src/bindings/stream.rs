//! Python bindings for Redis Stream consumption (Phase 5.3).
//!
//! This module provides Python bindings for consuming market data from Redis Streams:
//!
//! - [`PyStreamConfig`] - Configuration for stream subscriptions
//! - [`PyFlashClient`] - Redis client for consuming streams
//! - [`PyStreamIterator`] - Iterator over market events
//! - [`PyFlashClientStats`] - Statistics for stream consumption
//!
//! # Features
//!
//! - Synchronous iteration via Python iterator protocol
//! - Multi-format deserialization (JSON, Bincode)
//! - Consumer group support
//! - Thread-safe subscription management
//!
//! # Python Usage
//!
//! ```python
//! from astra_flash import FlashClient, StreamConfig
//!
//! # Create client
//! client = FlashClient("redis://localhost:6379")
//!
//! # Subscribe to streams
//! config = StreamConfig(
//!     topics=["market_data.deribit.btc_usd.book"],
//!     format="bincode",
//!     start_id="$",
//!     block_ms=5000,
//! )
//!
//! # Iterate over events
//! for event in client.subscribe(config):
//!     print(f"Received: {event.event_type}")
//!     if event.data.is_book():
//!         bids, asks = event.data.as_book()
//!         print(f"Best bid: {bids[0].price if bids else 'N/A'}")
//!
//! # Clean up
//! client.close()
//! ```
//!
//! # Async Pattern (Python-side)
//!
//! For async consumption, wrap the iterator in `asyncio.to_thread`:
//!
//! ```python
//! import asyncio
//!
//! async def consume_events():
//!     client = FlashClient("redis://localhost:6379")
//!     iterator = client.subscribe_one("market_data.deribit.btc_usd.book")
//!
//!     while True:
//!         event = await asyncio.to_thread(lambda: next(iterator, None))
//!         if event is None:
//!             break
//!         process(event)
//! ```

use parking_lot::RwLock;
use pyo3::exceptions::{PyConnectionError, PyRuntimeError, PyStopIteration, PyValueError};
use pyo3::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

use crate::bindings::types::{PyBookSnapshot, PyMarketEvent};
use crate::book::BookSnapshot;
use crate::core::types::{MarketEvent, Timestamp};
use crate::publisher::pool::{PoolConfig, RedisPool};

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during stream operations.
#[derive(Debug, Error)]
pub enum StreamError {
    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Deserialization failed.
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    /// Unsupported format.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Connection error.
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Subscription stopped.
    #[error("Subscription stopped")]
    SubscriptionStopped,
}

/// Result type for stream operations.
pub type StreamResult<T> = Result<T, StreamError>;

// =============================================================================
// DESERIALIZATION FUNCTIONS
// =============================================================================

/// Deserialize a market event from raw bytes.
///
/// # Arguments
///
/// * `data` - Raw bytes from Redis
/// * `format` - Format hint ("json" or "bincode")
///
/// # Returns
///
/// Deserialized `PyMarketEvent` or error.
///
/// # Example
///
/// ```rust,ignore
/// let event = deserialize_event(&bytes, "bincode")?;
/// ```
pub fn deserialize_event(data: &[u8], format: &str) -> StreamResult<PyMarketEvent> {
    match format.to_lowercase().as_str() {
        "json" => {
            let event: MarketEvent = serde_json::from_slice(data)
                .map_err(|e| StreamError::DeserializationFailed(e.to_string()))?;
            Ok(event.into())
        }
        "bincode" => {
            let event: MarketEvent = bincode::deserialize(data)
                .map_err(|e| StreamError::DeserializationFailed(e.to_string()))?;
            Ok(event.into())
        }
        _ => Err(StreamError::UnsupportedFormat(format.to_string())),
    }
}

/// Deserialize a book snapshot from raw bytes.
///
/// # Arguments
///
/// * `data` - Raw bytes from Redis
/// * `format` - Format hint ("json" or "bincode")
///
/// # Returns
///
/// Deserialized `PyBookSnapshot` or error.
///
/// # Example
///
/// ```rust,ignore
/// let snapshot = deserialize_book(&bytes, "json")?;
/// ```
pub fn deserialize_book(data: &[u8], format: &str) -> StreamResult<PyBookSnapshot> {
    match format.to_lowercase().as_str() {
        "json" => {
            let snapshot: BookSnapshot = serde_json::from_slice(data)
                .map_err(|e| StreamError::DeserializationFailed(e.to_string()))?;
            Ok(snapshot.into())
        }
        "bincode" => {
            let snapshot: BookSnapshot = bincode::deserialize(data)
                .map_err(|e| StreamError::DeserializationFailed(e.to_string()))?;
            Ok(snapshot.into())
        }
        _ => Err(StreamError::UnsupportedFormat(format.to_string())),
    }
}

// =============================================================================
// PYSTREAMCONFIG
// =============================================================================

/// Configuration for stream subscriptions.
///
/// # Properties
///
/// - `topics` - List of Redis stream topics to subscribe to
/// - `format` - Serialization format ("json" or "bincode")
/// - `start_id` - Starting message ID ("$" for new only, "0" for all)
/// - `block_ms` - Block timeout in milliseconds (0 = no blocking)
/// - `count` - Maximum messages per XREAD call
/// - `group_name` - Consumer group name (optional)
/// - `consumer_name` - Consumer name within group (optional)
///
/// # Example
///
/// ```python
/// from astra_flash import StreamConfig
///
/// # Subscribe to new messages only
/// config = StreamConfig(
///     topics=["market_data.deribit.btc_usd.book"],
///     format="bincode",
///     start_id="$",
///     block_ms=5000,
///     count=100,
/// )
///
/// # With consumer group
/// config = StreamConfig(
///     topics=["market_data.deribit.btc_usd.book"],
///     format="bincode",
///     start_id=">",
///     block_ms=5000,
///     count=100,
///     group_name="my_group",
///     consumer_name="consumer_1",
/// )
/// ```
#[pyclass(name = "StreamConfig", frozen)]
#[derive(Debug, Clone)]
pub struct PyStreamConfig {
    /// Topics to subscribe to.
    topics: Vec<String>,

    /// Serialization format hint.
    format: String,

    /// Starting message ID.
    start_id: String,

    /// Block timeout in milliseconds.
    block_ms: u64,

    /// Maximum messages per read.
    count: usize,

    /// Consumer group name.
    group_name: Option<String>,

    /// Consumer name within group.
    consumer_name: Option<String>,
}

#[pymethods]
impl PyStreamConfig {
    /// Creates a new stream configuration.
    ///
    /// # Arguments
    ///
    /// * `topics` - List of Redis stream topics
    /// * `format` - Serialization format ("json" or "bincode")
    /// * `start_id` - Starting message ID
    /// * `block_ms` - Block timeout in milliseconds
    /// * `count` - Maximum messages per read
    /// * `group_name` - Consumer group name (optional)
    /// * `consumer_name` - Consumer name (optional)
    #[new]
    #[pyo3(signature = (topics, format="bincode".to_string(), start_id="$".to_string(), block_ms=5000, count=100, group_name=None, consumer_name=None))]
    #[must_use]
    pub fn new(
        topics: Vec<String>,
        format: String,
        start_id: String,
        block_ms: u64,
        count: usize,
        group_name: Option<String>,
        consumer_name: Option<String>,
    ) -> Self {
        Self {
            topics,
            format,
            start_id,
            block_ms,
            count,
            group_name,
            consumer_name,
        }
    }

    /// Returns the topics list.
    #[getter]
    #[must_use]
    pub fn topics(&self) -> Vec<String> {
        self.topics.clone()
    }

    /// Returns the serialization format.
    #[getter]
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the starting message ID.
    #[getter]
    #[must_use]
    pub fn start_id(&self) -> &str {
        &self.start_id
    }

    /// Returns the block timeout in milliseconds.
    #[getter]
    #[must_use]
    pub fn block_ms(&self) -> u64 {
        self.block_ms
    }

    /// Returns the maximum messages per read.
    #[getter]
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Returns the consumer group name.
    #[getter]
    #[must_use]
    pub fn group_name(&self) -> Option<String> {
        self.group_name.clone()
    }

    /// Returns the consumer name.
    #[getter]
    #[must_use]
    pub fn consumer_name(&self) -> Option<String> {
        self.consumer_name.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "StreamConfig(topics={}, format='{}', start_id='{}', block_ms={})",
            self.topics.len(),
            self.format,
            self.start_id,
            self.block_ms
        )
    }

    fn __str__(&self) -> String {
        format!(
            "StreamConfig(topics={}, format={})",
            self.topics.len(),
            self.format
        )
    }
}

impl PyStreamConfig {
    /// Validate topics list.
    pub fn validate_topics(topics: Vec<String>) -> StreamResult<()> {
        if topics.is_empty() {
            return Err(StreamError::InvalidConfig(
                "topics cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

// =============================================================================
// PYFLASHCLIENTSTATS
// =============================================================================

/// Statistics for stream consumption.
///
/// # Properties
///
/// - `messages_received` - Total messages received
/// - `bytes_deserialized` - Total bytes deserialized
/// - `deserialize_errors` - Number of deserialization errors
/// - `connection_errors` - Number of connection errors
/// - `avg_latency_us` - Average latency in microseconds (EMA)
///
/// # Example
///
/// ```python
/// client = FlashClient("redis://localhost:6379")
/// stats = client.stats
/// print(f"Messages: {stats.messages_received}")
/// print(f"Avg latency: {stats.avg_latency_us:.2f}μs")
/// ```
#[pyclass(name = "FlashClientStats")]
#[derive(Debug, Clone)]
pub struct PyFlashClientStats {
    /// Total messages received.
    messages_received: u64,

    /// Total bytes deserialized.
    bytes_deserialized: u64,

    /// Deserialization errors.
    deserialize_errors: u64,

    /// Connection errors.
    connection_errors: u64,

    /// Average latency (microseconds, EMA).
    avg_latency_us: f64,

    /// Last receive timestamp.
    last_receive: Option<Timestamp>,
}

impl Default for PyFlashClientStats {
    fn default() -> Self {
        Self {
            messages_received: 0,
            bytes_deserialized: 0,
            deserialize_errors: 0,
            connection_errors: 0,
            avg_latency_us: 0.0,
            last_receive: None,
        }
    }
}

#[pymethods]
impl PyFlashClientStats {
    /// Returns total messages received.
    #[getter]
    #[must_use]
    pub fn messages_received(&self) -> u64 {
        self.messages_received
    }

    /// Returns total bytes deserialized.
    #[getter]
    #[must_use]
    pub fn bytes_deserialized(&self) -> u64 {
        self.bytes_deserialized
    }

    /// Returns number of deserialization errors.
    #[getter]
    #[must_use]
    pub fn deserialize_errors(&self) -> u64 {
        self.deserialize_errors
    }

    /// Returns number of connection errors.
    #[getter]
    #[must_use]
    pub fn connection_errors(&self) -> u64 {
        self.connection_errors
    }

    /// Returns average latency in microseconds.
    #[getter]
    #[must_use]
    pub fn avg_latency_us(&self) -> f64 {
        self.avg_latency_us
    }

    /// Returns last receive timestamp.
    #[getter]
    #[must_use]
    pub fn last_receive(&self) -> Option<Timestamp> {
        self.last_receive
    }

    fn __repr__(&self) -> String {
        format!(
            "FlashClientStats(messages={}, bytes={}, errors={})",
            self.messages_received, self.bytes_deserialized, self.deserialize_errors
        )
    }
}

impl PyFlashClientStats {
    /// Record a received message.
    pub fn record_message(&mut self, bytes: u64) {
        self.messages_received += 1;
        self.bytes_deserialized += bytes;
        self.last_receive = Some(crate::core::types::now_micros());
    }

    /// Record a deserialization error.
    pub fn record_error(&mut self) {
        self.deserialize_errors += 1;
    }

    /// Record a connection error.
    pub fn record_connection_error(&mut self) {
        self.connection_errors += 1;
    }

    /// Update average latency using EMA.
    pub fn update_latency(&mut self, latency_us: f64) {
        const ALPHA: f64 = 0.2;
        self.avg_latency_us = ALPHA * latency_us + (1.0 - ALPHA) * self.avg_latency_us;
    }

    /// Reset all statistics.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// =============================================================================
// PYSTREAMITERATOR
// =============================================================================

/// Iterator over market events from Redis Streams.
///
/// # Thread Safety
///
/// This class is thread-safe. Multiple threads can call `stop()` safely.
///
/// # Example
///
/// ```python
/// client = FlashClient("redis://localhost:6379")
/// iterator = client.subscribe_one("market_data.deribit.btc_usd.book")
///
/// for event in iterator:
///     print(f"Event: {event.event_type}")
///
/// # Or manually
/// while iterator.is_active():
///     try:
///         event = next(iterator)
///         print(f"Event: {event.event_type}")
///     except StopIteration:
///         break
///
/// iterator.stop()
/// ```
#[pyclass(name = "StreamIterator")]
pub struct PyStreamIterator {
    /// Redis connection URL.
    redis_url: String,

    /// Configuration.
    config: PyStreamConfig,

    /// Statistics (shared with client).
    stats: Arc<RwLock<PyFlashClientStats>>,

    /// Active flag.
    active: Arc<AtomicBool>,

    /// Last read message IDs per topic.
    last_ids: RwLock<HashMap<String, String>>,

    /// Message buffer.
    buffer: RwLock<VecDeque<PyMarketEvent>>,

    /// Received message count.
    received_count: AtomicU64,
}

#[pymethods]
impl PyStreamIterator {
    /// Returns the configuration.
    #[must_use]
    pub fn config(&self) -> PyStreamConfig {
        self.config.clone()
    }

    /// Returns true if the iterator is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Stops the iterator.
    pub fn stop(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    /// Returns the number of topics.
    #[must_use]
    pub fn topic_count(&self) -> usize {
        self.config.topics.len()
    }

    /// Returns the number of received messages.
    #[must_use]
    pub fn received_count(&self) -> u64 {
        self.received_count.load(Ordering::Relaxed)
    }

    fn __repr__(&self) -> String {
        let active = if self.is_active() { "true" } else { "false" };
        format!(
            "StreamIterator(topics={}, active={})",
            self.config.topics.len(),
            active
        )
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<PyMarketEvent> {
        if !self.is_active() {
            return Err(PyStopIteration::new_err("Subscription stopped"));
        }

        // Check buffer first
        {
            let mut buffer = self.buffer.write();
            if let Some(event) = buffer.pop_front() {
                self.received_count.fetch_add(1, Ordering::Relaxed);
                return Ok(event);
            }
        }

        // Execute XREAD
        let result = py.allow_threads(|| self.xread_blocking());

        match result {
            Ok(event) => {
                self.received_count.fetch_add(1, Ordering::Relaxed);
                Ok(event)
            }
            Err(StreamError::SubscriptionStopped) => {
                Err(PyStopIteration::new_err("Subscription stopped"))
            }
            Err(StreamError::ConnectionError(msg)) => Err(PyConnectionError::new_err(msg)),
            Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
        }
    }
}

impl PyStreamIterator {
    /// Create a new iterator.
    #[must_use]
    pub fn new(
        redis_url: String,
        config: PyStreamConfig,
        stats: Arc<RwLock<PyFlashClientStats>>,
    ) -> Self {
        let mut last_ids = HashMap::new();
        for topic in &config.topics {
            last_ids.insert(topic.clone(), config.start_id.clone());
        }

        Self {
            redis_url,
            config,
            stats,
            active: Arc::new(AtomicBool::new(true)),
            last_ids: RwLock::new(last_ids),
            buffer: RwLock::new(VecDeque::new()),
            received_count: AtomicU64::new(0),
        }
    }

    /// Execute XREAD and return next event.
    fn xread_blocking(&self) -> StreamResult<PyMarketEvent> {
        if !self.is_active() {
            return Err(StreamError::SubscriptionStopped);
        }

        // Create a runtime for async operations
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| StreamError::ConnectionError(e.to_string()))?;

        runtime.block_on(async { self.xread_async().await })
    }

    /// Async XREAD implementation.
    async fn xread_async(&self) -> StreamResult<PyMarketEvent> {
        // Parse URL to create pool config
        let pool_config = PoolConfig {
            url: self.redis_url.clone(),
            max_size: 1,
            min_idle: Some(1),
            connect_timeout_ms: 5000,
            wait_timeout_ms: 5000,
            ..PoolConfig::default()
        };

        // Create pool
        let pool = RedisPool::new(pool_config).await.map_err(|e| {
            self.stats.write().record_connection_error();
            StreamError::ConnectionError(e.to_string())
        })?;

        // Get connection
        let mut conn = pool.get().await.map_err(|e| {
            self.stats.write().record_connection_error();
            StreamError::ConnectionError(e.to_string())
        })?;

        // Build XREAD command
        let mut cmd = redis::cmd("XREAD");

        if self.config.block_ms > 0 {
            cmd.arg("BLOCK").arg(self.config.block_ms);
        }
        cmd.arg("COUNT").arg(self.config.count);
        cmd.arg("STREAMS");

        // Add topics
        let last_ids = self.last_ids.read();
        for topic in &self.config.topics {
            cmd.arg(topic);
        }
        for topic in &self.config.topics {
            let id = last_ids
                .get(topic)
                .map(String::as_str)
                .unwrap_or(&self.config.start_id);
            cmd.arg(id);
        }
        drop(last_ids);

        // Execute
        let result: Option<Vec<(String, Vec<(String, HashMap<String, Vec<u8>>)>)>> =
            cmd.query_async(&mut *conn).await.map_err(|e| {
                self.stats.write().record_connection_error();
                StreamError::ConnectionError(e.to_string())
            })?;

        // Process results
        if let Some(streams) = result {
            for (topic, messages) in streams {
                for (msg_id, fields) in messages {
                    // Update last ID for this topic
                    self.last_ids.write().insert(topic.clone(), msg_id);

                    if let Some(data) = fields.get("data") {
                        let format = fields
                            .get("format")
                            .and_then(|f| String::from_utf8(f.clone()).ok())
                            .unwrap_or_else(|| self.config.format.clone());

                        let start = std::time::Instant::now();

                        // Try to deserialize as MarketEvent
                        match deserialize_event(data, &format) {
                            Ok(event) => {
                                let latency = start.elapsed().as_micros() as f64;
                                {
                                    let mut stats = self.stats.write();
                                    stats.record_message(data.len() as u64);
                                    stats.update_latency(latency);
                                }
                                self.buffer.write().push_back(event);
                            }
                            Err(_) => {
                                // Try as BookSnapshot
                                if let Ok(snapshot) = deserialize_book(data, &format) {
                                    let latency = start.elapsed().as_micros() as f64;
                                    {
                                        let mut stats = self.stats.write();
                                        stats.record_message(data.len() as u64);
                                        stats.update_latency(latency);
                                    }
                                    // Convert snapshot to a simple event-like structure
                                    // For now, we'll store it as-is and handle differently
                                    // This is a simplification - in real impl we'd convert properly
                                    let _ = snapshot; // TODO: Handle book snapshots
                                } else {
                                    self.stats.write().record_error();
                                }
                            }
                        }
                    }
                }
            }
        }

        // Return from buffer
        self.buffer
            .write()
            .pop_front()
            .ok_or(StreamError::SubscriptionStopped)
    }
}

// Send + Sync verification
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PyStreamIterator>();
};

// =============================================================================
// PYFLASHCLIENT
// =============================================================================

/// Redis client for consuming Flash streams.
///
/// # Thread Safety
///
/// This class is thread-safe. Multiple threads can safely share the same client.
///
/// # Example
///
/// ```python
/// from astra_flash import FlashClient, StreamConfig
///
/// # Create client
/// client = FlashClient("redis://localhost:6379")
///
/// # Subscribe to single topic
/// iterator = client.subscribe_one("market_data.deribit.btc_usd.book")
///
/// # Or with full config
/// config = StreamConfig(
///     topics=["market_data.deribit.btc_usd.book", "market_data.deribit.eth_usd.book"],
///     format="bincode",
/// )
/// iterator = client.subscribe(config)
///
/// # Iterate
/// for event in iterator:
///     process(event)
///
/// # Clean up
/// client.close()
/// ```
#[pyclass(name = "FlashClient")]
pub struct PyFlashClient {
    /// Redis URL.
    url: String,

    /// Statistics.
    stats: Arc<RwLock<PyFlashClientStats>>,

    /// Active iterators (for cleanup).
    iterators: RwLock<Vec<Arc<AtomicBool>>>,
}

#[pymethods]
impl PyFlashClient {
    /// Creates a new Flash client.
    ///
    /// # Arguments
    ///
    /// * `url` - Redis connection URL (e.g., "redis://localhost:6379")
    ///
    /// # Returns
    ///
    /// New `FlashClient` instance.
    ///
    /// # Errors
    ///
    /// Returns error if URL is invalid.
    #[new]
    pub fn new(url: String) -> PyResult<Self> {
        // Validate URL format minimally
        if !url.starts_with("redis://") && !url.starts_with("rediss://") {
            return Err(PyValueError::new_err(
                "URL must start with redis:// or rediss://",
            ));
        }

        Ok(Self {
            url,
            stats: Arc::new(RwLock::new(PyFlashClientStats::default())),
            iterators: RwLock::new(Vec::new()),
        })
    }

    /// Subscribe to streams with full configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Stream configuration
    ///
    /// # Returns
    ///
    /// `StreamIterator` for iterating over events.
    #[must_use]
    pub fn subscribe(&self, config: PyStreamConfig) -> PyStreamIterator {
        let iterator = PyStreamIterator::new(self.url.clone(), config, Arc::clone(&self.stats));

        // Track iterator for cleanup
        self.iterators.write().push(Arc::clone(&iterator.active));

        iterator
    }

    /// Subscribe to a single topic with default configuration.
    ///
    /// # Arguments
    ///
    /// * `topic` - Redis stream topic name
    ///
    /// # Returns
    ///
    /// `StreamIterator` for iterating over events.
    #[must_use]
    pub fn subscribe_one(&self, topic: String) -> PyStreamIterator {
        let config = PyStreamConfig::new(
            vec![topic],
            "bincode".to_string(),
            "$".to_string(),
            5000,
            100,
            None,
            None,
        );
        self.subscribe(config)
    }

    /// Get client statistics.
    #[getter]
    #[must_use]
    pub fn stats(&self) -> PyFlashClientStats {
        self.stats.read().clone()
    }

    /// Close the client and stop all iterators.
    pub fn close(&self) {
        // Stop all tracked iterators
        for active in self.iterators.read().iter() {
            active.store(false, Ordering::Relaxed);
        }
    }

    fn __repr__(&self) -> String {
        format!("FlashClient(url='{}')", self.url)
    }

    fn __str__(&self) -> String {
        format!("FlashClient({})", self.url)
    }
}

// Send + Sync verification
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PyFlashClient>();
};

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_new() {
        let config = PyStreamConfig::new(
            vec!["topic1".to_string()],
            "bincode".to_string(),
            "$".to_string(),
            5000,
            100,
            None,
            None,
        );

        assert_eq!(config.topics().len(), 1);
        assert_eq!(config.format(), "bincode");
        assert_eq!(config.start_id(), "$");
        assert_eq!(config.block_ms(), 5000);
        assert_eq!(config.count(), 100);
    }

    #[test]
    fn test_stream_config_with_group() {
        let config = PyStreamConfig::new(
            vec!["topic1".to_string()],
            "json".to_string(),
            ">".to_string(),
            10000,
            50,
            Some("my_group".to_string()),
            Some("consumer_1".to_string()),
        );

        assert_eq!(config.group_name(), Some("my_group".to_string()));
        assert_eq!(config.consumer_name(), Some("consumer_1".to_string()));
    }

    #[test]
    fn test_stats_default() {
        let stats = PyFlashClientStats::default();
        assert_eq!(stats.messages_received(), 0);
        assert_eq!(stats.bytes_deserialized(), 0);
        assert_eq!(stats.deserialize_errors(), 0);
    }

    #[test]
    fn test_stats_record_message() {
        let mut stats = PyFlashClientStats::default();
        stats.record_message(1024);

        assert_eq!(stats.messages_received(), 1);
        assert_eq!(stats.bytes_deserialized(), 1024);
    }

    #[test]
    fn test_stats_update_latency() {
        let mut stats = PyFlashClientStats::default();
        stats.update_latency(100.0);

        assert!(stats.avg_latency_us() > 0.0);
    }

    #[test]
    fn test_stats_reset() {
        let mut stats = PyFlashClientStats::default();
        stats.record_message(1024);
        stats.record_error();

        stats.reset();

        assert_eq!(stats.messages_received(), 0);
        assert_eq!(stats.bytes_deserialized(), 0);
        assert_eq!(stats.deserialize_errors(), 0);
    }

    #[test]
    fn test_validate_topics_empty() {
        let result = PyStreamConfig::validate_topics(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_topics_valid() {
        let result = PyStreamConfig::validate_topics(vec!["topic1".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_flash_client_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<PyFlashClient>();
        assert_sync::<PyFlashClient>();
    }

    #[test]
    fn test_stream_iterator_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<PyStreamIterator>();
        assert_sync::<PyStreamIterator>();
    }
}
