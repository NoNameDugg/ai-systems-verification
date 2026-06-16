//! Unit tests for Flash error handling.
//!
//! # Test Categories
//!
//! 1. Display trait tests (error message formatting)
//! 2. From trait tests (error conversions)
//! 3. Helper method tests (is_recoverable, requires_snapshot, etc.)
//! 4. Thread safety tests (Send + Sync bounds)
//! 5. Error code tests
//! 6. Severity classification tests
//! 7. Edge case tests
//!
//! # TDI Methodology
//!
//! These tests were written BEFORE the implementation to ensure
//! proper test-driven development.

use astra_flash::core::error::{ErrorSeverity, FlashError, FlashResult};
use std::io;

// =============================================================================
// DISPLAY TRAIT TESTS
// =============================================================================

/// Test ConnectionFailed error message format.
#[test]
fn test_connection_failed_display() {
    let err = FlashError::ConnectionFailed("wss://example.com".into());
    assert_eq!(
        err.to_string(),
        "WebSocket connection failed: wss://example.com"
    );
}

/// Test Disconnected error message format with struct fields.
#[test]
fn test_disconnected_display() {
    let err = FlashError::Disconnected {
        reason: "Server closed connection".into(),
        will_retry: true,
    };
    assert_eq!(
        err.to_string(),
        "WebSocket disconnected: Server closed connection"
    );
}

/// Test ConnectionTimeout error message format.
#[test]
fn test_connection_timeout_display() {
    let err = FlashError::ConnectionTimeout { timeout_ms: 5000 };
    assert_eq!(err.to_string(), "Connection timeout after 5000ms");
}

/// Test SequenceGap error message format.
#[test]
fn test_sequence_gap_display() {
    let err = FlashError::SequenceGap {
        expected: 100,
        actual: 105,
    };
    assert_eq!(err.to_string(), "Sequence gap: expected 100, got 105");
}

/// Test ConfigError error message format.
#[test]
fn test_config_error_display() {
    let err = FlashError::ConfigError("Invalid port number".into());
    assert_eq!(err.to_string(), "Invalid configuration: Invalid port number");
}

/// Test PublishFailed error message format.
#[test]
fn test_publish_failed_display() {
    let err = FlashError::PublishFailed {
        attempts: 3,
        reason: "Connection refused".into(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to publish to Redis after 3 attempts: Connection refused"
    );
}

/// Test AuthenticationFailed error message format.
#[test]
fn test_authentication_failed_display() {
    let err = FlashError::AuthenticationFailed {
        exchange: "deribit".into(),
        reason: "Invalid API key".into(),
    };
    assert!(err.to_string().contains("deribit"));
    assert!(err.to_string().contains("Invalid API key"));
}

/// Test RateLimited error message format.
#[test]
fn test_rate_limited_display() {
    let err = FlashError::RateLimited {
        exchange: "binance".into(),
        retry_after_ms: 1000,
    };
    assert!(err.to_string().contains("binance"));
    assert!(err.to_string().contains("1000"));
}

// =============================================================================
// FROM TRAIT TESTS
// =============================================================================

/// Test conversion from serde_json::Error to FlashError.
#[test]
fn test_parse_error_from_conversion() {
    let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
    let flash_err: FlashError = json_err.into();
    assert!(matches!(flash_err, FlashError::ParseError(_)));
    assert!(flash_err.to_string().contains("Failed to parse"));
}

/// Test conversion from std::io::Error to FlashError.
#[test]
fn test_io_error_from_conversion() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let flash_err: FlashError = io_err.into();
    assert!(matches!(flash_err, FlashError::IoError(_)));
    assert!(flash_err.to_string().contains("I/O error"));
}

// =============================================================================
// IS_RECOVERABLE TESTS
// =============================================================================

