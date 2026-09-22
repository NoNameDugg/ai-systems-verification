//! Common utilities for exchange adapters.
//!
//! This module provides shared parsing utilities used by multiple adapters:
//!
//! - String to numeric conversions
//! - Timestamp parsing
//! - Common JSON access patterns

use crate::core::error::FlashError;
use crate::core::types::{PriceLevel, Quantity, Timestamp};
use rust_decimal::Decimal;
use std::str::FromStr;

// =============================================================================
// NUMERIC PARSING
// =============================================================================

/// Parse a price from a string or number JSON value.
///
/// # Arguments
///
/// * `value` - JSON value that may be a string or number
///
/// # Returns
///
/// The parsed f64 price, or an error if parsing fails.
///
/// # Example
///
/// ```ignore
/// let price = parse_price(&json["price"])?;
/// ```
pub fn parse_price(value: &serde_json::Value) -> Result<f64, FlashError> {
    match value {
        serde_json::Value::String(s) => s.parse::<f64>().map_err(|e| FlashError::InvalidPrice {
            price: 0.0,
            reason: format!("Failed to parse price string: {e}"),
        }),
        serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| FlashError::InvalidPrice {
            price: 0.0,
            reason: "Number not representable as f64".to_string(),
        }),
        _ => Err(FlashError::InvalidPrice {
            price: 0.0,
            reason: format!("Expected string or number, got {value:?}"),
        }),
    }
}

/// Parse a quantity from a string or number JSON value.
///
/// # Arguments
///
/// * `value` - JSON value that may be a string or number
///
/// # Returns
///
/// The parsed Decimal quantity, or an error if parsing fails.
pub fn parse_quantity(value: &serde_json::Value) -> Result<Quantity, FlashError> {
    match value {
        serde_json::Value::String(s) => {
            Decimal::from_str(s).map_err(|e| FlashError::InvalidQuantity {
                quantity: s.clone(),
                reason: format!("Failed to parse quantity: {e}"),
            })
        }
        serde_json::Value::Number(n) => {
            // Convert number to string first for precise parsing
            let s = n.to_string();
            Decimal::from_str(&s).map_err(|e| FlashError::InvalidQuantity {
                quantity: s,
                reason: format!("Failed to parse quantity: {e}"),
            })
        }
        _ => Err(FlashError::InvalidQuantity {
            quantity: format!("{value:?}"),
            reason: "Expected string or number".to_string(),
        }),
    }
}

/// Parse a timestamp from a JSON value.
///
/// Handles both millisecond and microsecond timestamps.
///
/// # Arguments
///
/// * `value` - JSON value containing the timestamp
/// * `is_millis` - If true, value is in milliseconds; if false, microseconds
///
/// # Returns
///
/// The timestamp in microseconds.
pub fn parse_timestamp(
    value: &serde_json::Value,
    is_millis: bool,
) -> Result<Timestamp, FlashError> {
    let ts = match value {
        serde_json::Value::Number(n) => n.as_i64().ok_or_else(|| {
            FlashError::ParseError(serde_json::from_str::<i64>("invalid").unwrap_err())
        })?,
        serde_json::Value::String(s) => s.parse::<i64>().map_err(|_| {
            FlashError::ParseError(serde_json::from_str::<i64>("invalid").unwrap_err())
        })?,
        _ => {
            return Err(FlashError::ParseError(
                serde_json::from_str::<i64>("invalid").unwrap_err(),
            ))
        }
    };

    // Convert to microseconds if needed
    if is_millis {
        Ok(ts * 1000)
    } else {
        Ok(ts)
    }
}

/// Parse an ISO 8601 timestamp string to microseconds.
///
/// # Arguments
///
/// * `s` - ISO 8601 timestamp string (e.g., "2023-12-29T00:00:00.000000000Z")
///
/// # Returns
///
/// The timestamp in microseconds.
pub fn parse_iso_timestamp(s: &str) -> Result<Timestamp, FlashError> {
    use chrono::{DateTime, Utc};

    let dt = DateTime::parse_from_rfc3339(s)
        .or_else(|_| {
            // Try parsing with nanoseconds
            DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ")
        })
        .map_err(|e| {
            FlashError::ParseError(serde_json::from_str::<i64>(&e.to_string()).unwrap_err())
        })?;

    Ok(dt.with_timezone(&Utc).timestamp_micros())
}

