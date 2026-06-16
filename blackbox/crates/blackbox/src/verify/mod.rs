//! Verify module - State hashing, checkpoints, comparison, and replay verification.
//!
//! This module provides tools for verifying replay accuracy
//! by comparing state hashes between original and replayed sessions.
//!
//! ## Components
//!
//! - [`StateHash`] - Deterministic state hashing using SHA-256
//! - [`Hashable`] - Trait for types that can be hashed
//! - [`Checkpoint`] - State snapshot with metadata
//! - [`CheckpointBuilder`] - Builder for creating checkpoints
//! - [`ReplayComparator`] - Compare original vs replayed states
//! - [`VerifyingReplayEngine`] - Replay engine with verification (T4.3)
//! - [`ComparisonReport`] - Report generator for verification results (T4.4)
//!
//! ## Usage
//!
//! ```rust
//! use blackbox::verify::{StateHash, Hashable, Checkpoint, CheckpointBuilder};
//!
//! // Create a state hash
//! let mut hasher = StateHash::new();
//! hasher.update_orderbook(b"bid:100@50000");
//! let hash = hasher.finalize();
//!
//! // Create a checkpoint
//! let checkpoint = CheckpointBuilder::new()
//!     .sequence(1)
//!     .timestamp(1704067200_000_000)
//!     .hash(hash)
//!     .build();
//! ```
//!
//! ## Replay Verification (T4.3/T4.4)
//!
//! ```rust,ignore
//! use blackbox::verify::{VerifyingReplayEngine, VerificationCallback, ComparisonReport};
//! use blackbox::replay::BufferedDataSource;
//!
//! // Define your state hasher
//! struct MyStateHasher { /* ... */ }
//!
//! impl VerificationCallback for MyStateHasher {
//!     fn compute_state_hash(&self) -> [u8; 32] {
//!         // Return current state hash
//!         [0u8; 32]
//!     }
//! }
//!
//! let source = BufferedDataSource::empty();
//! let mut engine = VerifyingReplayEngine::new(source, MyStateHasher);
//!
//! let stats = engine.run_with_verification();
//! let report = ComparisonReport::from_stats(&stats, engine.results());
//!
//! println!("{}", report);
//! ```

mod checkpoint;
mod comparator;
mod report;
mod state_hash;
mod verifying_engine;

pub use checkpoint::{Checkpoint, CheckpointBuilder, CheckpointData, CheckpointError};
pub use comparator::{CompareResult, MismatchRecord, ReplayComparator};
pub use report::{ComparisonReport, MismatchDetail, ReportEntry, ReportStatus, Severity};
pub use state_hash::{Hashable, StateHash};
pub use verifying_engine::{
    FixedHashCallback, VerificationCallback, VerificationResult, VerificationStats, VerifyConfig,
    VerifyingReplayEngine,
};
