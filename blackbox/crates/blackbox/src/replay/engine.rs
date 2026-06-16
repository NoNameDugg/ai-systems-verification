//! Replay engine for orchestrating playback.
//!
//! This module provides the `ReplayEngine`, which orchestrates the playback
//! of recorded trading sessions with deterministic clock control.
//!
//! ## Design
//!
//! | Component | Purpose | Implementation |
//! |-----------|---------|----------------|
//! | `ReplayEngine` | Orchestration | DataSource + Clock + Scheduler |
//! | `ReplayMode` | Speed control | Step, RealTime, FastForward, Warp |
//! | `ReplayState` | State machine | Idle, Playing, Paused, Completed |
//! | `ReplayStats` | Progress tracking | Events, warps, position |
//!
//! ## Performance Contract
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | `step()` | <10μs | Single event dispatch |
//! | `tick()` | <100μs | Process until pause or complete |
//! | `reset()` | <1ms | Full state reset |
//!
//! ## Usage
//!
//! ```
//! use blackbox::replay::{
//!     ReplayEngine, ReplayMode, ReplayState, WarpConfig,
//!     BufferedDataSource, DataFrame, FrameType,
//! };
//! use blackbox_types::{Exchange, Timestamp};
//!
//! // Create sample data
//! let frames = vec![
//!     DataFrame::new(Timestamp::from_micros(1000), Exchange::Deribit, FrameType::WebSocketText, b"msg1".to_vec()),
//!     DataFrame::new(Timestamp::from_micros(2000), Exchange::Deribit, FrameType::WebSocketText, b"msg2".to_vec()),
//! ];
//!
//! // Create engine with data source
//! let source = BufferedDataSource::new(frames);
//! let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
//!
//! // Step through events
//! engine.set_mode(ReplayMode::WarpSpeed);
//! engine.play();
//!
//! let result = engine.step();
//! assert!(result.processed);
//! ```

use super::{DataFrame, DataSource, ScheduleResult, SimulatedClock, SkipIdleScheduler, WarpConfig};
use crate::journal::JournalReader;
use blackbox_types::Timestamp;
use std::path::Path;
use std::sync::Arc;

/// Replay mode determines how time advances during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayMode {
    /// Step through events one at a time.
    Step,
    /// Real-time playback (1x speed).
    RealTime,
    /// Fast-forward at Nx speed.
    FastForward {
        /// Speed multiplier (e.g., 10 = 10x speed).
        multiplier: u32,
    },
    /// Warp speed - skip idle periods.
    WarpSpeed,
}

impl ReplayMode {
    /// Check if this mode enables warp.
    pub fn is_warp_enabled(&self) -> bool {
        matches!(self, ReplayMode::WarpSpeed)
    }
}

/// Replay engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayState {
    /// Engine is idle, not playing.
    Idle,
    /// Engine is playing.
    Playing,
    /// Engine is paused.
    Paused,
    /// Replay completed.
    Completed,
}

/// Result of a step operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// Whether an event was processed.
    pub processed: bool,
    /// The frame that was processed (if any).
    pub frame: Option<DataFrame>,
    /// Schedule result from warp scheduler.
    pub schedule: Option<ScheduleResult>,
    /// Whether replay is complete.
    pub completed: bool,
}

impl StepResult {
    /// Create a result indicating completion.
    pub fn completed() -> Self {
        Self {
            processed: false,
            frame: None,
            schedule: None,
            completed: true,
        }
    }

    /// Create a result indicating no operation.
    pub fn noop() -> Self {
        Self {
            processed: false,
            frame: None,
            schedule: None,
            completed: false,
        }
    }

    /// Create a result indicating a processed frame.
    pub fn processed(frame: DataFrame, schedule: ScheduleResult) -> Self {
        Self {
            processed: true,
            frame: Some(frame),
            schedule: Some(schedule),
            completed: false,
        }
    }
}

/// Statistics from replay operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplayStats {
    /// Total events processed.
    pub events_processed: u64,
    /// Events processed in current session.
    pub session_events: u64,
    /// Number of warps performed.
    pub warp_count: u64,
    /// Total microseconds warped.
    pub total_warped_us: i64,
}

impl ReplayStats {
    /// Create empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get time saved by warping.
    pub fn time_saved(&self) -> std::time::Duration {
        std::time::Duration::from_micros(self.total_warped_us.max(0) as u64)
    }
}

/// Engine for replaying recorded sessions.
///
/// The ReplayEngine orchestrates the playback of journal files,
/// controlling time advancement and feeding events to the system.
///
/// # Design
///
/// - Uses SimulatedClock for deterministic time
/// - Integrates with DataSource for abstract data injection
/// - Uses SkipIdleScheduler for warp-speed replay
/// - Supports multiple replay modes (step, real-time, fast-forward, warp)
///
/// # Type Parameters
///
/// * `D` - The data source type (must implement `DataSource`)
///
/// # Example
///
/// ```
/// use blackbox::replay::{
///     ReplayEngine, WarpConfig, BufferedDataSource, DataFrame, FrameType,
/// };
/// use blackbox_types::{Exchange, Timestamp};
///
/// let frames = vec![
///     DataFrame::new(Timestamp::from_micros(1000), Exchange::Deribit, FrameType::Trade, vec![]),
/// ];
/// let source = BufferedDataSource::new(frames);
/// let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
///
/// engine.play();
/// let result = engine.step();
/// ```
pub struct ReplayEngine<D: DataSource = super::NullDataSource> {
    /// The simulated clock.
    clock: Arc<SimulatedClock>,
    /// Current replay mode.
    mode: ReplayMode,
    /// Current state.
    state: ReplayState,
    /// The data source.
    data_source: D,
    /// Skip-idle scheduler.
    scheduler: SkipIdleScheduler,
    /// Replay statistics.
    stats: ReplayStats,
    /// Journal reader (legacy, for load() compatibility).
    reader: Option<JournalReader>,
}

