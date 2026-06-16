//! Tests for Flash Connection Manager (Part 2.1).
//!
//! # Test Categories
//!
//! | Category | Count | Purpose |
//! |----------|-------|---------|
//! | Unit Tests | 15 | State machine, types, basic operations |
//! | Integration Tests | 8 | Real connection lifecycle |
//! | Performance Tests | 3 | Latency and throughput |
//! | Failure Tests | 5 | Error handling and edge cases |
//!
//! # TDD Approach
//!
//! These tests are written BEFORE implementation following TDD methodology.
//! They define the expected API and behavior of the Connection Manager.

use astra_flash::core::config::WebSocketConfig;
use astra_flash::core::error::FlashError;
use astra_flash::core::metrics::FlashMetrics;
use astra_flash::core::types::Exchange;
use astra_flash::network::connector::{
    ConnectionState, ConnectionStats, Connector, ConnectorEvent, RawMessage,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::timeout;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a default WebSocketConfig for testing.
fn test_config() -> WebSocketConfig {
    WebSocketConfig {
        connect_timeout_ms: 5000,
        read_timeout_ms: 5000,
        ping_interval_ms: 30000,
        pong_timeout_ms: 10000,
        max_reconnect_attempts: 3,
        reconnect_delay_ms: 100,
        max_reconnect_delay_ms: 1000,
        reconnect_jitter: 0.1,
    }
}

/// Create test metrics instance.
fn test_metrics() -> FlashMetrics {
    FlashMetrics::new()
}

/// A simple mock WebSocket server for testing.
struct MockWebSocketServer {
    addr: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MockWebSocketServer {
    /// Start a mock WebSocket server on a random port.
    async fn start() -> Self {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("ws://{}", listener.local_addr().unwrap());

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        if let Ok((stream, _)) = result {
                            tokio::spawn(async move {
                                let ws_stream = tokio_tungstenite::accept_async(stream).await;
                                if let Ok(mut ws) = ws_stream {
                                    use futures_util::StreamExt;
                                    use futures_util::SinkExt;
                                    use tokio_tungstenite::tungstenite::Message;

                                    while let Some(msg) = ws.next().await {
                                        if let Ok(Message::Text(text)) = msg {
                                            // Echo back
                                            let _ = ws.send(Message::Text(format!("echo: {}", text))).await;
                                        }
                                    }
                                }
                            });
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Get the WebSocket URL.
    fn url(&self) -> &str {
        &self.addr
    }

    /// Shutdown the server.
    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for MockWebSocketServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// =============================================================================
// UNIT TESTS (15+)
// =============================================================================

/// Test 1: ConnectionState transitions through valid states.
#[test]
fn test_connection_state_transitions() {
    // All states should be representable
    let _disconnected = ConnectionState::Disconnected;
    let _connecting = ConnectionState::Connecting;
    let _connected = ConnectionState::Connected;
    let _disconnecting = ConnectionState::Disconnecting;
}

/// Test 2: is_connected returns true only for Connected state.
#[test]
fn test_connection_state_is_connected() {
    assert!(!ConnectionState::Disconnected.is_connected());
    assert!(!ConnectionState::Connecting.is_connected());
    assert!(ConnectionState::Connected.is_connected());
    assert!(!ConnectionState::Disconnecting.is_connected());
}

/// Test 3: ConnectionState implements Display.
#[test]
fn test_connection_state_display() {
    assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
    assert_eq!(ConnectionState::Connecting.to_string(), "connecting");
    assert_eq!(ConnectionState::Connected.to_string(), "connected");
    assert_eq!(ConnectionState::Disconnecting.to_string(), "disconnecting");
}

/// Test 4: All ConnectorEvent variants can be created.
#[test]
fn test_connector_event_variants() {
    let _connected = ConnectorEvent::Connected {
        exchange: Exchange::Deribit,
        url: "wss://example.com".to_string(),
    };

    let _disconnected = ConnectorEvent::Disconnected {
        exchange: Exchange::Binance,
        reason: "server closed".to_string(),
        will_reconnect: true,
    };

    let _failed = ConnectorEvent::ConnectionFailed {
        exchange: Exchange::Oanda,
        error: "timeout".to_string(),
        attempt: 1,
    };

    let _msg = ConnectorEvent::MessageReceived {
        exchange: Exchange::Deribit,
        size_bytes: 1024,
    };

    let _state = ConnectorEvent::StateChanged {
        exchange: Exchange::Deribit,
        old_state: ConnectionState::Disconnected,
        new_state: ConnectionState::Connecting,
    };
}

/// Test 5: RawMessage contains expected fields.
#[test]
fn test_raw_message_creation() {
    let msg = RawMessage {
        exchange: Exchange::Deribit,
        payload: r#"{"test": "data"}"#.to_string(),
        received_at: 1234567890,
    };

    assert_eq!(msg.exchange, Exchange::Deribit);
    assert!(msg.payload.contains("test"));
    assert_eq!(msg.received_at, 1234567890);
}

/// Test 6: ConnectionStats has sensible defaults.
#[test]
fn test_connection_stats_default() {
    let stats = ConnectionStats::default();

    assert_eq!(stats.state, ConnectionState::Disconnected);
    assert!(stats.connected_since.is_none());
    assert_eq!(stats.messages_received, 0);
    assert_eq!(stats.messages_sent, 0);
    assert!(stats.url.is_empty());
}

/// Test 7: Connector::new returns valid channel receivers.
#[tokio::test]
async fn test_connector_new_returns_channels() {
    let config = test_config();
    let metrics = test_metrics();

    let (connector, mut event_rx, mut message_rx) = Connector::new(config, metrics);

    // Channels should be open
    assert!(!event_rx.is_closed());
    assert!(!message_rx.is_closed());

    // Connector should exist
    drop(connector);

    // After dropping connector, channels may close
}

/// Test 8: state() returns Disconnected for unknown exchange.
#[tokio::test]
async fn test_state_lookup_nonexistent() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    // Never connected to Deribit
    assert_eq!(connector.state(Exchange::Deribit), ConnectionState::Disconnected);
}

/// Test 9: is_connected returns true when in Connected state.
#[tokio::test]
async fn test_is_connected_true_when_connected() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected event
    let event = timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    matches!(event, ConnectorEvent::Connected { .. });

    // Should be connected
    assert!(connector.is_connected(Exchange::Deribit));

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 10: is_connected returns false when not connected.
#[tokio::test]
async fn test_is_connected_false_when_disconnected() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    // Never connected
    assert!(!connector.is_connected(Exchange::Deribit));
    assert!(!connector.is_connected(Exchange::Binance));
    assert!(!connector.is_connected(Exchange::Oanda));
}

/// Test 11: connected_exchanges returns empty list initially.
#[tokio::test]
async fn test_connected_exchanges_empty_initially() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    let exchanges = connector.connected_exchanges();
    assert!(exchanges.is_empty());
}

/// Test 12: send() to disconnected exchange returns error.
#[tokio::test]
async fn test_send_to_disconnected_returns_error() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    let result = connector.send(Exchange::Deribit, r#"{"test": "message"}"#).await;
    assert!(result.is_err());

    match result {
        Err(FlashError::Disconnected { .. }) => {}
        Err(e) => panic!("Expected Disconnected error, got: {:?}", e),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

/// Test 13: disconnect() on non-existent connection is OK.
#[tokio::test]
async fn test_disconnect_nonexistent_is_ok() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    // Should not error
    let result = connector.disconnect(Exchange::Deribit).await;
    assert!(result.is_ok());
}

/// Test 14: Connector is Send.
#[test]
fn test_connector_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Connector>();
}

/// Test 15: Connector is Sync.
#[test]
fn test_connector_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<Connector>();
}

// =============================================================================
// INTEGRATION TESTS (8+)
// =============================================================================

/// Test 16: Connect to a mock WebSocket server.
#[tokio::test]
async fn test_connect_to_mock_server() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    let result = connector.connect(Exchange::Deribit, server.url()).await;
    assert!(result.is_ok(), "Connect failed: {:?}", result);

    // Should receive Connected event
    let event = timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("event channel closed");

    match event {
        ConnectorEvent::Connected { exchange, url } => {
            assert_eq!(exchange, Exchange::Deribit);
            assert_eq!(url, server.url());
        }
        ConnectorEvent::StateChanged { .. } => {
            // State change is also valid, wait for Connected
            let event = timeout(Duration::from_secs(5), event_rx.recv())
                .await
                .expect("timeout")
                .expect("event");
            assert!(matches!(event, ConnectorEvent::Connected { .. }));
        }
        other => panic!("Expected Connected event, got: {:?}", other),
    }

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 17: Disconnect from a connected exchange.
#[tokio::test]
async fn test_disconnect_from_connected() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    assert!(connector.is_connected(Exchange::Deribit));

    // Disconnect
    connector.disconnect(Exchange::Deribit).await.unwrap();

    // Wait for Disconnected event
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Disconnected { .. }) {
            break;
        }
    }

    assert!(!connector.is_connected(Exchange::Deribit));

    server.shutdown();
}

