//! WebSocket Connection Manager for Flash.
//!
//! This module provides the [`Connector`] struct for managing WebSocket
//! connections to multiple exchanges concurrently.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Connector                                    │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐ │
//! │  │ ConnectionState │  │   EventSender   │  │  MessageReceiver    │ │
//! │  │   DashMap       │  │    (mpsc)       │  │     (mpsc)          │ │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::core::config::WebSocketConfig;
//! use astra_flash::core::metrics::FlashMetrics;
//! use astra_flash::core::types::Exchange;
//! use astra_flash::network::connector::Connector;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = WebSocketConfig::default();
//!     let metrics = FlashMetrics::new();
//!     let (connector, mut events, mut messages) = Connector::new(config, metrics);
//!
//!     // Connect to Deribit
//!     connector.connect(Exchange::Deribit, "wss://www.deribit.com/ws/api/v2").await.unwrap();
//!
//!     // Process events and messages
//!     tokio::select! {
//!         Some(event) = events.recv() => {
//!             println!("Event: {:?}", event);
//!         }
//!         Some(msg) = messages.recv() => {
//!             println!("Message from {:?}: {}", msg.exchange, msg.payload);
//!         }
//!     }
//! }
//! ```

use crate::core::config::WebSocketConfig;
use crate::core::error::FlashError;
use crate::core::metrics::{FlashMetrics, MessageType};
use crate::core::types::{Exchange, Timestamp};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};

// BlackBox tap integration (optional feature)
#[cfg(feature = "blackbox")]
use crate::blackbox::{Tap, TapExt};

// =============================================================================
// CHANNEL BUFFER SIZES
// =============================================================================

/// Buffer size for event channel.
const EVENT_CHANNEL_SIZE: usize = 1000;

/// Buffer size for message channel.
const MESSAGE_CHANNEL_SIZE: usize = 10_000;

/// Buffer size for outgoing message channel per connection.
const OUTGOING_CHANNEL_SIZE: usize = 1000;

// =============================================================================
// CONNECTION STATE
// =============================================================================

/// State of a WebSocket connection.
///
/// # State Machine
///
/// ```text
/// Disconnected ──connect()──> Connecting ──success──> Connected
///      ▲                           │                      │
///      │                       timeout/error          disconnect()
///      │                           │                      │
///      └───────────────────────────┴───────────────< Disconnecting
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConnectionState {
    /// Not connected to the exchange.
    #[default]
    Disconnected,
    /// Connection attempt in progress.
    Connecting,
    /// Successfully connected and ready for communication.
    Connected,
    /// Graceful disconnection in progress.
    Disconnecting,
}

impl ConnectionState {
    /// Returns true if the connection is in the Connected state.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected => write!(f, "connected"),
            Self::Disconnecting => write!(f, "disconnecting"),
        }
    }
}

// =============================================================================
// CONNECTOR EVENT
// =============================================================================

/// Events emitted by the connection manager.
///
/// Subscribe to these events to monitor connection lifecycle changes.
#[derive(Debug, Clone)]
pub enum ConnectorEvent {
    /// Successfully connected to an exchange.
    Connected {
        /// The exchange that was connected.
        exchange: Exchange,
        /// The WebSocket URL that was connected to.
        url: String,
    },

    /// Disconnected from an exchange.
    Disconnected {
        /// The exchange that was disconnected.
        exchange: Exchange,
        /// Reason for disconnection.
        reason: String,
        /// Whether automatic reconnection will be attempted.
        will_reconnect: bool,
    },

    /// Connection attempt failed.
    ConnectionFailed {
        /// The exchange that failed to connect.
        exchange: Exchange,
        /// Error description.
        error: String,
        /// Which attempt this was (1-based).
        attempt: u32,
    },

    /// Message received (for logging/debugging).
    MessageReceived {
        /// The exchange the message was received from.
        exchange: Exchange,
        /// Size of the message in bytes.
        size_bytes: usize,
    },

    /// Connection state changed.
    StateChanged {
        /// The exchange whose state changed.
        exchange: Exchange,
        /// Previous state.
        old_state: ConnectionState,
        /// New state.
        new_state: ConnectionState,
    },
}

