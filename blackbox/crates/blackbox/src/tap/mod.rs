//! Tap module - Zero-overhead instrumentation.
//!
//! The Tap trait provides a uniform interface for recording events
//! from the trading system with minimal overhead.
//!
//! ## Implementations
//!
//! - `NullTap`: No-op implementation (zero overhead)
//! - `JournalTap`: Records to journal file

mod journal_tap;
mod null_tap;
mod traits;

pub use journal_tap::JournalTap;
pub use null_tap::NullTap;
pub use traits::Tap;
