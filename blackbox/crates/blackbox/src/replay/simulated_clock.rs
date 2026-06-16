//! Simulated clock for deterministic replay.
//!
//! This module provides a controllable clock implementation for replay and testing,
//! enabling deterministic execution of recorded trading sessions.
//!
//! ## Design
//!
//! | Component | Purpose | Implementation |
//! |-----------|---------|----------------|
//! | `current` | Current simulated time | `AtomicI64` |
//! | `paused` | Pause state for debugging | `AtomicBool` |
//! | `warp_enabled` | Enable idle-skipping | `AtomicBool` |
//!
//! ## Thread Safety
//!
//! All operations use atomic instructions with appropriate memory ordering:
//! - Reads: `Acquire` ordering
//! - Writes: `Release` ordering
//! - Read-modify-write: `AcqRel` ordering
//!
//! ## Performance Contract
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | `now()` | <10ns | Atomic load |
//! | `advance()` | <20ns | Atomic fetch-add |
//! | `pause()/resume()` | <10ns | Atomic store |
//! | `is_paused()` | <10ns | Atomic load |
//!
//! ## Usage with ReplayEngine
//!
//! ```ignore
//! use blackbox::replay::{ReplayEngine, SimulatedClock};
//!
//! // Create engine with simulated clock
//! let engine = ReplayEngine::new();
//! let clock = engine.clock();
//!
//! // Enable warp speed for fast replay
//! clock.enable_warp();
//!
//! // Pause for step-through debugging
//! clock.pause();
//! ```

use blackbox_types::{Clock, Timestamp};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

/// Internal state for SimulatedClock.
///
/// This is wrapped in an Arc to allow cloning with shared state.
#[derive(Debug)]
struct ClockState {
    /// Current simulated time in microseconds since Unix epoch.
    current: AtomicI64,
    /// Is the clock paused? (for step-through debugging)
    paused: AtomicBool,
    /// Is warp mode enabled? (skip idle periods)
    warp_enabled: AtomicBool,
}

// Compile-time size verification for inner state
const _: () = assert!(std::mem::size_of::<ClockState>() <= 32);

/// A controllable clock for replay and testing.
///
/// Unlike [`SystemClock`](blackbox_types::SystemClock), `SimulatedClock` allows
/// explicit time control for deterministic replay of recorded sessions.
///
/// # Features
///
/// - **Time Control**: Set, advance, or jump to specific timestamps
/// - **Pause/Resume**: Freeze time for step-through debugging
/// - **Warp Mode**: Enable idle-period skipping for fast replay
///
/// # Thread Safety
///
/// The clock uses atomic operations for thread-safe access.
/// Time can be set from one thread and read from another.
/// Cloned clocks share the same internal state.
///
/// # Cloning
///
/// `SimulatedClock` implements `Clone` with shared state semantics.
/// All clones reference the same underlying time, so advancing one
/// advances all clones.
///
/// # Example
///
/// ```
/// use blackbox::replay::SimulatedClock;
/// use blackbox_types::{Clock, Timestamp};
///
/// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
/// assert_eq!(clock.now().as_micros(), 1000);
///
/// // Advance time
/// clock.advance(500);
/// assert_eq!(clock.now().as_micros(), 1500);
///
/// // Set absolute time
/// clock.set(Timestamp::from_micros(2000));
/// assert_eq!(clock.now().as_micros(), 2000);
///
/// // Pause for debugging
/// clock.pause();
/// assert!(clock.is_paused());
///
/// // Warp mode (enabled by default)
/// assert!(clock.is_warp_enabled());
/// clock.disable_warp();
/// assert!(!clock.is_warp_enabled());
/// ```
#[derive(Debug, Clone)]
pub struct SimulatedClock {
    /// Shared internal state.
    state: Arc<ClockState>,
}

