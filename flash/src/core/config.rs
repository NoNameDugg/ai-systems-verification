//! Configuration system for Flash.
//!
//! This module provides centralized, type-safe configuration management
//! supporting YAML files and environment variable overrides.
//!
//! # Features
//!
//! - YAML configuration files (primary)
//! - Environment variable overrides (12-factor app compliance)
//! - Hierarchical configuration merging
//! - Validation of all configuration values
//! - Sensible defaults for all settings
//!
//! # Usage
//!
//! ```rust,ignore
//! use astra_flash::core::config::FlashConfig;
//!
//! // Load from file with optional environment overlay
//! let config = FlashConfig::load("config/flash.yaml", Some("prod"))?;
//!
//! // Load from YAML string (for testing)
//! let config = FlashConfig::from_yaml(yaml_string)?;
//!
//! // Use defaults
//! let config = FlashConfig::default();
//! ```
//!
//! # Environment Variables
//!
//! All configuration values can be overridden via environment variables
//! using the `ASTRA_FLASH_` prefix with underscores replacing dots:
//!
//! ```bash
//! export ASTRA_FLASH_WEBSOCKET_CONNECT_TIMEOUT_MS=10000
//! export ASTRA_FLASH_REDIS_URL="redis://production:6379"
//! ```

use crate::core::error::FlashError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

// =============================================================================
// CONFIGURATION VALIDATION ERRORS
// =============================================================================

/// Configuration validation error.
#[derive(Debug, Error)]
pub enum ConfigValidationError {
    /// Value is outside allowed range.
    #[error("{field} must be between {min} and {max}, got {value}")]
    OutOfRange {
        /// Field name
        field: String,
        /// Minimum allowed value
        min: String,
        /// Maximum allowed value
        max: String,
        /// Actual value
        value: String,
    },

    /// Value must not be empty.
    #[error("{field} must not be empty")]
    Empty {
        /// Field name
        field: String,
    },

    /// Value is invalid for other reasons.
    #[error("{field} has invalid value: {reason}")]
    Invalid {
        /// Field name
        field: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Two fields have incorrect ordering.
    #[error("{field1} must be greater than or equal to {field2}")]
    Ordering {
        /// First field name
        field1: String,
        /// Second field name
        field2: String,
    },

    /// Invalid URL format.
    #[error("Invalid URL for {field}: {url}")]
    InvalidUrl {
        /// Field name
        field: String,
        /// Invalid URL
        url: String,
    },
}

// =============================================================================
// ENUMERATIONS
// =============================================================================

/// Gap handling strategy for order book sequence gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GapHandling {
    /// Request a new snapshot when gap detected.
    #[default]
    Snapshot,
    /// Skip the gap and continue (may lose data).
    Skip,
}

/// Serialization format for Redis publishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SerializationFormat {
    /// JSON format (human-readable, slower).
    Json,
    /// Bincode format (binary, fast, recommended).
    #[default]
    Bincode,
    /// Rkyv format (zero-copy, fastest).
    Rkyv,
}

/// Backpressure action when queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BackpressureAction {
    /// Drop oldest messages.
    #[default]
    DropOldest,
    /// Drop newest messages (reject incoming).
    DropNewest,
    /// Block producer until space available.
    Block,
}

/// Log level enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum LogLevel {
    /// Trace level (most verbose).
    Trace,
    /// Debug level.
    Debug,
    /// Info level (default).
    #[default]
    Info,
    /// Warn level.
    Warn,
    /// Error level (least verbose).
    Error,
}

/// Log format enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum LogFormat {
    /// Pretty console output.
    Pretty,
    /// Structured JSON.
    #[default]
    Json,
}

// =============================================================================
// CONFIGURATION STRUCTS
// =============================================================================