/// Test that all expected errors are classified as recoverable.
#[test]
fn test_is_recoverable_true_cases() {
    // Network errors should be recoverable
    assert!(FlashError::ConnectionFailed("test".into()).is_recoverable());
    assert!(FlashError::Disconnected {
        reason: "test".into(),
        will_retry: true
    }
    .is_recoverable());
    assert!(FlashError::ConnectionTimeout { timeout_ms: 5000 }.is_recoverable());
    assert!(FlashError::ReadTimeout { timeout_ms: 5000 }.is_recoverable());

    // Rate limiting is recoverable (wait and retry)
    assert!(FlashError::RateLimited {
        exchange: "test".into(),
        retry_after_ms: 1000
    }
    .is_recoverable());

    // Sequence gaps are recoverable (request snapshot)
    assert!(FlashError::SequenceGap {
        expected: 1,
        actual: 2
    }
    .is_recoverable());
}

/// Test that all expected errors are classified as NOT recoverable.
#[test]
fn test_is_recoverable_false_cases() {
    // Config errors are not recoverable - require restart/fix
    assert!(!FlashError::ConfigError("test".into()).is_recoverable());
    assert!(!FlashError::ConfigFileNotFound { path: "test".into() }.is_recoverable());

    // Internal errors are not recoverable - indicate bugs
    assert!(!FlashError::InternalError("test".into()).is_recoverable());

    // Authentication failures are not recoverable without new credentials
    assert!(!FlashError::AuthenticationFailed {
        exchange: "test".into(),
        reason: "bad key".into()
    }
    .is_recoverable());

    // Validation errors indicate bad data
    assert!(!FlashError::InvalidPrice {
        price: -1.0,
        reason: "negative".into()
    }
    .is_recoverable());
}

// =============================================================================
// REQUIRES_SNAPSHOT TESTS
// =============================================================================

/// Test that errors requiring snapshot are correctly identified.
#[test]
fn test_requires_snapshot_true_cases() {
    assert!(FlashError::SequenceGap {
        expected: 1,
        actual: 5
    }
    .requires_snapshot());

    assert!(FlashError::InvalidBookState("corrupted".into()).requires_snapshot());
}

/// Test that errors NOT requiring snapshot are correctly identified.
#[test]
fn test_requires_snapshot_false_cases() {
    assert!(!FlashError::ConnectionFailed("test".into()).requires_snapshot());
    assert!(!FlashError::ConfigError("test".into()).requires_snapshot());
    assert!(!FlashError::InternalError("test".into()).requires_snapshot());
    assert!(!FlashError::RateLimited {
        exchange: "test".into(),
        retry_after_ms: 1000
    }
    .requires_snapshot());
}

// =============================================================================
// THREAD SAFETY TESTS
// =============================================================================

/// Verify FlashError is Send (can be sent across threads).
#[test]
fn test_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<FlashError>();
}

/// Verify FlashError is Sync (can be shared across threads).
#[test]
fn test_error_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<FlashError>();
}

/// Test that FlashError can actually be sent across threads.
#[test]
fn test_error_send_across_thread() {
    let err = FlashError::ConnectionFailed("test".into());
    let handle = std::thread::spawn(move || {
        assert!(err.is_recoverable());
    });
    handle.join().unwrap();
}

// =============================================================================
// ERROR CODE TESTS
// =============================================================================

/// Test that error codes are correctly assigned.
#[test]
fn test_error_code_assignment() {
    // Network errors: 100-199
    assert!(
        FlashError::ConnectionFailed("test".into()).error_code() >= 100
            && FlashError::ConnectionFailed("test".into()).error_code() < 200
    );

    // Config errors: 500-599
    assert!(
        FlashError::ConfigError("test".into()).error_code() >= 500
            && FlashError::ConfigError("test".into()).error_code() < 600
    );
}

