//! Replay comparator for verification.

/// Result of comparing original and replayed state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareResult {
    /// States match exactly.
    Match,
    /// States differ.
    Mismatch {
        /// Original state hash.
        original: [u8; 32],
        /// Replayed state hash.
        replayed: [u8; 32],
        /// Sequence number where mismatch occurred.
        sequence: u64,
    },
}

/// A mismatch record for reporting.
#[derive(Debug, Clone)]
pub struct MismatchRecord {
    /// Checkpoint sequence number.
    pub sequence: u64,
    /// Timestamp of checkpoint.
    pub timestamp: i64,
    /// Original hash.
    pub original_hash: [u8; 32],
    /// Replayed hash.
    pub replayed_hash: [u8; 32],
}

/// Comparator for verifying replay accuracy.
///
/// The ReplayComparator compares state hashes between the original
/// recorded session and a replayed session to detect divergence.
///
/// # Usage
///
/// 1. Load the original journal
/// 2. Replay the session with the trading system
/// 3. Compare checkpoints at each step
/// 4. Generate a report of any mismatches
pub struct ReplayComparator {
    /// Mismatches found during comparison.
    mismatches: Vec<MismatchRecord>,
    /// Total checkpoints compared.
    checkpoints_compared: u64,
    /// Checkpoints that matched.
    checkpoints_matched: u64,
}

impl ReplayComparator {
    /// Create a new comparator.
    pub fn new() -> Self {
        Self {
            mismatches: Vec::new(),
            checkpoints_compared: 0,
            checkpoints_matched: 0,
        }
    }

    /// Compare a checkpoint.
    pub fn compare(
        &mut self,
        sequence: u64,
        timestamp: i64,
        original: &[u8; 32],
        replayed: &[u8; 32],
    ) -> CompareResult {
        self.checkpoints_compared += 1;

        if original == replayed {
            self.checkpoints_matched += 1;
            CompareResult::Match
        } else {
            self.mismatches.push(MismatchRecord {
                sequence,
                timestamp,
                original_hash: *original,
                replayed_hash: *replayed,
            });
            CompareResult::Mismatch {
                original: *original,
                replayed: *replayed,
                sequence,
            }
        }
    }

    /// Get all mismatches.
    pub fn mismatches(&self) -> &[MismatchRecord] {
        &self.mismatches
    }

    /// Check if replay was successful (no mismatches).
    pub fn is_successful(&self) -> bool {
        self.mismatches.is_empty()
    }

    /// Get the total number of checkpoints compared.
    pub fn checkpoints_compared(&self) -> u64 {
        self.checkpoints_compared
    }

    /// Get the number of matching checkpoints.
    pub fn checkpoints_matched(&self) -> u64 {
        self.checkpoints_matched
    }

    /// Get the match rate as a percentage.
    pub fn match_rate(&self) -> f64 {
        if self.checkpoints_compared == 0 {
            100.0
        } else {
            (self.checkpoints_matched as f64 / self.checkpoints_compared as f64) * 100.0
        }
    }

    /// Generate a summary report.
    pub fn summary(&self) -> String {
        format!(
            "Replay Verification Summary:\n\
             - Checkpoints compared: {}\n\
             - Checkpoints matched: {}\n\
             - Mismatches: {}\n\
             - Match rate: {:.2}%\n\
             - Status: {}",
            self.checkpoints_compared,
            self.checkpoints_matched,
            self.mismatches.len(),
            self.match_rate(),
            if self.is_successful() { "PASS" } else { "FAIL" }
        )
    }
}

impl Default for ReplayComparator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comparator_match() {
        let mut comp = ReplayComparator::new();
        let hash = [0u8; 32];

        let result = comp.compare(1, 1000, &hash, &hash);
        assert_eq!(result, CompareResult::Match);
        assert!(comp.is_successful());
    }

    #[test]
    fn test_comparator_mismatch() {
        let mut comp = ReplayComparator::new();
        let hash1 = [0u8; 32];
        let hash2 = [1u8; 32];

        let result = comp.compare(1, 1000, &hash1, &hash2);
        assert!(matches!(result, CompareResult::Mismatch { .. }));
        assert!(!comp.is_successful());
        assert_eq!(comp.mismatches().len(), 1);
    }

    #[test]
    fn test_comparator_match_rate() {
        let mut comp = ReplayComparator::new();
        let hash = [0u8; 32];
        let bad = [1u8; 32];

        comp.compare(1, 1000, &hash, &hash);
        comp.compare(2, 2000, &hash, &hash);
        comp.compare(3, 3000, &hash, &bad);
        comp.compare(4, 4000, &hash, &hash);

        assert_eq!(comp.checkpoints_compared(), 4);
        assert_eq!(comp.checkpoints_matched(), 3);
        assert!((comp.match_rate() - 75.0).abs() < 0.01);
    }
}
