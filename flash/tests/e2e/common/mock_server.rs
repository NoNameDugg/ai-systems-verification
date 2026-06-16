//! Mock WebSocket server for E2E testing.
//!
//! Simulates exchange WebSocket behavior for testing the full pipeline.

use astra_flash::prelude::*;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// MOCK MESSAGE
// =============================================================================

/// A message to be sent by the mock server.
#[derive(Debug, Clone)]
pub struct MockMessage {
    /// The message data (typically JSON)
    pub data: String,
    /// Delay before sending this message
    pub delay: Duration,
    /// Whether to drop the connection after sending
    pub drop_after: bool,
    /// Whether this is a binary message
    pub is_binary: bool,
}

impl MockMessage {
    /// Create a new text message.
    pub fn text(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            delay: Duration::ZERO,
            drop_after: false,
            is_binary: false,
        }
    }

    /// Create a message with delay.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Create a message that drops connection after sending.
    pub fn and_disconnect(mut self) -> Self {
        self.drop_after = true;
        self
    }

    /// Create a binary message.
    pub fn binary(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            delay: Duration::ZERO,
            drop_after: false,
            is_binary: true,
        }
    }
}

// =============================================================================
// CHAOS CONFIGURATION
// =============================================================================

/// Configuration for chaos testing features.
#[derive(Debug, Clone)]
pub struct ChaosConfig {
    /// Probability of random disconnect (0.0 - 1.0)
    pub disconnect_probability: f64,
    /// Probability of sending malformed message (0.0 - 1.0)
    pub malformed_probability: f64,
    /// Random latency range to add
    pub latency_range: (Duration, Duration),
    /// Whether to inject sequence gaps
    pub inject_sequence_gaps: bool,
    /// Gap probability when gaps are enabled
    pub gap_probability: f64,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            disconnect_probability: 0.0,
            malformed_probability: 0.0,
            latency_range: (Duration::ZERO, Duration::ZERO),
            inject_sequence_gaps: false,
            gap_probability: 0.0,
        }
    }
}

impl ChaosConfig {
    /// Create a config with moderate chaos for testing.
    pub fn moderate() -> Self {
        Self {
            disconnect_probability: 0.01,
            malformed_probability: 0.001,
            latency_range: (Duration::from_millis(1), Duration::from_millis(10)),
            inject_sequence_gaps: true,
            gap_probability: 0.01,
        }
    }

    /// Create a config with high chaos for stress testing.
    pub fn aggressive() -> Self {
        Self {
            disconnect_probability: 0.05,
            malformed_probability: 0.01,
            latency_range: (Duration::from_millis(10), Duration::from_millis(100)),
            inject_sequence_gaps: true,
            gap_probability: 0.05,
        }
    }
}

// =============================================================================
// MOCK EXCHANGE SERVER
// =============================================================================

/// Mock WebSocket server simulating an exchange.
///
/// # Features
///
/// - Queue messages to be sent to clients
/// - Track connection count
/// - Inject chaos (disconnects, latency, malformed messages)
/// - Simulate multiple exchanges (Deribit, Binance, OANDA)
///
/// # Example
///
/// ```rust,ignore
/// let server = MockExchangeServer::start(Exchange::Deribit).await;
/// server.queue_message(MockMessage::text(r#"{"type": "snapshot"}"#)).await;
///
/// // Connect to server.url() and receive the queued message
/// ```
pub struct MockExchangeServer {
    /// Exchange this server simulates
    exchange: Exchange,
    /// Server address (set after start)
    addr: Option<SocketAddr>,
    /// Message queue
    messages: Arc<RwLock<VecDeque<MockMessage>>>,
    /// Active connection count
    connection_count: Arc<AtomicUsize>,
    /// Total connections ever made
    total_connections: Arc<AtomicUsize>,
    /// Chaos configuration
    chaos_config: ChaosConfig,
    /// Whether server is running
    running: Arc<AtomicBool>,
    /// Messages sent count
    messages_sent: Arc<AtomicUsize>,
}

