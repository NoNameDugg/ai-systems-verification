//! Flash - High-Frequency Market Data Adapter
//!
//! # Overview
//!
//! Flash is a Rust-based service that ingests raw market data at C++ speeds,
//! normalizes it with zero-copy parsing, and pushes clean data to downstream
//! consumers via Redis Streams.
//!
//! # Mission Statement
//!
//! > "Be faster than the garbage collector. Never drop a tick."
//!
//! # Architecture
//!
//! ```text
//! [Exchange WS] <===> [Flash (Rust)] ====> [Redis Stream] ====> [Python Strategy]
//! ```
//!
//! # Modules
//!
//! - [`core`] - Core types, error handling, configuration, metrics
//! - [`network`] - WebSocket connectivity and exchange adapters
//! - [`book`] - Order book management (L2/L3)
//! - [`publisher`] - Redis stream publishing
//! - [`bindings`] - Python bindings (PyO3)
//!
//! # Performance Targets
//!
//! | Metric | Target |
//! |--------|--------|
//! | Internal Latency | < 50 microseconds |
//! | Throughput | > 50,000 msg/sec |
//! | Memory Footprint | < 100 MB |
//! | Uptime | 99.999% |
//!
//! # Example
//!
//! ```ignore
//! use astra_flash::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Load configuration
//!     let config = Config::load("config/flash.yaml")?;
//!
//!     // Create Flash instance
//!     let flash = Flash::new(config).await?;
//!
//!     // Start processing
//!     flash.run().await?;
//!
//!     Ok(())
//! }
//! ```

// =============================================================================
// CLIPPY CONFIGURATION - MANDATORY (See STANDARDS.md)
// =============================================================================

// Treat warnings as errors
#![deny(warnings)]
// Standard Clippy lints
#![deny(clippy::all)]
// Note: More strict lints will be enabled after initial development
// #![deny(clippy::pedantic)]

// Require documentation for public items
#![warn(missing_docs)]
// Deny unsafe code by default (must opt-in with justification)
#![deny(unsafe_code)]
// Additional quality lints
#![deny(
    rust_2018_idioms,
    trivial_casts,
    trivial_numeric_casts,
    unused_import_braces,
    unused_qualifications
)]

// =============================================================================
// MODULES
// =============================================================================

/// Core types, error handling, configuration, and metrics.
pub mod core;

/// WebSocket connectivity and exchange adapters.
pub mod network;

/// Order book management (L2/L3).
pub mod book;

/// Redis stream publishing.
pub mod publisher;

/// Python bindings (PyO3).
#[cfg(feature = "python")]
pub mod bindings;

/// Flight-recorder integration (optional).
///
/// Provides tap points for recording events to enable deterministic replay.
/// Only available with `--features blackbox`.
#[cfg(feature = "blackbox")]
pub mod blackbox;

/// Order management module with egress-tap integration.
///
/// Provides order lifecycle management including submission, cancellation,
/// and modification with flight-recorder recording support.
pub mod order;

/// Signal-output integration module.
///
/// Provides the AlphaSignal format required for integration with a downstream
/// decision-aggregation engine. All signals published to the aggregator must
/// use this standardized format.
pub mod fusion;

/// Gateway integration module.
///
/// Provides Gateway-compatible data types for Redis publishing.
/// These types are optimized for JSON serialization and Gateway UI consumption.
///
/// # Types
///
/// - [`gateway::OrderBookSnapshot`] - Full order book state for Gateway UI
/// - [`gateway::OrderBookLevel`] - Single price level with f64 quantity
pub mod gateway;

/// SIMD-accelerated JSON parsing module.
///
/// Provides high-performance JSON parsing using `simd-json` which leverages
/// AVX2 SIMD instructions for 2-4x faster parsing compared to standard `serde_json`.
///
/// # Types
///
/// - [`parsing::SimdParser`] - High-performance parser with SIMD acceleration
///
/// # Functions
///
/// - [`parsing::parse_oanda_price_simd`] - Parse OANDA price with SIMD
/// - [`parsing::parse_oanda_price_serde`] - Parse OANDA price with serde_json
pub mod parsing;

// =============================================================================
// PRELUDE - Common imports
// =============================================================================

