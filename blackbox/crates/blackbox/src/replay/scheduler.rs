//! Skip-Idle Scheduler for warp-speed replay.
//!
//! This module provides the core functionality for skipping idle periods during replay,
//! enabling 24-hour trading sessions to be replayed in under 60 seconds.
//!
//! ## Design
//!
//! | Component | Purpose | Implementation |
//! |-----------|---------|----------------|
//! | `WarpConfig` | Configuration | Idle threshold, callbacks |
//! | `SkipIdleScheduler` | Core scheduler | Atomic stats tracking |
//! | `WarpStats` | Statistics | Time saved, warp count |
//!
//! ## Algorithm
//!
//! 1. Peek at next event timestamp
//! 2. Calculate gap = next_timestamp - current_time
//! 3. If gap > idle_threshold AND warp_enabled: WARP (instant jump)
//! 4. If gap <= idle_threshold: Real-time wait (or skip in benchmark mode)
//!
//! ## Performance Target
//!
//! A 24-hour trading session with:
//! - 8 hours of market activity (events every ~1ms)
//! - 16 hours of idle time (nights, weekends)
//!
//! Should replay in: <60 seconds (warp through idle)
//!
//! ## Usage
//!
//! ```
//! use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
//! use blackbox_types::{Clock, Timestamp};
//!
//! let config = WarpConfig::default();
//! let scheduler = SkipIdleScheduler::new(config);
//! let clock = SimulatedClock::new(Timestamp::from_micros(1000));
//!
//! // Schedule next event - will warp if gap > threshold
//! let next_ts = Timestamp::from_micros(1_000_000); // 1 second later
//! scheduler.schedule(&clock, next_ts);
//!
//! assert_eq!(clock.now().as_micros(), 1_000_000);
//! ```

use super::SimulatedClock;
use blackbox_types::{Clock, Timestamp};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Configuration for skip-idle behavior.
///
/// Controls how the scheduler handles idle periods during replay.
///
/// # Default Values
///
/// | Field | Default | Rationale |
/// |-------|---------|-----------|
/// | `idle_threshold_us` | 10,000 (10ms) | Gaps larger than 10ms are considered idle |
/// | `max_warp_factor` | `None` (instant) | Jump directly to event time |
///
/// # Example
///
/// ```
/// use blackbox::replay::WarpConfig;
///
/// // Default: 10ms threshold, instant warp
/// let default_config = WarpConfig::default();
///
/// // Custom: 100ms threshold
/// let custom_config = WarpConfig::with_threshold(100_000);
/// ```
#[derive(Debug, Clone)]
pub struct WarpConfig {
    /// Minimum idle gap to trigger warp (microseconds).
    ///
    /// Gaps smaller than this are considered "active" and may be
    /// played in real-time or at a fixed speed multiplier.
    ///
    /// Default: 10,000 (10ms)
    pub idle_threshold_us: i64,

    /// Maximum warp speed multiplier.
    ///
    /// - `None`: Instant jump (default, fastest)
    /// - `Some(100.0)`: Jump at 100x real-time max
    ///
    /// This is useful for UI animations or debugging.
    pub max_warp_factor: Option<f64>,
}

impl WarpConfig {
    /// Create a new WarpConfig with default settings.
    ///
    /// - Idle threshold: 10ms
    /// - Max warp factor: None (instant)
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a WarpConfig with a custom idle threshold.
    ///
    /// # Arguments
    ///
    /// * `threshold_us` - Idle gap threshold in microseconds
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::WarpConfig;
    ///
    /// // Warp only for gaps > 1 second
    /// let config = WarpConfig::with_threshold(1_000_000);
    /// assert_eq!(config.idle_threshold_us, 1_000_000);
    /// ```
    pub fn with_threshold(threshold_us: i64) -> Self {
        Self {
            idle_threshold_us: threshold_us,
            max_warp_factor: None,
        }
    }

    /// Create a WarpConfig for real-time replay (no warping).
    ///
    /// Sets threshold to `i64::MAX` so no gap is ever considered idle.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::WarpConfig;
    ///
    /// let config = WarpConfig::real_time();
    /// assert_eq!(config.idle_threshold_us, i64::MAX);
    /// ```
    pub fn real_time() -> Self {
        Self {
            idle_threshold_us: i64::MAX,
            max_warp_factor: None,
        }
    }

    /// Create a WarpConfig that always warps (0 threshold).
    ///
    /// Any positive gap will trigger a warp.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::WarpConfig;
    ///
    /// let config = WarpConfig::instant();
    /// assert_eq!(config.idle_threshold_us, 0);
    /// ```
    pub fn instant() -> Self {
        Self {
            idle_threshold_us: 0,
            max_warp_factor: None,
        }
    }