/// WebSocket connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSocketConfig {
    /// Connection timeout for initial WebSocket handshake (ms).
    pub connect_timeout_ms: u64,
    /// Read timeout - force reconnect if no data received for this duration (ms).
    ///
    /// OANDA streams heartbeats every ~5s. If we receive nothing for 5 seconds,
    /// the connection is dead. Don't rely on TCP keepalives - they're too slow
    /// (minutes) for trading systems.
    pub read_timeout_ms: u64,
    /// Interval between ping messages to keep connection alive (ms).
    pub ping_interval_ms: u64,
    /// Timeout waiting for pong response (ms).
    pub pong_timeout_ms: u64,
    /// Maximum reconnection attempts before giving up.
    pub max_reconnect_attempts: u32,
    /// Base delay between reconnection attempts (ms).
    pub reconnect_delay_ms: u64,
    /// Maximum delay between reconnection attempts (ms).
    pub max_reconnect_delay_ms: u64,
    /// Jitter percentage for reconnection delay (0.0-0.5).
    pub reconnect_jitter: f64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 5000,
            read_timeout_ms: 5000, // 5 seconds - force reconnect if no data
            ping_interval_ms: 30000,
            pong_timeout_ms: 10000,
            max_reconnect_attempts: 10,
            reconnect_delay_ms: 1000,
            max_reconnect_delay_ms: 30000,
            reconnect_jitter: 0.1,
        }
    }
}

impl WebSocketConfig {
    /// Get connect timeout as Duration.
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// Get read timeout as Duration.
    ///
    /// If no data is received within this duration, the connection
    /// is considered dead and should be reconnected.
    #[must_use]
    pub const fn read_timeout(&self) -> Duration {
        Duration::from_millis(self.read_timeout_ms)
    }

    /// Get ping interval as Duration.
    #[must_use]
    pub const fn ping_interval(&self) -> Duration {
        Duration::from_millis(self.ping_interval_ms)
    }

    /// Get pong timeout as Duration.
    #[must_use]
    pub const fn pong_timeout(&self) -> Duration {
        Duration::from_millis(self.pong_timeout_ms)
    }

    /// Get reconnect delay as Duration.
    #[must_use]
    pub const fn reconnect_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_delay_ms)
    }

    /// Get max reconnect delay as Duration.
    #[must_use]
    pub const fn max_reconnect_delay(&self) -> Duration {
        Duration::from_millis(self.max_reconnect_delay_ms)
    }

    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if !(1000..=60000).contains(&self.connect_timeout_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.connect_timeout_ms".to_string(),
                min: "1000".to_string(),
                max: "60000".to_string(),
                value: self.connect_timeout_ms.to_string(),
            });
        }

        // Read timeout: 1-30 seconds (trading systems need fast detection)
        if !(1000..=30000).contains(&self.read_timeout_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.read_timeout_ms".to_string(),
                min: "1000".to_string(),
                max: "30000".to_string(),
                value: self.read_timeout_ms.to_string(),
            });
        }

        if !(5000..=120000).contains(&self.ping_interval_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.ping_interval_ms".to_string(),
                min: "5000".to_string(),
                max: "120000".to_string(),
                value: self.ping_interval_ms.to_string(),
            });
        }

        if !(1000..=60000).contains(&self.pong_timeout_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.pong_timeout_ms".to_string(),
                min: "1000".to_string(),
                max: "60000".to_string(),
                value: self.pong_timeout_ms.to_string(),
            });
        }

        if !(1..=100).contains(&self.max_reconnect_attempts) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.max_reconnect_attempts".to_string(),
                min: "1".to_string(),
                max: "100".to_string(),
                value: self.max_reconnect_attempts.to_string(),
            });
        }

        if !(100..=30000).contains(&self.reconnect_delay_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.reconnect_delay_ms".to_string(),
                min: "100".to_string(),
                max: "30000".to_string(),
                value: self.reconnect_delay_ms.to_string(),
            });
        }

        if self.max_reconnect_delay_ms < self.reconnect_delay_ms
            || self.max_reconnect_delay_ms > 300000
        {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.max_reconnect_delay_ms".to_string(),
                min: format!("{} (reconnect_delay_ms)", self.reconnect_delay_ms),
                max: "300000".to_string(),
                value: self.max_reconnect_delay_ms.to_string(),
            });
        }

        if !(0.0..=0.5).contains(&self.reconnect_jitter) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "websocket.reconnect_jitter".to_string(),
                min: "0.0".to_string(),
                max: "0.5".to_string(),
                value: self.reconnect_jitter.to_string(),
            });
        }

        errors
    }
}