/// Commonly used types and traits.
///
/// Import with `use astra_flash::prelude::*;`
pub mod prelude {
    // Re-export error types
    pub use crate::core::error::{
        BookResult, ErrorSeverity, FlashError, FlashResult, NetworkResult,
    };

    // Re-export core types
    pub use crate::core::types::{
        datetime_to_timestamp, now_micros, timestamp_to_datetime, Exchange, Instrument, MarketData,
        MarketEvent, MarketEventType, Price, PriceLevel, Quantity, Side, Timestamp,
    };

    // Re-export configuration types
    pub use crate::core::config::{
        BackpressureAction, BackpressureConfig, FlashConfig, GapHandling, LogFormat, LogLevel,
        LoggingConfig, MetricsConfig, OrderBookConfig, PublisherConfig, RedisConfig,
        SerializationFormat, WebSocketConfig,
    };

    // Re-export metrics types
    pub use crate::core::metrics::{
        FlashMetrics, MessageType, MetricsError, ProcessingStage, ReconnectReason, TimingGuard,
        UpdateType,
    };

    // Re-export network types
    pub use crate::network::{
        ConnectionState, ConnectionStats, Connector, ConnectorEvent, ConnectorEventReceiver,
        RawMessage, RawMessageReceiver,
    };

    // Re-export heartbeat types
    pub use crate::network::{
        HealthStatus, HealthSummary, HeartbeatConfig, HeartbeatEvent, HeartbeatEventReceiver,
        HeartbeatManager,
    };

    // Re-export reconnection types
    pub use crate::network::{
        ReconnectionConfig, ReconnectionEvent, ReconnectionEventReceiver, ReconnectionManager,
        ReconnectionStats, ReconnectionStatus, SequenceGap,
    };

    // Re-export adapter types
    pub use crate::network::adapters::{
        create_adapter, BinanceAdapter, BookInterval, DeribitAdapter, ExchangeAdapter,
        OandaAdapter, RateLimit, UpdateInterval,
    };

    // Re-export order book types
    pub use crate::book::{BookSnapshot, OrderBook, OrderBookStats};

    // Re-export snapshot processing types
    pub use crate::book::{
        LevelChange, SnapshotDiff, SnapshotError, SnapshotMetadata, SnapshotProcessor,
        SnapshotProcessorConfig, SnapshotProcessorStats, SnapshotResult, SnapshotValidator,
        ValidationConfig,
    };

    // Re-export delta processing types
    pub use crate::book::{
        BatchApplyResult, DeltaApplyResult, DeltaBatch, DeltaError, DeltaProcessor,
        DeltaProcessorConfig, DeltaProcessorStats, DeltaResult, DeltaUpdate, DeltaValidationConfig,
        DeltaValidator, SequenceCheckResult, SequenceConfig, SequenceState, SequenceTracker,
        SnapshotRequest, SnapshotRequestReason,
    };

    // Re-export thread-safe order book types
    pub use crate::book::{
        new_shared_orderbook, OrderBookWriteHandle, ReadGuard, SharedOrderBook,
        ThreadSafeOrderBook, ThreadSafeStats, WriteGuard,
    };

    // Re-export order management types
    pub use crate::order::{
        CancelReason, ModifyRequest, OrderId, OrderManager, OrderManagerConfig, OrderManagerStats,
        OrderRequest, OrderSide, OrderStatus, OrderType, TimeInForce,
    };

    // Re-export Fusion integration types
    pub use crate::fusion::{
        build_fusion_topic, AlphaSignal, AlphaSignalMetadata, SignalDirection, FUSION_TOPIC_PREFIX,
        SOURCE_STRATEGY,
    };

    // Re-export Gateway integration types
    pub use crate::gateway::{OrderBookLevel, OrderBookSnapshot};

    // Re-export OANDA message types
    pub use crate::network::adapters::{OandaHeartbeat, OandaLevel, OandaPrice};

    // Re-export SIMD parsing types
    pub use crate::parsing::{
        parse_oanda_heartbeat_serde, parse_oanda_heartbeat_simd, parse_oanda_price_serde,
        parse_oanda_price_simd, SimdParser,
    };

    // Re-export common external types
    pub use anyhow::Result;
}

// =============================================================================
// VERSION INFO
// =============================================================================

/// Library version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = env!("CARGO_PKG_NAME");
