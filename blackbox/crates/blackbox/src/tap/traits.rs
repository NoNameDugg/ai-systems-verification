//! Tap trait definition.
//!
//! The Tap trait provides the core abstraction for instrumenting the trading
//! system with zero-overhead recording. This module is part of Phase 2 of
//! the BlackBox project.
//!
//! ## Design Principles
//!
//! 1. **Zero allocation** - All methods take references, no heap allocation in hot path
//! 2. **Thread-safe** - Trait requires `Send + Sync` for multi-threaded use
//! 3. **Generic** - Use with generics, not `dyn`, for zero vtable overhead
//! 4. **Inlineable** - Implementations should use `#[inline(always)]` for <100ns overhead
//!
//! ## Performance Contract
//!
//! | Method | Target Latency | Allocation Budget |
//! |--------|----------------|-------------------|
//! | `record_ingress` | <100ns | 0 bytes |
//! | `record_internal` | <100ns | 0 bytes |
//! | `record_egress` | <100ns | 0 bytes |
//! | `record_checkpoint` | <1μs | 0 bytes |
//! | `is_active` | <10ns | 0 bytes |
//!
//! ## Usage Pattern
//!
//! Always use generics, never `dyn Tap`:
//!
//! ```rust
//! use blackbox::tap::{Tap, NullTap};
//! use blackbox_types::{Exchange, Timestamp};
//!
//! fn process_frame<T: Tap>(tap: &T, exchange: Exchange, frame: &[u8], ts: Timestamp) {
//!     tap.record_ingress(exchange, frame, ts);
//!     // ... process frame ...
//! }
//!
//! // Production: use JournalTap
//! // Disabled:   use NullTap (zero overhead)
//! let tap = NullTap;
//! process_frame(&tap, Exchange::Deribit, b"test", Timestamp::from_micros(0));
//! ```

use blackbox_types::{Exchange, Timestamp};

/// Instrumentation tap for recording events.
///
/// The Tap trait defines the interface for event capture at
/// instrumentation points throughout the trading system.
///
/// # Thread Safety
///
/// This trait requires `Send + Sync` because taps may be shared across
/// multiple threads in the trading system. The ingress tap may be called
/// from the WebSocket reader thread while the egress tap is called from
/// the order submission thread.
///
/// # Design
///
/// - Methods take references to avoid allocation
/// - Implementations must be zero-allocation in hot path
/// - Generic over implementation type (no dyn)
/// - Requires `Send + Sync` for multi-threaded access
///
/// # Example
///
/// ```rust
/// use blackbox::tap::{Tap, NullTap};
/// use blackbox_types::{Exchange, Timestamp};
///
/// fn process_frame<T: Tap>(tap: &T, exchange: Exchange, frame: &[u8]) {
///     let timestamp = Timestamp::from_micros(0);
///     tap.record_ingress(exchange, frame, timestamp);
///     // ... process frame ...
/// }
///
/// let tap = NullTap;
/// process_frame(&tap, Exchange::Deribit, b"data");
/// ```
pub trait Tap: Send + Sync {
    /// Record an ingress event (incoming WebSocket frame).
    ///
    /// This is called on the hot path and must complete in <100ns.
    fn record_ingress(&self, exchange: Exchange, payload: &[u8], timestamp: Timestamp);

    /// Record an internal state change.
    fn record_internal(&self, event_type: u16, payload: &[u8], timestamp: Timestamp);

    /// Record an egress event (outgoing order).
    fn record_egress(&self, exchange: Exchange, payload: &[u8], timestamp: Timestamp);

    /// Record a checkpoint with state hash.
    fn record_checkpoint(&self, state_hash: &[u8; 32], timestamp: Timestamp);

    /// Check if the tap is active (recording).
    fn is_active(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;

    // ==================== Compile-Time Verification ====================

    /// Compile-time assertion that NullTap implements Send.
    #[test]
    fn test_null_tap_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<super::super::NullTap>();
    }

