//! Error handling for Flash.
//!
//! This module provides comprehensive error types for all Flash operations.
//! Every error is designed to be:
//!
//! - **Typed**: Specific variants for each error category
//! - **Actionable**: Clear messages describing what went wrong
//! - **Recoverable**: Self-identifies whether retry is possible
//! - **Traceable**: Unique error codes for logging and monitoring
//!
//! # Design Philosophy
//!
//! - Use `thiserror` for custom error types
//! - Use `anyhow` for error propagation with context
//! - Every error should be actionable (clear what went wrong and how to fix)
//! - Errors are Send + Sync for use across async tasks
//!
//! # Error Categories
//!
//! | Category | Code Range | Description |
//! |----------|------------|-------------|
//! | Network | 100-199 | WebSocket and connection errors |
//! | Parsing | 200-299 | JSON and message parsing errors |
//! | OrderBook | 300-399 | Order book state errors |
//! | Redis | 400-499 | Redis connection and publishing |
//! | Configuration | 500-599 | Config loading and validation |
//! | Validation | 600-699 | Data validation errors |
//! | Internal | 900-999 | Internal and generic errors |
//!
//! # Example
//!
//! ```
//! use astra_flash::core::error::{FlashError, FlashResult};
//!
//! fn connect(url: &str) -> FlashResult<()> {
//!     if url.is_empty() {
//!         return Err(FlashError::ConfigError("URL cannot be empty".into()));
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Recovery Example
//!
//! ```
//! use astra_flash::core::error::{FlashError, ErrorSeverity};
//!
//! fn handle_error(err: &FlashError) {
//!     if err.is_recoverable() {
//!         println!("Will retry: {}", err);
//!     } else {
//!         println!("Fatal error: {}", err);
//!     }
//!
//!     match err.severity() {
//!         ErrorSeverity::Warning => println!("Warning level"),
//!         ErrorSeverity::Error => println!("Error level"),
//!         ErrorSeverity::Critical => println!("Critical level"),
//!         ErrorSeverity::Fatal => println!("Fatal level"),
//!     }
//! }
//! ```

use thiserror::Error;

// =============================================================================
// SEVERITY CLASSIFICATION
// =============================================================================

/// Error severity levels for classification and alerting.
///
/// Severity helps determine appropriate response actions:
///
/// | Level | Response | Example |
/// |-------|----------|---------|
/// | Warning | Log, monitor | Rate limited |
/// | Error | Retry, alert | Connection failed |
/// | Critical | Escalate, degrade | Auth failed |
/// | Fatal | Shutdown | Config invalid |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorSeverity {
    /// Warning - operation can continue, temporary issue.
    ///
    /// Examples: rate limiting, temporary throttling.
    Warning,

    /// Error - operation failed but system can recover.
    ///
    /// Examples: connection failed, sequence gap detected.
    Error,

    /// Critical - system in degraded state, requires attention.
    ///
    /// Examples: authentication failed, exchange unavailable.
    Critical,

    /// Fatal - system cannot continue normal operation.
    ///
    /// Examples: configuration invalid, internal error.
    Fatal,
}

impl std::fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
            Self::Critical => write!(f, "CRITICAL"),
            Self::Fatal => write!(f, "FATAL"),
        }
    }
}

// =============================================================================
// RESULT TYPE ALIASES
// =============================================================================

/// Result type alias for Flash operations.
///
/// Use this for all fallible operations in the library.
///
/// # Example
///
/// ```
/// use astra_flash::core::error::{FlashError, FlashResult};
///
/// fn validate_price(price: f64) -> FlashResult<()> {
///     if price < 0.0 {
///         return Err(FlashError::InvalidPrice {
///             price,
///             reason: "Price cannot be negative".into(),
///         });
///     }
///     Ok(())
/// }
/// ```
pub type FlashResult<T> = Result<T, FlashError>;

/// Result type alias for order book operations.
pub type BookResult<T> = Result<T, FlashError>;

/// Result type alias for network operations.
pub type NetworkResult<T> = Result<T, FlashError>;

// =============================================================================
// FLASH ERROR ENUM
// =============================================================================