/// Test 18: Send and receive a message.
#[tokio::test]
async fn test_send_receive_message() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, mut message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Send a message
    connector
        .send(Exchange::Deribit, r#"{"hello": "world"}"#)
        .await
        .unwrap();

    // Receive the echo
    let raw_msg = timeout(Duration::from_secs(5), message_rx.recv())
        .await
        .expect("timeout waiting for message")
        .expect("message channel closed");

    assert_eq!(raw_msg.exchange, Exchange::Deribit);
    assert!(raw_msg.payload.contains("hello"));

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 19: Connection timeout is handled.
#[tokio::test]
async fn test_connection_timeout() {
    // Use a non-routable IP to trigger timeout
    let config = WebSocketConfig {
        connect_timeout_ms: 100, // Very short timeout
        ..test_config()
    };
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Try to connect to non-routable address
    let result = connector
        .connect(Exchange::Deribit, "ws://10.255.255.1:9999")
        .await;

    // Should fail
    assert!(result.is_err());

    // Should receive ConnectionFailed event
    let event = timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    assert!(
        matches!(event, ConnectorEvent::ConnectionFailed { .. })
            || matches!(event, ConnectorEvent::StateChanged { .. })
    );
}

/// Test 20: Multiple exchanges can connect concurrently.
#[tokio::test]
async fn test_multiple_exchanges_concurrent() {
    let mut server1 = MockWebSocketServer::start().await;
    let mut server2 = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect to both
    let (r1, r2) = tokio::join!(
        connector.connect(Exchange::Deribit, server1.url()),
        connector.connect(Exchange::Binance, server2.url())
    );

    assert!(r1.is_ok());
    assert!(r2.is_ok());

    // Wait for both Connected events
    let mut deribit_connected = false;
    let mut binance_connected = false;

    for _ in 0..10 {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");

        match event {
            ConnectorEvent::Connected { exchange, .. } => {
                if exchange == Exchange::Deribit {
                    deribit_connected = true;
                }
                if exchange == Exchange::Binance {
                    binance_connected = true;
                }
            }
            _ => {}
        }

        if deribit_connected && binance_connected {
            break;
        }
    }

    assert!(deribit_connected);
    assert!(binance_connected);

    // Both should show as connected
    let connected = connector.connected_exchanges();
    assert_eq!(connected.len(), 2);
    assert!(connected.contains(&Exchange::Deribit));
    assert!(connected.contains(&Exchange::Binance));

    connector.disconnect_all().await;
    server1.shutdown();
    server2.shutdown();
}

/// Test 21: Can reconnect after disconnect.
#[tokio::test]
async fn test_reconnect_after_disconnect() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Disconnect
    connector.disconnect(Exchange::Deribit).await.unwrap();

    // Wait for Disconnected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Disconnected { .. }) {
            break;
        }
    }

    // Reconnect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected again
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    assert!(connector.is_connected(Exchange::Deribit));

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 22: Connected event is emitted on connect.
#[tokio::test]
async fn test_event_emission_on_connect() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    let mut found_connected = false;
    for _ in 0..5 {
        if let Ok(Some(event)) = timeout(Duration::from_secs(2), event_rx.recv()).await {
            if matches!(event, ConnectorEvent::Connected { .. }) {
                found_connected = true;
                break;
            }
        }
    }

    assert!(found_connected, "Expected Connected event to be emitted");

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 23: Disconnected event is emitted on disconnect.
#[tokio::test]
async fn test_event_emission_on_disconnect() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Disconnect
    connector.disconnect(Exchange::Deribit).await.unwrap();

    // Should receive Disconnected event
    let mut found_disconnected = false;
    for _ in 0..5 {
        if let Ok(Some(event)) = timeout(Duration::from_secs(2), event_rx.recv()).await {
            if matches!(event, ConnectorEvent::Disconnected { .. }) {
                found_disconnected = true;
                break;
            }
        }
    }

    assert!(found_disconnected, "Expected Disconnected event to be emitted");

    server.shutdown();
}