/// Order book configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrderBookConfig {
    /// Soft limit: Maximum depth for serialization to Redis.
    pub max_depth: usize,
    /// Hard limit: Absolute maximum levels (dust attack protection).
    pub max_levels: usize,
    /// Whether to track L3 (order-level) data.
    pub track_orders: bool,
    /// Automatically prune levels with zero quantity.
    pub auto_prune: bool,
    /// Gap handling strategy.
    pub gap_handling: GapHandling,
}

impl Default for OrderBookConfig {
    fn default() -> Self {
        Self {
            max_depth: 50,
            max_levels: 100,
            track_orders: false,
            auto_prune: true,
            gap_handling: GapHandling::default(),
        }
    }
}

impl OrderBookConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if !(1..=1000).contains(&self.max_depth) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "orderbook.max_depth".to_string(),
                min: "1".to_string(),
                max: "1000".to_string(),
                value: self.max_depth.to_string(),
            });
        }

        if self.max_levels < self.max_depth {
            errors.push(ConfigValidationError::Ordering {
                field1: "orderbook.max_levels".to_string(),
                field2: "orderbook.max_depth".to_string(),
            });
        }

        if self.max_levels > 10000 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "orderbook.max_levels".to_string(),
                min: format!("{} (max_depth)", self.max_depth),
                max: "10000".to_string(),
                value: self.max_levels.to_string(),
            });
        }

        errors
    }
}

/// Redis connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RedisConfig {
    /// Redis connection URL.
    pub url: String,
    /// Database number (0-15).
    pub database: u8,
    /// Connection pool size.
    pub pool_size: u32,
    /// Connection timeout (ms).
    pub connect_timeout_ms: u64,
    /// Command timeout (ms, 0 = no timeout).
    pub command_timeout_ms: u64,
    /// Maximum message batch size before flush.
    pub batch_size: usize,
    /// Maximum time to wait before flushing batch (ms).
    pub batch_timeout_ms: u64,
    /// Health check interval (ms).
    pub health_check_interval_ms: u64,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_string(),
            database: 0,
            pool_size: 10,
            connect_timeout_ms: 5000,
            command_timeout_ms: 1000,
            batch_size: 100,
            batch_timeout_ms: 10,
            health_check_interval_ms: 5000,
        }
    }
}

impl RedisConfig {
    /// Parse the Redis URL and return parsed components.
    ///
    /// # Errors
    ///
    /// Returns error if URL is malformed.
    pub fn parsed_url(&self) -> Result<url::Url, url::ParseError> {
        url::Url::parse(&self.url)
    }

    /// Get connect timeout as Duration.
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// Get command timeout as Duration.
    #[must_use]
    pub const fn command_timeout(&self) -> Duration {
        Duration::from_millis(self.command_timeout_ms)
    }

    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        // Validate URL format
        if self.parsed_url().is_err() {
            errors.push(ConfigValidationError::InvalidUrl {
                field: "redis.url".to_string(),
                url: self.url.clone(),
            });
        }

        if self.database > 15 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "redis.database".to_string(),
                min: "0".to_string(),
                max: "15".to_string(),
                value: self.database.to_string(),
            });
        }

        if !(1..=100).contains(&self.pool_size) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "redis.pool_size".to_string(),
                min: "1".to_string(),
                max: "100".to_string(),
                value: self.pool_size.to_string(),
            });
        }

        if !(100..=30000).contains(&self.connect_timeout_ms) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "redis.connect_timeout_ms".to_string(),
                min: "100".to_string(),
                max: "30000".to_string(),
                value: self.connect_timeout_ms.to_string(),
            });
        }

        errors
    }
}