    /// Create a WarpConfig for fast-forward replay at Nx speed.
    ///
    /// This bounds the maximum time advance per warp to `threshold * multiplier`.
    ///
    /// # Arguments
    ///
    /// * `multiplier` - Speed multiplier (e.g., 10 = 10x speed)
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::WarpConfig;
    ///
    /// // 10x fast-forward
    /// let config = WarpConfig::fast_forward(10);
    /// assert_eq!(config.max_warp_factor, Some(10.0));
    /// ```
    pub fn fast_forward(multiplier: u32) -> Self {
        Self {
            idle_threshold_us: 10_000, // Default 10ms threshold
            max_warp_factor: Some(multiplier as f64),
        }
    }
}

impl Default for WarpConfig {
    fn default() -> Self {
        Self {
            idle_threshold_us: 10_000, // 10ms
            max_warp_factor: None,     // Instant jump
        }
    }
}

/// Statistics from warp operations.
///
/// Tracks how much time was saved by warping through idle periods.
///
/// # Example
///
/// ```
/// use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
/// use blackbox_types::Timestamp;
///
/// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
/// let clock = SimulatedClock::new(Timestamp::from_micros(0));
///
/// // Simulate warping through 1 second of idle time
/// scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
///
/// let stats = scheduler.stats();
/// assert!(stats.total_warped_us > 0);
/// assert!(stats.warp_count >= 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpStats {
    /// Total microseconds warped (skipped).
    pub total_warped_us: i64,
    /// Number of warp operations performed.
    pub warp_count: u64,
}

impl WarpStats {
    /// Create empty stats.
    pub fn new() -> Self {
        Self {
            total_warped_us: 0,
            warp_count: 0,
        }
    }

    /// Calculate the wall-clock time saved by warping.
    ///
    /// # Returns
    ///
    /// A `Duration` representing the time that would have been spent
    /// waiting if warping was disabled.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::WarpStats;
    ///
    /// let stats = WarpStats { total_warped_us: 1_000_000, warp_count: 10 };
    /// assert_eq!(stats.time_saved().as_secs(), 1);
    /// ```
    pub fn time_saved(&self) -> std::time::Duration {
        std::time::Duration::from_micros(self.total_warped_us.max(0) as u64)
    }

    /// Get the average warp duration.
    ///
    /// # Returns
    ///
    /// Average microseconds per warp, or 0 if no warps occurred.
    pub fn average_warp_us(&self) -> i64 {
        if self.warp_count == 0 {
            0
        } else {
            self.total_warped_us / self.warp_count as i64
        }
    }
}

impl Default for WarpStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Skip-Idle Scheduler - the core of warp-speed replay.
///
/// This scheduler detects idle periods (gaps between events) and warps
/// through them, enabling fast replay of long trading sessions.
///
/// # Thread Safety
///
/// The scheduler uses atomic operations for thread-safe statistics tracking.
/// Multiple threads can call `schedule()` safely, though typically only one
/// replay thread uses the scheduler.
///
/// # Performance
///
/// All operations are designed for minimal overhead:
///
/// | Operation | Target Latency | Notes |
/// |-----------|----------------|-------|
/// | `schedule()` | <100ns | Atomic ops + comparison |
/// | `stats()` | <50ns | Atomic loads |
/// | `reset()` | <50ns | Atomic stores |
///
/// # Bounded Warp (Fast-Forward)
///
/// When `max_warp_factor` is set, the scheduler limits time advancement per
/// warp to `idle_threshold_us * max_warp_factor`. This enables Nx speed
/// replay without instant jumps.
///
/// # Example
///
/// ```
/// use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
/// use blackbox_types::Timestamp;
///
/// // Create scheduler with 10ms threshold
/// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
/// let clock = SimulatedClock::new(Timestamp::from_micros(0));
///
/// // Small gap (1ms) - below threshold
/// let result = scheduler.schedule(&clock, Timestamp::from_micros(1000));
/// assert!(result.advanced);
/// assert!(!result.warped);
///
/// // Large gap (1s) - above threshold, will warp
/// let result = scheduler.schedule(&clock, Timestamp::from_micros(1_001_000));
/// assert!(result.advanced);
/// assert!(result.warped);
///
/// // Check stats
/// let stats = scheduler.stats();
/// assert_eq!(stats.warp_count, 1);
/// ```
#[derive(Debug)]
pub struct SkipIdleScheduler {
    /// Configuration for warp behavior.
    config: WarpConfig,
    /// Total microseconds warped (accumulated).
    total_warped_us: AtomicI64,
    /// Number of warp operations.
    warp_count: AtomicU64,
    /// Dynamic max warp factor (scaled by 1000, 0 = None).
    /// This overrides config.max_warp_factor when non-zero.
    dynamic_max_factor_scaled: AtomicU64,
}