    /// Compile-time assertion that NullTap implements Sync.
    #[test]
    fn test_null_tap_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<super::super::NullTap>();
    }

    /// Compile-time assertion that Tap implementors are Send + Sync.
    #[test]
    fn test_tap_trait_requires_send_sync() {
        fn assert_send_sync<T: Tap>() {}
        // This test would fail to compile if Tap didn't require Send + Sync
        assert_send_sync::<super::super::NullTap>();
        assert_send_sync::<CountingTap>();
    }

    // ==================== Mock Tap for Testing ====================

    /// A test tap that counts method calls.
    struct CountingTap {
        ingress_count: AtomicU64,
        internal_count: AtomicU64,
        egress_count: AtomicU64,
        checkpoint_count: AtomicU64,
        active: bool,
    }

    impl CountingTap {
        fn new(active: bool) -> Self {
            Self {
                ingress_count: AtomicU64::new(0),
                internal_count: AtomicU64::new(0),
                egress_count: AtomicU64::new(0),
                checkpoint_count: AtomicU64::new(0),
                active,
            }
        }

        fn ingress_count(&self) -> u64 {
            self.ingress_count.load(Ordering::Relaxed)
        }

        fn internal_count(&self) -> u64 {
            self.internal_count.load(Ordering::Relaxed)
        }

        fn egress_count(&self) -> u64 {
            self.egress_count.load(Ordering::Relaxed)
        }

        fn checkpoint_count(&self) -> u64 {
            self.checkpoint_count.load(Ordering::Relaxed)
        }
    }

    impl Tap for CountingTap {
        fn record_ingress(&self, _exchange: Exchange, _payload: &[u8], _timestamp: Timestamp) {
            self.ingress_count.fetch_add(1, Ordering::Relaxed);
        }

        fn record_internal(&self, _event_type: u16, _payload: &[u8], _timestamp: Timestamp) {
            self.internal_count.fetch_add(1, Ordering::Relaxed);
        }

        fn record_egress(&self, _exchange: Exchange, _payload: &[u8], _timestamp: Timestamp) {
            self.egress_count.fetch_add(1, Ordering::Relaxed);
        }

        fn record_checkpoint(&self, _state_hash: &[u8; 32], _timestamp: Timestamp) {
            self.checkpoint_count.fetch_add(1, Ordering::Relaxed);
        }

        fn is_active(&self) -> bool {
            self.active
        }
    }

    // ==================== Trait Method Tests ====================

    #[test]
    fn test_tap_record_ingress() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Deribit, b"test payload", ts);

        assert_eq!(tap.ingress_count(), 1);
    }

    #[test]
    fn test_tap_record_internal() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0010, b"state change", ts);

        assert_eq!(tap.internal_count(), 1);
    }

    #[test]
    fn test_tap_record_egress() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Binance, b"order data", ts);

        assert_eq!(tap.egress_count(), 1);
    }

    #[test]
    fn test_tap_record_checkpoint() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);
        let hash = [0u8; 32];

        tap.record_checkpoint(&hash, ts);

        assert_eq!(tap.checkpoint_count(), 1);
    }

    #[test]
    fn test_tap_is_active_true() {
        let tap = CountingTap::new(true);
        assert!(tap.is_active());
    }

    #[test]
    fn test_tap_is_active_false() {
        let tap = CountingTap::new(false);
        assert!(!tap.is_active());
    }

    // ==================== Thread Safety Tests ====================

    #[test]
    fn test_tap_thread_safety_concurrent_ingress() {
        let tap = Arc::new(CountingTap::new(true));
        let mut handles = vec![];

        for _ in 0..4 {
            let tap_clone = Arc::clone(&tap);
            handles.push(thread::spawn(move || {
                let ts = Timestamp::from_micros(1000);
                for _ in 0..100 {
                    tap_clone.record_ingress(Exchange::Deribit, b"data", ts);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(tap.ingress_count(), 400);
    }

    #[test]
    fn test_tap_thread_safety_mixed_operations() {
        let tap = Arc::new(CountingTap::new(true));
        let mut handles = vec![];
        let ts = Timestamp::from_micros(1000);

        // Thread 1: ingress
        let tap1 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                tap1.record_ingress(Exchange::Deribit, b"ingress", ts);
            }
        }));

        // Thread 2: egress
        let tap2 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                tap2.record_egress(Exchange::Binance, b"egress", ts);
            }
        }));

        // Thread 3: internal
        let tap3 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                tap3.record_internal(0x0010, b"internal", ts);
            }
        }));

        // Thread 4: checkpoint
        let tap4 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            let hash = [0u8; 32];
            for _ in 0..50 {
                tap4.record_checkpoint(&hash, ts);
            }
        }));

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(tap.ingress_count(), 50);
        assert_eq!(tap.egress_count(), 50);
        assert_eq!(tap.internal_count(), 50);
        assert_eq!(tap.checkpoint_count(), 50);
    }

    // ==================== Generic Usage Tests ====================

    #[test]
    fn test_tap_generic_function() {
        fn process_event<T: Tap>(tap: &T, data: &[u8]) -> bool {
            let ts = Timestamp::from_micros(1000);
            if tap.is_active() {
                tap.record_ingress(Exchange::Bybit, data, ts);
                true
            } else {
                false
            }
        }

        let active_tap = CountingTap::new(true);
        let inactive_tap = CountingTap::new(false);

        assert!(process_event(&active_tap, b"test"));
        assert!(!process_event(&inactive_tap, b"test"));
        assert_eq!(active_tap.ingress_count(), 1);
        assert_eq!(inactive_tap.ingress_count(), 0);
    }

    #[test]
    fn test_tap_all_exchanges() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Unknown, b"unknown", ts);
        tap.record_ingress(Exchange::Deribit, b"deribit", ts);
        tap.record_ingress(Exchange::Binance, b"binance", ts);
        tap.record_ingress(Exchange::Bybit, b"bybit", ts);
        tap.record_ingress(Exchange::OKX, b"okx", ts);

        assert_eq!(tap.ingress_count(), 5);
    }

    #[test]
    fn test_tap_empty_payload() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Deribit, b"", ts);
        tap.record_egress(Exchange::Deribit, b"", ts);
        tap.record_internal(0, b"", ts);

        assert_eq!(tap.ingress_count(), 1);
        assert_eq!(tap.egress_count(), 1);
        assert_eq!(tap.internal_count(), 1);
    }

    #[test]
    fn test_tap_large_payload() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);
        let large_payload = vec![0u8; 1024 * 1024]; // 1MB

        tap.record_ingress(Exchange::Deribit, &large_payload, ts);

        assert_eq!(tap.ingress_count(), 1);
    }

    // ==================== Event Type Tests ====================

    #[test]
    fn test_tap_internal_event_types() {
        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        // Test various internal event types
        tap.record_internal(0x0010, b"book_snapshot", ts); // BOOK_SNAPSHOT
        tap.record_internal(0x0011, b"book_delta", ts); // BOOK_DELTA
        tap.record_internal(0x0030, b"internal_event", ts); // INTERNAL_EVENT

        assert_eq!(tap.internal_count(), 3);
    }

    // ==================== Checkpoint Hash Tests ====================

    #[test]
    fn test_tap_checkpoint_with_real_hash() {
        use sha2::{Digest, Sha256};

        let tap = CountingTap::new(true);
        let ts = Timestamp::from_micros(1000);

        let mut hasher = Sha256::new();
        hasher.update(b"orderbook state");
        let result = hasher.finalize();

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);

        tap.record_checkpoint(&hash, ts);

        assert_eq!(tap.checkpoint_count(), 1);
    }
}
