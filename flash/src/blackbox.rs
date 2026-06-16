//! Flight-recorder integration module for Flash.
//!
//! This module provides integration with an external flight recorder
//! for deterministic replay and debugging. It is only compiled when the
//! `blackbox` feature is enabled.
//!
//! # Feature Flag
//!
//! This module is feature-gated behind `blackbox`. It is wired to the sibling
//! `blackbox` flight-recorder crate in this repository, so building with
//! `--features blackbox` records tap-point events for deterministic replay. The
//! feature is off by default, so the core build stays standalone (no
//! cross-component dependency unless you opt in).
//!
//! # Integration Points
//!
//! The BlackBox integration provides tap points at:
//!
//! | Tap | Location | Data Captured |
//! |-----|----------|---------------|
//! | Ingress | `Connector` | Raw WebSocket frames |
//! | Internal | `OrderBook` | State changes (snapshots, deltas) |
//! | Egress | TBD | Order submissions |
//!
//! # Usage
//!
//! ```rust,ignore
//! use astra_flash::blackbox::Tap;
//! use astra_flash::network::Connector;
//!
//! // Create a tap (JournalTap for recording, NullTap for disabled)
//! let tap = astra_flash::blackbox::NullTap;
//!
//! // Inject tap into connector
//! let connector = Connector::with_tap(config, metrics, tap);
//! ```

// Re-export core flight-recorder types for convenience (the sibling `blackbox`
// crate, pulled in only under the `blackbox` feature; `package = "blackbox"` is
// aliased to `flight_recorder` here to avoid a feature/dependency name clash).
pub use flight_recorder::tap::{JournalTap, NullTap, Tap};
pub use flight_recorder::{Exchange as BlackBoxExchange, Timestamp as BlackBoxTimestamp};

/// Event types for internal tap recording.
///
/// These map to the record types defined in the flight-recorder STANDARDS doc.
pub mod event_types {
    /// Book snapshot event (full orderbook state).
    pub const BOOK_SNAPSHOT: u16 = 0x0010;
    /// Book delta event (incremental update).
    pub const BOOK_DELTA: u16 = 0x0011;
    /// Internal state change.
    pub const STATE_CHANGE: u16 = 0x0030;
}

// =============================================================================
// TYPE CONVERSIONS
// =============================================================================

/// Convert Flash Exchange to BlackBox Exchange.
///
/// # Mapping
///
/// | Flash | BlackBox |
/// |-------|----------|
/// | Deribit | Deribit |
/// | Binance | Binance |
/// | Oanda | Unknown |
///
/// Note: Oanda maps to Unknown because it's not in the BlackBox Exchange enum.
/// This is acceptable for recording purposes - the payload contains the full context.
#[inline]
pub fn to_blackbox_exchange(exchange: crate::core::types::Exchange) -> BlackBoxExchange {
    match exchange {
        crate::core::types::Exchange::Deribit => BlackBoxExchange::Deribit,
        crate::core::types::Exchange::Binance => BlackBoxExchange::Binance,
        crate::core::types::Exchange::Oanda => BlackBoxExchange::Unknown,
    }
}

/// Convert Flash Timestamp (i64 micros) to BlackBox Timestamp.
///
/// Both use microseconds since Unix epoch, so this is a direct conversion.
#[inline]
pub fn to_blackbox_timestamp(timestamp: crate::core::types::Timestamp) -> BlackBoxTimestamp {
    BlackBoxTimestamp::from_micros(timestamp)
}

/// Convert BlackBox Exchange back to Flash Exchange.
///
/// # Mapping
///
/// | BlackBox | Flash |
/// |----------|-------|
/// | Deribit | Deribit |
/// | Binance | Binance |
/// | Unknown | Oanda (fallback) |
/// | Bybit | Binance (closest match) |
/// | OKX | Binance (closest match) |
///
/// Note: Some BlackBox exchanges don't have Flash equivalents. This is used
/// during replay when the original exchange info may not be available.
#[inline]
pub fn from_blackbox_exchange(exchange: BlackBoxExchange) -> crate::core::types::Exchange {
    match exchange {
        BlackBoxExchange::Deribit => crate::core::types::Exchange::Deribit,
        BlackBoxExchange::Binance => crate::core::types::Exchange::Binance,
        // Fallbacks for exchanges not in Flash
        BlackBoxExchange::Unknown => crate::core::types::Exchange::Oanda,
        BlackBoxExchange::Bybit => crate::core::types::Exchange::Binance,
        BlackBoxExchange::OKX => crate::core::types::Exchange::Binance,
    }
}