/// Test that all error codes are unique.
#[test]
fn test_error_code_uniqueness() {
    let errors: Vec<FlashError> = vec![
        FlashError::ConnectionFailed("test".into()),
        FlashError::Disconnected {
            reason: "test".into(),
            will_retry: true,
        },
        FlashError::ConnectionTimeout { timeout_ms: 5000 },
        FlashError::ReadTimeout { timeout_ms: 5000 },
        FlashError::UnknownMessageFormat("test".into()),
        FlashError::SequenceGap {
            expected: 1,
            actual: 2,
        },
        FlashError::InvalidBookState("test".into()),
        FlashError::PublishFailed {
            attempts: 1,
            reason: "test".into(),
        },
        FlashError::ConfigError("test".into()),
        FlashError::ConfigFileNotFound { path: "test".into() },
        FlashError::InternalError("test".into()),
        FlashError::AuthenticationFailed {
            exchange: "test".into(),
            reason: "test".into(),
        },
        FlashError::RateLimited {
            exchange: "test".into(),
            retry_after_ms: 1000,
        },
        FlashError::InvalidPrice {
            price: 0.0,
            reason: "test".into(),
        },
        FlashError::InvalidQuantity {
            quantity: "0".into(),
            reason: "test".into(),
        },
        FlashError::ChannelClosed {
            channel_name: "test".into(),
        },
        FlashError::SerializationError("test".into()),
    ];

    let codes: Vec<u32> = errors.iter().map(|e| e.error_code()).collect();
    let unique_codes: std::collections::HashSet<u32> = codes.iter().cloned().collect();

    assert_eq!(
        codes.len(),
        unique_codes.len(),
        "Error codes must be unique"
    );
}

// =============================================================================
// SEVERITY TESTS
// =============================================================================

/// Test severity classification for different error types.
#[test]
fn test_severity_classification() {
    // Rate limiting is a warning - temporary
    assert_eq!(
        FlashError::RateLimited {
            exchange: "test".into(),
            retry_after_ms: 1000
        }
        .severity(),
        ErrorSeverity::Warning
    );

    // Connection failures are errors - recoverable but significant
    assert_eq!(
        FlashError::ConnectionFailed("test".into()).severity(),
        ErrorSeverity::Error
    );

    // Sequence gaps are errors - require action
    assert_eq!(
        FlashError::SequenceGap {
            expected: 1,
            actual: 5
        }
        .severity(),
        ErrorSeverity::Error
    );

    // Config errors are critical - system misconfigured
    assert_eq!(
        FlashError::ConfigError("test".into()).severity(),
        ErrorSeverity::Critical
    );

    // Internal errors are fatal - indicate bugs
    assert_eq!(
        FlashError::InternalError("test".into()).severity(),
        ErrorSeverity::Fatal
    );
}

// =============================================================================
// DEBUG TRAIT TESTS
// =============================================================================

/// Test that Debug trait is implemented and produces useful output.
#[test]
fn test_error_debug_format() {
    let err = FlashError::SequenceGap {
        expected: 100,
        actual: 105,
    };
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("SequenceGap"));
    assert!(debug_str.contains("100"));
    assert!(debug_str.contains("105"));
}

// =============================================================================
// RESULT TYPE ERGONOMICS TESTS
// =============================================================================

/// Test that FlashResult works with ? operator.
#[test]
fn test_result_with_question_mark() {
    fn inner_function() -> FlashResult<i32> {
        Err(FlashError::ConfigError("test".into()))
    }

    fn outer_function() -> FlashResult<i32> {
        let _value = inner_function()?;
        Ok(42)
    }

    let result = outer_function();
    assert!(result.is_err());
}