/// Backpressure configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackpressureConfig {
    /// Channel capacity.
    pub capacity: usize,
    /// Warning threshold (percentage of capacity).
    pub warn_threshold: f64,
    /// Critical threshold (percentage of capacity).
    pub critical_threshold: f64,
    /// Action when critical threshold exceeded.
    pub action: BackpressureAction,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            capacity: 10000,
            warn_threshold: 0.75,
            critical_threshold: 0.95,
            action: BackpressureAction::default(),
        }
    }
}

impl BackpressureConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if !(100..=1_000_000).contains(&self.capacity) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "publisher.backpressure.capacity".to_string(),
                min: "100".to_string(),
                max: "1000000".to_string(),
                value: self.capacity.to_string(),
            });
        }

        if !(0.1..=0.95).contains(&self.warn_threshold) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "publisher.backpressure.warn_threshold".to_string(),
                min: "0.1".to_string(),
                max: "0.95".to_string(),
                value: self.warn_threshold.to_string(),
            });
        }

        if self.critical_threshold <= self.warn_threshold {
            errors.push(ConfigValidationError::Ordering {
                field1: "publisher.backpressure.critical_threshold".to_string(),
                field2: "publisher.backpressure.warn_threshold".to_string(),
            });
        }

        if !(self.warn_threshold..=1.0).contains(&self.critical_threshold) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "publisher.backpressure.critical_threshold".to_string(),
                min: format!("{} (warn_threshold)", self.warn_threshold),
                max: "1.0".to_string(),
                value: self.critical_threshold.to_string(),
            });
        }

        errors
    }
}

/// Publisher configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PublisherConfig {
    /// Serialization format for Redis streams.
    pub format: SerializationFormat,
    /// Fallback to JSON for debugging.
    pub debug_json_fallback: bool,
    /// Topic prefix for all market data streams.
    pub topic_prefix: String,
    /// Backpressure settings.
    pub backpressure: BackpressureConfig,
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self {
            format: SerializationFormat::default(),
            debug_json_fallback: false,
            // FUSION WIRING: Changed from "market_data" to "astra:signals:flash"
            // This ensures Flash publishes to the stream that Fusion subscribes to.
            // Topic format: astra:signals:flash:{exchange}:{symbol}
            topic_prefix: "astra:signals:flash".to_string(),
            backpressure: BackpressureConfig::default(),
        }
    }
}

impl PublisherConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if self.topic_prefix.is_empty() {
            errors.push(ConfigValidationError::Empty {
                field: "publisher.topic_prefix".to_string(),
            });
        }

        errors.extend(self.backpressure.validate());

        errors
    }
}

/// Exchange-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExchangeConfig {
    /// Whether this exchange is enabled.
    pub enabled: bool,
    /// WebSocket URL.
    pub ws_url: String,
    /// API key (for authenticated endpoints).
    pub api_key: Option<String>,
    /// Account ID (for OANDA).
    pub account_id: Option<String>,
    /// Rate limit (requests per second).
    pub rate_limit_per_second: u32,
    /// Instruments to subscribe.
    pub instruments: Vec<String>,
}

impl Default for ExchangeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ws_url: String::new(),
            api_key: None,
            account_id: None,
            rate_limit_per_second: 20,
            instruments: Vec::new(),
        }
    }
}

/// All exchange configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExchangesConfig {
    /// Deribit exchange configuration.
    pub deribit: ExchangeConfig,
    /// Binance exchange configuration.
    pub binance: ExchangeConfig,
    /// OANDA exchange configuration.
    pub oanda: ExchangeConfig,
}