/// Result of a schedule operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleResult {
    /// Whether the clock was advanced.
    pub advanced: bool,
    /// Whether a warp (skip) occurred.
    pub warped: bool,
    /// The gap in microseconds (original gap to target).
    pub gap_us: i64,
    /// Remaining time to target after bounded warp (0 if fully reached).
    pub remaining_us: i64,
}

impl SkipIdleScheduler {
    /// Create a new scheduler with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Warp configuration
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{SkipIdleScheduler, WarpConfig};
    ///
    /// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    /// ```
    pub fn new(config: WarpConfig) -> Self {
        // Initialize dynamic factor from config (or 0 if None)
        let initial_factor = config
            .max_warp_factor
            .map(|f| (f * 1000.0).round() as u64)
            .unwrap_or(0);

        Self {
            config,
            total_warped_us: AtomicI64::new(0),
            warp_count: AtomicU64::new(0),
            dynamic_max_factor_scaled: AtomicU64::new(initial_factor),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &WarpConfig {
        &self.config
    }

    /// Set the maximum warp factor dynamically.
    ///
    /// This allows changing the warp speed at runtime without recreating
    /// the scheduler. Set to `None` for unbounded (instant) warp.
    ///
    /// # Arguments
    ///
    /// * `factor` - Maximum warp factor, or `None` for instant warp
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{SkipIdleScheduler, WarpConfig};
    ///
    /// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    ///
    /// // Set to 10x fast-forward
    /// scheduler.set_max_warp_factor(Some(10.0));
    /// assert_eq!(scheduler.max_warp_factor(), Some(10.0));
    ///
    /// // Reset to instant warp
    /// scheduler.set_max_warp_factor(None);
    /// assert!(scheduler.max_warp_factor().is_none());
    /// ```
    pub fn set_max_warp_factor(&self, factor: Option<f64>) {
        let scaled = factor.map(|f| (f * 1000.0).round() as u64).unwrap_or(0);
        self.dynamic_max_factor_scaled
            .store(scaled, Ordering::Relaxed);
    }

    /// Get the current maximum warp factor.
    ///
    /// Returns `None` if warping is unbounded (instant).
    #[inline]
    pub fn max_warp_factor(&self) -> Option<f64> {
        let scaled = self.dynamic_max_factor_scaled.load(Ordering::Relaxed);
        if scaled == 0 {
            None
        } else {
            Some(scaled as f64 / 1000.0)
        }
    }

    /// Schedule the next event, advancing the clock as needed.
    ///
    /// If the gap to `next_event_ts` exceeds the idle threshold AND warp
    /// mode is enabled on the clock, the clock is advanced (potentially
    /// bounded by `max_warp_factor`). Otherwise, the clock is advanced
    /// normally to the event timestamp.
    ///
    /// # Bounded Warp (Fast-Forward)
    ///
    /// When `max_warp_factor` is set, the maximum time advance per warp is:
    /// `idle_threshold_us * max_warp_factor`
    ///
    /// If the gap exceeds this limit, the clock advances by the limit and
    /// `remaining_us` in the result indicates the remaining time to target.
    ///
    /// # Arguments
    ///
    /// * `clock` - The simulated clock to advance
    /// * `next_event_ts` - Timestamp of the next event
    ///
    /// # Returns
    ///
    /// A `ScheduleResult` indicating whether the clock was advanced,
    /// whether warping occurred, and any remaining time.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
    /// use blackbox_types::{Clock, Timestamp};
    ///
    /// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    /// let clock = SimulatedClock::new(Timestamp::from_micros(0));
    ///
    /// let result = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
    ///
    /// assert!(result.warped);
    /// assert_eq!(clock.now().as_micros(), 1_000_000);
    /// assert_eq!(result.remaining_us, 0);
    /// ```
    #[inline]
    pub fn schedule(&self, clock: &SimulatedClock, next_event_ts: Timestamp) -> ScheduleResult {
        let current = clock.now();
        let gap = next_event_ts.as_micros() - current.as_micros();

        // Event is in the past or now - no advancement needed
        if gap <= 0 {
            return ScheduleResult {
                advanced: false,
                warped: false,
                gap_us: gap,
                remaining_us: 0,
            };
        }

        // Determine if we should warp
        let should_warp = gap > self.config.idle_threshold_us && clock.is_warp_enabled();

        if should_warp {
            // Check for bounded warp (fast-forward mode)
            let max_factor = self.max_warp_factor();

            let (actual_advance, remaining) = if let Some(factor) = max_factor {
                // Bounded warp: max advance = threshold * factor
                let max_advance = (self.config.idle_threshold_us as f64 * factor).round() as i64;
                if gap > max_advance {
                    // Cap at max advance
                    (max_advance, gap - max_advance)
                } else {
                    // Gap is within limit, advance fully
                    (gap, 0)
                }
            } else {
                // Unbounded (instant) warp
                (gap, 0)
            };

            // Advance clock
            let new_time = Timestamp::from_micros(current.as_micros() + actual_advance);
            clock.set(new_time);

            // Track statistics (only track actual advance, not target gap)
            self.total_warped_us
                .fetch_add(actual_advance, Ordering::Relaxed);
            self.warp_count.fetch_add(1, Ordering::Relaxed);

            ScheduleResult {
                advanced: true,
                warped: true,
                gap_us: gap,
                remaining_us: remaining,
            }
        } else {
            // Normal advancement (gap <= threshold or warp disabled)
            clock.set(next_event_ts);

            ScheduleResult {
                advanced: true,
                warped: false,
                gap_us: gap,
                remaining_us: 0,
            }
        }
    }

    /// Check if a gap would trigger a warp.
    ///
    /// This is a pure function for testing purposes.
    ///
    /// # Arguments
    ///
    /// * `gap_us` - Gap in microseconds
    /// * `warp_enabled` - Whether warp mode is enabled
    ///
    /// # Returns
    ///
    /// `true` if this gap would trigger a warp.
    #[inline]
    pub fn would_warp(&self, gap_us: i64, warp_enabled: bool) -> bool {
        gap_us > self.config.idle_threshold_us && warp_enabled
    }

    /// Get current warp statistics.
    ///
    /// # Returns
    ///
    /// A snapshot of the current statistics.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{SkipIdleScheduler, WarpConfig};
    ///
    /// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    /// let stats = scheduler.stats();
    ///
    /// assert_eq!(stats.total_warped_us, 0);
    /// assert_eq!(stats.warp_count, 0);
    /// ```
    #[inline]
    pub fn stats(&self) -> WarpStats {
        WarpStats {
            total_warped_us: self.total_warped_us.load(Ordering::Relaxed),
            warp_count: self.warp_count.load(Ordering::Relaxed),
        }
    }

    /// Reset statistics to zero.
    ///
    /// Call this when restarting a replay session.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
    /// use blackbox_types::Timestamp;
    ///
    /// let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    /// let clock = SimulatedClock::new(Timestamp::from_micros(0));
    ///
    /// // Do some warps
    /// scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
    /// assert!(scheduler.stats().warp_count > 0);
    ///
    /// // Reset
    /// scheduler.reset();
    /// assert_eq!(scheduler.stats().warp_count, 0);
    /// ```
    pub fn reset(&self) {
        self.total_warped_us.store(0, Ordering::Relaxed);
        self.warp_count.store(0, Ordering::Relaxed);
    }

    /// Get the idle threshold in microseconds.
    #[inline]
    pub fn idle_threshold_us(&self) -> i64 {
        self.config.idle_threshold_us
    }
}

impl Default for SkipIdleScheduler {
    fn default() -> Self {
        Self::new(WarpConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ============================================================
    // WARPCONFIG TESTS
    // ============================================================

    #[test]
    fn test_warp_config_default() {
        let config = WarpConfig::default();
        assert_eq!(config.idle_threshold_us, 10_000); // 10ms
        assert!(config.max_warp_factor.is_none());
    }

    #[test]
    fn test_warp_config_new() {
        let config = WarpConfig::new();
        assert_eq!(config.idle_threshold_us, 10_000);
    }

    #[test]
    fn test_warp_config_with_threshold() {
        let config = WarpConfig::with_threshold(50_000);
        assert_eq!(config.idle_threshold_us, 50_000);
        assert!(config.max_warp_factor.is_none());
    }

    #[test]
    fn test_warp_config_real_time() {
        let config = WarpConfig::real_time();
        assert_eq!(config.idle_threshold_us, i64::MAX);
    }

    #[test]
    fn test_warp_config_instant() {
        let config = WarpConfig::instant();
        assert_eq!(config.idle_threshold_us, 0);
    }

    #[test]
    fn test_warp_config_clone() {
        let config = WarpConfig::with_threshold(25_000);
        let cloned = config.clone();
        assert_eq!(config.idle_threshold_us, cloned.idle_threshold_us);
    }

    #[test]
    fn test_warp_config_debug() {
        let config = WarpConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("WarpConfig"));
        assert!(debug.contains("idle_threshold_us"));
    }

    // ============================================================
    // WARPSTATS TESTS
    // ============================================================

    #[test]
    fn test_warp_stats_new() {
        let stats = WarpStats::new();
        assert_eq!(stats.total_warped_us, 0);
        assert_eq!(stats.warp_count, 0);
    }

    #[test]
    fn test_warp_stats_default() {
        let stats = WarpStats::default();
        assert_eq!(stats.total_warped_us, 0);
        assert_eq!(stats.warp_count, 0);
    }

    #[test]
    fn test_warp_stats_time_saved() {
        let stats = WarpStats {
            total_warped_us: 1_000_000, // 1 second
            warp_count: 5,
        };
        assert_eq!(stats.time_saved().as_secs(), 1);
        assert_eq!(stats.time_saved().as_micros(), 1_000_000);
    }

    #[test]
    fn test_warp_stats_time_saved_zero() {
        let stats = WarpStats::new();
        assert_eq!(stats.time_saved().as_micros(), 0);
    }

    #[test]
    fn test_warp_stats_average_warp() {
        let stats = WarpStats {
            total_warped_us: 1_000_000,
            warp_count: 10,
        };
        assert_eq!(stats.average_warp_us(), 100_000);
    }

    #[test]
    fn test_warp_stats_average_warp_zero_count() {
        let stats = WarpStats::new();
        assert_eq!(stats.average_warp_us(), 0);
    }

    #[test]
    fn test_warp_stats_clone() {
        let stats = WarpStats {
            total_warped_us: 500,
            warp_count: 2,
        };
        let cloned = stats;
        assert_eq!(stats, cloned);
    }

    #[test]
    fn test_warp_stats_debug() {
        let stats = WarpStats::new();
        let debug = format!("{:?}", stats);
        assert!(debug.contains("WarpStats"));
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - BASIC
    // ============================================================

    #[test]
    fn test_scheduler_new() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert_eq!(scheduler.idle_threshold_us(), 10_000);
    }

    #[test]
    fn test_scheduler_default() {
        let scheduler = SkipIdleScheduler::default();
        assert_eq!(scheduler.idle_threshold_us(), 10_000);
    }

    #[test]
    fn test_scheduler_initial_stats() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let stats = scheduler.stats();
        assert_eq!(stats.total_warped_us, 0);
        assert_eq!(stats.warp_count, 0);
    }

    #[test]
    fn test_scheduler_config() {
        let config = WarpConfig::with_threshold(50_000);
        let scheduler = SkipIdleScheduler::new(config.clone());
        assert_eq!(scheduler.config().idle_threshold_us, 50_000);
    }

    #[test]
    fn test_scheduler_debug() {
        let scheduler = SkipIdleScheduler::default();
        let debug = format!("{:?}", scheduler);
        assert!(debug.contains("SkipIdleScheduler"));
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - SCHEDULING
    // ============================================================

    #[test]
    fn test_schedule_small_gap_no_warp() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default()); // 10ms threshold
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Gap of 1ms (1000us) - below 10ms threshold
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1000));

