//! Replay module - Clock control and replay engine.
//!
//! This module provides the infrastructure for deterministic replay:
//! - SimulatedClock for controllable time
//! - ReplayEngine for orchestrating playback
//! - DataSource trait for abstract data injection
//! - SkipIdleScheduler for warp-speed replay
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │ JournalFile │────▶│ DataSource   │────▶│ ReplayEngine│
//! └─────────────┘     │ (abstract)   │     └─────────────┘
//!                     └──────────────┘           │
//! ┌─────────────┐           │              ┌─────▼───────┐
//! │ WebSocket   │───────────┘              │ Simulated   │
//! └─────────────┘                          │ Clock       │
//!                                          └─────┬───────┘
//!                                                │
//!                                          ┌─────▼───────┐
//!                                          │ SkipIdle    │
//!                                          │ Scheduler   │
//!                                          └─────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust
//! use blackbox::replay::{DataSource, NullDataSource, BufferedDataSource};
//!
//! // Use NullDataSource for disabled/testing
//! fn process<D: DataSource>(source: &mut D) {
//!     while let Some(frame) = source.next() {
//!         // Process frame...
//!     }
//! }
//! ```
//!
//! ## Warp-Speed Replay
//!
//! ```rust
//! use blackbox::replay::{SkipIdleScheduler, WarpConfig, SimulatedClock};
//! use blackbox_types::{Clock, Timestamp};
//!
//! // Create scheduler with 10ms threshold
//! let scheduler = SkipIdleScheduler::new(WarpConfig::default());
//! let clock = SimulatedClock::new(Timestamp::from_micros(0));
//!
//! // Gaps > 10ms will be warped through
//! scheduler.schedule(&clock, Timestamp::from_micros(1_000_000));
//! assert_eq!(clock.now().as_micros(), 1_000_000);
//! ```

mod data_source;
mod engine;
mod scheduler;
mod simulated_clock;

pub use data_source::{
    BufferedDataSource, DataFrame, DataSource, FrameType, JournalDataSource, LiveDataSource,
    NullDataSource,
};
pub use engine::{ReplayEngine, ReplayMode, ReplayState, ReplayStats, StepResult};
pub use scheduler::{ScheduleResult, SkipIdleScheduler, WarpConfig, WarpStats};
pub use simulated_clock::SimulatedClock;

// Re-export channel types for LiveDataSource usage
pub use crossbeam::channel::{Receiver as DataFrameReceiver, Sender as DataFrameSender};