impl Default for ExchangesConfig {
    fn default() -> Self {
        Self {
            deribit: ExchangeConfig {
                enabled: false,
                ws_url: "wss://www.deribit.com/ws/api/v2".to_string(),
                rate_limit_per_second: 20,
                ..Default::default()
            },
            binance: ExchangeConfig {
                enabled: false,
                ws_url: "wss://stream.binance.com:9443/ws".to_string(),
                rate_limit_per_second: 20,
                ..Default::default()
            },
            oanda: ExchangeConfig {
                enabled: false,
                ws_url: "wss://stream-fxpractice.oanda.com/v3/accounts".to_string(),
                rate_limit_per_second: 100,
                ..Default::default()
            },
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level.
    pub level: LogLevel,
    /// Output format.
    pub format: LogFormat,
    /// Log file path (empty = stdout only).
    pub file_path: Option<String>,
    /// Maximum log file size in MB before rotation.
    pub max_file_size_mb: u32,
    /// Number of rotated files to keep.
    pub max_files: u32,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::default(),
            format: LogFormat::default(),
            file_path: None,
            max_file_size_mb: 100,
            max_files: 5,
        }
    }
}

impl LoggingConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if !(1..=10000).contains(&self.max_file_size_mb) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "logging.max_file_size_mb".to_string(),
                min: "1".to_string(),
                max: "10000".to_string(),
                value: self.max_file_size_mb.to_string(),
            });
        }

        if !(1..=100).contains(&self.max_files) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "logging.max_files".to_string(),
                min: "1".to_string(),
                max: "100".to_string(),
                value: self.max_files.to_string(),
            });
        }

        errors
    }
}

/// Metrics (Prometheus) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics endpoint.
    pub enabled: bool,
    /// Port for metrics HTTP server.
    pub port: u16,
    /// Bind address for metrics server.
    pub bind_address: String,
    /// Metrics endpoint path.
    pub path: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9090,
            bind_address: "0.0.0.0".to_string(),
            path: "/metrics".to_string(),
        }
    }
}

impl MetricsConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        if !(1024..=65535).contains(&self.port) {
            errors.push(ConfigValidationError::OutOfRange {
                field: "metrics.port".to_string(),
                min: "1024".to_string(),
                max: "65535".to_string(),
                value: self.port.to_string(),
            });
        }

        if !self.path.starts_with('/') {
            errors.push(ConfigValidationError::Invalid {
                field: "metrics.path".to_string(),
                reason: "path must start with /".to_string(),
            });
        }

        errors
    }
}

/// Shadow Mode configuration.
///
/// Shadow Mode allows this engine to run in parallel with an existing producer,
/// writing to separate Redis namespaces for validation before cutover.
///
/// # Redis Key Namespaces
///
/// | Mode | Orderbook Key | Signal Key |
/// |------|---------------|------------|
/// | Production | `market:orderbook:EUR_USD` | `astra:signals:flash:oanda:EUR_USD` |
/// | Shadow | `market:orderbook:rust:EUR_USD` | `astra:signals:flash:rust:oanda:EUR_USD` |
///
/// # Example
///
/// ```rust
/// use astra_flash::core::config::ShadowModeConfig;
///
/// let config = ShadowModeConfig {
///     enabled: true,
///     namespace: "rust".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowModeConfig {
    /// Enable shadow mode (writes to separate namespace).
    ///
    /// When enabled, all Redis keys will include the namespace segment.
    pub enabled: bool,

    /// Namespace to insert into Redis keys when shadow mode is enabled.
    ///
    /// Default: "rust"
    /// Example: `market:orderbook:rust:EUR_USD`
    pub namespace: String,
}

impl Default for ShadowModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            namespace: "rust".to_string(),
        }
    }
}

impl ShadowModeConfig {
    /// Validate this configuration section.
    fn validate(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        // Namespace must not be empty when shadow mode is enabled
        if self.enabled && self.namespace.is_empty() {
            errors.push(ConfigValidationError::Empty {
                field: "shadow_mode.namespace".to_string(),
            });
        }

        // Namespace should not contain colons or special characters
        if self.enabled && self.namespace.contains(':') {
            errors.push(ConfigValidationError::Invalid {
                field: "shadow_mode.namespace".to_string(),
                reason: "namespace must not contain colons".to_string(),
            });
        }

        errors
    }
}