/// Convert BlackBox Timestamp back to Flash Timestamp.
#[inline]
pub fn from_blackbox_timestamp(timestamp: BlackBoxTimestamp) -> crate::core::types::Timestamp {
    timestamp.as_micros()
}

// =============================================================================
// TAP HELPER TRAITS
// =============================================================================

/// Extension trait for recording ingress events with Flash types.
///
/// This trait provides a convenient way to record ingress events using
/// Flash types, handling the conversion to BlackBox types internally.
pub trait TapExt: Tap {
    /// Record an ingress event using Flash types.
    ///
    /// Converts Flash Exchange and Timestamp to BlackBox types before recording.
    fn record_flash_ingress(
        &self,
        exchange: crate::core::types::Exchange,
        payload: &[u8],
        timestamp: crate::core::types::Timestamp,
    ) {
        self.record_ingress(
            to_blackbox_exchange(exchange),
            payload,
            to_blackbox_timestamp(timestamp),
        );
    }

    /// Record an internal state change using Flash types.
    fn record_flash_internal(
        &self,
        event_type: u16,
        payload: &[u8],
        timestamp: crate::core::types::Timestamp,
    ) {
        self.record_internal(event_type, payload, to_blackbox_timestamp(timestamp));
    }

    /// Record an egress event using Flash types.
    fn record_flash_egress(
        &self,
        exchange: crate::core::types::Exchange,
        payload: &[u8],
        timestamp: crate::core::types::Timestamp,
    ) {
        self.record_egress(
            to_blackbox_exchange(exchange),
            payload,
            to_blackbox_timestamp(timestamp),
        );
    }
}

// Implement TapExt for all types that implement Tap (including dyn Tap)
impl<T: Tap + ?Sized> TapExt for T {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::Exchange as FlashExchange;

    #[test]
    fn test_exchange_conversion_deribit() {
        let flash = FlashExchange::Deribit;
        let bb = to_blackbox_exchange(flash);
        assert_eq!(bb, BlackBoxExchange::Deribit);

        let back = from_blackbox_exchange(bb);
        assert_eq!(back, FlashExchange::Deribit);
    }

    #[test]
    fn test_exchange_conversion_binance() {
        let flash = FlashExchange::Binance;
        let bb = to_blackbox_exchange(flash);
        assert_eq!(bb, BlackBoxExchange::Binance);

        let back = from_blackbox_exchange(bb);
        assert_eq!(back, FlashExchange::Binance);
    }

    #[test]
    fn test_exchange_conversion_oanda() {
        let flash = FlashExchange::Oanda;
        let bb = to_blackbox_exchange(flash);
        // Oanda maps to Unknown in BlackBox
        assert_eq!(bb, BlackBoxExchange::Unknown);
    }

    #[test]
    fn test_timestamp_conversion() {
        let flash_ts: i64 = 1_704_067_200_000_000; // 2024-01-01 00:00:00 UTC
        let bb_ts = to_blackbox_timestamp(flash_ts);
        assert_eq!(bb_ts.as_micros(), flash_ts);

        let back = from_blackbox_timestamp(bb_ts);
        assert_eq!(back, flash_ts);
    }

    #[test]
    fn test_tap_ext_with_null_tap() {
        let tap = NullTap;

        // These should not panic
        tap.record_flash_ingress(FlashExchange::Deribit, b"test", 1000);
        tap.record_flash_internal(event_types::BOOK_SNAPSHOT, b"state", 2000);
        tap.record_flash_egress(FlashExchange::Binance, b"order", 3000);

        // NullTap is always inactive
        assert!(!tap.is_active());
    }

    #[test]
    fn test_event_types_constants() {
        assert_eq!(event_types::BOOK_SNAPSHOT, 0x0010);
        assert_eq!(event_types::BOOK_DELTA, 0x0011);
        assert_eq!(event_types::STATE_CHANGE, 0x0030);
    }
}