impl MockExchangeServer {
    /// Create a new mock server for the given exchange.
    pub fn new(exchange: Exchange) -> Self {
        Self {
            exchange,
            addr: None,
            messages: Arc::new(RwLock::new(VecDeque::new())),
            connection_count: Arc::new(AtomicUsize::new(0)),
            total_connections: Arc::new(AtomicUsize::new(0)),
            chaos_config: ChaosConfig::default(),
            running: Arc::new(AtomicBool::new(false)),
            messages_sent: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a server with chaos configuration.
    pub fn with_chaos(mut self, config: ChaosConfig) -> Self {
        self.chaos_config = config;
        self
    }

    /// Start the mock server (placeholder - actual WebSocket server would be started here).
    ///
    /// Note: In a real implementation, this would start an actual WebSocket server
    /// using tokio-tungstenite or axum. For now, we provide a mock implementation
    /// that tracks state but doesn't actually listen on a socket.
    pub async fn start(exchange: Exchange) -> Self {
        let mut server = Self::new(exchange);
        server.running.store(true, Ordering::SeqCst);
        // In real implementation: spawn WebSocket server task
        server
    }

    /// Get the server URL.
    ///
    /// Note: Returns a placeholder URL. In real implementation, would return
    /// the actual bound address.
    pub fn url(&self) -> String {
        match &self.addr {
            Some(addr) => format!("ws://{}", addr),
            None => format!("ws://127.0.0.1:0/{:?}", self.exchange),
        }
    }

    /// Queue a message to be sent to connected clients.
    pub async fn queue_message(&self, message: MockMessage) {
        self.messages.write().push_back(message);
    }

    /// Queue multiple messages.
    pub async fn queue_messages(&self, messages: Vec<MockMessage>) {
        let mut queue = self.messages.write();
        for message in messages {
            queue.push_back(message);
        }
    }

    /// Get the next message from the queue.
    pub fn next_message(&self) -> Option<MockMessage> {
        self.messages.write().pop_front()
    }

    /// Get current connection count.
    pub fn connection_count(&self) -> usize {
        self.connection_count.load(Ordering::SeqCst)
    }

    /// Get total connections ever made.
    pub fn total_connections(&self) -> usize {
        self.total_connections.load(Ordering::SeqCst)
    }

    /// Get messages sent count.
    pub fn messages_sent(&self) -> usize {
        self.messages_sent.load(Ordering::SeqCst)
    }

    /// Get pending message count.
    pub fn pending_messages(&self) -> usize {
        self.messages.read().len()
    }

    /// Simulate a client connecting.
    pub fn simulate_connect(&self) {
        self.connection_count.fetch_add(1, Ordering::SeqCst);
        self.total_connections.fetch_add(1, Ordering::SeqCst);
    }

    /// Simulate a client disconnecting.
    pub fn simulate_disconnect(&self) {
        let count = self.connection_count.load(Ordering::SeqCst);
        if count > 0 {
            self.connection_count.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Simulate sending a message.
    pub fn simulate_send(&self) -> Option<MockMessage> {
        let message = self.next_message();
        if message.is_some() {
            self.messages_sent.fetch_add(1, Ordering::SeqCst);
        }
        message
    }

    /// Force disconnect all clients.
    pub fn force_disconnect_all(&self) {
        self.connection_count.store(0, Ordering::SeqCst);
    }

    /// Check if server is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stop the server.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get the exchange type.
    pub fn exchange(&self) -> Exchange {
        self.exchange
    }

    /// Get the chaos configuration.
    pub fn chaos_config(&self) -> &ChaosConfig {
        &self.chaos_config
    }

    /// Clear all queued messages.
    pub fn clear_queue(&self) {
        self.messages.write().clear();
    }
}

// =============================================================================
// MOCK SERVER BUILDER
// =============================================================================

/// Builder for MockExchangeServer.
pub struct MockServerBuilder {
    exchange: Exchange,
    chaos_config: ChaosConfig,
    initial_messages: Vec<MockMessage>,
}

impl MockServerBuilder {
    /// Create a new builder for the given exchange.
    pub fn new(exchange: Exchange) -> Self {
        Self {
            exchange,
            chaos_config: ChaosConfig::default(),
            initial_messages: Vec::new(),
        }
    }

    /// Set chaos configuration.
    pub fn chaos(mut self, config: ChaosConfig) -> Self {
        self.chaos_config = config;
        self
    }

    /// Add initial messages to queue.
    pub fn with_messages(mut self, messages: Vec<MockMessage>) -> Self {
        self.initial_messages = messages;
        self
    }

    /// Build and start the server.
    pub async fn start(self) -> MockExchangeServer {
        let server = MockExchangeServer::new(self.exchange).with_chaos(self.chaos_config);

        for message in self.initial_messages {
            server.queue_message(message).await;
        }

        server.running.store(true, Ordering::SeqCst);
        server
    }
}

// =============================================================================
// MULTI-EXCHANGE MOCK
// =============================================================================

/// Manages multiple mock exchange servers for multi-exchange testing.
pub struct MultiExchangeMock {
    /// Servers by exchange
    servers: Vec<MockExchangeServer>,
}

impl MultiExchangeMock {
    /// Create mocks for all supported exchanges.
    pub async fn all_exchanges() -> Self {
        let servers = vec![
            MockExchangeServer::start(Exchange::Deribit).await,
            MockExchangeServer::start(Exchange::Binance).await,
            MockExchangeServer::start(Exchange::Oanda).await,
        ];

        Self { servers }
    }

    /// Get a server by exchange.
    pub fn get(&self, exchange: Exchange) -> Option<&MockExchangeServer> {
        self.servers.iter().find(|s| s.exchange() == exchange)
    }

    /// Get all servers.
    pub fn servers(&self) -> &[MockExchangeServer] {
        &self.servers
    }

    /// Get total connection count across all exchanges.
    pub fn total_connections(&self) -> usize {
        self.servers.iter().map(|s| s.connection_count()).sum()
    }

    /// Stop all servers.
    pub fn stop_all(&self) {
        for server in &self.servers {
            server.stop();
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_creation() {
        let server = MockExchangeServer::start(Exchange::Deribit).await;
        assert!(server.is_running());
        assert_eq!(server.exchange(), Exchange::Deribit);
        assert_eq!(server.connection_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_server_message_queue() {
        let server = MockExchangeServer::start(Exchange::Binance).await;

        server.queue_message(MockMessage::text("message 1")).await;
        server.queue_message(MockMessage::text("message 2")).await;

        assert_eq!(server.pending_messages(), 2);

        let msg1 = server.next_message().unwrap();
        assert_eq!(msg1.data, "message 1");

        let msg2 = server.next_message().unwrap();
        assert_eq!(msg2.data, "message 2");

        assert!(server.next_message().is_none());
    }

    #[tokio::test]
    async fn test_mock_server_connection_tracking() {
        let server = MockExchangeServer::start(Exchange::Oanda).await;

        server.simulate_connect();
        server.simulate_connect();
        assert_eq!(server.connection_count(), 2);
        assert_eq!(server.total_connections(), 2);

        server.simulate_disconnect();
        assert_eq!(server.connection_count(), 1);
        assert_eq!(server.total_connections(), 2);

        server.force_disconnect_all();
        assert_eq!(server.connection_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_message_builder() {
        let msg = MockMessage::text("test")
            .with_delay(Duration::from_millis(100))
            .and_disconnect();

        assert_eq!(msg.data, "test");
        assert_eq!(msg.delay, Duration::from_millis(100));
        assert!(msg.drop_after);
        assert!(!msg.is_binary);
    }

    #[tokio::test]
    async fn test_mock_server_builder() {
        let server = MockServerBuilder::new(Exchange::Deribit)
            .chaos(ChaosConfig::moderate())
            .with_messages(vec![MockMessage::text("initial")])
            .start()
            .await;

        assert!(server.is_running());
        assert_eq!(server.pending_messages(), 1);
        assert!(server.chaos_config().inject_sequence_gaps);
    }

    #[tokio::test]
    async fn test_multi_exchange_mock() {
        let mock = MultiExchangeMock::all_exchanges().await;

        assert_eq!(mock.servers().len(), 3);
        assert!(mock.get(Exchange::Deribit).is_some());
        assert!(mock.get(Exchange::Binance).is_some());
        assert!(mock.get(Exchange::Oanda).is_some());

        mock.get(Exchange::Deribit).unwrap().simulate_connect();
        mock.get(Exchange::Binance).unwrap().simulate_connect();

        assert_eq!(mock.total_connections(), 2);

        mock.stop_all();
        assert!(!mock.get(Exchange::Deribit).unwrap().is_running());
    }

    #[test]
    fn test_chaos_config_defaults() {
        let config = ChaosConfig::default();
        assert_eq!(config.disconnect_probability, 0.0);
        assert_eq!(config.malformed_probability, 0.0);
        assert!(!config.inject_sequence_gaps);
    }

    #[test]
    fn test_chaos_config_moderate() {
        let config = ChaosConfig::moderate();
        assert!(config.disconnect_probability > 0.0);
        assert!(config.inject_sequence_gaps);
    }
}
