//! Core module - Foundational types for Flash.
//!
//! This module contains the core building blocks:
//!
//! - [`types`] - Core data types (MarketEvent, Price, Quantity, etc.)
//! - [`error`] - Error types and Result aliases
//! - [`config`] - Configuration loading and validation
//! - [`metrics`] - Prometheus metrics
//!
//! # Phase 1 Implementation
//!
//! This module is implemented in Phase 1 of the TDI roadmap:
//!
//! - Part 1.1: Core Types ✓
//! - Part 1.2: Error Handling ✓
//! - Part 1.3: Configuration System ✓
//! - Part 1.4: Metrics Foundation ✓

// Submodules
pub mod config;
pub mod error;
pub mod metrics;
pub mod types;

// Re-exports for convenience
pub use config::{
    BackpressureAction, BackpressureConfig, ConfigValidationError, ExchangeConfig, ExchangesConfig,
    FlashConfig, GapHandling, LogFormat, LogLevel, LoggingConfig, MetricsConfig, OrderBookConfig,
    PublisherConfig, RedisConfig, SerializationFormat, ShadowModeConfig, WebSocketConfig,
};
pub use error::{BookResult, ErrorSeverity, FlashError, FlashResult, NetworkResult};
pub use metrics::{
    FlashMetrics, MessageType, MetricsError, ProcessingStage, ReconnectReason, TimingGuard,
    UpdateType,
};
pub use types::{
    datetime_to_timestamp, now_micros, timestamp_to_datetime, Exchange, Instrument, MarketData,
    MarketEvent, MarketEventType, Price, PriceLevel, Quantity, Side, Timestamp,
};