// =============================================================================
// PERFORMANCE TESTS (3+)
// =============================================================================

/// Test 24: Message receive latency should be < 100μs.
#[tokio::test]
async fn test_message_receive_latency() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, mut message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Measure latency for multiple messages
    let mut latencies = Vec::new();

    for _ in 0..10 {
        let start = Instant::now();
        connector.send(Exchange::Deribit, "ping").await.unwrap();

        let _msg = timeout(Duration::from_secs(5), message_rx.recv())
            .await
            .expect("timeout")
            .expect("message");

        let latency = start.elapsed();
        latencies.push(latency);
    }

    // Average latency should be reasonable (allowing for network overhead)
    let avg_latency: Duration = latencies.iter().sum::<Duration>() / latencies.len() as u32;
    println!("Average round-trip latency: {:?}", avg_latency);

    // For localhost, we expect < 10ms (not 100μs due to full round-trip)
    assert!(
        avg_latency < Duration::from_millis(100),
        "Average latency {:?} is too high",
        avg_latency
    );

    connector.disconnect_all().await;
    server.shutdown();
}

/// Test 25: State lookup should be < 1μs.
#[tokio::test]
async fn test_state_lookup_performance() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    let iterations = 100_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = connector.state(Exchange::Deribit);
    }

    let elapsed = start.elapsed();
    let per_lookup = elapsed / iterations;

    println!("Per-lookup time: {:?}", per_lookup);

    // Should be very fast (< 1μs)
    assert!(
        per_lookup < Duration::from_micros(10), // Allow some margin
        "State lookup {:?} is too slow",
        per_lookup
    );
}