impl SimulatedClock {
    /// Create a new simulated clock at the given time.
    ///
    /// The clock starts in a non-paused state with warp mode enabled.
    ///
    /// # Arguments
    ///
    /// * `start` - The initial timestamp for the clock
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000));
    /// ```
    pub fn new(start: Timestamp) -> Self {
        Self {
            state: Arc::new(ClockState {
                current: AtomicI64::new(start.as_micros()),
                paused: AtomicBool::new(false),
                warp_enabled: AtomicBool::new(true), // Warp by default for fast replay
            }),
        }
    }

    /// Create a new simulated clock at the Unix epoch (1970-01-01 00:00:00 UTC).
    ///
    /// The clock starts in a non-paused state with warp mode enabled.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::{Clock, Timestamp};
    ///
    /// let clock = SimulatedClock::at_epoch();
    /// assert_eq!(clock.now(), Timestamp::EPOCH);
    /// ```
    pub fn at_epoch() -> Self {
        Self::new(Timestamp::EPOCH)
    }

    /// Set the current time to an absolute timestamp.
    ///
    /// This operation works regardless of the current paused state.
    ///
    /// # Arguments
    ///
    /// * `time` - The timestamp to set
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::{Clock, Timestamp};
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    /// clock.set(Timestamp::from_micros(5000));
    /// assert_eq!(clock.now().as_micros(), 5000);
    /// ```
    pub fn set(&self, time: Timestamp) {
        self.state
            .current
            .store(time.as_micros(), Ordering::Release);
    }

    /// Advance the clock by the given number of microseconds.
    ///
    /// Can advance forward (positive) or backward (negative).
    /// This operation works regardless of the current paused state.
    ///
    /// # Arguments
    ///
    /// * `micros` - The number of microseconds to advance (can be negative)
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::{Clock, Timestamp};
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    /// clock.advance(500);
    /// assert_eq!(clock.now().as_micros(), 1500);
    ///
    /// clock.advance(-200);
    /// assert_eq!(clock.now().as_micros(), 1300);
    /// ```
    pub fn advance(&self, micros: i64) {
        self.state.current.fetch_add(micros, Ordering::AcqRel);
    }

    /// Advance the clock to a specific timestamp if it's in the future.
    ///
    /// This is useful for jumping to the next event timestamp during replay,
    /// as it prevents accidentally going backwards in time.
    ///
    /// # Arguments
    ///
    /// * `target` - The target timestamp to advance to
    ///
    /// # Returns
    ///
    /// `true` if the clock was advanced, `false` if the target is in the past.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::{Clock, Timestamp};
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    ///
    /// // Advance to future timestamp
    /// assert!(clock.advance_to(Timestamp::from_micros(2000)));
    /// assert_eq!(clock.now().as_micros(), 2000);
    ///
    /// // Don't go backwards
    /// assert!(!clock.advance_to(Timestamp::from_micros(1500)));
    /// assert_eq!(clock.now().as_micros(), 2000);
    /// ```
    pub fn advance_to(&self, target: Timestamp) -> bool {
        let current = self.state.current.load(Ordering::Acquire);
        if target.as_micros() > current {
            self.state
                .current
                .store(target.as_micros(), Ordering::Release);
            true
        } else {
            false
        }
    }

    // ========================================================
    // PAUSE/RESUME FUNCTIONALITY (T3.1)
    // ========================================================

    /// Pause the clock.
    ///
    /// When paused, the clock's time is frozen. The `SkipIdleScheduler`
    /// and `ReplayEngine` check this state to halt event processing.
    ///
    /// Note: `set()` and `advance()` still work when paused, allowing
    /// manual time manipulation during step-through debugging.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    /// clock.pause();
    /// assert!(clock.is_paused());
    /// ```
    #[inline]
    pub fn pause(&self) {
        self.state.paused.store(true, Ordering::Release);
    }

    /// Resume the clock from a paused state.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    /// clock.pause();
    /// assert!(clock.is_paused());
    ///
    /// clock.resume();
    /// assert!(!clock.is_paused());
    /// ```
    #[inline]
    pub fn resume(&self) {
        self.state.paused.store(false, Ordering::Release);
    }

    /// Check if the clock is currently paused.
    ///
    /// # Returns
    ///
    /// `true` if the clock is paused, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1000));
    /// assert!(!clock.is_paused());
    ///
    /// clock.pause();
    /// assert!(clock.is_paused());
    /// ```
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.state.paused.load(Ordering::Acquire)
    }

    // ========================================================
    // WARP MODE FUNCTIONALITY (T3.1)
    // ========================================================

    /// Enable warp mode (skip idle periods).
    ///
    /// When warp mode is enabled, the `SkipIdleScheduler` will jump
    /// through idle periods (gaps > threshold) instead of waiting.
    /// This enables 24-hour sessions to replay in under 60 seconds.
    ///
    /// Warp mode is **enabled by default** for optimal replay speed.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(0));
    /// clock.disable_warp();
    /// assert!(!clock.is_warp_enabled());
    ///
    /// clock.enable_warp();
    /// assert!(clock.is_warp_enabled());
    /// ```
    #[inline]
    pub fn enable_warp(&self) {
        self.state.warp_enabled.store(true, Ordering::Release);
    }

    /// Disable warp mode (real-time or fixed-speed replay).
    ///
    /// When warp mode is disabled, the `SkipIdleScheduler` will
    /// respect actual time gaps, useful for:
    /// - Real-time replay (1x speed)
    /// - Fixed-speed replay (Nx speed)
    /// - Observing system behavior during idle periods
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(0));
    /// assert!(clock.is_warp_enabled()); // enabled by default
    ///
    /// clock.disable_warp();
    /// assert!(!clock.is_warp_enabled());
    /// ```
    #[inline]
    pub fn disable_warp(&self) {
        self.state.warp_enabled.store(false, Ordering::Release);
    }

    /// Check if warp mode is enabled.
    ///
    /// # Returns
    ///
    /// `true` if warp mode is enabled, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(0));
    /// assert!(clock.is_warp_enabled()); // enabled by default
    /// ```
    #[inline]
    pub fn is_warp_enabled(&self) -> bool {
        self.state.warp_enabled.load(Ordering::Acquire)
    }
}

