//! Error types for codec operations.
//!
//! This module defines all error types that can occur during
//! message encoding and decoding.

use std::fmt;

/// Errors that can occur during message decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Buffer is too small to contain the message.
    BufferTooSmall {
        /// Number of bytes expected.
        expected: usize,
        /// Number of bytes available.
        available: usize,
    },
    /// Invalid message type identifier.
    InvalidMessageType(u16),
    /// Invalid enum value.
    InvalidEnumValue {
        /// Name of the enum type.
        enum_type: &'static str,
        /// The invalid value.
        value: u64,
    },
    /// Invalid UTF-8 in string field.
    InvalidUtf8 {
        /// Name of the field.
        field: &'static str,
        /// The UTF-8 error message.
        message: String,
    },
    /// Variable-length data exceeds maximum size.
    VarDataTooLarge {
        /// Name of the field.
        field: &'static str,
        /// Maximum allowed size.
        max_size: usize,
        /// Actual size.
        actual_size: usize,
    },
    /// Checksum mismatch.
    ChecksumMismatch {
        /// Expected checksum.
        expected: u32,
        /// Actual checksum.
        actual: u32,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall {
                expected,
                available,
            } => {
                write!(
                    f,
                    "Buffer too small: expected {} bytes, got {}",
                    expected, available
                )
            }
            Self::InvalidMessageType(id) => {
                write!(f, "Invalid message type: 0x{:04X}", id)
            }
            Self::InvalidEnumValue { enum_type, value } => {
                write!(f, "Invalid {} value: {}", enum_type, value)
            }
            Self::InvalidUtf8 { field, message } => {
                write!(f, "Invalid UTF-8 in field '{}': {}", field, message)
            }
            Self::VarDataTooLarge {
                field,
                max_size,
                actual_size,
            } => {
                write!(
                    f,
                    "Variable data '{}' too large: max {}, got {}",
                    field, max_size, actual_size
                )
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Checksum mismatch: expected 0x{:08X}, got 0x{:08X}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Errors that can occur during message encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// Buffer is too small to contain the encoded message.
    BufferTooSmall {
        /// Number of bytes required.
        required: usize,
        /// Number of bytes available.
        available: usize,
    },
    /// String is too long for the fixed-size field.
    StringTooLong {
        /// Name of the field.
        field: &'static str,
        /// Maximum allowed length.
        max_length: usize,
        /// Actual length.
        actual_length: usize,
    },
    /// Variable-length data is too large.
    VarDataTooLarge {
        /// Name of the field.
        field: &'static str,
        /// Maximum allowed size.
        max_size: usize,
        /// Actual size.
        actual_size: usize,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall {
                required,
                available,
            } => {
                write!(
                    f,
                    "Buffer too small: need {} bytes, have {}",
                    required, available
                )
            }
            Self::StringTooLong {
                field,
                max_length,
                actual_length,
            } => {
                write!(
                    f,
                    "String '{}' too long: max {}, got {}",
                    field, max_length, actual_length
                )
            }
            Self::VarDataTooLarge {
                field,
                max_size,
                actual_size,
            } => {
                write!(
                    f,
                    "Variable data '{}' too large: max {}, got {}",
                    field, max_size, actual_size
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Errors that can occur when working with the codec registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Requested codec version is not supported.
    UnsupportedVersion {
        /// Requested major version.
        major: u8,
        /// Requested minor version.
        minor: u8,
    },
    /// Failed to parse schema for dynamic decoding.
    SchemaParseError(String),
    /// Decode error.
    Decode(DecodeError),
    /// Encode error.
    Encode(EncodeError),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "Unsupported codec version: {}.{}", major, minor)
            }
            Self::SchemaParseError(msg) => {
                write!(f, "Schema parse error: {}", msg)
            }
            Self::Decode(e) => write!(f, "Decode error: {}", e),
            Self::Encode(e) => write!(f, "Encode error: {}", e),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(e) => Some(e),
            Self::Encode(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DecodeError> for CodecError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<EncodeError> for CodecError {
    fn from(e: EncodeError) -> Self {
        Self::Encode(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    // ==================== DecodeError Tests ====================

    #[test]
    fn test_decode_error_buffer_too_small_display() {
        let err = DecodeError::BufferTooSmall {
            expected: 100,
            available: 50,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn test_decode_error_invalid_message_type_display() {
        let err = DecodeError::InvalidMessageType(0x1234);
        assert!(err.to_string().contains("1234"));
    }

    #[test]
    fn test_decode_error_invalid_enum_value_display() {
        let err = DecodeError::InvalidEnumValue {
            enum_type: "Exchange",
            value: 99,
        };
        assert!(err.to_string().contains("Exchange"));
        assert!(err.to_string().contains("99"));
    }

    #[test]
    fn test_decode_error_invalid_utf8_display() {
        let err = DecodeError::InvalidUtf8 {
            field: "symbol",
            message: "invalid byte".to_string(),
        };
        assert!(err.to_string().contains("symbol"));
        assert!(err.to_string().contains("invalid byte"));
    }

    #[test]
    fn test_decode_error_checksum_mismatch_display() {
        let err = DecodeError::ChecksumMismatch {
            expected: 0xDEADBEEF,
            actual: 0xCAFEBABE,
        };
        assert!(err.to_string().contains("DEADBEEF"));
        assert!(err.to_string().contains("CAFEBABE"));
    }

    // ==================== EncodeError Tests ====================

    #[test]
    fn test_encode_error_buffer_too_small_display() {
        let err = EncodeError::BufferTooSmall {
            required: 200,
            available: 100,
        };
        assert!(err.to_string().contains("200"));
        assert!(err.to_string().contains("100"));
    }

    #[test]
    fn test_encode_error_string_too_long_display() {
        let err = EncodeError::StringTooLong {
            field: "symbol",
            max_length: 32,
            actual_length: 64,
        };
        assert!(err.to_string().contains("symbol"));
        assert!(err.to_string().contains("32"));
        assert!(err.to_string().contains("64"));
    }

    // ==================== CodecError Tests ====================

    #[test]
    fn test_codec_error_unsupported_version_display() {
        let err = CodecError::UnsupportedVersion { major: 2, minor: 1 };
        assert!(err.to_string().contains("2.1"));
    }

    #[test]
    fn test_codec_error_from_decode_error() {
        let decode_err = DecodeError::InvalidMessageType(0x9999);
        let codec_err: CodecError = decode_err.clone().into();
        assert!(matches!(codec_err, CodecError::Decode(_)));
    }

    #[test]
    fn test_codec_error_from_encode_error() {
        let encode_err = EncodeError::BufferTooSmall {
            required: 100,
            available: 50,
        };
        let codec_err: CodecError = encode_err.clone().into();
        assert!(matches!(codec_err, CodecError::Encode(_)));
    }

    #[test]
    fn test_codec_error_source() {
        let decode_err = DecodeError::InvalidMessageType(0x1111);
        let codec_err = CodecError::Decode(decode_err);
        assert!(codec_err.source().is_some());

        let unsupported = CodecError::UnsupportedVersion { major: 1, minor: 0 };
        assert!(unsupported.source().is_none());
    }
}