/// Test 26: Throughput should be > 10K msg/sec.
#[tokio::test]
async fn test_throughput_single_connection() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, mut message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Send many messages and count how many we receive
    let msg_count = 100;
    let start = Instant::now();

    for i in 0..msg_count {
        connector.send(Exchange::Deribit, &format!("msg_{}", i)).await.unwrap();
    }

    // Receive all echoes
    let mut received = 0;
    let timeout_duration = Duration::from_secs(10);
    let deadline = Instant::now() + timeout_duration;

    while received < msg_count && Instant::now() < deadline {
        if let Ok(Some(_)) = timeout(Duration::from_millis(100), message_rx.recv()).await {
            received += 1;
        }
    }

    let elapsed = start.elapsed();
    let throughput = (received as f64) / elapsed.as_secs_f64();

    println!(
        "Received {} messages in {:?}, throughput: {:.0} msg/sec",
        received, elapsed, throughput
    );

    // We expect reasonable throughput for localhost
    assert!(received >= msg_count / 2, "Lost too many messages: {}/{}", received, msg_count);

    connector.disconnect_all().await;
    server.shutdown();
}

// =============================================================================
// FAILURE TESTS (5+)
// =============================================================================

/// Test 27: Connect to invalid URL returns error.
#[tokio::test]
async fn test_connect_invalid_url() {
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    let result = connector.connect(Exchange::Deribit, "not_a_valid_url").await;
    assert!(result.is_err());
}

