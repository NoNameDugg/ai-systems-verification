//! Network module - WebSocket connectivity for Flash.
//!
//! This module handles all WebSocket communication with exchanges:
//!
//! - [`connector`] - Connection management and state machine
//! - [`heartbeat`] - Ping/pong and health monitoring
//! - [`reconnect`] - Automatic reconnection with exponential backoff
//! - [`adapters`] - Exchange-specific message parsing
//!
//! # Phase 2 Implementation
//!
//! This module is implemented in Phase 2 of the TDI roadmap:
//!
//! - Part 2.1: Connection Manager (COMPLETE)
//! - Part 2.2: Heartbeat & Health Monitoring (COMPLETE)
//! - Part 2.3: Reconnection Logic (COMPLETE)
//! - Part 2.4: Exchange Adapters (COMPLETE)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                     Connector                            │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
//! │  │  Heartbeat   │  │  Reconnect   │  │   Adapter    │  │
//! │  └──────────────┘  └──────────────┘  └──────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//!                           │
//!                           ▼
//!              [tokio-tungstenite WebSocket]
//! ```

// Submodules
pub mod adapters;
pub mod connector;
pub mod heartbeat;
pub mod reconnect;

// Re-exports for convenience
pub use connector::{
    ConnectionState, ConnectionStats, Connector, ConnectorEvent, ConnectorEventReceiver,
    RawMessage, RawMessageReceiver,
};

pub use heartbeat::{
    HealthStatus, HealthSummary, HeartbeatConfig, HeartbeatEvent, HeartbeatEventReceiver,
    HeartbeatManager,
};

pub use reconnect::{
    ReconnectionConfig, ReconnectionEvent, ReconnectionEventReceiver, ReconnectionManager,
    ReconnectionStats, ReconnectionStatus, SequenceGap,
};