// =============================================================================
// MAIN CONFIGURATION STRUCT
// =============================================================================

/// Main configuration for Flash.
///
/// This struct contains all configuration for the application,
/// organized into logical sections.
///
/// # Examples
///
/// ```rust,ignore
/// use astra_flash::core::config::FlashConfig;
///
/// // Load from file
/// let config = FlashConfig::load("config/flash.yaml", None)?;
///
/// // Access values
/// println!("Redis URL: {}", config.redis.url);
/// println!("Connect timeout: {:?}", config.websocket.connect_timeout());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct FlashConfig {
    /// WebSocket connection settings.
    pub websocket: WebSocketConfig,
    /// Order book settings.
    pub orderbook: OrderBookConfig,
    /// Redis connection and publishing settings.
    pub redis: RedisConfig,
    /// Message publisher settings.
    pub publisher: PublisherConfig,
    /// Exchange-specific settings.
    pub exchanges: ExchangesConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
    /// Metrics (Prometheus) configuration.
    pub metrics: MetricsConfig,
    /// Shadow Mode configuration (Batch 5.1).
    ///
    /// Enables writing to separate Redis namespace for parallel validation.
    pub shadow_mode: ShadowModeConfig,
}

impl FlashConfig {
    /// Load configuration from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the base configuration file
    /// * `environment` - Optional environment name (dev, staging, prod)
    ///
    /// # Returns
    ///
    /// Validated `FlashConfig` or error.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - File doesn't exist
    /// - YAML parsing fails
    /// - Validation fails
    pub fn load(config_path: &str, environment: Option<&str>) -> Result<Self, FlashError> {
        let path = Path::new(config_path);

        if !path.exists() {
            return Err(FlashError::ConfigFileNotFound {
                path: config_path.to_string(),
            });
        }

        // Read base config
        let yaml = fs::read_to_string(path).map_err(FlashError::IoError)?;

        let mut config: Self =
            serde_yaml::from_str(&yaml).map_err(|e| FlashError::ConfigError(e.to_string()))?;

        // Load environment-specific overlay if provided
        if let Some(env) = environment {
            let env_path = path.with_file_name(format!(
                "{}.{}.yaml",
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("flash"),
                env
            ));

            if env_path.exists() {
                let env_yaml = fs::read_to_string(&env_path).map_err(FlashError::IoError)?;
                let env_config: serde_yaml::Value = serde_yaml::from_str(&env_yaml)
                    .map_err(|e| FlashError::ConfigError(e.to_string()))?;

                // Merge environment config over base
                config = Self::merge_yaml(&config, &env_config)?;
            }
        }

        // Apply environment variable overrides
        config = Self::apply_env_overrides(config)?;

        // Validate
        let errors = config.validate_all();
        if !errors.is_empty() {
            return Err(FlashError::ConfigError(format!(
                "Configuration validation failed: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }

        Ok(config)
    }

    /// Load configuration from a YAML string.
    ///
    /// # Arguments
    ///
    /// * `yaml` - YAML configuration string
    ///
    /// # Returns
    ///
    /// Parsed `FlashConfig` (not validated - call `validate()` separately).
    ///
    /// # Errors
    ///
    /// Returns error if YAML parsing fails.
    pub fn from_yaml(yaml: &str) -> Result<Self, FlashError> {
        if yaml.trim().is_empty() {
            return Ok(Self::default());
        }

        serde_yaml::from_str(yaml)
            .map_err(|e| FlashError::ConfigError(format!("YAML parse error: {e}")))
    }

    /// Load configuration from a YAML string with environment variable overrides.
    ///
    /// # Arguments
    ///
    /// * `yaml` - YAML configuration string
    ///
    /// # Returns
    ///
    /// Parsed `FlashConfig` with env var overrides applied.
    ///
    /// # Errors
    ///
    /// Returns error if YAML parsing fails.
    pub fn from_yaml_with_env(yaml: &str) -> Result<Self, FlashError> {
        let config = Self::from_yaml(yaml)?;
        Self::apply_env_overrides(config)
    }

    /// Load configuration from environment variables only.
    ///
    /// # Returns
    ///
    /// `FlashConfig` with defaults overridden by environment variables.
    ///
    /// # Errors
    ///
    /// Returns error if environment variable parsing fails.
    pub fn from_env() -> Result<Self, FlashError> {
        Self::apply_env_overrides(Self::default())
    }

    /// Validate the configuration.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, `Err(errors)` if invalid.
    pub fn validate(&self) -> Result<(), Vec<ConfigValidationError>> {
        let errors = self.validate_all();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Collect all validation errors.
    fn validate_all(&self) -> Vec<ConfigValidationError> {
        let mut errors = Vec::new();

        errors.extend(self.websocket.validate());
        errors.extend(self.orderbook.validate());
        errors.extend(self.redis.validate());
        errors.extend(self.publisher.validate());
        errors.extend(self.logging.validate());
        errors.extend(self.metrics.validate());
        errors.extend(self.shadow_mode.validate());

        errors
    }

    /// Apply environment variable overrides to configuration.
    fn apply_env_overrides(mut config: Self) -> Result<Self, FlashError> {
        // WebSocket overrides
        if let Ok(val) = std::env::var("ASTRA_FLASH_WEBSOCKET_CONNECT_TIMEOUT_MS") {
            config.websocket.connect_timeout_ms = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_WEBSOCKET_CONNECT_TIMEOUT_MS: {val}"
                ))
            })?;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_WEBSOCKET_PING_INTERVAL_MS") {
            config.websocket.ping_interval_ms = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_WEBSOCKET_PING_INTERVAL_MS: {val}"
                ))
            })?;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_WEBSOCKET_MAX_RECONNECT_ATTEMPTS") {
            config.websocket.max_reconnect_attempts = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_WEBSOCKET_MAX_RECONNECT_ATTEMPTS: {val}"
                ))
            })?;
        }

        // Redis overrides
        if let Ok(val) = std::env::var("ASTRA_FLASH_REDIS_URL") {
            config.redis.url = val;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_REDIS_POOL_SIZE") {
            config.redis.pool_size = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_REDIS_POOL_SIZE: {val}"
                ))
            })?;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_REDIS_DATABASE") {
            config.redis.database = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_REDIS_DATABASE: {val}"
                ))
            })?;
        }

        // Publisher overrides
        if let Ok(val) = std::env::var("ASTRA_FLASH_PUBLISHER_FORMAT") {
            config.publisher.format = match val.to_lowercase().as_str() {
                "json" => SerializationFormat::Json,
                "bincode" => SerializationFormat::Bincode,
                "rkyv" => SerializationFormat::Rkyv,
                _ => {
                    return Err(FlashError::ConfigError(format!(
                        "Invalid value for ASTRA_FLASH_PUBLISHER_FORMAT: {val}"
                    )))
                }
            };
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_PUBLISHER_BACKPRESSURE_CAPACITY") {
            config.publisher.backpressure.capacity = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_PUBLISHER_BACKPRESSURE_CAPACITY: {val}"
                ))
            })?;
        }

        // Logging overrides
        if let Ok(val) = std::env::var("ASTRA_FLASH_LOGGING_LEVEL") {
            config.logging.level = match val.to_lowercase().as_str() {
                "trace" => LogLevel::Trace,
                "debug" => LogLevel::Debug,
                "info" => LogLevel::Info,
                "warn" => LogLevel::Warn,
                "error" => LogLevel::Error,
                _ => {
                    return Err(FlashError::ConfigError(format!(
                        "Invalid value for ASTRA_FLASH_LOGGING_LEVEL: {val}"
                    )))
                }
            };
        }

        // Metrics overrides
        if let Ok(val) = std::env::var("ASTRA_FLASH_METRICS_ENABLED") {
            config.metrics.enabled = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_METRICS_ENABLED: {val}"
                ))
            })?;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_METRICS_PORT") {
            config.metrics.port = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_METRICS_PORT: {val}"
                ))
            })?;
        }

        // Shadow Mode overrides (Batch 5.1)
        if let Ok(val) = std::env::var("ASTRA_FLASH_SHADOW_MODE_ENABLED") {
            config.shadow_mode.enabled = val.parse().map_err(|_| {
                FlashError::ConfigError(format!(
                    "Invalid value for ASTRA_FLASH_SHADOW_MODE_ENABLED: {val}"
                ))
            })?;
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_SHADOW_MODE_NAMESPACE") {
            config.shadow_mode.namespace = val;
        }

        // OANDA credentials (Sprint E-2 migration from YAML-baked to env-var primary read)
        if let Ok(val) = std::env::var("ASTRA_FLASH_OANDA_API_KEY") {
            config.exchanges.oanda.api_key = Some(val);
        }
        if let Ok(val) = std::env::var("ASTRA_FLASH_OANDA_ACCOUNT_ID") {
            config.exchanges.oanda.account_id = Some(val);
        }

        Ok(config)
    }

    /// Merge a YAML overlay into the configuration.
    fn merge_yaml(base: &Self, overlay: &serde_yaml::Value) -> Result<Self, FlashError> {
        // Serialize base to Value
        let mut base_value = serde_yaml::to_value(base)
            .map_err(|e| FlashError::ConfigError(format!("Serialization error: {e}")))?;

        // Deep merge
        Self::deep_merge(&mut base_value, overlay);

        // Deserialize back
        serde_yaml::from_value(base_value)
            .map_err(|e| FlashError::ConfigError(format!("Merge error: {e}")))
    }

    /// Deep merge two YAML values.
    fn deep_merge(base: &mut serde_yaml::Value, overlay: &serde_yaml::Value) {
        match (base, overlay) {
            (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(overlay_map)) => {
                for (key, value) in overlay_map {
                    if let Some(base_value) = base_map.get_mut(key) {
                        Self::deep_merge(base_value, value);
                    } else {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
            (base, overlay) => {
                *base = overlay.clone();
            }
        }
    }
}

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creates_valid_instance() {
        let config = FlashConfig::default();
        assert_eq!(config.websocket.connect_timeout_ms, 5000);
        assert_eq!(config.redis.url, "redis://127.0.0.1:6379");
    }

    #[test]
    fn test_serialization_format_display() {
        assert_eq!(
            serde_yaml::to_string(&SerializationFormat::Json)
                .unwrap()
                .trim(),
            "json"
        );
    }

    #[test]
    fn test_gap_handling_default() {
        assert_eq!(GapHandling::default(), GapHandling::Snapshot);
    }

    #[test]
    fn test_websocket_duration_helpers() {
        let config = WebSocketConfig::default();
        assert_eq!(config.connect_timeout().as_millis(), 5000);
        assert_eq!(config.ping_interval().as_millis(), 30000);
    }

    #[test]
    fn test_empty_yaml_returns_defaults() {
        let config = FlashConfig::from_yaml("").unwrap();
        assert_eq!(config.websocket.connect_timeout_ms, 5000);
    }

    #[test]
    fn test_partial_yaml_merges_with_defaults() {
        let yaml = r#"
            websocket:
                connect_timeout_ms: 10000
        "#;
        let config = FlashConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.websocket.connect_timeout_ms, 10000);
        assert_eq!(config.websocket.ping_interval_ms, 30000); // Default
    }

    #[test]
    fn test_validation_catches_invalid_timeout() {
        let mut config = FlashConfig::default();
        config.websocket.connect_timeout_ms = 500; // Below minimum
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_catches_empty_topic_prefix() {
        let mut config = FlashConfig::default();
        config.publisher.topic_prefix = String::new();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_redis_url_parsing() {
        let config = RedisConfig::default();
        let url = config.parsed_url().unwrap();
        assert_eq!(url.scheme(), "redis");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(6379));
    }
}
