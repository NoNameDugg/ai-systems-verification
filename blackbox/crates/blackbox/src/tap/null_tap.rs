//! Null tap - No-op implementation with zero overhead.
//!
//! The `NullTap` provides a zero-cost implementation of the [`Tap`] trait
//! for use when recording is disabled. As a zero-sized type (ZST), it has
//! no runtime footprint and all method calls are fully inlined by the compiler.
//!
//! ## Zero-Overhead Guarantee
//!
//! - **Size**: 0 bytes (verified at compile time)
//! - **Alignment**: 1 byte
//! - **Methods**: All marked `#[inline(always)]` - compiler eliminates completely
//! - **Allocations**: None
//!
//! ## Usage Pattern
//!
//! Use `NullTap` as the default tap when BlackBox recording is disabled:
//!
//! ```rust
//! use blackbox::tap::{NullTap, Tap};
//!
//! // Generic trading function that works with any Tap implementation
//! fn process_market_data<T: Tap>(tap: &T, data: &[u8]) {
//!     use blackbox_types::{Exchange, Timestamp};
//!
//!     // With NullTap, this call is optimized away completely
//!     tap.record_ingress(Exchange::Deribit, data, Timestamp::from_micros(0));
//! }
//!
//! // Production with recording disabled - zero overhead
//! let tap = NullTap;
//! process_market_data(&tap, b"market data");
//! ```
//!
//! ## Feature Flag Pattern
//!
//! Typically used with feature flags:
//!
//! ```rust
//! use blackbox::tap::{NullTap, Tap};
//!
//! // Choose tap based on feature flag
//! #[cfg(feature = "blackbox")]
//! type ActiveTap = blackbox::tap::JournalTap;
//!
//! #[cfg(not(feature = "blackbox"))]
//! type ActiveTap = NullTap;
//! ```

use super::traits::Tap;
use blackbox_types::{Exchange, Timestamp};

/// No-op tap implementation with zero runtime overhead.
///
/// `NullTap` is a zero-sized type (ZST) that discards all events.
/// It's the default tap when recording is disabled.
///
/// # Performance Contract
///
/// | Property | Value |
/// |----------|-------|
/// | Size | 0 bytes |
/// | All methods | ~0ns (fully inlined, eliminated by optimizer) |
/// | Allocations | None |
/// | `is_active()` | Always returns `false` |
///
/// # Thread Safety
///
/// `NullTap` is `Send + Sync` and can be safely shared across threads.
/// Since all methods are no-ops, there are no synchronization concerns.
///
/// # Example
///
/// ```
/// use blackbox::tap::{NullTap, Tap};
/// use blackbox_types::{Exchange, Timestamp};
///
/// // NullTap is zero-sized and implements Copy
/// let tap = NullTap;
/// let tap_copy = tap; // Copy, not move
///
/// // All operations are no-ops
/// tap.record_ingress(Exchange::Deribit, b"data", Timestamp::from_micros(0));
/// tap_copy.record_egress(Exchange::Binance, b"order", Timestamp::from_micros(0));
///
/// // Always inactive
/// assert!(!tap.is_active());
/// assert_eq!(std::mem::size_of::<NullTap>(), 0);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct NullTap;

impl Tap for NullTap {
    #[inline(always)]
    fn record_ingress(&self, _exchange: Exchange, _payload: &[u8], _timestamp: Timestamp) {
        // No-op
    }

    #[inline(always)]
    fn record_internal(&self, _event_type: u16, _payload: &[u8], _timestamp: Timestamp) {
        // No-op
    }

    #[inline(always)]
    fn record_egress(&self, _exchange: Exchange, _payload: &[u8], _timestamp: Timestamp) {
        // No-op
    }

    #[inline(always)]
    fn record_checkpoint(&self, _state_hash: &[u8; 32], _timestamp: Timestamp) {
        // No-op
    }

    #[inline(always)]
    fn is_active(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ==================== Size & Layout Tests ====================

    #[test]
    fn test_null_tap_is_zero_sized() {
        assert_eq!(std::mem::size_of::<NullTap>(), 0);
    }

    #[test]
    fn test_null_tap_alignment() {
        assert_eq!(std::mem::align_of::<NullTap>(), 1);
    }

    // ==================== Active State Tests ====================

    #[test]
    fn test_null_tap_is_inactive() {
        let tap = NullTap;
        assert!(!tap.is_active());
    }

    #[test]
    fn test_null_tap_is_always_inactive() {
        // Verify is_active() is consistent across multiple calls
        let tap = NullTap;
        for _ in 0..100 {
            assert!(!tap.is_active());
        }
    }

    // ==================== Trait Method Tests (No Panic) ====================

    #[test]
    fn test_null_tap_record_ingress_no_panic() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        // Should not panic with valid data
        tap.record_ingress(Exchange::Deribit, b"test data", ts);
    }

