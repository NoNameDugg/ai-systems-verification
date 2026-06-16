//! Verifying replay engine for state verification during replay.
//!
//! This module provides `VerifyingReplayEngine`, which extends the replay
//! engine with checkpoint verification capabilities. During replay, it
//! compares state hashes at checkpoint boundaries to detect divergence.
//!
//! ## Design (T4.3)
//!
//! | Component | Purpose | Implementation |
//! |-----------|---------|----------------|
//! | `VerifyingReplayEngine` | Replay with verification | Wraps ReplayEngine |
//! | `VerificationCallback` | User-defined verification | Trait-based hook |
//! | `VerificationResult` | Result of verification | Match/Mismatch/Error |
//! | `VerificationStats` | Aggregate statistics | Checkpoint counts |
//!
//! ## Verification Flow
//!
//! ```text
//! Journal File
//!     │
//!     ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │  VerifyingReplayEngine                                       │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │ For each event:                                         ││
//! │  │   1. Process event via DataSource                       ││
//! │  │   2. If checkpoint: extract expected hash               ││
//! │  │   3. Call user's state hasher                           ││
//! │  │   4. Compare hashes                                     ││
//! │  │   5. Record result in ReplayComparator                  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//!     │
//!     ▼
//! VerificationStats + ComparisonReport
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use blackbox::verify::{VerifyingReplayEngine, VerificationCallback};
//! use blackbox::replay::BufferedDataSource;
//!
//! // Define state hasher
//! struct MyStateHasher { /* trading system state */ }
//!
//! impl VerificationCallback for MyStateHasher {
//!     fn compute_state_hash(&self) -> [u8; 32] {
//!         // Hash current state
//!     }
//! }
//!
//! let source = BufferedDataSource::new(frames);
//! let mut engine = VerifyingReplayEngine::new(source, MyStateHasher);
//!
//! // Run with verification
//! let stats = engine.run_with_verification();
//! assert!(stats.is_successful());
//! ```

use super::{Checkpoint, CompareResult, ReplayComparator};
use crate::replay::{DataFrame, DataSource, FrameType, ReplayEngine, StepResult, WarpConfig};

/// Callback trait for computing state hashes during verification.
///
/// Implement this trait to provide state hashing for your trading system.
/// The engine will call `compute_state_hash()` at each checkpoint to
/// compare against the recorded hash.
pub trait VerificationCallback: Send {
    /// Compute the current state hash.
    ///
    /// This should return a SHA-256 hash of all relevant state:
    /// - Order book state
    /// - Position state
    /// - Open orders
    /// - Any other state that affects trading decisions
    fn compute_state_hash(&self) -> [u8; 32];

    /// Called when processing a data frame (optional).
    ///
    /// Override this to update your state based on incoming data.
    fn on_frame(&mut self, _frame: &DataFrame) {}

    /// Called before verification at a checkpoint (optional).
    ///
    /// Override this to prepare state for verification.
    fn before_verify(&mut self, _checkpoint: &Checkpoint) {}
}

/// A simple callback that returns a fixed hash (for testing).
#[derive(Debug, Clone)]
pub struct FixedHashCallback {
    hash: [u8; 32],
}

impl FixedHashCallback {
    /// Create a callback that always returns the given hash.
    pub fn new(hash: [u8; 32]) -> Self {
        Self { hash }
    }

    /// Create a callback that always returns zeros.
    pub fn zeros() -> Self {
        Self { hash: [0u8; 32] }
    }
}

impl VerificationCallback for FixedHashCallback {
    fn compute_state_hash(&self) -> [u8; 32] {
        self.hash
    }
}

/// Result of a single verification check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// State hashes match.
    Match {
        /// Checkpoint sequence.
        sequence: u64,
        /// Checkpoint timestamp.
        timestamp: i64,
    },
    /// State hashes differ.
    Mismatch {
        /// Checkpoint sequence.
        sequence: u64,
        /// Checkpoint timestamp.
        timestamp: i64,
        /// Expected (recorded) hash.
        expected: [u8; 32],
        /// Actual (computed) hash.
        actual: [u8; 32],
    },
    /// Error during verification.
    Error {
        /// Error description.
        message: String,
    },
}