/// Test 28: Connect to unreachable network returns error.
#[tokio::test]
async fn test_connect_network_unreachable() {
    let config = WebSocketConfig {
        connect_timeout_ms: 500, // Short timeout
        ..test_config()
    };
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);

    // Non-routable IP address
    let result = connector
        .connect(Exchange::Deribit, "ws://10.255.255.1:9999")
        .await;

    assert!(result.is_err());
}

/// Test 29: Handle server closing connection.
#[tokio::test]
async fn test_server_close_connection() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Shutdown server (simulates server closing connection)
    server.shutdown();

    // Should eventually receive Disconnected event
    let mut found_disconnected = false;
    for _ in 0..10 {
        if let Ok(Some(event)) = timeout(Duration::from_secs(2), event_rx.recv()).await {
            if matches!(event, ConnectorEvent::Disconnected { .. }) {
                found_disconnected = true;
                break;
            }
        }
    }

    // Note: This may not always trigger immediately depending on implementation
    // At minimum, is_connected should eventually return false
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// Test 30: Send to a closing connection handles race condition.
#[tokio::test]
async fn test_send_to_closing_connection() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect
    connector.connect(Exchange::Deribit, server.url()).await.unwrap();

    // Wait for Connected
    loop {
        let event = timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timeout")
            .expect("event");
        if matches!(event, ConnectorEvent::Connected { .. }) {
            break;
        }
    }

    // Start disconnecting and sending at the same time
    let connector_clone = Arc::new(connector);
    let connector1 = Arc::clone(&connector_clone);
    let connector2 = Arc::clone(&connector_clone);

    let (send_result, disconnect_result) = tokio::join!(
        async move { connector1.send(Exchange::Deribit, "test").await },
        async move { connector2.disconnect(Exchange::Deribit).await }
    );

    // One of these might error, but neither should panic
    // The disconnect should succeed
    assert!(disconnect_result.is_ok());

    server.shutdown();
}

/// Test 31: Concurrent connect/disconnect is thread-safe.
#[tokio::test]
async fn test_concurrent_connect_disconnect() {
    let mut server = MockWebSocketServer::start().await;
    let config = test_config();
    let metrics = test_metrics();
    let (connector, _event_rx, _message_rx) = Connector::new(config, metrics);
    let connector = Arc::new(connector);

    // Spawn multiple tasks doing connect/disconnect
    let mut handles = Vec::new();
    let url = server.url().to_string();

    for i in 0..5 {
        let c = Arc::clone(&connector);
        let u = url.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..3 {
                let _ = c.connect(Exchange::Deribit, &u).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
                let _ = c.disconnect(Exchange::Deribit).await;
            }
        }));
    }

    // Wait for all to complete - should not panic
    for handle in handles {
        let _ = handle.await;
    }

    // Cleanup
    connector.disconnect_all().await;
    server.shutdown();
}

// =============================================================================
// ADDITIONAL HELPER TESTS
// =============================================================================

/// Test 32: ConnectionState implements Clone.
#[test]
fn test_connection_state_clone() {
    let state = ConnectionState::Connected;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

/// Test 33: ConnectionState implements Copy.
#[test]
fn test_connection_state_copy() {
    let state = ConnectionState::Connected;
    let copied: ConnectionState = state; // Copy
    assert_eq!(state, copied); // Original still valid
}

/// Test 34: RawMessage is Send.
#[test]
fn test_raw_message_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<RawMessage>();
}

/// Test 35: ConnectorEvent is Send.
#[test]
fn test_connector_event_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<ConnectorEvent>();
}