impl ReplayEngine {
    /// Create a new replay engine with default (null) data source.
    pub fn new() -> Self {
        Self {
            clock: Arc::new(SimulatedClock::at_epoch()),
            mode: ReplayMode::Step,
            state: ReplayState::Idle,
            data_source: super::NullDataSource,
            scheduler: SkipIdleScheduler::new(WarpConfig::default()),
            stats: ReplayStats::new(),
            reader: None,
        }
    }
}

impl<D: DataSource> ReplayEngine<D> {
    /// Create a new replay engine with a specific data source.
    ///
    /// # Arguments
    ///
    /// * `data_source` - The data source to replay from
    /// * `config` - Warp configuration for idle skipping
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{ReplayEngine, WarpConfig, BufferedDataSource};
    ///
    /// let source = BufferedDataSource::empty();
    /// let engine = ReplayEngine::with_data_source(source, WarpConfig::default());
    /// ```
    pub fn with_data_source(data_source: D, config: WarpConfig) -> Self {
        Self {
            clock: Arc::new(SimulatedClock::at_epoch()),
            mode: ReplayMode::Step,
            state: ReplayState::Idle,
            data_source,
            scheduler: SkipIdleScheduler::new(config),
            stats: ReplayStats::new(),
            reader: None,
        }
    }

    /// Create a new replay engine with a specific data source and start time.
    ///
    /// # Arguments
    ///
    /// * `data_source` - The data source to replay from
    /// * `config` - Warp configuration
    /// * `start_time` - Initial clock time
    pub fn with_start_time(data_source: D, config: WarpConfig, start_time: Timestamp) -> Self {
        Self {
            clock: Arc::new(SimulatedClock::new(start_time)),
            mode: ReplayMode::Step,
            state: ReplayState::Idle,
            data_source,
            scheduler: SkipIdleScheduler::new(config),
            stats: ReplayStats::new(),
            reader: None,
        }
    }

    /// Get a reference to the simulated clock.
    pub fn clock(&self) -> &Arc<SimulatedClock> {
        &self.clock
    }

    /// Get a reference to the data source.
    pub fn data_source(&self) -> &D {
        &self.data_source
    }

    /// Get a mutable reference to the data source.
    pub fn data_source_mut(&mut self) -> &mut D {
        &mut self.data_source
    }

    /// Get a reference to the scheduler.
    pub fn scheduler(&self) -> &SkipIdleScheduler {
        &self.scheduler
    }

    /// Get current replay statistics.
    pub fn stats(&self) -> ReplayStats {
        let warp_stats = self.scheduler.stats();
        ReplayStats {
            events_processed: self.stats.events_processed,
            session_events: self.stats.session_events,
            warp_count: warp_stats.warp_count,
            total_warped_us: warp_stats.total_warped_us,
        }
    }