impl VerificationResult {
    /// Check if this is a match.
    pub fn is_match(&self) -> bool {
        matches!(self, VerificationResult::Match { .. })
    }

    /// Check if this is a mismatch.
    pub fn is_mismatch(&self) -> bool {
        matches!(self, VerificationResult::Mismatch { .. })
    }

    /// Check if this is an error.
    pub fn is_error(&self) -> bool {
        matches!(self, VerificationResult::Error { .. })
    }
}

/// Statistics from a verification run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationStats {
    /// Total events processed.
    pub events_processed: u64,
    /// Checkpoints encountered.
    pub checkpoints_found: u64,
    /// Checkpoints that matched.
    pub checkpoints_matched: u64,
    /// Checkpoints that mismatched.
    pub checkpoints_mismatched: u64,
    /// Errors during verification.
    pub errors: u64,
    /// First mismatch sequence (if any).
    pub first_mismatch_sequence: Option<u64>,
}

impl VerificationStats {
    /// Create empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if verification was successful (no mismatches or errors).
    pub fn is_successful(&self) -> bool {
        self.checkpoints_mismatched == 0 && self.errors == 0
    }

    /// Get the match rate as a percentage.
    pub fn match_rate(&self) -> f64 {
        if self.checkpoints_found == 0 {
            100.0
        } else {
            (self.checkpoints_matched as f64 / self.checkpoints_found as f64) * 100.0
        }
    }

    /// Generate a summary string.
    pub fn summary(&self) -> String {
        format!(
            "Verification Summary:\n\
             - Events processed: {}\n\
             - Checkpoints found: {}\n\
             - Checkpoints matched: {}\n\
             - Checkpoints mismatched: {}\n\
             - Errors: {}\n\
             - Match rate: {:.2}%\n\
             - Status: {}",
            self.events_processed,
            self.checkpoints_found,
            self.checkpoints_matched,
            self.checkpoints_mismatched,
            self.errors,
            self.match_rate(),
            if self.is_successful() { "PASS" } else { "FAIL" }
        )
    }
}

/// Configuration for the verifying replay engine.
#[derive(Debug, Clone)]
pub struct VerifyConfig {
    /// Stop on first mismatch.
    pub stop_on_mismatch: bool,
    /// Stop on first error.
    pub stop_on_error: bool,
    /// Warp configuration for replay.
    pub warp_config: WarpConfig,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            stop_on_mismatch: false,
            stop_on_error: true,
            warp_config: WarpConfig::default(),
        }
    }
}

impl VerifyConfig {
    /// Create config that stops on first mismatch.
    pub fn stop_on_mismatch() -> Self {
        Self {
            stop_on_mismatch: true,
            ..Self::default()
        }
    }

    /// Create config that continues on all errors.
    pub fn continue_on_error() -> Self {
        Self {
            stop_on_error: false,
            ..Self::default()
        }
    }
}

/// Replay engine with checkpoint verification.
///
/// This engine wraps a standard `ReplayEngine` and adds verification
/// at checkpoint boundaries. It uses a `VerificationCallback` to
/// compute state hashes and compares them against recorded checkpoints.
pub struct VerifyingReplayEngine<D: DataSource, C: VerificationCallback> {
    /// Inner replay engine.
    engine: ReplayEngine<D>,
    /// Verification callback.
    callback: C,
    /// Comparator for tracking results.
    comparator: ReplayComparator,
    /// Configuration.
    config: VerifyConfig,
    /// Statistics.
    stats: VerificationStats,
    /// Recent verification results.
    results: Vec<VerificationResult>,
}

