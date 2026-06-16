//! Clock trait for time abstraction.
//!
//! The Clock trait provides a generic interface for time sources,
//! enabling deterministic replay by swapping implementations.
//!
//! ## Why Generic (Not `dyn`)?
//!
//! Using `<C: Clock>` instead of `Box<dyn Clock>` eliminates vtable
//! overhead in the hot path. The compiler monomorphizes the code,
//! resulting in direct function calls.
//!
//! ## Implementations
//!
//! - `SystemClock`: Zero-sized type using real system time
//! - `SimulatedClock`: Controllable time for replay (in blackbox)

use crate::Timestamp;

/// A time source for the trading system.
///
/// This trait is designed to be used with generics (`<C: Clock>`) rather
/// than trait objects (`dyn Clock`) to avoid vtable overhead.
///
/// # Example
///
/// ```
/// use blackbox_types::{Clock, SystemClock, Timestamp};
///
/// fn process_event<C: Clock>(clock: &C) {
///     let now = clock.now();
///     println!("Event at: {}", now.as_micros());
/// }
///
/// let clock = SystemClock;
/// process_event(&clock);
/// ```
pub trait Clock {
    /// Returns the current timestamp.
    ///
    /// For `SystemClock`, this returns the actual system time.
    /// For `SimulatedClock`, this returns the simulated time.
    fn now(&self) -> Timestamp;

    /// Returns the current timestamp in microseconds since Unix epoch.
    ///
    /// This is a convenience method equivalent to `self.now().as_micros()`.
    #[inline]
    fn now_micros(&self) -> i64 {
        self.now().as_micros()
    }
}

/// System clock using real time.
///
/// This is a zero-sized type (ZST) that directly calls the system clock.
/// Using a ZST means no memory overhead and optimal inlining.
///
/// # Example
///
/// ```
/// use blackbox_types::{Clock, SystemClock};
///
/// let clock = SystemClock;
/// let ts = clock.now();
/// assert!(ts.as_micros() > 0);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[inline]
    fn now(&self) -> Timestamp {
        // Use std::time for microsecond precision without external deps
        use std::time::{SystemTime, UNIX_EPOCH};

        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before Unix epoch");

        Timestamp::from_micros(duration.as_micros() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_clock_returns_positive_timestamp() {
        let clock = SystemClock;
        let ts = clock.now();
        assert!(ts.as_micros() > 0);
    }

    #[test]
    fn test_system_clock_is_monotonic() {
        let clock = SystemClock;
        let t1 = clock.now();
        let t2 = clock.now();
        assert!(t2.as_micros() >= t1.as_micros());
    }

    #[test]
    fn test_system_clock_is_zero_sized() {
        assert_eq!(std::mem::size_of::<SystemClock>(), 0);
    }

    #[test]
    fn test_now_micros_convenience() {
        let clock = SystemClock;
        // Just verify that now_micros() returns a positive value
        // (cannot compare exact values due to time passing between calls)
        let micros = clock.now_micros();
        assert!(micros > 0, "now_micros should return positive value");
    }
}