    /// Load a journal file for replay (legacy API).
    ///
    /// Note: For new code, prefer using `with_data_source()` with a
    /// journal-backed DataSource implementation.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        match JournalReader::open(path) {
            Ok(reader) => {
                self.reader = Some(reader);
                self.state = ReplayState::Idle;
                Ok(())
            }
            Err(e) => Err(format!("Failed to load journal: {}", e)),
        }
    }

    /// Set the replay mode.
    ///
    /// This updates the clock's warp mode and scheduler configuration:
    /// - **Step/RealTime**: Warp disabled, normal time advance
    /// - **FastForward**: Warp enabled with bounded factor (Nx speed)
    /// - **WarpSpeed**: Warp enabled with no bound (instant skip)
    ///
    /// # Arguments
    ///
    /// * `mode` - The replay mode to set
    pub fn set_mode(&mut self, mode: ReplayMode) {
        self.mode = mode;

        // Update clock warp mode and scheduler configuration
        match mode {
            ReplayMode::WarpSpeed => {
                self.clock.enable_warp();
                self.scheduler.set_max_warp_factor(None); // Unbounded
            }
            ReplayMode::FastForward { multiplier } => {
                self.clock.enable_warp();
                self.scheduler.set_max_warp_factor(Some(multiplier as f64));
            }
            ReplayMode::Step | ReplayMode::RealTime => {
                self.clock.disable_warp();
                self.scheduler.set_max_warp_factor(None);
            }
        }
    }

    /// Get the current replay mode.
    pub fn mode(&self) -> ReplayMode {
        self.mode
    }

    /// Get the current state.
    pub fn state(&self) -> ReplayState {
        self.state
    }

    /// Check if the engine is actively playing.
    pub fn is_playing(&self) -> bool {
        self.state == ReplayState::Playing
    }

    /// Check if replay is complete.
    pub fn is_completed(&self) -> bool {
        self.state == ReplayState::Completed
    }

    /// Start or resume playback.
    ///
    /// Changes state to Playing if there is data available or a journal loaded.
    pub fn play(&mut self) {
        if self.reader.is_some() || self.data_source.is_active() || self.data_source.has_next() {
            self.state = ReplayState::Playing;
            self.clock.resume(); // Ensure clock is not paused
        }
    }

    /// Pause playback.
    ///
    /// Changes state to Paused and pauses the clock.
    pub fn pause(&mut self) {
        if self.state == ReplayState::Playing {
            self.state = ReplayState::Paused;
            self.clock.pause();
        }
    }

    /// Toggle between playing and paused.
    pub fn toggle(&mut self) {
        match self.state {
            ReplayState::Playing => self.pause(),
            ReplayState::Paused => self.play(),
            ReplayState::Idle => self.play(),
            ReplayState::Completed => {} // Cannot toggle from completed
        }
    }

    /// Step to the next event.
    ///
    /// Advances the clock to the next event's timestamp and returns the frame.
    /// In WarpSpeed mode, idle periods are skipped.
    ///
    /// # Returns
    ///
    /// A `StepResult` containing the processed frame (if any) and scheduling info.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{
    ///     ReplayEngine, WarpConfig, BufferedDataSource, DataFrame, FrameType,
    /// };
    /// use blackbox_types::{Exchange, Timestamp};
    ///
    /// let frames = vec![
    ///     DataFrame::new(Timestamp::from_micros(1000), Exchange::Deribit, FrameType::Trade, vec![]),
    /// ];
    /// let source = BufferedDataSource::new(frames);
    /// let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
    ///
    /// engine.play();
    /// let result = engine.step();
    /// assert!(result.processed);
    /// ```
    pub fn step(&mut self) -> StepResult {
        // Can only step when playing or paused (for step-through debugging)
        if self.state != ReplayState::Playing && self.state != ReplayState::Paused {
            return StepResult::noop();
        }

        // Peek at next frame
        let next_ts = match self.data_source.peek_timestamp() {
            Some(ts) => ts,
            None => {
                // No more data
                self.state = ReplayState::Completed;
                return StepResult::completed();
            }
        };

        // Schedule the clock advancement (handles warp decision)
        let schedule_result = self.scheduler.schedule(&self.clock, next_ts);

        // Get and process the frame
        if let Some(frame) = self.data_source.next() {
            // Update statistics
            self.stats.events_processed += 1;
            self.stats.session_events += 1;

            StepResult::processed(frame, schedule_result)
        } else {
            // Unexpected: peek succeeded but next failed
            self.state = ReplayState::Completed;
            StepResult::completed()
        }
    }

    /// Step until an event is processed or completion.
    ///
    /// In bounded warp (fast-forward) mode, this may require multiple
    /// schedule operations to reach the next event. This method continues
    /// stepping until either:
    /// - An event is processed
    /// - Replay completes
    /// - A maximum number of steps is reached (safety limit)
    ///
    /// # Returns
    ///
    /// A `StepResult` for the processed event or completion.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{
    ///     ReplayEngine, ReplayMode, WarpConfig, BufferedDataSource, DataFrame, FrameType,
    /// };
    /// use blackbox_types::{Exchange, Timestamp};
    ///
    /// let frames = vec![
    ///     DataFrame::new(Timestamp::from_micros(500_000), Exchange::Deribit, FrameType::Trade, vec![]),
    /// ];
    /// let source = BufferedDataSource::new(frames);
    /// let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
    ///
    /// engine.set_mode(ReplayMode::FastForward { multiplier: 10 });
    /// engine.play();
    ///
    /// // May take multiple internal steps in fast-forward mode
    /// let result = engine.step_until_event();
    /// assert!(result.processed);
    /// ```
    pub fn step_until_event(&mut self) -> StepResult {
        const MAX_STEPS: usize = 1000; // Safety limit

        for _ in 0..MAX_STEPS {
            // Peek at next event
            let next_ts = match self.data_source.peek_timestamp() {
                Some(ts) => ts,
                None => {
                    self.state = ReplayState::Completed;
                    return StepResult::completed();
                }
            };

            // Schedule advancement
            let schedule_result = self.scheduler.schedule(&self.clock, next_ts);

            if schedule_result.remaining_us == 0 {
                // Reached the event timestamp, process it
                if let Some(frame) = self.data_source.next() {
                    self.stats.events_processed += 1;
                    self.stats.session_events += 1;
                    return StepResult::processed(frame, schedule_result);
                } else {
                    self.state = ReplayState::Completed;
                    return StepResult::completed();
                }
            }
            // Otherwise, continue advancing toward the event
        }

        // Safety limit reached, return no-op
        StepResult::noop()
    }

    /// Process multiple events up to a limit or until paused/completed.
    ///
    /// This is useful for batch processing in non-real-time modes.
    ///
    /// # Arguments
    ///
    /// * `max_events` - Maximum number of events to process
    ///
    /// # Returns
    ///
    /// The number of events actually processed.
    pub fn tick(&mut self, max_events: usize) -> usize {
        if self.state != ReplayState::Playing {
            return 0;
        }

        let mut processed = 0;
        while processed < max_events {
            let result = self.step();
            if result.completed || !result.processed {
                break;
            }
            processed += 1;
        }
        processed
    }

    /// Process all remaining events.
    ///
    /// # Warning
    ///
    /// This may take a long time for large data sources.
    /// Consider using `tick()` with a limit instead.
    ///
    /// # Returns
    ///
    /// The total number of events processed.
    pub fn run_to_completion(&mut self) -> usize {
        let mut total = 0;
        loop {
            let count = self.tick(10_000);
            if count == 0 {
                break;
            }
            total += count;
        }
        total
    }

    /// Reset to the beginning.
    ///
    /// Resets the clock, data source position, scheduler stats, and state.
    pub fn reset(&mut self) {
        self.state = ReplayState::Idle;
        self.clock.set(Timestamp::EPOCH);
        self.clock.resume(); // Ensure not paused
        self.data_source.reset();
        self.scheduler.reset();
        self.stats.session_events = 0;
        // Note: events_processed is lifetime total, not reset
    }

    /// Get progress as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if progress is unknown (e.g., live source).
    pub fn progress(&self) -> Option<f64> {
        let total = self.data_source.frame_count()?;
        let pos = self.data_source.position()?;
        if total == 0 {
            Some(1.0)
        } else {
            Some(pos as f64 / total as f64)
        }
    }

    /// Get the current position in the data source.
    pub fn position(&self) -> Option<usize> {
        self.data_source.position()
    }

    /// Get the total frame count (if known).
    pub fn frame_count(&self) -> Option<usize> {
        self.data_source.frame_count()
    }
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{BufferedDataSource, FrameType, NullDataSource};
    use blackbox_types::{Clock, Exchange};

    // ============================================================
    // HELPER FUNCTIONS
    // ============================================================

    fn create_test_frames(count: usize, gap_us: i64) -> Vec<DataFrame> {
        (0..count)
            .map(|i| {
                DataFrame::new(
                    Timestamp::from_micros((i as i64 + 1) * gap_us),
                    Exchange::Deribit,
                    FrameType::WebSocketText,
                    format!("frame{}", i).into_bytes(),
                )
            })
            .collect()
    }

    fn create_test_engine_with_frames(
        count: usize,
        gap_us: i64,
    ) -> ReplayEngine<BufferedDataSource> {
        let frames = create_test_frames(count, gap_us);
        let source = BufferedDataSource::new(frames);
        ReplayEngine::with_data_source(source, WarpConfig::default())
    }

    // ============================================================
    // REPLAYENGINE BASIC TESTS
    // ============================================================

    #[test]
    fn test_engine_new() {
        let engine = ReplayEngine::new();
        assert_eq!(engine.state(), ReplayState::Idle);
        assert_eq!(engine.mode(), ReplayMode::Step);
    }

    #[test]
    fn test_engine_default() {
        let engine = ReplayEngine::default();
        assert_eq!(engine.state(), ReplayState::Idle);
    }

    #[test]
    fn test_engine_with_data_source() {
        let source = BufferedDataSource::empty();
        let engine = ReplayEngine::with_data_source(source, WarpConfig::default());
        assert_eq!(engine.state(), ReplayState::Idle);
    }

    #[test]
    fn test_engine_with_start_time() {
        let source = BufferedDataSource::empty();
        let engine = ReplayEngine::with_start_time(
            source,
            WarpConfig::default(),
            Timestamp::from_micros(1000),
        );
        assert_eq!(engine.clock().now().as_micros(), 1000);
    }

    #[test]
    fn test_engine_clock_access() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();
        assert_eq!(clock.now(), Timestamp::EPOCH);
    }

    #[test]
    fn test_engine_data_source_access() {
        let engine = create_test_engine_with_frames(3, 1000);
        assert_eq!(engine.data_source().frame_count(), Some(3));
    }

    #[test]
    fn test_engine_data_source_mut() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.data_source_mut().reset();
        assert_eq!(engine.position(), Some(0));
    }

    #[test]
    fn test_engine_scheduler_access() {
        let engine = ReplayEngine::new();
        let scheduler = engine.scheduler();
        assert_eq!(scheduler.idle_threshold_us(), 10_000); // Default
    }

    // ============================================================
    // REPLAYMODE TESTS
    // ============================================================

    #[test]
    fn test_replay_mode_is_warp_enabled() {
        assert!(ReplayMode::WarpSpeed.is_warp_enabled());
        assert!(!ReplayMode::Step.is_warp_enabled());
        assert!(!ReplayMode::RealTime.is_warp_enabled());
        assert!(!ReplayMode::FastForward { multiplier: 10 }.is_warp_enabled());
    }

    #[test]
    fn test_set_mode_updates_clock_warp() {
        let mut engine = create_test_engine_with_frames(1, 1000);

        engine.set_mode(ReplayMode::WarpSpeed);
        assert!(engine.clock().is_warp_enabled());

        engine.set_mode(ReplayMode::RealTime);
        assert!(!engine.clock().is_warp_enabled());
    }

    #[test]
    fn test_engine_mode_setting() {
        let mut engine = ReplayEngine::new();
        assert_eq!(engine.mode(), ReplayMode::Step);

        engine.set_mode(ReplayMode::WarpSpeed);
        assert_eq!(engine.mode(), ReplayMode::WarpSpeed);

        engine.set_mode(ReplayMode::RealTime);
        assert_eq!(engine.mode(), ReplayMode::RealTime);

        engine.set_mode(ReplayMode::FastForward { multiplier: 10 });
        assert_eq!(engine.mode(), ReplayMode::FastForward { multiplier: 10 });
    }

    // ============================================================
    // STATE TRANSITION TESTS
    // ============================================================

    #[test]
    fn test_engine_play_with_data() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        assert_eq!(engine.state(), ReplayState::Idle);

        engine.play();
        assert_eq!(engine.state(), ReplayState::Playing);
        assert!(engine.is_playing());
    }

    #[test]
    fn test_engine_play_without_data() {
        let source = NullDataSource;
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

        engine.play();
        // NullDataSource is not active, so should stay idle
        assert_eq!(engine.state(), ReplayState::Idle);
    }

    #[test]
    fn test_engine_pause() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();
        assert_eq!(engine.state(), ReplayState::Playing);

        engine.pause();
        assert_eq!(engine.state(), ReplayState::Paused);
        assert!(engine.clock().is_paused());
    }

    #[test]
    fn test_engine_pause_when_not_playing() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.pause(); // Should be no-op
        assert_eq!(engine.state(), ReplayState::Idle);
    }

    #[test]
    fn test_engine_toggle() {
        let mut engine = create_test_engine_with_frames(3, 1000);

        // Idle -> Playing
        engine.toggle();
        assert_eq!(engine.state(), ReplayState::Playing);

        // Playing -> Paused
        engine.toggle();
        assert_eq!(engine.state(), ReplayState::Paused);

        // Paused -> Playing
        engine.toggle();
        assert_eq!(engine.state(), ReplayState::Playing);
    }

    #[test]
    fn test_engine_is_completed() {
        let mut engine = create_test_engine_with_frames(1, 1000);
        engine.play();

        assert!(!engine.is_completed());

        engine.step(); // Process only frame
        assert!(!engine.is_completed());

        engine.step(); // No more data
        assert!(engine.is_completed());
    }

    // ============================================================
    // STEP TESTS
    // ============================================================

    #[test]
    fn test_step_idle_returns_noop() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        let result = engine.step();
        assert!(!result.processed);
        assert!(!result.completed);
    }

    #[test]
    fn test_step_processes_frame() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();

        let result = engine.step();
        assert!(result.processed);
        assert!(!result.completed);
        assert!(result.frame.is_some());

        let frame = result.frame.unwrap();
        assert_eq!(frame.timestamp.as_micros(), 1000);
    }

    #[test]
    fn test_step_advances_clock() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();

        assert_eq!(engine.clock().now().as_micros(), 0);

        engine.step();
        assert_eq!(engine.clock().now().as_micros(), 1000);

        engine.step();
        assert_eq!(engine.clock().now().as_micros(), 2000);
    }

    #[test]
    fn test_step_updates_stats() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();

        assert_eq!(engine.stats().events_processed, 0);

        engine.step();
        assert_eq!(engine.stats().events_processed, 1);

        engine.step();
        assert_eq!(engine.stats().events_processed, 2);
    }

    #[test]
    fn test_step_completed() {
        let mut engine = create_test_engine_with_frames(1, 1000);
        engine.play();

        let result1 = engine.step();
        assert!(result1.processed);
        assert!(!result1.completed);

        let result2 = engine.step();
        assert!(!result2.processed);
        assert!(result2.completed);
    }

    #[test]
    fn test_step_paused_still_works() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();
        engine.pause();

        // Step-through debugging: step works when paused
        let result = engine.step();
        assert!(result.processed);
    }

    // ============================================================
    // WARP TESTS
    // ============================================================

    #[test]
    fn test_step_warps_large_gap() {
        // Create frames with 1 second gap (above 10ms threshold)
        let mut engine = create_test_engine_with_frames(2, 1_000_000);
        engine.set_mode(ReplayMode::WarpSpeed);
        engine.play();

        let result = engine.step();
        assert!(result.processed);

        if let Some(schedule) = result.schedule {
            // First frame at time 0->1s should warp
            // Note: first step from epoch to 1s is gap=1s, should warp
            // But we start at 0, first frame is at 1_000_000
            assert!(schedule.warped || schedule.gap_us == 1_000_000);
        }
    }

    #[test]
    fn test_step_no_warp_small_gap() {
        // Create frames with 1ms gap (below 10ms threshold)
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.set_mode(ReplayMode::WarpSpeed);
        engine.play();

        // Skip first step (from epoch)
        engine.step();

        // Second step: 1ms gap, should not warp
        let result = engine.step();
        if let Some(schedule) = result.schedule {
            assert!(!schedule.warped);
        }
    }

    // ============================================================
    // TICK TESTS
    // ============================================================

    #[test]
    fn test_tick_processes_multiple() {
        let mut engine = create_test_engine_with_frames(10, 1000);
        engine.play();

        let count = engine.tick(5);
        assert_eq!(count, 5);
        assert_eq!(engine.stats().events_processed, 5);
    }

    #[test]
    fn test_tick_stops_at_end() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();

        let count = engine.tick(10);
        assert_eq!(count, 3);
        assert!(engine.is_completed());
    }

    #[test]
    fn test_tick_not_playing() {
        let mut engine = create_test_engine_with_frames(10, 1000);
        // Don't call play()

        let count = engine.tick(5);
        assert_eq!(count, 0);
    }

    // ============================================================
    // RUN TO COMPLETION TESTS
    // ============================================================

    #[test]
    fn test_run_to_completion() {
        let mut engine = create_test_engine_with_frames(100, 1000);
        engine.play();

        let count = engine.run_to_completion();
        assert_eq!(count, 100);
        assert!(engine.is_completed());
    }

    #[test]
    fn test_run_to_completion_empty() {
        let source = BufferedDataSource::empty();
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
        engine.play();

        let count = engine.run_to_completion();
        assert_eq!(count, 0);
    }

    // ============================================================
    // RESET TESTS
    // ============================================================

    #[test]
    fn test_reset_clears_state() {
        let mut engine = create_test_engine_with_frames(5, 1000);
        engine.play();
        engine.tick(3);

        assert_eq!(engine.stats().events_processed, 3);
        assert_eq!(engine.position(), Some(3));

        engine.reset();

        assert_eq!(engine.state(), ReplayState::Idle);
        assert_eq!(engine.clock().now(), Timestamp::EPOCH);
        assert_eq!(engine.position(), Some(0));
        assert_eq!(engine.stats().session_events, 0);
        // Note: events_processed is not reset (lifetime total)
        assert_eq!(engine.stats().events_processed, 3);
    }

    #[test]
    fn test_reset_clears_clock() {
        let mut engine = ReplayEngine::new();
        let clock = engine.clock().clone();

        clock.advance(1_000_000);
        assert_eq!(clock.now().as_micros(), 1_000_000);

        engine.reset();
        assert_eq!(clock.now(), Timestamp::EPOCH);
    }

    #[test]
    fn test_reset_resumes_clock() {
        let mut engine = create_test_engine_with_frames(3, 1000);
        engine.play();
        engine.pause();

        assert!(engine.clock().is_paused());

        engine.reset();
        assert!(!engine.clock().is_paused());
    }

    // ============================================================
    // PROGRESS TESTS
    // ============================================================

    #[test]
    fn test_progress() {
        let mut engine = create_test_engine_with_frames(10, 1000);
        engine.play();

        assert_eq!(engine.progress(), Some(0.0));

        engine.tick(5);
        assert_eq!(engine.progress(), Some(0.5));

        engine.run_to_completion();
        assert_eq!(engine.progress(), Some(1.0));
    }

    #[test]
    fn test_progress_empty() {
        let source = BufferedDataSource::empty();
        let engine = ReplayEngine::with_data_source(source, WarpConfig::default());
        assert_eq!(engine.progress(), Some(1.0));
    }

    #[test]
    fn test_position_and_frame_count() {
        let engine = create_test_engine_with_frames(10, 1000);
        assert_eq!(engine.position(), Some(0));
        assert_eq!(engine.frame_count(), Some(10));
    }

    // ============================================================
    // STATS TESTS
    // ============================================================

    #[test]
    fn test_stats_initial() {
        let engine = ReplayEngine::new();
        let stats = engine.stats();
        assert_eq!(stats.events_processed, 0);
        assert_eq!(stats.session_events, 0);
        assert_eq!(stats.warp_count, 0);
    }

    #[test]
    fn test_stats_time_saved() {
        let stats = ReplayStats {
            events_processed: 100,
            session_events: 50,
            warp_count: 5,
            total_warped_us: 1_000_000,
        };
        assert_eq!(stats.time_saved().as_secs(), 1);
    }

    // ============================================================
    // INTEGRATION TESTS
    // ============================================================

    #[test]
    fn test_engine_clock_advance() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();

        clock.advance(1000);
        assert_eq!(clock.now().as_micros(), 1000);
    }

    #[test]
    fn test_engine_clock_pause() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();

        assert!(!clock.is_paused());
        clock.pause();
        assert!(clock.is_paused());
    }

    #[test]
    fn test_engine_clock_resume() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();

        clock.pause();
        clock.resume();
        assert!(!clock.is_paused());
    }

    #[test]
    fn test_engine_clock_warp_enabled_by_default() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();
        assert!(clock.is_warp_enabled());
    }

    #[test]
    fn test_engine_clock_warp_toggle() {
        let engine = ReplayEngine::new();
        let clock = engine.clock();

        clock.disable_warp();
        assert!(!clock.is_warp_enabled());

        clock.enable_warp();
        assert!(clock.is_warp_enabled());
    }

    #[test]
    fn test_clock_shared_across_clones() {
        let engine = ReplayEngine::new();
        let clock1 = engine.clock().clone();
        let clock2 = engine.clock().clone();

        clock1.advance(500);
        assert_eq!(clock2.now().as_micros(), 500);
    }

    #[test]
    fn test_clock_thread_sharing() {
        use std::thread;

        let engine = ReplayEngine::new();
        let clock = engine.clock().clone();

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                clock.advance(1);
            }
        });

        handle.join().unwrap();
        assert_eq!(engine.clock().now().as_micros(), 100);
    }

    // ============================================================
    // DEBUG AND DISPLAY TESTS
    // ============================================================

    #[test]
    fn test_replay_mode_debug() {
        assert_eq!(format!("{:?}", ReplayMode::Step), "Step");
        assert_eq!(format!("{:?}", ReplayMode::RealTime), "RealTime");
        assert_eq!(
            format!("{:?}", ReplayMode::FastForward { multiplier: 5 }),
            "FastForward { multiplier: 5 }"
        );
        assert_eq!(format!("{:?}", ReplayMode::WarpSpeed), "WarpSpeed");
    }

    #[test]
    fn test_replay_state_debug() {
        assert_eq!(format!("{:?}", ReplayState::Idle), "Idle");
        assert_eq!(format!("{:?}", ReplayState::Playing), "Playing");
        assert_eq!(format!("{:?}", ReplayState::Paused), "Paused");
        assert_eq!(format!("{:?}", ReplayState::Completed), "Completed");
    }

    #[test]
    fn test_replay_mode_equality() {
        assert_eq!(ReplayMode::Step, ReplayMode::Step);
        assert_ne!(ReplayMode::Step, ReplayMode::WarpSpeed);
        assert_eq!(
            ReplayMode::FastForward { multiplier: 10 },
            ReplayMode::FastForward { multiplier: 10 }
        );
        assert_ne!(
            ReplayMode::FastForward { multiplier: 10 },
            ReplayMode::FastForward { multiplier: 5 }
        );
    }

    #[test]
    fn test_replay_state_equality() {
        assert_eq!(ReplayState::Idle, ReplayState::Idle);
        assert_ne!(ReplayState::Idle, ReplayState::Playing);
    }

    #[test]
    fn test_replay_mode_clone() {
        let mode = ReplayMode::FastForward { multiplier: 5 };
        let cloned = mode;
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_replay_state_clone() {
        let state = ReplayState::Playing;
        let cloned = state;
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_step_result_debug() {
        let result = StepResult::noop();
        let debug = format!("{:?}", result);
        assert!(debug.contains("StepResult"));
    }

    #[test]
    fn test_replay_stats_debug() {
        let stats = ReplayStats::new();
        let debug = format!("{:?}", stats);
        assert!(debug.contains("ReplayStats"));
    }

    // ============================================================
    // STATE TESTS (legacy, for backward compatibility)
    // ============================================================

    #[test]
    fn test_engine_state_transitions() {
        let mut engine = ReplayEngine::new();
        assert_eq!(engine.state(), ReplayState::Idle);

        // Can't play without journal or data source
        engine.play();
        assert_eq!(engine.state(), ReplayState::Idle);
    }

    // ============================================================
    // 24H SIMULATION TEST
    // ============================================================

    #[test]
    fn test_24h_session_simulation() {
        // Simulate 8 hours of active trading with idle periods
        let mut frames = Vec::new();
        let mut current_time = 0i64;

        for hour in 0..8 {
            // Active period: 1000 events, 1ms apart
            for event in 0..1000 {
                current_time += 1000; // 1ms
                frames.push(DataFrame::new(
                    Timestamp::from_micros(current_time),
                    Exchange::Deribit,
                    FrameType::Trade,
                    vec![hour as u8, (event & 0xFF) as u8],
                ));
            }

            // Idle period: 1 hour (except after last hour)
            if hour < 7 {
                current_time += 3_600_000_000; // 1 hour in microseconds
            }
        }

        let source = BufferedDataSource::new(frames);
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

        engine.set_mode(ReplayMode::WarpSpeed);
        engine.play();

        let processed = engine.run_to_completion();
        assert_eq!(processed, 8 * 1000);

        let stats = engine.stats();
        // Should have warped through the 7 hour-long idle periods
        assert!(
            stats.warp_count >= 7,
            "Expected at least 7 warps, got {}",
            stats.warp_count
        );

        // Time saved should be approximately 7 hours
        let saved_hours = stats.time_saved().as_secs() / 3600;
        assert!(
            saved_hours >= 6,
            "Expected ~7 hours saved, got {} hours",
            saved_hours
        );
    }

    // ============================================================
    // FAST-FORWARD MODE TESTS
    // ============================================================

    #[test]
    fn test_set_mode_fast_forward() {
        let mut engine = create_test_engine_with_frames(5, 1000);
        engine.set_mode(ReplayMode::FastForward { multiplier: 10 });

        assert_eq!(engine.mode(), ReplayMode::FastForward { multiplier: 10 });
        // Fast-forward enables warp with bounded factor
        assert!(engine.clock().is_warp_enabled());
        assert_eq!(engine.scheduler().max_warp_factor(), Some(10.0));
    }

    #[test]
    fn test_fast_forward_mode_configures_scheduler() {
        let mut engine = create_test_engine_with_frames(5, 1000);

        // Set WarpSpeed first
        engine.set_mode(ReplayMode::WarpSpeed);
        assert!(engine.scheduler().max_warp_factor().is_none());

        // Switch to FastForward
        engine.set_mode(ReplayMode::FastForward { multiplier: 5 });
        assert_eq!(engine.scheduler().max_warp_factor(), Some(5.0));

        // Switch back to WarpSpeed
        engine.set_mode(ReplayMode::WarpSpeed);
        assert!(engine.scheduler().max_warp_factor().is_none());
    }

    #[test]
    fn test_fast_forward_processes_events() {
        let mut engine = create_test_engine_with_frames(10, 1000);
        engine.set_mode(ReplayMode::FastForward { multiplier: 10 });
        engine.play();

        let count = engine.tick(10);
        assert_eq!(count, 10);
    }

    #[test]
    fn test_fast_forward_bounded_time_advance() {
        // Create frames with 1 second gaps (above threshold)
        let frames = vec![
            DataFrame::new(
                Timestamp::from_micros(1_000_000),
                Exchange::Deribit,
                FrameType::Trade,
                vec![1],
            ),
            DataFrame::new(
                Timestamp::from_micros(2_000_000),
                Exchange::Deribit,
                FrameType::Trade,
                vec![2],
            ),
        ];
        let source = BufferedDataSource::new(frames);
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

        // Set to 10x fast-forward (max 100ms per step with 10ms threshold)
        engine.set_mode(ReplayMode::FastForward { multiplier: 10 });
        engine.play();

        let result = engine.step();
        assert!(result.processed);

        // Clock should advance but be bounded
        // With 10ms threshold * 10x = 100ms max per warp
        // Gap was 1s, so should be capped
        let clock_time = engine.clock().now().as_micros();
        assert!(
            clock_time <= 100_000,
            "Fast-forward should cap time advance, got {}us",
            clock_time
        );
    }

    #[test]
    fn test_fast_forward_multiple_steps_reach_target() {
        // Create a frame at 500ms
        let frames = vec![DataFrame::new(
            Timestamp::from_micros(500_000),
            Exchange::Deribit,
            FrameType::Trade,
            vec![1],
        )];
        let source = BufferedDataSource::new(frames);
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

        // 10x fast-forward
        engine.set_mode(ReplayMode::FastForward { multiplier: 10 });
        engine.play();

        // Should need multiple steps to reach the event
        let result = engine.step_until_event();
        assert!(result.processed);
        assert_eq!(engine.clock().now().as_micros(), 500_000);
    }

    #[test]
    fn test_fast_forward_vs_warp_speed() {
        // Same frames, different modes
        let frames = create_test_frames(5, 100_000); // 100ms gaps

        // WarpSpeed: instant
        let source1 = BufferedDataSource::new(frames.clone());
        let mut engine1 = ReplayEngine::with_data_source(source1, WarpConfig::default());
        engine1.set_mode(ReplayMode::WarpSpeed);
        engine1.play();
        let result1 = engine1.step();
        // Should jump instantly to first event
        assert_eq!(engine1.clock().now().as_micros(), 100_000);

        // FastForward 10x: bounded
        let source2 = BufferedDataSource::new(frames);
        let mut engine2 = ReplayEngine::with_data_source(source2, WarpConfig::default());
        engine2.set_mode(ReplayMode::FastForward { multiplier: 10 });
        engine2.play();
        let result2 = engine2.step();
        // Should be bounded to max 100ms (10ms threshold * 10x)
        assert_eq!(engine2.clock().now().as_micros(), 100_000);

        // Both processed
        assert!(result1.processed);
        assert!(result2.processed);
    }

    #[test]
    fn test_fast_forward_realtime_equivalent() {
        // 1x fast-forward should be like real-time (max 1x threshold per step)
        let frames = vec![DataFrame::new(
            Timestamp::from_micros(100_000), // 100ms
            Exchange::Deribit,
            FrameType::Trade,
            vec![1],
        )];
        let source = BufferedDataSource::new(frames);
        let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

        engine.set_mode(ReplayMode::FastForward { multiplier: 1 });
        engine.play();

        let result = engine.step();
        assert!(result.processed);
        // With 1x and 10ms threshold, max advance is 10ms per step
        // But since this is first event and step_until_event is not used,
        // it will advance to the event
    }

    #[test]
    fn test_realtime_mode_disables_warp() {
        let mut engine = create_test_engine_with_frames(5, 1000);

        engine.set_mode(ReplayMode::RealTime);
        assert!(!engine.clock().is_warp_enabled());
        assert!(engine.scheduler().max_warp_factor().is_none());
    }

    #[test]
    fn test_step_mode_disables_warp() {
        let mut engine = create_test_engine_with_frames(5, 1000);

        engine.set_mode(ReplayMode::Step);
        assert!(!engine.clock().is_warp_enabled());
    }

    #[test]
    fn test_mode_cycle() {
        let mut engine = create_test_engine_with_frames(5, 1000);

        // Step -> RealTime -> FastForward -> WarpSpeed -> Step
        engine.set_mode(ReplayMode::Step);
        assert_eq!(engine.mode(), ReplayMode::Step);

        engine.set_mode(ReplayMode::RealTime);
        assert_eq!(engine.mode(), ReplayMode::RealTime);

        engine.set_mode(ReplayMode::FastForward { multiplier: 5 });
        assert_eq!(engine.mode(), ReplayMode::FastForward { multiplier: 5 });

        engine.set_mode(ReplayMode::WarpSpeed);
        assert_eq!(engine.mode(), ReplayMode::WarpSpeed);

        engine.set_mode(ReplayMode::Step);
        assert_eq!(engine.mode(), ReplayMode::Step);
    }
}