/// Errors that can occur in Flash.
///
/// This enum covers all error categories:
///
/// - **Network**: WebSocket connection and communication errors
/// - **Parsing**: JSON deserialization errors
/// - **OrderBook**: Order book state errors
/// - **Redis**: Redis connection and publishing errors
/// - **Configuration**: Configuration loading and validation errors
/// - **Validation**: Data validation errors
/// - **Internal**: Internal errors indicating bugs
///
/// # Thread Safety
///
/// `FlashError` is both `Send` and `Sync`, making it safe to use
/// across async task boundaries and threads.
///
/// # Example
///
/// ```
/// use astra_flash::core::error::FlashError;
///
/// let err = FlashError::SequenceGap { expected: 100, actual: 105 };
/// assert!(err.requires_snapshot());
/// assert!(err.is_recoverable());
/// ```
#[derive(Debug, Error)]
pub enum FlashError {
    // =========================================================================
    // Network Errors (100-199)
    // =========================================================================
    /// WebSocket connection failed.
    ///
    /// # Causes
    /// - Invalid URL
    /// - Network unreachable
    /// - TLS handshake failure
    /// - DNS resolution failure
    ///
    /// # Recovery
    /// Automatic reconnection will be attempted with exponential backoff.
    ///
    /// # Error Code
    /// 101
    #[error("WebSocket connection failed: {0}")]
    ConnectionFailed(String),

    /// WebSocket disconnected unexpectedly.
    ///
    /// # Fields
    /// - `reason`: Description of why disconnection occurred
    /// - `will_retry`: Whether automatic reconnection will be attempted
    ///
    /// # Error Code
    /// 102
    #[error("WebSocket disconnected: {reason}")]
    Disconnected {
        /// Reason for disconnection.
        reason: String,
        /// Whether reconnection will be attempted.
        will_retry: bool,
    },