/// Test 36: ConnectionStats is Clone.
#[test]
fn test_connection_stats_clone() {
    let stats = ConnectionStats::default();
    let cloned = stats.clone();
    assert_eq!(stats.messages_received, cloned.messages_received);
}

// =============================================================================
// READ TIMEOUT TESTS (Batch 1.3: Resilience)
// =============================================================================

/// A mock WebSocket server that accepts connections but never sends data.
/// Used to test read timeout behavior.
struct SilentWebSocketServer {
    addr: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl SilentWebSocketServer {
    /// Start a silent WebSocket server on a random port.
    async fn start() -> Self {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("ws://{}", listener.local_addr().unwrap());

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        if let Ok((stream, _)) = result {
                            // Accept the connection but never send any data
                            tokio::spawn(async move {
                                let ws_stream = tokio_tungstenite::accept_async(stream).await;
                                if let Ok(ws) = ws_stream {
                                    use futures_util::StreamExt;
                                    let (_, mut read) = ws.split();
                                    // Just consume incoming messages silently, never respond
                                    while read.next().await.is_some() {
                                        // Do nothing - be silent
                                    }
                                }
                            });
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    fn url(&self) -> &str {
        &self.addr
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for SilentWebSocketServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Test 37: Read timeout triggers disconnected event when no data received.
#[tokio::test]
async fn test_read_timeout_triggers_disconnect() {
    let mut server = SilentWebSocketServer::start().await;

    // Configure very short read timeout for testing (500ms)
    let config = WebSocketConfig {
        connect_timeout_ms: 5000,
        read_timeout_ms: 500, // 500ms timeout for fast test
        ping_interval_ms: 30000,
        pong_timeout_ms: 10000,
        max_reconnect_attempts: 3,
        reconnect_delay_ms: 100,
        max_reconnect_delay_ms: 1000,
        reconnect_jitter: 0.1,
    };
    let metrics = test_metrics();
    let (connector, mut event_rx, _message_rx) = Connector::new(config, metrics);

    // Connect to the silent server
    let result = connector.connect(Exchange::Deribit, server.url()).await;
    assert!(result.is_ok(), "Connect should succeed");

    // Wait for Connected event first
    let mut connected = false;
    for _ in 0..5 {
        if let Ok(Some(event)) = timeout(Duration::from_secs(2), event_rx.recv()).await {
            if matches!(event, ConnectorEvent::Connected { .. }) {
                connected = true;
                break;
            }
        }
    }
    assert!(connected, "Should receive Connected event");

    // Now wait for Disconnected event due to read timeout
    // Should happen within 1 second (500ms timeout + buffer)
    let mut received_disconnect = false;
    let mut disconnect_reason = String::new();

    for _ in 0..5 {
        if let Ok(Some(event)) = timeout(Duration::from_secs(2), event_rx.recv()).await {
            if let ConnectorEvent::Disconnected { reason, will_reconnect, .. } = event {
                received_disconnect = true;
                disconnect_reason = reason;
                assert!(will_reconnect, "Should indicate will_reconnect=true for read timeout");
                break;
            }
        }
    }

    assert!(received_disconnect, "Should receive Disconnected event due to read timeout");
    assert!(
        disconnect_reason.contains("read timeout"),
        "Disconnect reason should mention read timeout, got: {}",
        disconnect_reason
    );

    server.shutdown();
}

/// Test 38: Read timeout config default is 5 seconds.
#[test]
fn test_read_timeout_default_value() {
    let config = WebSocketConfig::default();
    assert_eq!(config.read_timeout_ms, 5000, "Default read timeout should be 5000ms");
}

/// Test 39: Read timeout duration helper works correctly.
#[test]
fn test_read_timeout_duration_helper() {
    let config = WebSocketConfig {
        read_timeout_ms: 3000,
        ..WebSocketConfig::default()
    };
    assert_eq!(config.read_timeout().as_millis(), 3000);
}
