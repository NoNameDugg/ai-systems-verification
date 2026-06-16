//! Test consumer for E2E testing.
//!
//! Provides utilities for consuming and verifying messages from Redis streams.

use astra_flash::prelude::*;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::LatencyStats;

// =============================================================================
// RECEIVED MESSAGE
// =============================================================================

/// A message received from Redis stream.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    /// Redis message ID (e.g., "1234567890-0")
    pub id: String,
    /// Stream key the message was received from
    pub stream: String,
    /// When the message was received (local timestamp)
    pub received_at: Timestamp,
    /// The deserialized snapshot (if successful)
    pub snapshot: Option<BookSnapshot>,
    /// Raw data (if deserialization failed)
    pub raw_data: Option<Vec<u8>>,
    /// Format of the data
    pub format: String,
    /// End-to-end latency in microseconds
    pub e2e_latency_us: i64,
    /// Sequence number (if present)
    pub sequence: Option<u64>,
}

impl ReceivedMessage {
    /// Create a new received message from a snapshot.
    pub fn from_snapshot(id: String, stream: String, snapshot: BookSnapshot, format: &str) -> Self {
        let received_at = now_micros();
        let e2e_latency_us = received_at - snapshot.timestamp;

        Self {
            id,
            stream,
            received_at,
            snapshot: Some(snapshot),
            raw_data: None,
            format: format.to_string(),
            e2e_latency_us,
            sequence: None,
        }
    }

    /// Create a message from raw data (deserialization failed).
    pub fn from_raw(id: String, stream: String, data: Vec<u8>, format: &str) -> Self {
        Self {
            id,
            stream,
            received_at: now_micros(),
            snapshot: None,
            raw_data: Some(data),
            format: format.to_string(),
            e2e_latency_us: 0,
            sequence: None,
        }
    }

    /// Check if this message contains a valid snapshot.
    pub fn is_valid(&self) -> bool {
        self.snapshot.is_some()
    }

    /// Get the snapshot if present.
    pub fn snapshot(&self) -> Option<&BookSnapshot> {
        self.snapshot.as_ref()
    }
}

// =============================================================================
// TEST CONSUMER
// =============================================================================

/// Redis stream consumer for test verification.
///
/// Collects messages from Redis streams and provides verification utilities.
///
/// # Example
///
/// ```rust,ignore
/// let consumer = TestConsumer::new("market_data.deribit.btc_usd.book");
/// consumer.receive(message).await;
///
/// // Wait for expected messages
/// let messages = consumer.wait_for_count(10, Duration::from_secs(5)).await;
/// assert_eq!(messages.len(), 10);
///
/// // Verify latency
/// let stats = consumer.latency_stats();
/// assert!(stats.p95_within_budget(50));
/// ```
pub struct TestConsumer {
    /// Stream key being consumed
    stream: String,
    /// Received messages
    messages: Arc<RwLock<VecDeque<ReceivedMessage>>>,
    /// Total messages received
    total_received: Arc<AtomicU64>,
    /// Expected message count
    expected_count: Option<usize>,
    /// Whether consumer is active
    active: Arc<AtomicBool>,
    /// Deserialization format
    format: String,
}

impl TestConsumer {
    /// Create a new test consumer for the given stream.
    pub fn new(stream: impl Into<String>) -> Self {
        Self {
            stream: stream.into(),
            messages: Arc::new(RwLock::new(VecDeque::new())),
            total_received: Arc::new(AtomicU64::new(0)),
            expected_count: None,
            active: Arc::new(AtomicBool::new(true)),
            format: "bincode".to_string(),
        }
    }

    /// Set the expected message count.
    pub fn expect_count(mut self, count: usize) -> Self {
        self.expected_count = Some(count);
        self
    }

    /// Set the deserialization format.
    pub fn with_format(mut self, format: impl Into<String>) -> Self {
        self.format = format.into();
        self
    }