// =============================================================================
// RAW MESSAGE
// =============================================================================

/// Raw message received from a WebSocket.
///
/// This is the unprocessed message content before parsing
/// by exchange adapters.
#[derive(Debug, Clone)]
pub struct RawMessage {
    /// Exchange this message came from.
    pub exchange: Exchange,
    /// Raw message content (typically JSON).
    pub payload: String,
    /// Local timestamp when the message was received (microseconds).
    pub received_at: Timestamp,
}

// =============================================================================
// CONNECTION STATS
// =============================================================================

/// Statistics for a WebSocket connection.
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    /// Current connection state.
    pub state: ConnectionState,
    /// When the connection was established (None if not connected).
    pub connected_since: Option<Instant>,
    /// Total messages received on this connection.
    pub messages_received: u64,
    /// Total messages sent on this connection.
    pub messages_sent: u64,
    /// Time of last activity (send or receive).
    pub last_activity: Instant,
    /// WebSocket URL.
    pub url: String,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            connected_since: None,
            messages_received: 0,
            messages_sent: 0,
            last_activity: Instant::now(),
            url: String::new(),
        }
    }
}

// =============================================================================
// CONNECTION HANDLE (INTERNAL)
// =============================================================================

/// Internal handle to a single WebSocket connection.
struct ConnectionHandle {
    /// Current state of this connection.
    state: ConnectionState,
    /// Sender for outgoing messages.
    outgoing_tx: mpsc::Sender<String>,
    /// Handle to the connection task.
    task_handle: JoinHandle<()>,
    /// URL this connection is connected to.
    url: String,
    /// Time when connected.
    connected_at: Instant,
    /// Last activity time.
    last_activity: Instant,
    /// Messages received counter.
    messages_received: Arc<AtomicU64>,
    /// Messages sent counter.
    messages_sent: Arc<AtomicU64>,
    /// Shutdown signal sender.
    shutdown_tx: mpsc::Sender<()>,
}

// =============================================================================
// TYPE ALIASES
// =============================================================================

/// Receiver for connection events.
pub type ConnectorEventReceiver = mpsc::Receiver<ConnectorEvent>;

/// Receiver for raw messages.
pub type RawMessageReceiver = mpsc::Receiver<RawMessage>;

// =============================================================================
// CONNECTOR
// =============================================================================

/// WebSocket connection manager for multiple exchanges.
///
/// Manages concurrent WebSocket connections to different exchanges,
/// handling connection lifecycle, message routing, and event emission.
///
/// # Thread Safety
///
/// `Connector` is both `Send` and `Sync`, making it safe to share
/// across async tasks and threads.
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::network::connector::Connector;
///
/// let (connector, events, messages) = Connector::new(config, metrics);
///
/// // Connect to an exchange
/// connector.connect(Exchange::Deribit, "wss://example.com").await?;
///
/// // Send a message
/// connector.send(Exchange::Deribit, r#"{"type": "subscribe"}"#).await?;
///
/// // Check connection status
/// if connector.is_connected(Exchange::Deribit) {
///     println!("Connected!");
/// }
/// ```
pub struct Connector {
    /// Configuration for WebSocket connections.
    config: WebSocketConfig,
    /// Active connections indexed by exchange.
    connections: DashMap<Exchange, ConnectionHandle>,
    /// Channel for emitting connection events.
    event_tx: mpsc::Sender<ConnectorEvent>,
    /// Channel for forwarding raw messages.
    message_tx: mpsc::Sender<RawMessage>,
    /// Metrics for monitoring.
    metrics: FlashMetrics,
    /// BlackBox tap for recording events (optional feature).
    #[cfg(feature = "blackbox")]
    tap: Arc<dyn Tap>,
}