impl Clock for SimulatedClock {
    #[inline]
    fn now(&self) -> Timestamp {
        Timestamp::from_micros(self.state.current.load(Ordering::Acquire))
    }
}

impl Default for SimulatedClock {
    fn default() -> Self {
        Self::at_epoch()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ============================================================
    // EXISTING TESTS (preserved)
    // ============================================================

    #[test]
    fn test_simulated_clock_initial() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        assert_eq!(clock.now().as_micros(), 1000);
    }

    #[test]
    fn test_simulated_clock_advance() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.advance(500);
        assert_eq!(clock.now().as_micros(), 1500);
    }

    #[test]
    fn test_simulated_clock_set() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.set(Timestamp::from_micros(5000));
        assert_eq!(clock.now().as_micros(), 5000);
    }

    #[test]
    fn test_simulated_clock_advance_to() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Advance to future
        assert!(clock.advance_to(Timestamp::from_micros(2000)));
        assert_eq!(clock.now().as_micros(), 2000);

        // Don't go backwards
        assert!(!clock.advance_to(Timestamp::from_micros(1500)));
        assert_eq!(clock.now().as_micros(), 2000);
    }

    #[test]
    fn test_simulated_clock_thread_safe() {
        let clock = Arc::new(SimulatedClock::new(Timestamp::from_micros(0)));
        let clock2 = Arc::clone(&clock);

        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                clock2.advance(1);
            }
        });

        for _ in 0..1000 {
            let _ = clock.now();
        }

        handle.join().unwrap();
        assert_eq!(clock.now().as_micros(), 1000);
    }

    // ============================================================
    // T3.1 NEW TESTS: PAUSE/RESUME FUNCTIONALITY
    // ============================================================

    #[test]
    fn test_initial_not_paused() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        assert!(!clock.is_paused(), "Clock should not be paused initially");
    }

    #[test]
    fn test_pause() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        assert!(clock.is_paused(), "Clock should be paused after pause()");
    }

    #[test]
    fn test_resume() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        assert!(clock.is_paused());
        clock.resume();
        assert!(
            !clock.is_paused(),
            "Clock should not be paused after resume()"
        );
    }

    #[test]
    fn test_pause_idempotent() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        clock.pause();
        clock.pause();
        assert!(clock.is_paused(), "Multiple pauses should still be paused");
    }

    #[test]
    fn test_resume_idempotent() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.resume();
        clock.resume();
        assert!(!clock.is_paused(), "Resume when not paused is no-op");
    }

    #[test]
    fn test_pause_preserves_time() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        assert_eq!(clock.now().as_micros(), 1000, "Pause should preserve time");
    }

    #[test]
    fn test_advance_while_paused() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        clock.advance(500);
        // Advance should still work on internal state even when paused
        // (the SkipIdleScheduler controls whether to advance)
        assert_eq!(clock.now().as_micros(), 1500);
    }

    #[test]
    fn test_set_while_paused() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.pause();
        clock.set(Timestamp::from_micros(5000));
        assert_eq!(clock.now().as_micros(), 5000);
    }

    #[test]
    fn test_pause_resume_cycle() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Cycle 1
        clock.pause();
        assert!(clock.is_paused());
        clock.resume();
        assert!(!clock.is_paused());

        // Cycle 2
        clock.pause();
        assert!(clock.is_paused());
        clock.resume();
        assert!(!clock.is_paused());

        // Time should be unchanged
        assert_eq!(clock.now().as_micros(), 1000);
    }

    // ============================================================
    // T3.1 NEW TESTS: WARP MODE FUNCTIONALITY
    // ============================================================

    #[test]
    fn test_warp_enabled_by_default() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        assert!(
            clock.is_warp_enabled(),
            "Warp mode should be enabled by default"
        );
    }

    #[test]
    fn test_disable_warp() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.disable_warp();
        assert!(
            !clock.is_warp_enabled(),
            "Warp should be disabled after disable_warp()"
        );
    }

    #[test]
    fn test_enable_warp() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.disable_warp();
        assert!(!clock.is_warp_enabled());
        clock.enable_warp();
        assert!(
            clock.is_warp_enabled(),
            "Warp should be enabled after enable_warp()"
        );
    }

    #[test]
    fn test_warp_toggle_idempotent() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Multiple enables
        clock.enable_warp();
        clock.enable_warp();
        assert!(clock.is_warp_enabled());

        // Multiple disables
        clock.disable_warp();
        clock.disable_warp();
        assert!(!clock.is_warp_enabled());
    }

    #[test]
    fn test_warp_and_pause_independent() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Both on
        assert!(clock.is_warp_enabled());
        assert!(!clock.is_paused());

        // Pause but warp still on
        clock.pause();
        assert!(clock.is_warp_enabled());
        assert!(clock.is_paused());

        // Disable warp but still paused
        clock.disable_warp();
        assert!(!clock.is_warp_enabled());
        assert!(clock.is_paused());

        // Resume but warp still off
        clock.resume();
        assert!(!clock.is_warp_enabled());
        assert!(!clock.is_paused());

        // Enable warp
        clock.enable_warp();
        assert!(clock.is_warp_enabled());
        assert!(!clock.is_paused());
    }

    // ============================================================
    // T3.1 NEW TESTS: SIZE CONSTRAINTS
    // ============================================================

    #[test]
    fn test_simulated_clock_size() {
        // Per the design notes: 3 atomics = 24 bytes, must be <= 32 bytes
        let size = std::mem::size_of::<SimulatedClock>();
        assert!(
            size <= 32,
            "SimulatedClock should be <= 32 bytes, got {} bytes",
            size
        );
    }

    #[test]
    fn test_simulated_clock_alignment() {
        let align = std::mem::align_of::<SimulatedClock>();
        assert!(
            align <= 8,
            "SimulatedClock alignment should be <= 8, got {}",
            align
        );
    }

    // ============================================================
    // T3.1 NEW TESTS: TRAIT IMPLEMENTATIONS
    // ============================================================

    #[test]
    fn test_clock_trait() {
        fn use_clock<C: Clock>(clock: &C) -> i64 {
            clock.now_micros()
        }

        let clock = SimulatedClock::new(Timestamp::from_micros(42));
        assert_eq!(use_clock(&clock), 42);
    }

    #[test]
    fn test_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<SimulatedClock>();
        assert_sync::<SimulatedClock>();
    }

    #[test]
    fn test_debug_impl() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        let debug_str = format!("{:?}", clock);
        assert!(
            debug_str.contains("SimulatedClock"),
            "Debug should contain type name"
        );
    }

    #[test]
    fn test_default_impl() {
        let clock = SimulatedClock::default();
        assert_eq!(clock.now().as_micros(), 0, "Default should be at epoch");
        assert!(!clock.is_paused(), "Default should not be paused");
        assert!(clock.is_warp_enabled(), "Default should have warp enabled");
    }

    // ============================================================
    // T3.1 NEW TESTS: CONCURRENT ACCESS
    // ============================================================

    #[test]
    fn test_concurrent_pause_resume() {
        let clock = Arc::new(SimulatedClock::new(Timestamp::from_micros(0)));
        let mut handles = vec![];

        // Spawn threads that pause/resume
        for i in 0..4 {
            let c = Arc::clone(&clock);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    if i % 2 == 0 {
                        c.pause();
                    } else {
                        c.resume();
                    }
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Just verify no panic - final state is indeterminate
        let _ = clock.is_paused();
    }

    #[test]
    fn test_concurrent_warp_toggle() {
        let clock = Arc::new(SimulatedClock::new(Timestamp::from_micros(0)));
        let mut handles = vec![];

        for i in 0..4 {
            let c = Arc::clone(&clock);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    if i % 2 == 0 {
                        c.enable_warp();
                    } else {
                        c.disable_warp();
                    }
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Just verify no panic
        let _ = clock.is_warp_enabled();
    }

    #[test]
    fn test_concurrent_mixed_operations() {
        let clock = Arc::new(SimulatedClock::new(Timestamp::from_micros(0)));
        let mut handles = vec![];

        // Reader thread
        let c1 = Arc::clone(&clock);
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                let _ = c1.now();
                let _ = c1.is_paused();
                let _ = c1.is_warp_enabled();
            }
        }));

        // Writer thread - time
        let c2 = Arc::clone(&clock);
        handles.push(thread::spawn(move || {
            for i in 0..1000 {
                c2.advance(1);
                if i % 100 == 0 {
                    c2.set(Timestamp::from_micros(i as i64));
                }
            }
        }));

        // Writer thread - pause
        let c3 = Arc::clone(&clock);
        handles.push(thread::spawn(move || {
            for _ in 0..500 {
                c3.pause();
                c3.resume();
            }
        }));

        // Writer thread - warp
        let c4 = Arc::clone(&clock);
        handles.push(thread::spawn(move || {
            for _ in 0..500 {
                c4.disable_warp();
                c4.enable_warp();
            }
        }));

        for h in handles {
            h.join().unwrap();
        }
    }

    // ============================================================
    // T3.1 NEW TESTS: EDGE CASES
    // ============================================================

    #[test]
    fn test_timestamp_min() {
        let clock = SimulatedClock::new(Timestamp::MIN);
        assert_eq!(clock.now(), Timestamp::MIN);
        clock.pause();
        assert!(clock.is_paused());
    }

    #[test]
    fn test_timestamp_max() {
        let clock = SimulatedClock::new(Timestamp::MAX);
        assert_eq!(clock.now(), Timestamp::MAX);
        clock.pause();
        assert!(clock.is_paused());
    }

    #[test]
    fn test_timestamp_epoch() {
        let clock = SimulatedClock::new(Timestamp::EPOCH);
        assert_eq!(clock.now(), Timestamp::EPOCH);
    }

    #[test]
    fn test_advance_negative() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        clock.advance(-500);
        assert_eq!(clock.now().as_micros(), 500, "Negative advance should work");
    }

    #[test]
    fn test_at_epoch_constructor() {
        let clock = SimulatedClock::at_epoch();
        assert_eq!(clock.now(), Timestamp::EPOCH);
        assert!(!clock.is_paused());
        assert!(clock.is_warp_enabled());
    }

    // ============================================================
    // T3.1 NEW TESTS: INTEGRATION WITH CLOCK TRAIT
    // ============================================================

    #[test]
    fn test_now_micros_method() {
        let clock = SimulatedClock::new(Timestamp::from_micros(12345));
        assert_eq!(clock.now_micros(), 12345);
    }

    #[test]
    fn test_generic_clock_usage() {
        fn advance_and_read<C: Clock>(clock: &C) -> i64 {
            clock.now_micros()
        }

        let clock = SimulatedClock::new(Timestamp::from_micros(100));
        clock.advance(50);
        assert_eq!(advance_and_read(&clock), 150);
    }

    // ============================================================
    // T3.1 NEW TESTS: DOCUMENTATION EXAMPLES
    // ============================================================

    /// Test the example from module docs
    #[test]
    fn test_doc_example() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));
        assert_eq!(clock.now().as_micros(), 1000);

        clock.advance(500);
        assert_eq!(clock.now().as_micros(), 1500);

        clock.set(Timestamp::from_micros(2000));
        assert_eq!(clock.now().as_micros(), 2000);
    }

    /// Test pause/resume workflow
    #[test]
    fn test_pause_resume_workflow() {
        let clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000));

        // Normal operation
        assert!(!clock.is_paused());
        clock.advance(1_000_000); // 1 second

        // Pause for inspection
        clock.pause();
        assert!(clock.is_paused());
        let paused_time = clock.now();

        // Resume
        clock.resume();
        assert!(!clock.is_paused());
        assert_eq!(clock.now(), paused_time);
    }

    /// Test warp mode workflow
    #[test]
    fn test_warp_mode_workflow() {
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Warp enabled by default for fast replay
        assert!(clock.is_warp_enabled());

        // Disable for real-time replay
        clock.disable_warp();
        assert!(!clock.is_warp_enabled());

        // Re-enable for fast-forward
        clock.enable_warp();
        assert!(clock.is_warp_enabled());
    }
}