    #[test]
    fn test_null_tap_record_internal_no_panic() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0010, b"state change", ts);
    }

    #[test]
    fn test_null_tap_record_egress_no_panic() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Binance, b"order data", ts);
    }

    #[test]
    fn test_null_tap_record_checkpoint_no_panic() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);
        let hash = [0xABu8; 32];

        tap.record_checkpoint(&hash, ts);
    }

    // ==================== Exchange Variant Tests ====================

    #[test]
    fn test_null_tap_all_exchanges_ingress() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Unknown, b"unknown", ts);
        tap.record_ingress(Exchange::Deribit, b"deribit", ts);
        tap.record_ingress(Exchange::Binance, b"binance", ts);
        tap.record_ingress(Exchange::Bybit, b"bybit", ts);
        tap.record_ingress(Exchange::OKX, b"okx", ts);
    }

    #[test]
    fn test_null_tap_all_exchanges_egress() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Unknown, b"order", ts);
        tap.record_egress(Exchange::Deribit, b"order", ts);
        tap.record_egress(Exchange::Binance, b"order", ts);
        tap.record_egress(Exchange::Bybit, b"order", ts);
        tap.record_egress(Exchange::OKX, b"order", ts);
    }

    // ==================== Payload Edge Case Tests ====================

    #[test]
    fn test_null_tap_empty_payload() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Deribit, b"", ts);
        tap.record_internal(0, b"", ts);
        tap.record_egress(Exchange::Deribit, b"", ts);
    }

    #[test]
    fn test_null_tap_large_payload() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);
        let large_payload = vec![0u8; 1024 * 1024]; // 1MB

        tap.record_ingress(Exchange::Deribit, &large_payload, ts);
        tap.record_egress(Exchange::Deribit, &large_payload, ts);
    }

    #[test]
    fn test_null_tap_checkpoint_all_zeros_hash() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);
        let hash = [0u8; 32];

        tap.record_checkpoint(&hash, ts);
    }

    #[test]
    fn test_null_tap_checkpoint_all_ones_hash() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);
        let hash = [0xFFu8; 32];

        tap.record_checkpoint(&hash, ts);
    }

    // ==================== Timestamp Edge Case Tests ====================

    #[test]
    fn test_null_tap_epoch_timestamp() {
        let tap = NullTap;
        let ts = Timestamp::EPOCH;

        tap.record_ingress(Exchange::Deribit, b"data", ts);
    }

    #[test]
    fn test_null_tap_min_timestamp() {
        let tap = NullTap;
        let ts = Timestamp::MIN;

        tap.record_ingress(Exchange::Deribit, b"data", ts);
    }

    #[test]
    fn test_null_tap_max_timestamp() {
        let tap = NullTap;
        let ts = Timestamp::MAX;

        tap.record_ingress(Exchange::Deribit, b"data", ts);
    }

    // ==================== Event Type Tests ====================

    #[test]
    fn test_null_tap_internal_event_type_zero() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0000, b"event", ts);
    }

    #[test]
    fn test_null_tap_internal_event_type_max() {
        let tap = NullTap;
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0xFFFF, b"event", ts);
    }

    // ==================== Trait Implementation Tests ====================

    #[test]
    fn test_null_tap_default() {
        // Use Default trait method explicitly to verify it works
        let tap: NullTap = Default::default();
        assert!(!tap.is_active());
    }

    #[test]
    fn test_null_tap_clone() {
        let tap1 = NullTap;
        // Use Clone trait method explicitly to verify it works
        let tap2 = Clone::clone(&tap1);
        assert!(!tap1.is_active());
        assert!(!tap2.is_active());
    }

    #[test]
    fn test_null_tap_copy() {
        let tap1 = NullTap;
        let tap2 = tap1; // Copy
        assert!(!tap1.is_active()); // tap1 still usable (Copy)
        assert!(!tap2.is_active());
    }

    #[test]
    fn test_null_tap_debug() {
        let tap = NullTap;
        let debug_str = format!("{:?}", tap);
        assert_eq!(debug_str, "NullTap");
    }

    // ==================== Thread Safety Tests ====================

    #[test]
    fn test_null_tap_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<NullTap>();
    }

    #[test]
    fn test_null_tap_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<NullTap>();
    }

    #[test]
    fn test_null_tap_concurrent_access() {
        let tap = Arc::new(NullTap);
        let mut handles = vec![];

        for _ in 0..4 {
            let tap_clone = Arc::clone(&tap);
            handles.push(thread::spawn(move || {
                let ts = Timestamp::from_micros(1000);
                for _ in 0..100 {
                    tap_clone.record_ingress(Exchange::Deribit, b"data", ts);
                    tap_clone.record_egress(Exchange::Binance, b"order", ts);
                    tap_clone.record_internal(0x0010, b"state", ts);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // All operations completed without panic
        assert!(!tap.is_active());
    }

    // ==================== Generic Usage Tests ====================

    #[test]
    fn test_null_tap_generic_function() {
        fn process_event<T: Tap>(tap: &T, data: &[u8]) -> bool {
            let ts = Timestamp::from_micros(1000);
            if tap.is_active() {
                tap.record_ingress(Exchange::Deribit, data, ts);
                true
            } else {
                false
            }
        }

        let tap = NullTap;
        // NullTap is always inactive, so should return false
        assert!(!process_event(&tap, b"test"));
    }

    #[test]
    fn test_null_tap_in_box() {
        let tap: Box<dyn Tap> = Box::new(NullTap);
        assert!(!tap.is_active());
        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::from_micros(0));
    }

    #[test]
    fn test_null_tap_in_arc() {
        let tap: Arc<dyn Tap> = Arc::new(NullTap);
        assert!(!tap.is_active());
        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::from_micros(0));
    }
}
