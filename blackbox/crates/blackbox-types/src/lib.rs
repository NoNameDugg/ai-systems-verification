//! # blackbox-types
//!
//! Shared types for the BlackBox flight recorder.
//!
//! This crate contains fundamental types used across all BlackBox components:
//! - `Clock` trait for time abstraction (generic, no vtable overhead)
//! - `Exchange` enum for supported exchanges
//! - `Side` enum for order sides
//! - `Instrument` struct for trading instruments (Copy-able)
//! - `Timestamp` type for microsecond precision timestamps
//!
//! ## Design Principles
//!
//! 1. **Zero Allocation**: All types are stack-allocated or Copy
//! 2. **No Dynamic Dispatch**: Clock uses generics, not `dyn`
//! 3. **Minimal Dependencies**: This crate has no external dependencies
//! 4. **Breaking Cycle**: Shared between Flash and BlackBox

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod clock;
mod exchange;
mod instrument;
mod side;
mod timestamp;

pub use clock::{Clock, SystemClock};
pub use exchange::Exchange;
pub use instrument::Instrument;
pub use side::Side;
pub use timestamp::Timestamp;