        assert!(result.advanced);
        assert!(!result.warped);
        assert_eq!(result.gap_us, 1000);
        assert_eq!(clock.now().as_micros(), 1000);

        // Stats should show no warp
        let stats = scheduler.stats();
        assert_eq!(stats.warp_count, 0);
    }

    #[test]
    fn test_schedule_large_gap_warp() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default()); // 10ms threshold
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Gap of 1 second (1,000,000us) - above 10ms threshold
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));

        assert!(result.advanced);
        assert!(result.warped);
        assert_eq!(result.gap_us, 1_000_000);
        assert_eq!(clock.now().as_micros(), 1_000_000);

        // Stats should show 1 warp
        let stats = scheduler.stats();
        assert_eq!(stats.warp_count, 1);
        assert_eq!(stats.total_warped_us, 1_000_000);
    }

    #[test]
    fn test_schedule_exact_threshold_no_warp() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::with_threshold(10_000));
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Exactly at threshold - should NOT warp (> not >=)
        let result = scheduler.schedule(&clock, Timestamp::from_micros(10_000));

        assert!(result.advanced);
        assert!(!result.warped);
        assert_eq!(clock.now().as_micros(), 10_000);
    }

    #[test]
    fn test_schedule_just_above_threshold_warp() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::with_threshold(10_000));
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Just above threshold - should warp
        let result = scheduler.schedule(&clock, Timestamp::from_micros(10_001));

        assert!(result.advanced);
        assert!(result.warped);
        assert_eq!(scheduler.stats().warp_count, 1);
    }

    #[test]
    fn test_schedule_past_event() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Event in the past
        let result = scheduler.schedule(&clock, Timestamp::from_micros(500));

        assert!(!result.advanced);
        assert!(!result.warped);
        assert!(result.gap_us < 0);
        assert_eq!(clock.now().as_micros(), 1000); // Clock unchanged
    }

    #[test]
    fn test_schedule_same_time() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // Same time
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1000));

        assert!(!result.advanced);
        assert!(!result.warped);
        assert_eq!(result.gap_us, 0);
    }

    #[test]
    fn test_schedule_warp_disabled() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Disable warp mode
        clock.disable_warp();

        // Large gap but warp disabled
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));

        assert!(result.advanced);
        assert!(!result.warped); // Should not warp
        assert_eq!(clock.now().as_micros(), 1_000_000);
        assert_eq!(scheduler.stats().warp_count, 0);
    }

    #[test]
    fn test_schedule_real_time_config() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::real_time());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Any gap with real_time config should not warp
        let result = scheduler.schedule(&clock, Timestamp::from_micros(100_000_000)); // 100 seconds

        assert!(result.advanced);
        assert!(!result.warped);
        assert_eq!(scheduler.stats().warp_count, 0);
    }

    #[test]
    fn test_schedule_instant_config() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::instant());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Any positive gap with instant config should warp
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1));

        assert!(result.advanced);
        assert!(result.warped);
        assert_eq!(scheduler.stats().warp_count, 1);
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - CUMULATIVE
    // ============================================================

    #[test]
    fn test_schedule_multiple_warps() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Warp 1: 0 -> 1,000,000 (1 second)
        scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
        // Warp 2: 1,000,000 -> 2,000,000 (1 second)
        scheduler.schedule(&clock, Timestamp::from_micros(2_000_000));
        // Warp 3: 2,000,000 -> 3,000,000 (1 second)
        scheduler.schedule(&clock, Timestamp::from_micros(3_000_000));

        let stats = scheduler.stats();
        assert_eq!(stats.warp_count, 3);
        assert_eq!(stats.total_warped_us, 3_000_000);
    }

    #[test]
    fn test_schedule_mixed_warps_and_normal() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::with_threshold(10_000));
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Small gap (5ms) - no warp
        scheduler.schedule(&clock, Timestamp::from_micros(5_000));
        assert_eq!(scheduler.stats().warp_count, 0);

        // Large gap (1s) - warp
        scheduler.schedule(&clock, Timestamp::from_micros(1_005_000));
        assert_eq!(scheduler.stats().warp_count, 1);

        // Small gap (3ms) - no warp
        scheduler.schedule(&clock, Timestamp::from_micros(1_008_000));
        assert_eq!(scheduler.stats().warp_count, 1);

        // Large gap (500ms) - warp
        scheduler.schedule(&clock, Timestamp::from_micros(1_508_000));
        assert_eq!(scheduler.stats().warp_count, 2);
    }

    #[test]
    fn test_schedule_high_volume() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::with_threshold(100));
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        let mut expected_warps = 0u64;
        for i in 1..=1000 {
            let gap = if i % 10 == 0 { 1000 } else { 50 }; // Every 10th is a warp
            let target = clock.now().as_micros() + gap;

            let result = scheduler.schedule(&clock, Timestamp::from_micros(target));
            if result.warped {
                expected_warps += 1;
            }
        }

        assert_eq!(scheduler.stats().warp_count, expected_warps);
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - RESET
    // ============================================================

    #[test]
    fn test_reset_clears_stats() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Do some warps
        scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
        scheduler.schedule(&clock, Timestamp::from_micros(2_000_000));

        assert!(scheduler.stats().warp_count > 0);
        assert!(scheduler.stats().total_warped_us > 0);

        // Reset
        scheduler.reset();

        assert_eq!(scheduler.stats().warp_count, 0);
        assert_eq!(scheduler.stats().total_warped_us, 0);
    }

    #[test]
    fn test_reset_preserves_config() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::with_threshold(50_000));
        scheduler.reset();
        assert_eq!(scheduler.idle_threshold_us(), 50_000);
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - WOULD_WARP
    // ============================================================

    #[test]
    fn test_would_warp_true() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert!(scheduler.would_warp(100_000, true)); // 100ms > 10ms threshold
    }

    #[test]
    fn test_would_warp_false_below_threshold() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert!(!scheduler.would_warp(5_000, true)); // 5ms < 10ms threshold
    }

    #[test]
    fn test_would_warp_false_warp_disabled() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert!(!scheduler.would_warp(100_000, false)); // Warp disabled
    }

    #[test]
    fn test_would_warp_negative_gap() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert!(!scheduler.would_warp(-1000, true)); // Negative gap
    }

    #[test]
    fn test_would_warp_zero_gap() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        assert!(!scheduler.would_warp(0, true)); // Zero gap
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - EDGE CASES
    // ============================================================

    #[test]
    fn test_schedule_timestamp_max() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        let result = scheduler.schedule(&clock, Timestamp::MAX);

        assert!(result.advanced);
        assert!(result.warped);
        assert_eq!(clock.now(), Timestamp::MAX);
    }

    #[test]
    fn test_schedule_timestamp_min_from_zero() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // MIN is negative, so from 0 it's in the past
        let result = scheduler.schedule(&clock, Timestamp::MIN);

        assert!(!result.advanced);
        assert!(!result.warped);
    }

    #[test]
    fn test_schedule_very_small_positive_gap() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::instant());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        let result = scheduler.schedule(&clock, Timestamp::from_micros(1)); // 1 microsecond

        assert!(result.advanced);
        assert!(result.warped); // instant config warps everything
        assert_eq!(clock.now().as_micros(), 1);
    }

    // ============================================================
    // SKIPIDLE SCHEDULER TESTS - THREAD SAFETY
    // ============================================================

    #[test]
    fn test_scheduler_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SkipIdleScheduler>();
    }

    #[test]
    fn test_scheduler_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<SkipIdleScheduler>();
    }

    #[test]
    fn test_scheduler_concurrent_stats() {
        let scheduler = Arc::new(SkipIdleScheduler::new(WarpConfig::default()));
        let mut handles = vec![];

        // Multiple threads reading stats
        for _ in 0..4 {
            let s = Arc::clone(&scheduler);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let _ = s.stats();
                    let _ = s.idle_threshold_us();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_scheduler_concurrent_reset() {
        let scheduler = Arc::new(SkipIdleScheduler::new(WarpConfig::default()));
        let mut handles = vec![];

        for _ in 0..4 {
            let s = Arc::clone(&scheduler);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    s.reset();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    // ============================================================
    // INTEGRATION TESTS: SkipIdleScheduler + SimulatedClock
    // ============================================================

    #[test]
    fn test_scheduler_24h_simulation() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default()); // 10ms threshold
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Simulate a simplified 24-hour session:
        // - 100 events per "hour" with 1ms gaps (active)
        // - 1 hour gap between "active" periods (idle)

        let events_per_hour = 100;
        let event_gap_us = 1_000; // 1ms between events
        let hour_gap_us = 3_600_000_000i64; // 1 hour
        let hours = 8; // 8 active hours with idle between

        let mut total_events = 0;
        let mut current_time: i64 = 0;

        for hour in 0..hours {
            // Active period: many small gaps
            for _ in 0..events_per_hour {
                current_time += event_gap_us;
                scheduler.schedule(&clock, Timestamp::from_micros(current_time));
                total_events += 1;
            }

            // Idle period (except after last hour)
            if hour < hours - 1 {
                current_time += hour_gap_us;
                scheduler.schedule(&clock, Timestamp::from_micros(current_time));
                total_events += 1;
            }
        }

        let stats = scheduler.stats();

        // Should have warped through the idle periods (7 transitions)
        assert!(
            stats.warp_count >= 7,
            "Expected at least 7 warps, got {}",
            stats.warp_count
        );

        // Total time saved should be significant (7 hours of idle)
        let expected_saved_us = 7 * hour_gap_us;
        assert!(
            stats.total_warped_us >= expected_saved_us / 2,
            "Expected significant time saved"
        );

        // Verify we processed all events
        assert_eq!(total_events, hours * events_per_hour + hours - 1);
    }

    #[test]
    fn test_scheduler_pause_respected() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Pause the clock
        clock.pause();

        // Schedule still works (just advances the clock)
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));

        // Clock is paused but we still advanced it
        assert!(result.advanced);
        assert_eq!(clock.now().as_micros(), 1_000_000);
        assert!(clock.is_paused()); // Still paused
    }

    #[test]
    fn test_scheduler_with_warp_toggle() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Warp enabled - should warp
        let result1 = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
        assert!(result1.warped);

        // Disable warp
        clock.disable_warp();

        // Large gap but no warp
        let result2 = scheduler.schedule(&clock, Timestamp::from_micros(2_000_000));
        assert!(result2.advanced);
        assert!(!result2.warped);

        // Enable warp again
        clock.enable_warp();

        // Should warp again
        let result3 = scheduler.schedule(&clock, Timestamp::from_micros(3_000_000));
        assert!(result3.warped);

        // Only 2 warps total
        assert_eq!(scheduler.stats().warp_count, 2);
    }

    // ============================================================
    // FAST-FORWARD (BOUNDED WARP) TESTS
    // ============================================================

    #[test]
    fn test_warp_config_with_max_factor() {
        let config = WarpConfig {
            idle_threshold_us: 10_000,
            max_warp_factor: Some(10.0),
        };
        assert_eq!(config.idle_threshold_us, 10_000);
        assert_eq!(config.max_warp_factor, Some(10.0));
    }

    #[test]
    fn test_warp_config_fast_forward() {
        let config = WarpConfig::fast_forward(10);
        assert_eq!(config.max_warp_factor, Some(10.0));
    }

    #[test]
    fn test_scheduler_bounded_warp_small_gap() {
        // With max_warp_factor=10, and threshold=10ms, max jump = 100ms per step
        let config = WarpConfig {
            idle_threshold_us: 10_000,   // 10ms threshold
            max_warp_factor: Some(10.0), // 10x speed
        };
        let scheduler = SkipIdleScheduler::new(config);
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Gap of 50ms - within bounded warp limit (100ms), should fully warp
        let result = scheduler.schedule(&clock, Timestamp::from_micros(50_000));

        assert!(result.advanced);
        assert!(result.warped);
        assert_eq!(clock.now().as_micros(), 50_000); // Jumped fully
    }

    #[test]
    fn test_scheduler_bounded_warp_large_gap() {
        // With max_warp_factor=10, and threshold=10ms, max jump = 100ms per step
        let config = WarpConfig {
            idle_threshold_us: 10_000,   // 10ms threshold
            max_warp_factor: Some(10.0), // 10x speed
        };
        let scheduler = SkipIdleScheduler::new(config);
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Gap of 1 second - exceeds bounded warp limit (100ms)
        let result = scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));

        assert!(result.advanced);
        assert!(result.warped);
        // Should be capped at max_warp_factor * threshold = 100ms
        assert_eq!(clock.now().as_micros(), 100_000);
        // Remaining time not warped is tracked separately
        assert_eq!(result.remaining_us, 900_000);
    }

    #[test]
    fn test_scheduler_bounded_warp_incremental() {
        // Simulate reaching target through multiple bounded warps
        let config = WarpConfig {
            idle_threshold_us: 10_000,   // 10ms threshold
            max_warp_factor: Some(10.0), // 10x speed, max 100ms per warp
        };
        let scheduler = SkipIdleScheduler::new(config);
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        let target = Timestamp::from_micros(500_000); // 500ms target

        // Should take 5 warps to reach target (100ms each)
        let mut warps = 0;
        while clock.now() < target {
            let result = scheduler.schedule(&clock, target);
            if result.warped {
                warps += 1;
            }
            if !result.advanced || warps > 10 {
                break; // Safety limit
            }
        }

        assert_eq!(clock.now().as_micros(), 500_000);
        assert_eq!(
            warps, 5,
            "Expected 5 warps to reach 500ms at 100ms per warp"
        );
    }

    #[test]
    fn test_scheduler_unbounded_vs_bounded() {
        let clock = SimulatedClock::new(Timestamp::from_micros(0));
        let target = Timestamp::from_micros(1_000_000);

        // Unbounded (instant)
        let unbounded = SkipIdleScheduler::new(WarpConfig::instant());
        let result = unbounded.schedule(&clock, target);
        assert_eq!(clock.now().as_micros(), 1_000_000); // Instant jump
        assert_eq!(result.remaining_us, 0);

        // Reset
        clock.set(Timestamp::from_micros(0));

        // Bounded (10x)
        let bounded = SkipIdleScheduler::new(WarpConfig::fast_forward(10));
        let result = bounded.schedule(&clock, target);
        // With 10x and default threshold, should be capped
        assert!(
            clock.now().as_micros() < 1_000_000,
            "Bounded warp should cap"
        );
        assert!(result.remaining_us > 0);
    }

    #[test]
    fn test_scheduler_set_max_warp_factor() {
        let scheduler = SkipIdleScheduler::new(WarpConfig::default());

        // Initially no factor
        assert!(scheduler.config().max_warp_factor.is_none());

        // Set factor
        scheduler.set_max_warp_factor(Some(5.0));
        assert_eq!(scheduler.max_warp_factor(), Some(5.0));

        // Clear factor
        scheduler.set_max_warp_factor(None);
        assert!(scheduler.max_warp_factor().is_none());
    }

    #[test]
    fn test_scheduler_fast_forward_1x_no_warp() {
        // 1x speed should be equivalent to real-time (no warp)
        let config = WarpConfig {
            idle_threshold_us: 10_000,
            max_warp_factor: Some(1.0),
        };
        let scheduler = SkipIdleScheduler::new(config);
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Gap of 50ms - above threshold but 1x means max 10ms per step
        let result = scheduler.schedule(&clock, Timestamp::from_micros(50_000));

        assert!(result.advanced);
        // With 1x factor, max advance is threshold * 1 = 10ms
        assert_eq!(clock.now().as_micros(), 10_000);
        assert_eq!(result.remaining_us, 40_000);
    }

    #[test]
    fn test_scheduler_fast_forward_stats() {
        let config = WarpConfig::fast_forward(10);
        let scheduler = SkipIdleScheduler::new(config);
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        // Do a bounded warp
        scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));

        let stats = scheduler.stats();
        assert!(stats.warp_count >= 1);
        // Warped time should be the amount actually advanced, not the full gap
        assert!(stats.total_warped_us <= 1_000_000);
    }
}