// =============================================================================
// PRICE LEVEL PARSING
// =============================================================================

/// Parse a price level from a [price, quantity] array.
///
/// Common format used by Deribit and Binance.
///
/// # Arguments
///
/// * `arr` - JSON array with [price, quantity]
/// * `timestamp` - Timestamp for the level
///
/// # Returns
///
/// The parsed PriceLevel.
pub fn parse_level_array(
    arr: &[serde_json::Value],
    timestamp: Timestamp,
) -> Result<PriceLevel, FlashError> {
    if arr.len() < 2 {
        return Err(FlashError::ParseError(
            serde_json::from_str::<()>("[]").unwrap_err(),
        ));
    }

    let price = parse_price(&arr[0])?;
    let quantity = parse_quantity(&arr[1])?;

    Ok(PriceLevel::new(price, quantity, timestamp))
}

/// Parse multiple price levels from a JSON array of [price, quantity] arrays.
///
/// # Arguments
///
/// * `arr` - JSON array of arrays
/// * `timestamp` - Timestamp for all levels
///
/// # Returns
///
/// Vector of parsed PriceLevels.
pub fn parse_levels_array(
    arr: &serde_json::Value,
    timestamp: Timestamp,
) -> Result<Vec<PriceLevel>, FlashError> {
    let arr = arr.as_array().ok_or_else(|| {
        FlashError::ParseError(serde_json::from_str::<()>("not_array").unwrap_err())
    })?;

    let mut levels = Vec::with_capacity(arr.len());
    for item in arr {
        if let Some(level_arr) = item.as_array() {
            levels.push(parse_level_array(level_arr, timestamp)?);
        }
    }
    Ok(levels)
}

// =============================================================================
// JSON UTILITIES
// =============================================================================

/// Get a string field from a JSON object, or return empty string.
pub fn get_str<'a>(json: &'a serde_json::Value, key: &str) -> &'a str {
    json.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Get an i64 field from a JSON object, or return 0.
#[allow(dead_code)]
pub fn get_i64(json: &serde_json::Value, key: &str) -> i64 {
    json.get(key)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

/// Get a u64 field from a JSON object, or return 0.
#[allow(dead_code)]
pub fn get_u64(json: &serde_json::Value, key: &str) -> u64 {
    json.get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

/// Check if a JSON string contains a substring (fast check before parsing).
#[inline]
pub fn contains_field(raw: &str, field: &str) -> bool {
    raw.contains(field)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    #[test]
    fn test_parse_price_string() {
        let value = json!("50000.50");
        let price = parse_price(&value).unwrap();
        assert!((price - 50000.50).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_price_number() {
        let value = json!(50000.50);
        let price = parse_price(&value).unwrap();
        assert!((price - 50000.50).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_quantity_string() {
        let value = json!("10.5");
        let qty = parse_quantity(&value).unwrap();
        assert_eq!(qty, dec!(10.5));
    }

    #[test]
    fn test_parse_quantity_number() {
        let value = json!(10.5);
        let qty = parse_quantity(&value).unwrap();
        assert_eq!(qty, dec!(10.5));
    }

    #[test]
    fn test_parse_timestamp_millis() {
        let value = json!(1703808000000i64);
        let ts = parse_timestamp(&value, true).unwrap();
        assert_eq!(ts, 1703808000000000i64); // Converted to micros
    }

    #[test]
    fn test_parse_level_array() {
        let arr = vec![json!("50000.0"), json!("10.5")];
        let level = parse_level_array(&arr, 1234567890).unwrap();
        assert!((level.price - 50000.0).abs() < f64::EPSILON);
        assert_eq!(level.quantity, dec!(10.5));
    }

    #[test]
    fn test_parse_levels_array() {
        let arr = json!([["50000.0", "10.5"], ["49999.0", "5.0"]]);
        let levels = parse_levels_array(&arr, 1234567890).unwrap();
        assert_eq!(levels.len(), 2);
    }

    #[test]
    fn test_get_str() {
        let json = json!({"key": "value"});
        assert_eq!(get_str(&json, "key"), "value");
        assert_eq!(get_str(&json, "missing"), "");
    }

    #[test]
    fn test_contains_field() {
        assert!(contains_field(r#"{"method":"heartbeat"}"#, "heartbeat"));
        assert!(!contains_field(r#"{"method":"data"}"#, "heartbeat"));
    }
}