impl Connector {
    /// Create a new connection manager.
    ///
    /// Returns the connector along with receivers for events and messages.
    ///
    /// # Arguments
    ///
    /// * `config` - WebSocket configuration
    /// * `metrics` - Metrics instance for monitoring
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - The `Connector` instance
    /// - A receiver for connection events
    /// - A receiver for raw messages from exchanges
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (connector, mut events, mut messages) = Connector::new(config, metrics);
    ///
    /// tokio::spawn(async move {
    ///     while let Some(event) = events.recv().await {
    ///         println!("Event: {:?}", event);
    ///     }
    /// });
    /// ```
    #[must_use]
    pub fn new(
        config: WebSocketConfig,
        metrics: FlashMetrics,
    ) -> (Self, ConnectorEventReceiver, RawMessageReceiver) {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (message_tx, message_rx) = mpsc::channel(MESSAGE_CHANNEL_SIZE);

        let connector = Self {
            config,
            connections: DashMap::new(),
            event_tx,
            message_tx,
            metrics,
            #[cfg(feature = "blackbox")]
            tap: Arc::new(crate::blackbox::NullTap),
        };

        (connector, event_rx, message_rx)
    }

    /// Create a new connection manager with BlackBox tap for recording.
    ///
    /// This constructor is only available when the `blackbox` feature is enabled.
    /// The tap will record all ingress events (raw WebSocket frames) to the
    /// configured journal for later replay.
    ///
    /// # Arguments
    ///
    /// * `config` - WebSocket configuration
    /// * `metrics` - Metrics instance for monitoring
    /// * `tap` - BlackBox tap for recording events
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - The `Connector` instance with tap enabled
    /// - A receiver for connection events
    /// - A receiver for raw messages from exchanges
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use astra_flash::blackbox::{JournalTap, Tap};
    ///
    /// let journal_tap = JournalTap::new(writer);
    /// let (connector, events, messages) = Connector::with_tap(config, metrics, journal_tap);
    /// ```
    #[cfg(feature = "blackbox")]
    #[must_use]
    pub fn with_tap<T: Tap + 'static>(
        config: WebSocketConfig,
        metrics: FlashMetrics,
        tap: T,
    ) -> (Self, ConnectorEventReceiver, RawMessageReceiver) {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (message_tx, message_rx) = mpsc::channel(MESSAGE_CHANNEL_SIZE);

        let connector = Self {
            config,
            connections: DashMap::new(),
            event_tx,
            message_tx,
            metrics,
            tap: Arc::new(tap),
        };

        (connector, event_rx, message_rx)
    }

    /// Connect to an exchange WebSocket.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to connect to
    /// * `url` - WebSocket URL
    ///
    /// # Returns
    ///
    /// `Ok(())` if connection initiated successfully, `Err` otherwise.
    ///
    /// # Errors
    ///
    /// - `FlashError::ConnectionFailed` - WebSocket handshake failed
    /// - `FlashError::ConnectionTimeout` - Connection timeout exceeded
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// connector.connect(Exchange::Deribit, "wss://www.deribit.com/ws/api/v2").await?;
    /// ```
    pub async fn connect(&self, exchange: Exchange, url: &str) -> Result<(), FlashError> {
        // Emit state change to Connecting
        self.emit_state_change(
            exchange,
            ConnectionState::Disconnected,
            ConnectionState::Connecting,
        );

        // Parse and validate URL
        let ws_url = url
            .parse::<url::Url>()
            .map_err(|e| FlashError::ConnectionFailed(format!("Invalid URL: {e}")))?;

        // Attempt connection with timeout
        let connect_timeout = Duration::from_millis(self.config.connect_timeout_ms);
        let ws_stream = match timeout(connect_timeout, connect_async(ws_url.as_str())).await {
            Ok(Ok((stream, _response))) => stream,
            Ok(Err(e)) => {
                self.emit_connection_failed(exchange, &e.to_string(), 1);
                self.emit_state_change(
                    exchange,
                    ConnectionState::Connecting,
                    ConnectionState::Disconnected,
                );
                return Err(FlashError::ConnectionFailed(e.to_string()));
            }
            Err(_) => {
                self.emit_connection_failed(exchange, "Connection timeout", 1);
                self.emit_state_change(
                    exchange,
                    ConnectionState::Connecting,
                    ConnectionState::Disconnected,
                );
                return Err(FlashError::ConnectionTimeout {
                    timeout_ms: self.config.connect_timeout_ms,
                });
            }
        };

        // Create channels for this connection
        let (outgoing_tx, outgoing_rx) = mpsc::channel(OUTGOING_CHANNEL_SIZE);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        // Counters
        let messages_received = Arc::new(AtomicU64::new(0));
        let messages_sent = Arc::new(AtomicU64::new(0));

        // Spawn the connection task
        let task_handle = self.spawn_connection_task(
            exchange,
            ws_stream,
            outgoing_rx,
            shutdown_rx,
            Arc::clone(&messages_received),
            Arc::clone(&messages_sent),
        );

        // Store the handle
        let handle = ConnectionHandle {
            state: ConnectionState::Connected,
            outgoing_tx,
            task_handle,
            url: url.to_string(),
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            messages_received,
            messages_sent,
            shutdown_tx,
        };

        self.connections.insert(exchange, handle);

        // Emit connected event
        self.emit_connected(exchange, url);
        self.emit_state_change(
            exchange,
            ConnectionState::Connecting,
            ConnectionState::Connected,
        );

        // Update metrics
        self.metrics
            .set_websocket_connected(exchange.as_str(), true);

        Ok(())
    }

    /// Disconnect from an exchange.
    ///
    /// Gracefully closes the WebSocket connection.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to disconnect from
    ///
    /// # Returns
    ///
    /// `Ok(())` always (disconnect of non-existent connection is not an error).
    pub async fn disconnect(&self, exchange: Exchange) -> Result<(), FlashError> {
        if let Some((_, handle)) = self.connections.remove(&exchange) {
            // Signal shutdown
            let _ = handle.shutdown_tx.send(()).await;

            // Wait for task to complete (with timeout)
            let _ = timeout(Duration::from_secs(5), handle.task_handle).await;

            // Emit disconnected event
            self.emit_disconnected(exchange, "user requested", false);

            // Update metrics
            self.metrics
                .set_websocket_connected(exchange.as_str(), false);
        }

        Ok(())
    }

    /// Send a message to a connected exchange.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange to send to
    /// * `message` - The message content (typically JSON)
    ///
    /// # Returns
    ///
    /// `Ok(())` if message was queued, `Err` if not connected.
    ///
    /// # Errors
    ///
    /// - `FlashError::Disconnected` - Not connected to the exchange
    pub async fn send(&self, exchange: Exchange, message: &str) -> Result<(), FlashError> {
        if let Some(mut handle) = self.connections.get_mut(&exchange) {
            if handle.state != ConnectionState::Connected {
                return Err(FlashError::Disconnected {
                    reason: format!("Exchange {exchange} is not connected"),
                    will_retry: false,
                });
            }

            handle
                .outgoing_tx
                .send(message.to_string())
                .await
                .map_err(|_| FlashError::ChannelClosed {
                    channel_name: format!("outgoing_{exchange}"),
                })?;

            handle.messages_sent.fetch_add(1, Ordering::Relaxed);
            handle.last_activity = Instant::now();

            Ok(())
        } else {
            Err(FlashError::Disconnected {
                reason: format!("Not connected to {exchange}"),
                will_retry: false,
            })
        }
    }

    /// Get the current state of a connection.
    ///
    /// Returns `ConnectionState::Disconnected` if not connected.
    #[must_use]
    pub fn state(&self, exchange: Exchange) -> ConnectionState {
        self.connections
            .get(&exchange)
            .map_or(ConnectionState::Disconnected, |h| h.state)
    }

    /// Check if an exchange is connected.
    #[must_use]
    pub fn is_connected(&self, exchange: Exchange) -> bool {
        self.state(exchange).is_connected()
    }

    /// Get statistics for a connection.
    ///
    /// Returns `None` if not connected.
    #[must_use]
    pub fn stats(&self, exchange: Exchange) -> Option<ConnectionStats> {
        self.connections.get(&exchange).map(|h| ConnectionStats {
            state: h.state,
            connected_since: Some(h.connected_at),
            messages_received: h.messages_received.load(Ordering::Relaxed),
            messages_sent: h.messages_sent.load(Ordering::Relaxed),
            last_activity: h.last_activity,
            url: h.url.clone(),
        })
    }

    /// Disconnect all connections.
    ///
    /// Gracefully closes all active WebSocket connections.
    pub async fn disconnect_all(&self) {
        let exchanges: Vec<Exchange> = self.connections.iter().map(|e| *e.key()).collect();

        for exchange in exchanges {
            let _ = self.disconnect(exchange).await;
        }
    }

    /// Get list of connected exchanges.
    #[must_use]
    pub fn connected_exchanges(&self) -> Vec<Exchange> {
        self.connections
            .iter()
            .filter(|e| e.state.is_connected())
            .map(|e| *e.key())
            .collect()
    }

    // =========================================================================
    // PRIVATE METHODS
    // =========================================================================

    /// Spawn a task to handle the WebSocket connection.
    ///
    /// This function handles the WebSocket read/write loop including:
    /// - Incoming message processing (Text, Binary, Ping, Pong, Close)
    /// - Outgoing message sending
    /// - Read timeout detection for connection health
    /// - Graceful shutdown
    #[allow(clippy::too_many_lines)]
    fn spawn_connection_task(
        &self,
        exchange: Exchange,
        ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
        mut outgoing_rx: mpsc::Receiver<String>,
        mut shutdown_rx: mpsc::Receiver<()>,
        messages_received: Arc<AtomicU64>,
        messages_sent: Arc<AtomicU64>,
    ) -> JoinHandle<()> {
        let message_tx = self.message_tx.clone();
        let event_tx = self.event_tx.clone();
        let metrics = self.metrics.clone();
        let exchange_str = exchange.as_str().to_string();
        let read_timeout_ms = self.config.read_timeout_ms;

        // Clone tap for the spawned task (BlackBox feature)
        #[cfg(feature = "blackbox")]
        let tap = Arc::clone(&self.tap);

        tokio::spawn(async move {
            let (mut write, mut read) = ws_stream.split();
            let read_timeout = Duration::from_millis(read_timeout_ms);

            loop {
                tokio::select! {
                    // Handle incoming messages with read timeout
                    // If no data received for read_timeout, connection is dead
                    result = timeout(read_timeout, read.next()) => {
                        let msg = if let Ok(msg) = result {
                            msg
                        } else {
                            // Read timeout - no data received for configured duration
                            // Connection is likely dead (silent disconnect)
                            let _ = event_tx.send(ConnectorEvent::Disconnected {
                                exchange,
                                reason: format!("read timeout: no data for {read_timeout_ms}ms"),
                                will_reconnect: true,
                            }).await;
                            break;
                        };
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                messages_received.fetch_add(1, Ordering::Relaxed);

                                // Record metrics
                                metrics.record_message_received(
                                    &exchange_str,
                                    "unknown", // Adapter will parse
                                    MessageType::Unknown,
                                );

                                // Emit message received event
                                let _ = event_tx.send(ConnectorEvent::MessageReceived {
                                    exchange,
                                    size_bytes: text.len(),
                                }).await;

                                // Capture timestamp for message
                                let received_at = chrono::Utc::now().timestamp_micros();

                                // TAP-1 (Ingress): Record raw frame to BlackBox journal
                                #[cfg(feature = "blackbox")]
                                tap.record_flash_ingress(exchange, text.as_bytes(), received_at);

                                // Forward to message channel
                                let raw_msg = RawMessage {
                                    exchange,
                                    payload: text,
                                    received_at,
                                };

                                if message_tx.send(raw_msg).await.is_err() {
                                    // Message channel closed, exit
                                    break;
                                }
                            }
                            Some(Ok(Message::Binary(data))) => {
                                messages_received.fetch_add(1, Ordering::Relaxed);

                                // Capture timestamp for message
                                let received_at = chrono::Utc::now().timestamp_micros();

                                // TAP-1 (Ingress): Record raw binary frame to BlackBox journal
                                #[cfg(feature = "blackbox")]
                                tap.record_flash_ingress(exchange, &data, received_at);

                                // Convert to string if possible
                                if let Ok(text) = String::from_utf8(data) {
                                    let raw_msg = RawMessage {
                                        exchange,
                                        payload: text,
                                        received_at,
                                    };

                                    if message_tx.send(raw_msg).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Some(Ok(Message::Ping(data))) => {
                                // Respond to ping with pong
                                let _ = write.send(Message::Pong(data)).await;
                            }
                            Some(Ok(Message::Pong(_))) => {
                                // Pong received, connection is healthy
                            }
                            Some(Ok(Message::Close(_))) => {
                                // Server initiated close
                                let _ = event_tx.send(ConnectorEvent::Disconnected {
                                    exchange,
                                    reason: "server closed connection".to_string(),
                                    will_reconnect: false,
                                }).await;
                                break;
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame, ignore
                            }
                            Some(Err(e)) => {
                                // Error reading
                                let _ = event_tx.send(ConnectorEvent::Disconnected {
                                    exchange,
                                    reason: e.to_string(),
                                    will_reconnect: false,
                                }).await;
                                break;
                            }
                            None => {
                                // Stream ended
                                let _ = event_tx.send(ConnectorEvent::Disconnected {
                                    exchange,
                                    reason: "stream ended".to_string(),
                                    will_reconnect: false,
                                }).await;
                                break;
                            }
                        }
                    }

                    // Handle outgoing messages
                    Some(msg) = outgoing_rx.recv() => {
                        if write.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                        messages_sent.fetch_add(1, Ordering::Relaxed);
                    }

                    // Handle shutdown signal
                    _ = shutdown_rx.recv() => {
                        // Send close frame
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }
                }
            }

            // Update metrics on exit
            metrics.set_websocket_connected(&exchange_str, false);
        })
    }

    /// Emit a Connected event.
    fn emit_connected(&self, exchange: Exchange, url: &str) {
        let event = ConnectorEvent::Connected {
            exchange,
            url: url.to_string(),
        };
        let _ = self.event_tx.try_send(event);
    }

    /// Emit a Disconnected event.
    fn emit_disconnected(&self, exchange: Exchange, reason: &str, will_reconnect: bool) {
        let event = ConnectorEvent::Disconnected {
            exchange,
            reason: reason.to_string(),
            will_reconnect,
        };
        let _ = self.event_tx.try_send(event);
    }

    /// Emit a ConnectionFailed event.
    fn emit_connection_failed(&self, exchange: Exchange, error: &str, attempt: u32) {
        let event = ConnectorEvent::ConnectionFailed {
            exchange,
            error: error.to_string(),
            attempt,
        };
        let _ = self.event_tx.try_send(event);
    }

    /// Emit a StateChanged event.
    fn emit_state_change(
        &self,
        exchange: Exchange,
        old_state: ConnectionState,
        new_state: ConnectionState,
    ) {
        let event = ConnectorEvent::StateChanged {
            exchange,
            old_state,
            new_state,
        };
        let _ = self.event_tx.try_send(event);
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

// Connector is Send + Sync via DashMap and Arc internals
// This is verified by the test_connector_is_send and test_connector_is_sync tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_display_values() {
        assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
        assert_eq!(ConnectionState::Connecting.to_string(), "connecting");
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Disconnecting.to_string(), "disconnecting");
    }

    #[test]
    fn test_connection_state_is_connected_values() {
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(!ConnectionState::Connecting.is_connected());
        assert!(ConnectionState::Connected.is_connected());
        assert!(!ConnectionState::Disconnecting.is_connected());
    }

    #[test]
    fn test_connection_stats_default_values() {
        let stats = ConnectionStats::default();
        assert_eq!(stats.state, ConnectionState::Disconnected);
        assert!(stats.connected_since.is_none());
        assert_eq!(stats.messages_received, 0);
        assert_eq!(stats.messages_sent, 0);
        assert!(stats.url.is_empty());
    }

    #[test]
    fn test_raw_message_fields() {
        let msg = RawMessage {
            exchange: Exchange::Deribit,
            payload: "test".to_string(),
            received_at: 12345,
        };
        assert_eq!(msg.exchange, Exchange::Deribit);
        assert_eq!(msg.payload, "test");
        assert_eq!(msg.received_at, 12345);
    }
}