    /// Connection timeout.
    ///
    /// # Error Code
    /// 103
    #[error("Connection timeout after {timeout_ms}ms")]
    ConnectionTimeout {
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// Authentication failed with exchange.
    ///
    /// # Causes
    /// - Invalid API key
    /// - Invalid API secret
    /// - Expired credentials
    /// - IP not whitelisted
    ///
    /// # Recovery
    /// Not recoverable - requires new credentials.
    ///
    /// # Error Code
    /// 104
    #[error("Authentication failed for {exchange}: {reason}")]
    AuthenticationFailed {
        /// Exchange that rejected authentication.
        exchange: String,
        /// Reason for authentication failure.
        reason: String,
    },

    /// Rate limited by exchange.
    ///
    /// # Recovery
    /// Wait for `retry_after_ms` milliseconds before retrying.
    ///
    /// # Error Code
    /// 105
    #[error("Rate limited by {exchange}, retry after {retry_after_ms}ms")]
    RateLimited {
        /// Exchange that rate limited us.
        exchange: String,
        /// Time to wait before retry in milliseconds.
        retry_after_ms: u64,
    },

    /// Read timeout - no data received for configured duration.
    ///
    /// # Causes
    /// - Connection silently dropped (no TCP RST)
    /// - Network partition
    /// - Exchange stopped sending data
    /// - Firewall blocking traffic
    ///
    /// # Recovery
    /// Automatic reconnection will be attempted with exponential backoff.
    /// This error indicates the connection is dead and should be replaced.
    ///
    /// # Error Code
    /// 106
    #[error("Read timeout: no data received for {timeout_ms}ms")]
    ReadTimeout {
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    // =========================================================================
    // Parsing Errors (200-299)
    // =========================================================================
    /// Failed to parse JSON message.
    ///
    /// # Error Code
    /// 201
    #[error("Failed to parse message: {0}")]
    ParseError(#[from] serde_json::Error),

    /// Unknown message format from exchange.
    ///
    /// # Error Code
    /// 202
    #[error("Unknown message format: {0}")]
    UnknownMessageFormat(String),

    /// Serialization error (bincode, rkyv, etc.).
    ///
    /// # Error Code
    /// 203
    #[error("Serialization error: {0}")]
    SerializationError(String),

    // =========================================================================
    // Order Book Errors (300-399)
    // =========================================================================
    /// Sequence gap detected in order book updates.
    ///
    /// This indicates missed messages and requires a snapshot request.
    ///
    /// # Recovery
    /// Request a full snapshot from the exchange.
    ///
    /// # Error Code
    /// 301
    #[error("Sequence gap: expected {expected}, got {actual}")]
    SequenceGap {
        /// Expected sequence number.
        expected: u64,
        /// Actual sequence number received.
        actual: u64,
    },

    /// Order book is in an invalid state.
    ///
    /// # Error Code
    /// 302
    #[error("Invalid order book state: {0}")]
    InvalidBookState(String),

    // =========================================================================
    // Redis Errors (400-499)
    // =========================================================================
    /// Redis connection or command failed.
    ///
    /// # Error Code
    /// 401
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    /// Redis publish failed after retries.
    ///
    /// # Error Code
    /// 402
    #[error("Failed to publish to Redis after {attempts} attempts: {reason}")]
    PublishFailed {
        /// Number of attempts made.
        attempts: u32,
        /// Reason for failure.
        reason: String,
    },

    // =========================================================================
    // Configuration Errors (500-599)
    // =========================================================================
    /// Configuration is invalid.
    ///
    /// # Error Code
    /// 501
    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    /// Configuration file not found.
    ///
    /// # Error Code
    /// 502
    #[error("Configuration file not found: {path}")]
    ConfigFileNotFound {
        /// Path to the configuration file.
        path: String,
    },

    // =========================================================================
    // Validation Errors (600-699)
    // =========================================================================
    /// Invalid price value.
    ///
    /// # Causes
    /// - Negative price
    /// - NaN or Infinity
    /// - Zero price (if not allowed)
    ///
    /// # Error Code
    /// 601
    #[error("Invalid price {price}: {reason}")]
    InvalidPrice {
        /// The invalid price value.
        price: f64,
        /// Reason why the price is invalid.
        reason: String,
    },

    /// Invalid quantity value.
    ///
    /// # Causes
    /// - Negative quantity
    /// - Zero quantity (if not allowed)
    /// - Exceeds maximum
    ///
    /// # Error Code
    /// 602
    #[error("Invalid quantity {quantity}: {reason}")]
    InvalidQuantity {
        /// The invalid quantity value (as string for Decimal).
        quantity: String,
        /// Reason why the quantity is invalid.
        reason: String,
    },

    /// Exchange-specific error.
    ///
    /// # Error Code
    /// 603
    #[error("Exchange error from {exchange} (code {code}): {message}")]
    ExchangeError {
        /// Exchange name.
        exchange: String,
        /// Exchange-specific error code.
        code: i32,
        /// Error message from exchange.
        message: String,
    },

    // =========================================================================
    // Internal Errors (900-999)
    // =========================================================================
    /// Generic I/O error.
    ///
    /// # Error Code
    /// 901
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Internal channel closed unexpectedly.
    ///
    /// # Error Code
    /// 902
    #[error("Channel closed: {channel_name}")]
    ChannelClosed {
        /// Name of the closed channel.
        channel_name: String,
    },

    /// Internal error (should not happen in normal operation).
    ///
    /// This indicates a bug in the code and should be reported.
    ///
    /// # Error Code
    /// 999
    #[error("Internal error: {0}")]
    InternalError(String),
}

// =============================================================================
// HELPER METHODS
// =============================================================================

impl FlashError {
    /// Returns true if this error is recoverable.
    ///
    /// Recoverable errors may resolve themselves with retry or reconnection.
    /// Non-recoverable errors require manual intervention or configuration changes.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::error::FlashError;
    ///
    /// let err = FlashError::ConnectionFailed("timeout".into());
    /// assert!(err.is_recoverable());
    ///
    /// let err = FlashError::ConfigError("invalid port".into());
    /// assert!(!err.is_recoverable());
    /// ```
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed(_)
                | Self::Disconnected { .. }
                | Self::ConnectionTimeout { .. }
                | Self::RateLimited { .. }
                | Self::ReadTimeout { .. }
                | Self::SequenceGap { .. }
                | Self::RedisError(_)
                | Self::PublishFailed { .. }
                | Self::ParseError(_)
                | Self::UnknownMessageFormat(_)
        )
    }

    /// Alias for `is_recoverable()` - more intuitive in some contexts.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::error::FlashError;
    ///
    /// let err = FlashError::RateLimited { exchange: "binance".into(), retry_after_ms: 1000 };
    /// if err.is_retryable() {
    ///     // Wait and retry
    /// }
    /// ```
    #[must_use]
    #[inline]
    pub fn is_retryable(&self) -> bool {
        self.is_recoverable()
    }

    /// Returns true if this error requires a full order book snapshot.
    ///
    /// Some errors indicate that the local order book state may be inconsistent
    /// with the exchange, requiring a full snapshot to resynchronize.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::error::FlashError;
    ///
    /// let err = FlashError::SequenceGap { expected: 100, actual: 105 };
    /// assert!(err.requires_snapshot());
    /// ```
    #[must_use]
    pub const fn requires_snapshot(&self) -> bool {
        matches!(
            self,
            Self::SequenceGap { .. } | Self::InvalidBookState(_)
        )
    }

    /// Returns a unique error code for logging and monitoring.
    ///
    /// Error codes are organized by category:
    /// - 100-199: Network errors
    /// - 200-299: Parsing errors
    /// - 300-399: OrderBook errors
    /// - 400-499: Redis errors
    /// - 500-599: Configuration errors
    /// - 600-699: Validation errors
    /// - 900-999: Internal errors
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::error::FlashError;
    ///
    /// let err = FlashError::ConnectionFailed("test".into());
    /// assert_eq!(err.error_code(), 101);
    /// ```
    #[must_use]
    pub const fn error_code(&self) -> u32 {
        match self {
            // Network errors (100-199)
            Self::ConnectionFailed(_) => 101,
            Self::Disconnected { .. } => 102,
            Self::ConnectionTimeout { .. } => 103,
            Self::AuthenticationFailed { .. } => 104,
            Self::RateLimited { .. } => 105,
            Self::ReadTimeout { .. } => 106,

            // Parsing errors (200-299)
            Self::ParseError(_) => 201,
            Self::UnknownMessageFormat(_) => 202,
            Self::SerializationError(_) => 203,

            // OrderBook errors (300-399)
            Self::SequenceGap { .. } => 301,
            Self::InvalidBookState(_) => 302,

            // Redis errors (400-499)
            Self::RedisError(_) => 401,
            Self::PublishFailed { .. } => 402,

            // Configuration errors (500-599)
            Self::ConfigError(_) => 501,
            Self::ConfigFileNotFound { .. } => 502,

            // Validation errors (600-699)
            Self::InvalidPrice { .. } => 601,
            Self::InvalidQuantity { .. } => 602,
            Self::ExchangeError { .. } => 603,

            // Internal errors (900-999)
            Self::IoError(_) => 901,
            Self::ChannelClosed { .. } => 902,
            Self::InternalError(_) => 999,
        }
    }

    /// Returns the severity level of this error.
    ///
    /// Severity levels help determine appropriate response actions:
    ///
    /// - `Warning`: Log and monitor, operation can continue
    /// - `Error`: Retry or take corrective action
    /// - `Critical`: Escalate, system in degraded state
    /// - `Fatal`: Cannot continue, requires intervention
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::core::error::{FlashError, ErrorSeverity};
    ///
    /// let err = FlashError::InternalError("bug".into());
    /// assert_eq!(err.severity(), ErrorSeverity::Fatal);
    /// ```
    #[must_use]
    pub const fn severity(&self) -> ErrorSeverity {
        match self {
            // Warnings - temporary issues
            Self::RateLimited { .. } => ErrorSeverity::Warning,

            // Errors - recoverable with action
            Self::ConnectionFailed(_)
            | Self::Disconnected { .. }
            | Self::ConnectionTimeout { .. }
            | Self::ReadTimeout { .. }
            | Self::SequenceGap { .. }
            | Self::ParseError(_)
            | Self::UnknownMessageFormat(_)
            | Self::SerializationError(_)
            | Self::RedisError(_)
            | Self::PublishFailed { .. }
            | Self::ExchangeError { .. } => ErrorSeverity::Error,

            // Critical - system degraded
            Self::AuthenticationFailed { .. }
            | Self::InvalidBookState(_)
            | Self::ConfigError(_)
            | Self::ConfigFileNotFound { .. }
            | Self::InvalidPrice { .. }
            | Self::InvalidQuantity { .. } => ErrorSeverity::Critical,

            // Fatal - cannot continue
            Self::IoError(_)
            | Self::ChannelClosed { .. }
            | Self::InternalError(_) => ErrorSeverity::Fatal,
        }
    }
}

// =============================================================================
// INLINE TESTS (for quick validation during development)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_failed_is_recoverable() {
        let err = FlashError::ConnectionFailed("test".into());
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_config_error_is_not_recoverable() {
        let err = FlashError::ConfigError("test".into());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_sequence_gap_requires_snapshot() {
        let err = FlashError::SequenceGap {
            expected: 100,
            actual: 105,
        };
        assert!(err.requires_snapshot());
    }

    #[test]
    fn test_error_display() {
        let err = FlashError::SequenceGap {
            expected: 100,
            actual: 105,
        };
        assert_eq!(err.to_string(), "Sequence gap: expected 100, got 105");
    }

    #[test]
    fn test_error_code_ranges() {
        // Network: 100-199
        assert!((100..200).contains(&FlashError::ConnectionFailed("".into()).error_code()));

        // Parsing: 200-299
        let json_err = serde_json::from_str::<i32>("bad").unwrap_err();
        assert!((200..300).contains(&FlashError::ParseError(json_err).error_code()));

        // Config: 500-599
        assert!((500..600).contains(&FlashError::ConfigError("".into()).error_code()));
    }

    #[test]
    fn test_severity_levels() {
        assert_eq!(
            FlashError::RateLimited {
                exchange: "".into(),
                retry_after_ms: 0
            }
            .severity(),
            ErrorSeverity::Warning
        );
        assert_eq!(
            FlashError::ConnectionFailed("".into()).severity(),
            ErrorSeverity::Error
        );
        assert_eq!(
            FlashError::ConfigError("".into()).severity(),
            ErrorSeverity::Critical
        );
        assert_eq!(
            FlashError::InternalError("".into()).severity(),
            ErrorSeverity::Fatal
        );
    }

    #[test]
    fn test_is_retryable_alias() {
        let err = FlashError::ConnectionFailed("test".into());
        assert_eq!(err.is_retryable(), err.is_recoverable());
    }
}