impl<D: DataSource, C: VerificationCallback> VerifyingReplayEngine<D, C> {
    /// Create a new verifying replay engine.
    pub fn new(data_source: D, callback: C) -> Self {
        Self::with_config(data_source, callback, VerifyConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(data_source: D, callback: C, config: VerifyConfig) -> Self {
        let engine = ReplayEngine::with_data_source(data_source, config.warp_config.clone());
        Self {
            engine,
            callback,
            comparator: ReplayComparator::new(),
            config,
            stats: VerificationStats::new(),
            results: Vec::new(),
        }
    }

    /// Get the inner replay engine.
    pub fn engine(&self) -> &ReplayEngine<D> {
        &self.engine
    }

    /// Get a mutable reference to the inner engine.
    pub fn engine_mut(&mut self) -> &mut ReplayEngine<D> {
        &mut self.engine
    }

    /// Get the callback.
    pub fn callback(&self) -> &C {
        &self.callback
    }

    /// Get a mutable reference to the callback.
    pub fn callback_mut(&mut self) -> &mut C {
        &mut self.callback
    }

    /// Get the comparator.
    pub fn comparator(&self) -> &ReplayComparator {
        &self.comparator
    }

    /// Get current statistics.
    pub fn stats(&self) -> &VerificationStats {
        &self.stats
    }

    /// Get verification results.
    pub fn results(&self) -> &[VerificationResult] {
        &self.results
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.engine.play();
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.engine.pause();
    }

    /// Step one event with verification.
    ///
    /// Returns the step result and any verification result.
    pub fn step(&mut self) -> (StepResult, Option<VerificationResult>) {
        let step_result = self.engine.step();

        if step_result.completed {
            return (step_result, None);
        }

        if !step_result.processed {
            return (step_result, None);
        }

        self.stats.events_processed += 1;

        // Check if this is a checkpoint frame
        if let Some(ref frame) = step_result.frame {
            // Call on_frame callback
            self.callback.on_frame(frame);

            // Check for checkpoint
            if frame.frame_type == FrameType::Checkpoint {
                return (step_result.clone(), Some(self.verify_checkpoint(frame)));
            }
        }

        (step_result, None)
    }

    /// Verify a checkpoint frame.
    fn verify_checkpoint(&mut self, frame: &DataFrame) -> VerificationResult {
        self.stats.checkpoints_found += 1;

        // Parse checkpoint from frame payload
        let checkpoint = match Checkpoint::from_bytes(&frame.payload) {
            Ok(cp) => cp,
            Err(e) => {
                self.stats.errors += 1;
                let result = VerificationResult::Error {
                    message: format!("Failed to parse checkpoint: {}", e),
                };
                self.results.push(result.clone());
                return result;
            }
        };

        // Call before_verify callback
        self.callback.before_verify(&checkpoint);

        // Compute actual state hash
        let actual_hash = self.callback.compute_state_hash();
        let expected_hash = *checkpoint.state_hash();

        // Compare
        let compare_result = self.comparator.compare(
            checkpoint.sequence(),
            checkpoint.timestamp(),
            &expected_hash,
            &actual_hash,
        );

        let result = match compare_result {
            CompareResult::Match => {
                self.stats.checkpoints_matched += 1;
                VerificationResult::Match {
                    sequence: checkpoint.sequence(),
                    timestamp: checkpoint.timestamp(),
                }
            }
            CompareResult::Mismatch {
                original,
                replayed,
                sequence,
            } => {
                self.stats.checkpoints_mismatched += 1;
                if self.stats.first_mismatch_sequence.is_none() {
                    self.stats.first_mismatch_sequence = Some(sequence);
                }
                VerificationResult::Mismatch {
                    sequence,
                    timestamp: checkpoint.timestamp(),
                    expected: original,
                    actual: replayed,
                }
            }
        };

        self.results.push(result.clone());
        result
    }

    /// Run replay with verification until completion.
    ///
    /// Returns statistics and whether verification was successful.
    pub fn run_with_verification(&mut self) -> VerificationStats {
        self.play();

        loop {
            let (step_result, verify_result) = self.step();

            if step_result.completed {
                break;
            }

            // Check for stop conditions
            if let Some(ref vr) = verify_result {
                if self.config.stop_on_mismatch && vr.is_mismatch() {
                    break;
                }
                if self.config.stop_on_error && vr.is_error() {
                    break;
                }
            }
        }

        self.stats.clone()
    }

    /// Reset the engine for another run.
    pub fn reset(&mut self) {
        self.engine.reset();
        self.comparator = ReplayComparator::new();
        self.stats = VerificationStats::new();
        self.results.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{BufferedDataSource, DataFrame, FrameType, NullDataSource};
    use crate::verify::{Checkpoint, CheckpointBuilder, StateHash};
    use blackbox_types::{Exchange, Timestamp};

    // ==================== Helper Functions ====================

    fn create_test_frame(ts: i64, payload: Vec<u8>) -> DataFrame {
        DataFrame::new(
            Timestamp::from_micros(ts),
            Exchange::Deribit,
            FrameType::WebSocketText,
            payload,
        )
    }

    fn create_checkpoint_frame(seq: u64, ts: i64, hash: [u8; 32]) -> DataFrame {
        let checkpoint = CheckpointBuilder::new()
            .sequence(seq)
            .timestamp(ts)
            .hash(hash)
            .build();
        DataFrame::new(
            Timestamp::from_micros(ts),
            Exchange::Unknown, // Checkpoints are internal system frames
            FrameType::Checkpoint,
            checkpoint.to_bytes().to_vec(),
        )
    }

    // ==================== FixedHashCallback Tests ====================

    #[test]
    fn test_fixed_hash_callback_new() {
        let hash = [0xAB; 32];
        let callback = FixedHashCallback::new(hash);
        assert_eq!(callback.compute_state_hash(), hash);
    }

    #[test]
    fn test_fixed_hash_callback_zeros() {
        let callback = FixedHashCallback::zeros();
        assert_eq!(callback.compute_state_hash(), [0u8; 32]);
    }

    // ==================== VerificationResult Tests ====================

    #[test]
    fn test_verification_result_match() {
        let result = VerificationResult::Match {
            sequence: 1,
            timestamp: 1000,
        };
        assert!(result.is_match());
        assert!(!result.is_mismatch());
        assert!(!result.is_error());
    }

    #[test]
    fn test_verification_result_mismatch() {
        let result = VerificationResult::Mismatch {
            sequence: 1,
            timestamp: 1000,
            expected: [0xAA; 32],
            actual: [0xBB; 32],
        };
        assert!(!result.is_match());
        assert!(result.is_mismatch());
        assert!(!result.is_error());
    }

    #[test]
    fn test_verification_result_error() {
        let result = VerificationResult::Error {
            message: "Test error".to_string(),
        };
        assert!(!result.is_match());
        assert!(!result.is_mismatch());
        assert!(result.is_error());
    }

    // ==================== VerificationStats Tests ====================

    #[test]
    fn test_verification_stats_new() {
        let stats = VerificationStats::new();
        assert_eq!(stats.events_processed, 0);
        assert_eq!(stats.checkpoints_found, 0);
        assert!(stats.is_successful());
    }

    #[test]
    fn test_verification_stats_is_successful() {
        let mut stats = VerificationStats::new();
        assert!(stats.is_successful());

        stats.checkpoints_mismatched = 1;
        assert!(!stats.is_successful());
    }

    #[test]
    fn test_verification_stats_is_successful_with_errors() {
        let mut stats = VerificationStats::new();
        stats.errors = 1;
        assert!(!stats.is_successful());
    }

    #[test]
    fn test_verification_stats_match_rate_empty() {
        let stats = VerificationStats::new();
        assert_eq!(stats.match_rate(), 100.0);
    }

    #[test]
    fn test_verification_stats_match_rate() {
        let stats = VerificationStats {
            checkpoints_found: 10,
            checkpoints_matched: 8,
            checkpoints_mismatched: 2,
            ..Default::default()
        };
        assert!((stats.match_rate() - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_verification_stats_summary() {
        let stats = VerificationStats {
            events_processed: 100,
            checkpoints_found: 5,
            checkpoints_matched: 5,
            ..Default::default()
        };
        let summary = stats.summary();
        assert!(summary.contains("Events processed: 100"));
        assert!(summary.contains("Checkpoints found: 5"));
        assert!(summary.contains("PASS"));
    }

    #[test]
    fn test_verification_stats_summary_fail() {
        let stats = VerificationStats {
            checkpoints_found: 5,
            checkpoints_matched: 3,
            checkpoints_mismatched: 2,
            ..Default::default()
        };
        let summary = stats.summary();
        assert!(summary.contains("FAIL"));
    }

    // ==================== VerifyConfig Tests ====================

    #[test]
    fn test_verify_config_default() {
        let config = VerifyConfig::default();
        assert!(!config.stop_on_mismatch);
        assert!(config.stop_on_error);
    }

    #[test]
    fn test_verify_config_stop_on_mismatch() {
        let config = VerifyConfig::stop_on_mismatch();
        assert!(config.stop_on_mismatch);
    }

    #[test]
    fn test_verify_config_continue_on_error() {
        let config = VerifyConfig::continue_on_error();
        assert!(!config.stop_on_error);
    }

    // ==================== VerifyingReplayEngine Basic Tests ====================

    #[test]
    fn test_verifying_engine_new() {
        let source = NullDataSource;
        let callback = FixedHashCallback::zeros();
        let engine = VerifyingReplayEngine::new(source, callback);

        assert_eq!(engine.stats().events_processed, 0);
        assert!(engine.results().is_empty());
    }

    #[test]
    fn test_verifying_engine_with_config() {
        let source = NullDataSource;
        let callback = FixedHashCallback::zeros();
        let config = VerifyConfig::stop_on_mismatch();
        let engine = VerifyingReplayEngine::with_config(source, callback, config);

        assert_eq!(engine.stats().events_processed, 0);
    }

    #[test]
    fn test_verifying_engine_accessors() {
        let source = BufferedDataSource::empty();
        let callback = FixedHashCallback::zeros();
        let engine = VerifyingReplayEngine::new(source, callback);

        let _ = engine.engine();
        let _ = engine.callback();
        let _ = engine.comparator();
        let _ = engine.stats();
        let _ = engine.results();
    }

    #[test]
    fn test_verifying_engine_mutable_accessors() {
        let source = BufferedDataSource::empty();
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let _ = engine.engine_mut();
        let _ = engine.callback_mut();
    }

    // ==================== Verification Flow Tests ====================

    #[test]
    fn test_verifying_engine_step_no_data() {
        let source = BufferedDataSource::empty();
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        let (result, verify) = engine.step();

        assert!(result.completed);
        assert!(verify.is_none());
    }

    #[test]
    fn test_verifying_engine_step_non_checkpoint() {
        let frames = vec![create_test_frame(1000, b"test".to_vec())];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        let (result, verify) = engine.step();

        assert!(result.processed);
        assert!(verify.is_none());
        assert_eq!(engine.stats().events_processed, 1);
        assert_eq!(engine.stats().checkpoints_found, 0);
    }

    #[test]
    fn test_verifying_engine_checkpoint_match() {
        let hash = [0xAB; 32];
        let frames = vec![create_checkpoint_frame(1, 1000, hash)];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(hash); // Same hash
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        let (result, verify) = engine.step();

        assert!(result.processed);
        assert!(verify.is_some());

        let vr = verify.unwrap();
        assert!(vr.is_match());

        assert_eq!(engine.stats().checkpoints_found, 1);
        assert_eq!(engine.stats().checkpoints_matched, 1);
        assert_eq!(engine.stats().checkpoints_mismatched, 0);
    }

    #[test]
    fn test_verifying_engine_checkpoint_mismatch() {
        let recorded_hash = [0xAA; 32];
        let computed_hash = [0xBB; 32];
        let frames = vec![create_checkpoint_frame(1, 1000, recorded_hash)];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(computed_hash); // Different hash
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        let (_, verify) = engine.step();

        let vr = verify.unwrap();
        assert!(vr.is_mismatch());

        if let VerificationResult::Mismatch {
            expected, actual, ..
        } = vr
        {
            assert_eq!(expected, recorded_hash);
            assert_eq!(actual, computed_hash);
        }

        assert_eq!(engine.stats().checkpoints_mismatched, 1);
        assert_eq!(engine.stats().first_mismatch_sequence, Some(1));
    }

    #[test]
    fn test_verifying_engine_checkpoint_parse_error() {
        // Create a checkpoint frame with invalid data
        let frames = vec![DataFrame::new(
            Timestamp::from_micros(1000),
            Exchange::Unknown, // Checkpoints are internal system frames
            FrameType::Checkpoint,
            vec![0x00; 10], // Too short to be valid
        )];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        let (_, verify) = engine.step();

        let vr = verify.unwrap();
        assert!(vr.is_error());
        assert_eq!(engine.stats().errors, 1);
    }

    // ==================== Run With Verification Tests ====================

    #[test]
    fn test_run_with_verification_empty() {
        let source = BufferedDataSource::empty();
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(stats.is_successful());
        assert_eq!(stats.events_processed, 0);
    }

    #[test]
    fn test_run_with_verification_all_match() {
        let hash = [0xCD; 32];
        let frames = vec![
            create_test_frame(1000, b"event1".to_vec()),
            create_checkpoint_frame(1, 2000, hash),
            create_test_frame(3000, b"event2".to_vec()),
            create_checkpoint_frame(2, 4000, hash),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(stats.is_successful());
        assert_eq!(stats.events_processed, 4);
        assert_eq!(stats.checkpoints_found, 2);
        assert_eq!(stats.checkpoints_matched, 2);
    }

    #[test]
    fn test_run_with_verification_mismatch() {
        let recorded = [0xAA; 32];
        let computed = [0xBB; 32];
        let frames = vec![
            create_checkpoint_frame(1, 1000, recorded),
            create_checkpoint_frame(2, 2000, recorded),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(computed);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(!stats.is_successful());
        assert_eq!(stats.checkpoints_mismatched, 2);
    }

    #[test]
    fn test_run_with_verification_stop_on_mismatch() {
        let recorded = [0xAA; 32];
        let computed = [0xBB; 32];
        let frames = vec![
            create_checkpoint_frame(1, 1000, recorded),
            create_checkpoint_frame(2, 2000, recorded),
            create_checkpoint_frame(3, 3000, recorded),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(computed);
        let config = VerifyConfig::stop_on_mismatch();
        let mut engine = VerifyingReplayEngine::with_config(source, callback, config);

        let stats = engine.run_with_verification();

        // Should stop after first mismatch
        assert_eq!(stats.checkpoints_mismatched, 1);
        assert_eq!(stats.first_mismatch_sequence, Some(1));
    }

    // ==================== Reset Tests ====================

    #[test]
    fn test_verifying_engine_reset() {
        let hash = [0xAB; 32];
        let frames = vec![create_checkpoint_frame(1, 1000, hash)];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        // Run first time
        engine.run_with_verification();
        assert_eq!(engine.stats().checkpoints_found, 1);

        // Reset
        engine.reset();

        assert_eq!(engine.stats().checkpoints_found, 0);
        assert!(engine.results().is_empty());
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_verifying_engine_mixed_events() {
        let hash1 = StateHash::hash_once(b"state1");
        let hash2 = StateHash::hash_once(b"state2");

        // Simulate a session with different state hashes
        let frames = vec![
            create_test_frame(1000, b"market_data1".to_vec()),
            create_test_frame(2000, b"market_data2".to_vec()),
            create_checkpoint_frame(1, 3000, hash1),
            create_test_frame(4000, b"market_data3".to_vec()),
            // Second checkpoint with different hash - should mismatch if callback returns hash1
            create_checkpoint_frame(2, 5000, hash2),
        ];
        let source = BufferedDataSource::new(frames);

        // Callback always returns hash1
        let callback = FixedHashCallback::new(hash1);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        // First checkpoint matches (hash1 == hash1)
        // Second checkpoint mismatches (hash2 != hash1)
        assert_eq!(stats.checkpoints_matched, 1);
        assert_eq!(stats.checkpoints_mismatched, 1);
        assert!(!stats.is_successful());
    }

    #[test]
    fn test_verifying_engine_high_volume() {
        // Generate 100 checkpoints
        let hash = [0xEF; 32];
        let frames: Vec<DataFrame> = (0..100)
            .map(|i| create_checkpoint_frame(i as u64, (i + 1) * 1000, hash))
            .collect();

        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(stats.is_successful());
        assert_eq!(stats.checkpoints_found, 100);
        assert_eq!(stats.checkpoints_matched, 100);
        assert_eq!(engine.results().len(), 100);
    }

    // ==================== Callback Tests ====================

    struct CountingCallback {
        hash: [u8; 32],
        frame_count: std::cell::Cell<u32>,
        verify_count: std::cell::Cell<u32>,
    }

    impl CountingCallback {
        fn new(hash: [u8; 32]) -> Self {
            Self {
                hash,
                frame_count: std::cell::Cell::new(0),
                verify_count: std::cell::Cell::new(0),
            }
        }
    }

    impl VerificationCallback for CountingCallback {
        fn compute_state_hash(&self) -> [u8; 32] {
            self.hash
        }

        fn on_frame(&mut self, _frame: &DataFrame) {
            self.frame_count.set(self.frame_count.get() + 1);
        }

        fn before_verify(&mut self, _checkpoint: &Checkpoint) {
            self.verify_count.set(self.verify_count.get() + 1);
        }
    }

    #[test]
    fn test_callback_on_frame_called() {
        let hash = [0xAB; 32];
        let frames = vec![
            create_test_frame(1000, b"test1".to_vec()),
            create_test_frame(2000, b"test2".to_vec()),
            create_checkpoint_frame(1, 3000, hash),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = CountingCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.run_with_verification();

        assert_eq!(engine.callback().frame_count.get(), 3);
    }

    #[test]
    fn test_callback_before_verify_called() {
        let hash = [0xAB; 32];
        let frames = vec![
            create_checkpoint_frame(1, 1000, hash),
            create_checkpoint_frame(2, 2000, hash),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = CountingCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.run_with_verification();

        assert_eq!(engine.callback().verify_count.get(), 2);
    }

    // ==================== Play/Pause Tests ====================

    #[test]
    fn test_verifying_engine_play_pause() {
        let frames = vec![
            create_test_frame(1000, b"test".to_vec()),
            create_test_frame(2000, b"test".to_vec()),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        engine.play();
        engine.step();
        engine.pause();

        assert_eq!(engine.stats().events_processed, 1);
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_verifying_engine_single_checkpoint() {
        let hash = [0xFF; 32];
        let frames = vec![create_checkpoint_frame(1, 1000, hash)];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::new(hash);
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(stats.is_successful());
        assert_eq!(stats.events_processed, 1);
        assert_eq!(stats.checkpoints_found, 1);
    }

    #[test]
    fn test_verifying_engine_no_checkpoints() {
        let frames = vec![
            create_test_frame(1000, b"event1".to_vec()),
            create_test_frame(2000, b"event2".to_vec()),
        ];
        let source = BufferedDataSource::new(frames);
        let callback = FixedHashCallback::zeros();
        let mut engine = VerifyingReplayEngine::new(source, callback);

        let stats = engine.run_with_verification();

        assert!(stats.is_successful());
        assert_eq!(stats.events_processed, 2);
        assert_eq!(stats.checkpoints_found, 0);
    }

    #[test]
    fn test_verification_result_debug() {
        let result = VerificationResult::Match {
            sequence: 1,
            timestamp: 1000,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("Match"));
    }

    #[test]
    fn test_verification_stats_default() {
        let stats = VerificationStats::default();
        assert_eq!(stats.events_processed, 0);
    }

    #[test]
    fn test_verify_config_debug() {
        let config = VerifyConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("VerifyConfig"));
    }
}