    /// Get the stream key.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// Receive a message.
    pub async fn receive(&self, message: ReceivedMessage) {
        if self.active.load(Ordering::SeqCst) {
            self.messages.write().push_back(message);
            self.total_received.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Get all received messages.
    pub fn messages(&self) -> Vec<ReceivedMessage> {
        self.messages.read().iter().cloned().collect()
    }

    /// Get the number of received messages.
    pub fn count(&self) -> usize {
        self.messages.read().len()
    }

    /// Get total messages ever received.
    pub fn total_received(&self) -> u64 {
        self.total_received.load(Ordering::SeqCst)
    }

    /// Clear all messages.
    pub fn clear(&self) {
        self.messages.write().clear();
    }

    /// Wait for a specific number of messages with timeout.
    ///
    /// Returns the messages received, which may be less than expected if timeout occurs.
    pub async fn wait_for_count(&self, count: usize, timeout: Duration) -> Vec<ReceivedMessage> {
        let start = std::time::Instant::now();

        while self.count() < count && start.elapsed() < timeout {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        self.messages()
    }

    /// Wait until consumer has received expected count.
    pub async fn wait_for_expected(&self, timeout: Duration) -> Vec<ReceivedMessage> {
        let count = self.expected_count.unwrap_or(0);
        self.wait_for_count(count, timeout).await
    }

    /// Stop consuming messages.
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    /// Check if consumer is active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Verify that all messages were received in order.
    pub fn verify_ordering(&self) -> bool {
        let messages = self.messages.read();
        if messages.len() < 2 {
            return true;
        }

        let mut prev_timestamp = i64::MIN;
        for msg in messages.iter() {
            if let Some(snapshot) = &msg.snapshot {
                if snapshot.timestamp < prev_timestamp {
                    return false;
                }
                prev_timestamp = snapshot.timestamp;
            }
        }

        true
    }

    /// Verify that all sequences are contiguous (no gaps).
    pub fn verify_sequences(&self) -> (bool, Vec<u64>) {
        let messages = self.messages.read();
        let mut gaps = Vec::new();
        let mut prev_seq = None;

        for msg in messages.iter() {
            if let Some(seq) = msg.sequence {
                if let Some(prev) = prev_seq {
                    if seq != prev + 1 {
                        gaps.push(seq);
                    }
                }
                prev_seq = Some(seq);
            }
        }

        (gaps.is_empty(), gaps)
    }

    /// Calculate latency statistics.
    pub fn latency_stats(&self) -> LatencyStats {
        let messages = self.messages.read();
        let mut latencies: Vec<i64> = messages
            .iter()
            .filter(|m| m.is_valid())
            .map(|m| m.e2e_latency_us)
            .collect();

        LatencyStats::from_samples(&mut latencies)
    }

    /// Get only valid messages (with successful deserialization).
    pub fn valid_messages(&self) -> Vec<ReceivedMessage> {
        self.messages
            .read()
            .iter()
            .filter(|m| m.is_valid())
            .cloned()
            .collect()
    }

    /// Get only invalid messages (deserialization failed).
    pub fn invalid_messages(&self) -> Vec<ReceivedMessage> {
        self.messages
            .read()
            .iter()
            .filter(|m| !m.is_valid())
            .cloned()
            .collect()
    }

    /// Get message count by instrument.
    pub fn count_by_instrument(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();

        for msg in self.messages.read().iter() {
            if let Some(snapshot) = &msg.snapshot {
                let key = format!(
                    "{:?}/{}/{}",
                    snapshot.instrument.exchange,
                    snapshot.instrument.base,
                    snapshot.instrument.quote
                );
                *counts.entry(key).or_insert(0) += 1;
            }
        }

        counts
    }
}

// =============================================================================
// MULTI-STREAM CONSUMER
// =============================================================================

/// Consumes from multiple streams for multi-exchange testing.
pub struct MultiStreamConsumer {
    /// Consumers by stream name
    consumers: Vec<TestConsumer>,
}

impl MultiStreamConsumer {
    /// Create a new multi-stream consumer.
    pub fn new(streams: Vec<String>) -> Self {
        let consumers = streams.into_iter().map(TestConsumer::new).collect();
        Self { consumers }
    }

    /// Get a consumer by stream name.
    pub fn get(&self, stream: &str) -> Option<&TestConsumer> {
        self.consumers.iter().find(|c| c.stream() == stream)
    }

    /// Get all consumers.
    pub fn consumers(&self) -> &[TestConsumer] {
        &self.consumers
    }

    /// Get total messages across all streams.
    pub fn total_count(&self) -> usize {
        self.consumers.iter().map(|c| c.count()).sum()
    }

    /// Wait for total count across all streams.
    pub async fn wait_for_total(&self, total: usize, timeout: Duration) -> usize {
        let start = std::time::Instant::now();

        while self.total_count() < total && start.elapsed() < timeout {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        self.total_count()
    }

    /// Get combined latency statistics.
    pub fn combined_latency_stats(&self) -> LatencyStats {
        let mut all_latencies: Vec<i64> = Vec::new();

        for consumer in &self.consumers {
            for msg in consumer.messages().iter().filter(|m| m.is_valid()) {
                all_latencies.push(msg.e2e_latency_us);
            }
        }

        LatencyStats::from_samples(&mut all_latencies)
    }

    /// Stop all consumers.
    pub fn stop_all(&self) {
        for consumer in &self.consumers {
            consumer.stop();
        }
    }
}

// =============================================================================
// MOCK REDIS CONSUMER
// =============================================================================

/// In-memory mock for testing without actual Redis.
///
/// Simulates Redis stream consumption for unit tests.
pub struct MockRedisConsumer {
    /// Internal consumer
    consumer: TestConsumer,
    /// Message generator ID
    next_id: AtomicU64,
}

impl MockRedisConsumer {
    /// Create a new mock Redis consumer.
    pub fn new(stream: impl Into<String>) -> Self {
        Self {
            consumer: TestConsumer::new(stream),
            next_id: AtomicU64::new(0),
        }
    }

    /// Generate a Redis-style message ID.
    fn generate_id(&self) -> String {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        format!("{}-{}", now_micros() / 1000, id)
    }

    /// Simulate receiving a book snapshot.
    pub async fn receive_snapshot(&self, snapshot: BookSnapshot) {
        let id = self.generate_id();
        let stream = self.consumer.stream().to_string();
        let message = ReceivedMessage::from_snapshot(id, stream, snapshot, "bincode");
        self.consumer.receive(message).await;
    }

    /// Get the underlying consumer.
    pub fn consumer(&self) -> &TestConsumer {
        &self.consumer
    }

    /// Get all messages.
    pub fn messages(&self) -> Vec<ReceivedMessage> {
        self.consumer.messages()
    }

    /// Get latency stats.
    pub fn latency_stats(&self) -> LatencyStats {
        self.consumer.latency_stats()
    }

    /// Clear all messages.
    pub fn clear(&self) {
        self.consumer.clear();
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_snapshot() -> BookSnapshot {
        BookSnapshot {
            instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
            timestamp: now_micros(),
            bids: vec![PriceLevel {
                price: 50000.0,
                quantity: dec!(1),
                order_count: None,
                timestamp: now_micros(),
            }],
            asks: vec![PriceLevel {
                price: 50010.0,
                quantity: dec!(1),
                order_count: None,
                timestamp: now_micros(),
            }],
        }
    }

    #[tokio::test]
    async fn test_consumer_receive() {
        let consumer = TestConsumer::new("test_stream");

        let msg = ReceivedMessage::from_snapshot(
            "1-0".to_string(),
            "test_stream".to_string(),
            test_snapshot(),
            "bincode",
        );

        consumer.receive(msg).await;
        assert_eq!(consumer.count(), 1);
        assert_eq!(consumer.total_received(), 1);
    }

    #[tokio::test]
    async fn test_consumer_wait_for_count() {
        let consumer = Arc::new(TestConsumer::new("test_stream"));
        let consumer_clone = consumer.clone();

        // Spawn task to add messages
        tokio::spawn(async move {
            for i in 0..5 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let msg = ReceivedMessage::from_snapshot(
                    format!("{}-0", i),
                    "test_stream".to_string(),
                    test_snapshot(),
                    "bincode",
                );
                consumer_clone.receive(msg).await;
            }
        });

        let messages = consumer.wait_for_count(5, Duration::from_secs(1)).await;
        assert_eq!(messages.len(), 5);
    }

    #[tokio::test]
    async fn test_consumer_verify_ordering() {
        let consumer = TestConsumer::new("test_stream");

        // Add messages with increasing timestamps
        for i in 0..5 {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let msg = ReceivedMessage::from_snapshot(
                format!("{}-0", i),
                "test_stream".to_string(),
                test_snapshot(),
                "bincode",
            );
            consumer.receive(msg).await;
        }

        assert!(consumer.verify_ordering());
    }

    #[tokio::test]
    async fn test_latency_stats() {
        let consumer = TestConsumer::new("test_stream");

        // Add messages with known latencies
        for i in 0..100 {
            let mut snapshot = test_snapshot();
            snapshot.timestamp = now_micros() - (i as i64 * 10); // Varying latency

            let msg = ReceivedMessage::from_snapshot(
                format!("{}-0", i),
                "test_stream".to_string(),
                snapshot,
                "bincode",
            );
            consumer.receive(msg).await;
        }

        let stats = consumer.latency_stats();
        assert_eq!(stats.count, 100);
        assert!(stats.min_us >= 0);
        assert!(stats.max_us >= stats.min_us);
    }

    #[tokio::test]
    async fn test_mock_redis_consumer() {
        let mock = MockRedisConsumer::new("test_stream");

        mock.receive_snapshot(test_snapshot()).await;
        mock.receive_snapshot(test_snapshot()).await;

        assert_eq!(mock.messages().len(), 2);
    }

    #[tokio::test]
    async fn test_multi_stream_consumer() {
        let streams = vec![
            "stream1".to_string(),
            "stream2".to_string(),
            "stream3".to_string(),
        ];

        let multi = MultiStreamConsumer::new(streams);

        assert_eq!(multi.consumers().len(), 3);
        assert!(multi.get("stream1").is_some());
        assert!(multi.get("stream2").is_some());
        assert!(multi.get("unknown").is_none());
    }

    #[test]
    fn test_received_message_from_snapshot() {
        let snapshot = test_snapshot();
        let msg =
            ReceivedMessage::from_snapshot("1-0".to_string(), "test".to_string(), snapshot, "json");

        assert!(msg.is_valid());
        assert!(msg.snapshot().is_some());
        assert_eq!(msg.format, "json");
    }

    #[test]
    fn test_received_message_from_raw() {
        let msg = ReceivedMessage::from_raw(
            "1-0".to_string(),
            "test".to_string(),
            vec![1, 2, 3],
            "unknown",
        );

        assert!(!msg.is_valid());
        assert!(msg.raw_data.is_some());
    }
}
