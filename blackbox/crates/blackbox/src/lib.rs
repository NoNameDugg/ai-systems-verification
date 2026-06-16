//! # blackbox
//!
//! Flight Recorder for trading systems.
//!
//! This crate provides zero-allocation journaling and deterministic replay
//! capabilities for debugging and analysis of trading sessions.
//!
//! ## Features
//!
//! - **Journal**: High-speed binary writer/reader (<1μs writes)
//! - **Tap**: Zero-overhead instrumentation points
//! - **Replay**: Deterministic playback with clock control
//! - **Verify**: State hash comparison for regression testing
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       Trading System                         │
//! │  ┌─────────┐    ┌──────────┐    ┌─────────────┐             │
//! │  │WebSocket│───▶│ TAP #1   │───▶│  OrderBook  │             │
//! │  │  Feed   │    │(Ingress) │    │  Engine     │             │
//! │  └─────────┘    └────┬─────┘    └──────┬──────┘             │
//! │                      │                  │                    │
//! │                      ▼                  ▼                    │
//! │              ┌───────────────┐   ┌───────────────┐          │
//! │              │  RingBuffer   │   │   TAP #2      │          │
//! │              │  (Lock-free)  │   │  (Internal)   │          │
//! │              └───────┬───────┘   └───────┬───────┘          │
//! │                      │                   │                   │
//! │                      ▼                   ▼                   │
//! │              ┌─────────────────────────────────┐            │
//! │              │      MMAP Journal Writer        │            │
//! │              │    (Background Thread)          │            │
//! │              └─────────────────────────────────┘            │
//! │                              │                               │
//! └──────────────────────────────┼───────────────────────────────┘
//!                                ▼
//!                     ┌─────────────────────┐
//!                     │   .journal file     │
//!                     │   (Binary, MMAP)    │
//!                     └─────────────────────┘
//! ```
//!
//! ## Modules
//!
//! - `journal` - Binary file format and I/O
//! - `tap` - Instrumentation trait and implementations
//! - `replay` - Clock control and replay engine
//! - `verify` - State hashing and comparison

#![warn(missing_docs)]

pub mod cli;
pub mod codec;
pub mod journal;
pub mod replay;
pub mod tap;
pub mod verify;

// Re-export common types from blackbox-types
pub use blackbox_types::{Clock, Exchange, Instrument, Side, SystemClock, Timestamp};