/// Test that FlashResult can be matched.
#[test]
fn test_result_pattern_matching() {
    let result: FlashResult<i32> = Err(FlashError::ConnectionFailed("test".into()));

    match result {
        Ok(_) => panic!("Expected error"),
        Err(FlashError::ConnectionFailed(msg)) => {
            assert_eq!(msg, "test");
        }
        Err(_) => panic!("Expected ConnectionFailed"),
    }
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

/// Test handling of empty string in error messages.
#[test]
fn test_empty_string_errors() {
    let err = FlashError::ConnectionFailed(String::new());
    assert_eq!(err.to_string(), "WebSocket connection failed: ");

    let err = FlashError::ConfigError(String::new());
    assert_eq!(err.to_string(), "Invalid configuration: ");
}

/// Test handling of very long error messages.
#[test]
fn test_long_error_message() {
    let long_msg = "x".repeat(10000);
    let err = FlashError::InternalError(long_msg.clone());
    assert!(err.to_string().contains(&long_msg));
}

/// Test error with special characters in message.
#[test]
fn test_special_characters_in_message() {
    let msg = "Error with special chars: \n\t\"'\\";
    let err = FlashError::InternalError(msg.into());
    assert!(err.to_string().contains(msg));
}

// =============================================================================
// VALIDATION ERROR TESTS
// =============================================================================

/// Test InvalidPrice error.
#[test]
fn test_invalid_price_error() {
    let err = FlashError::InvalidPrice {
        price: -1.0,
        reason: "Price cannot be negative".into(),
    };
    assert!(err.to_string().contains("-1"));
    assert!(err.to_string().contains("negative"));
}

/// Test InvalidQuantity error.
#[test]
fn test_invalid_quantity_error() {
    let err = FlashError::InvalidQuantity {
        quantity: "-5.5".into(),
        reason: "Quantity cannot be negative".into(),
    };
    assert!(err.to_string().contains("-5.5"));
    assert!(err.to_string().contains("negative"));
}

// =============================================================================
// CHANNEL ERROR TESTS
// =============================================================================

/// Test ChannelClosed error.
#[test]
fn test_channel_closed_error() {
    let err = FlashError::ChannelClosed {
        channel_name: "market_data".into(),
    };
    assert!(err.to_string().contains("market_data"));
    assert!(!err.is_recoverable());
}

// =============================================================================
// SERIALIZATION ERROR TESTS
// =============================================================================

/// Test SerializationError.
#[test]
fn test_serialization_error() {
    let err = FlashError::SerializationError("Failed to encode as bincode".into());
    assert!(err.to_string().contains("bincode"));
}

// =============================================================================
// EXCHANGE ERROR TESTS
// =============================================================================

/// Test ExchangeError.
#[test]
fn test_exchange_error() {
    let err = FlashError::ExchangeError {
        exchange: "deribit".into(),
        code: 10001,
        message: "Instrument not found".into(),
    };
    assert!(err.to_string().contains("deribit"));
    assert!(err.to_string().contains("10001"));
    assert!(err.to_string().contains("Instrument not found"));
}

// =============================================================================
// READ TIMEOUT ERROR TESTS (Batch 1.3: Resilience)
// =============================================================================

/// Test ReadTimeout error message format.
#[test]
fn test_read_timeout_display() {
    let err = FlashError::ReadTimeout { timeout_ms: 5000 };
    assert_eq!(err.to_string(), "Read timeout: no data received for 5000ms");
}

/// Test ReadTimeout error code.
#[test]
fn test_read_timeout_error_code() {
    let err = FlashError::ReadTimeout { timeout_ms: 5000 };
    assert_eq!(err.error_code(), 106, "ReadTimeout should have error code 106");
}

/// Test ReadTimeout is recoverable.
#[test]
fn test_read_timeout_is_recoverable() {
    let err = FlashError::ReadTimeout { timeout_ms: 5000 };
    assert!(err.is_recoverable(), "ReadTimeout should be recoverable");
}

/// Test ReadTimeout severity is Error (recoverable with action).
#[test]
fn test_read_timeout_severity() {
    let err = FlashError::ReadTimeout { timeout_ms: 5000 };
    assert_eq!(
        err.severity(),
        ErrorSeverity::Error,
        "ReadTimeout should have Error severity"
    );
}

/// Test ReadTimeout does not require snapshot.
#[test]
fn test_read_timeout_does_not_require_snapshot() {
    let err = FlashError::ReadTimeout { timeout_ms: 5000 };
    assert!(
        !err.requires_snapshot(),
        "ReadTimeout should not require snapshot"
    );
}
