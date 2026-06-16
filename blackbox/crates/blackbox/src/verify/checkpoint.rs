//! Checkpoint records for state verification.
//!
//! Checkpoints are periodic snapshots of system state that enable:
//! - Replay verification by comparing state hashes
//! - Recovery point detection for replay optimization
//! - Divergence detection during replay
//!
//! ## Binary Format (T4.2)
//!
//! Checkpoint records are stored in the journal with RecordType::Checkpoint (0x0003).
//! The payload is a 48-byte CheckpointData structure:
//!
//! ```text
//! Offset  Size  Field           Description
//! ─────────────────────────────────────────────────────
//! 0x00    8     sequence        Checkpoint sequence number
//! 0x08    8     timestamp       Checkpoint timestamp (μs)
//! 0x10    32    state_hash      SHA-256 hash of state
//! ─────────────────────────────────────────────────────
//! Total: 48 bytes
//! ```
//!
//! ## Usage
//!
//! ```rust
//! use blackbox::verify::{Checkpoint, CheckpointBuilder, StateHash};
//!
//! // Create a checkpoint using the builder
//! let mut hasher = StateHash::new();
//! hasher.update_orderbook(b"bid:100@50000");
//! let hash = hasher.finalize();
//!
//! let checkpoint = CheckpointBuilder::new()
//!     .sequence(1)
//!     .timestamp(1704067200_000_000)
//!     .hash(hash)
//!     .build();
//!
//! // Serialize for journal storage
//! let data = checkpoint.to_data();
//! let bytes = data.to_bytes();
//! assert_eq!(bytes.len(), 48);
//!
//! // Deserialize from journal
//! let recovered = Checkpoint::from_bytes(&bytes).unwrap();
//! assert_eq!(recovered.sequence(), 1);
//! ```

use super::StateHash;

/// Size of checkpoint data in bytes.
pub const CHECKPOINT_DATA_SIZE: usize = 48;

/// A checkpoint record containing state hash and metadata.
///
/// Checkpoints are created at specific points during recording to
/// enable verification during replay. Each checkpoint contains:
/// - Sequence number for ordering
/// - Timestamp for temporal reference
/// - State hash for verification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    sequence: u64,
    timestamp: i64,
    state_hash: [u8; 32],
}

impl Checkpoint {
    /// Create a new checkpoint.
    pub fn new(sequence: u64, timestamp: i64, state_hash: [u8; 32]) -> Self {
        Self {
            sequence,
            timestamp,
            state_hash,
        }
    }

    /// Get the sequence number.
    #[inline]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Get the timestamp in microseconds.
    #[inline]
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// Get the state hash.
    #[inline]
    pub fn state_hash(&self) -> &[u8; 32] {
        &self.state_hash
    }

    /// Get the state hash as a hex string.
    pub fn state_hash_hex(&self) -> String {
        StateHash::to_hex(&self.state_hash)
    }

    /// Convert to serializable CheckpointData.
    pub fn to_data(&self) -> CheckpointData {
        CheckpointData {
            sequence: self.sequence,
            timestamp: self.timestamp,
            state_hash: self.state_hash,
        }
    }

    /// Create from raw bytes (journal payload).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CheckpointError> {
        if bytes.len() < CHECKPOINT_DATA_SIZE {
            return Err(CheckpointError::InvalidSize {
                expected: CHECKPOINT_DATA_SIZE,
                actual: bytes.len(),
            });
        }

        let data = CheckpointData::from_bytes(bytes)?;
        Ok(Self::from_data(data))
    }

    /// Create from CheckpointData.
    pub fn from_data(data: CheckpointData) -> Self {
        Self {
            sequence: data.sequence,
            timestamp: data.timestamp,
            state_hash: data.state_hash,
        }
    }

    /// Verify this checkpoint matches the expected state hash.
    pub fn verify(&self, expected: &[u8; 32]) -> bool {
        self.state_hash == *expected
    }

    /// Check if this checkpoint is later than another.
    pub fn is_after(&self, other: &Checkpoint) -> bool {
        self.sequence > other.sequence
    }

    /// Convert to bytes for journal storage.
    pub fn to_bytes(&self) -> [u8; CHECKPOINT_DATA_SIZE] {
        self.to_data().to_bytes()
    }
}

/// Builder for creating Checkpoint instances.
///
/// Provides a fluent API for constructing checkpoints with
/// optional fields and validation.
///
/// # Example
///
/// ```rust
/// use blackbox::verify::{CheckpointBuilder, StateHash};
///
/// let hash = StateHash::hash_once(b"state data");
///
/// let checkpoint = CheckpointBuilder::new()
///     .sequence(1)
///     .timestamp(1704067200_000_000)
///     .hash(hash)
///     .build();
///
/// assert_eq!(checkpoint.sequence(), 1);
/// ```
#[derive(Debug, Default)]
pub struct CheckpointBuilder {
    sequence: u64,
    timestamp: i64,
    state_hash: Option<[u8; 32]>,
}

impl CheckpointBuilder {
    /// Create a new checkpoint builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the sequence number.
    pub fn sequence(mut self, seq: u64) -> Self {
        self.sequence = seq;
        self
    }

    /// Set the timestamp in microseconds.
    pub fn timestamp(mut self, ts: i64) -> Self {
        self.timestamp = ts;
        self
    }

    /// Set the state hash.
    pub fn hash(mut self, hash: [u8; 32]) -> Self {
        self.state_hash = Some(hash);
        self
    }

    /// Set the state hash from a StateHash hasher.
    pub fn from_hasher(mut self, hasher: StateHash) -> Self {
        self.state_hash = Some(hasher.finalize());
        self
    }

    /// Build the checkpoint.
    ///
    /// If no hash was provided, uses a zero hash.
    pub fn build(self) -> Checkpoint {
        Checkpoint {
            sequence: self.sequence,
            timestamp: self.timestamp,
            state_hash: self.state_hash.unwrap_or([0u8; 32]),
        }
    }

    /// Build the checkpoint, returning an error if hash is missing.
    pub fn build_checked(self) -> Result<Checkpoint, CheckpointError> {
        let state_hash = self.state_hash.ok_or(CheckpointError::MissingHash)?;
        Ok(Checkpoint {
            sequence: self.sequence,
            timestamp: self.timestamp,
            state_hash,
        })
    }
}

/// Binary serialization of checkpoint data.
///
/// This is the exact layout used in journal checkpoint records.
/// Layout (48 bytes):
/// - 0x00: sequence (u64, little-endian)
/// - 0x08: timestamp (i64, little-endian)
/// - 0x10: state_hash (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct CheckpointData {
    /// Checkpoint sequence number.
    pub sequence: u64,
    /// Timestamp in microseconds.
    pub timestamp: i64,
    /// SHA-256 state hash.
    pub state_hash: [u8; 32],
}

impl CheckpointData {
    /// Create new checkpoint data.
    pub fn new(sequence: u64, timestamp: i64, state_hash: [u8; 32]) -> Self {
        Self {
            sequence,
            timestamp,
            state_hash,
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; CHECKPOINT_DATA_SIZE] {
        let mut bytes = [0u8; CHECKPOINT_DATA_SIZE];
        bytes[0..8].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.timestamp.to_le_bytes());
        bytes[16..48].copy_from_slice(&self.state_hash);
        bytes
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CheckpointError> {
        if bytes.len() < CHECKPOINT_DATA_SIZE {
            return Err(CheckpointError::InvalidSize {
                expected: CHECKPOINT_DATA_SIZE,
                actual: bytes.len(),
            });
        }

        let sequence = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let timestamp = i64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let mut state_hash = [0u8; 32];
        state_hash.copy_from_slice(&bytes[16..48]);

        Ok(Self {
            sequence,
            timestamp,
            state_hash,
        })
    }

    /// Get the hash portion only (for Tap::record_checkpoint compatibility).
    pub fn hash_only(&self) -> &[u8; 32] {
        &self.state_hash
    }
}

/// Errors that can occur when working with checkpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointError {
    /// Invalid byte slice size.
    InvalidSize {
        /// Expected size in bytes.
        expected: usize,
        /// Actual size in bytes.
        actual: usize,
    },
    /// Missing state hash in builder.
    MissingHash,
    /// Checkpoint verification failed.
    VerificationFailed {
        /// Checkpoint sequence number.
        sequence: u64,
        /// Expected state hash.
        expected: [u8; 32],
        /// Actual state hash.
        actual: [u8; 32],
    },
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize { expected, actual } => {
                write!(
                    f,
                    "Invalid checkpoint size: expected {} bytes, got {}",
                    expected, actual
                )
            }
            Self::MissingHash => write!(f, "Missing state hash in checkpoint builder"),
            Self::VerificationFailed {
                sequence,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Checkpoint {} verification failed: expected {}, got {}",
                    sequence,
                    StateHash::to_hex(expected),
                    StateHash::to_hex(actual)
                )
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Checkpoint Basic Tests ====================

    #[test]
    fn test_checkpoint_new() {
        let hash = [0xAB; 32];
        let cp = Checkpoint::new(1, 1000, hash);

        assert_eq!(cp.sequence(), 1);
        assert_eq!(cp.timestamp(), 1000);
        assert_eq!(cp.state_hash(), &hash);
    }

    #[test]
    fn test_checkpoint_state_hash_hex() {
        let hash = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        let cp = Checkpoint::new(1, 1000, hash);

        assert_eq!(
            cp.state_hash_hex(),
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
        );
    }

    #[test]
    fn test_checkpoint_verify() {
        let hash = [0xAB; 32];
        let cp = Checkpoint::new(1, 1000, hash);

        assert!(cp.verify(&hash));
        assert!(!cp.verify(&[0x00; 32]));
    }

    #[test]
    fn test_checkpoint_is_after() {
        let cp1 = Checkpoint::new(1, 1000, [0; 32]);
        let cp2 = Checkpoint::new(2, 2000, [0; 32]);

        assert!(cp2.is_after(&cp1));
        assert!(!cp1.is_after(&cp2));
        assert!(!cp1.is_after(&cp1));
    }

    #[test]
    fn test_checkpoint_equality() {
        let hash = [0xAB; 32];
        let cp1 = Checkpoint::new(1, 1000, hash);
        let cp2 = Checkpoint::new(1, 1000, hash);
        let cp3 = Checkpoint::new(2, 1000, hash);

        assert_eq!(cp1, cp2);
        assert_ne!(cp1, cp3);
    }

    #[test]
    fn test_checkpoint_clone() {
        let hash = [0xAB; 32];
        let cp1 = Checkpoint::new(1, 1000, hash);
        let cp2 = cp1.clone();

        assert_eq!(cp1, cp2);
    }

    // ==================== CheckpointBuilder Tests ====================

    #[test]
    fn test_builder_new() {
        let builder = CheckpointBuilder::new();
        let cp = builder.build();

        assert_eq!(cp.sequence(), 0);
        assert_eq!(cp.timestamp(), 0);
        assert_eq!(cp.state_hash(), &[0u8; 32]);
    }

    #[test]
    fn test_builder_sequence() {
        let cp = CheckpointBuilder::new().sequence(42).build();
        assert_eq!(cp.sequence(), 42);
    }

    #[test]
    fn test_builder_timestamp() {
        let cp = CheckpointBuilder::new()
            .timestamp(1_704_067_200_000_000)
            .build();
        assert_eq!(cp.timestamp(), 1_704_067_200_000_000);
    }

    #[test]
    fn test_builder_hash() {
        let hash = [0xCD; 32];
        let cp = CheckpointBuilder::new().hash(hash).build();
        assert_eq!(cp.state_hash(), &hash);
    }

    #[test]
    fn test_builder_from_hasher() {
        let mut hasher = StateHash::new();
        hasher.update_orderbook(b"test");
        let expected = {
            let mut h = StateHash::new();
            h.update_orderbook(b"test");
            h.finalize()
        };

        let mut hasher = StateHash::new();
        hasher.update_orderbook(b"test");

        let cp = CheckpointBuilder::new().from_hasher(hasher).build();
        assert_eq!(cp.state_hash(), &expected);
    }

    #[test]
    fn test_builder_chained() {
        let hash = [0xEF; 32];
        let cp = CheckpointBuilder::new()
            .sequence(10)
            .timestamp(2000)
            .hash(hash)
            .build();

        assert_eq!(cp.sequence(), 10);
        assert_eq!(cp.timestamp(), 2000);
        assert_eq!(cp.state_hash(), &hash);
    }

    #[test]
    fn test_builder_build_checked_success() {
        let hash = [0xAB; 32];
        let result = CheckpointBuilder::new()
            .sequence(1)
            .timestamp(1000)
            .hash(hash)
            .build_checked();

        assert!(result.is_ok());
        let cp = result.unwrap();
        assert_eq!(cp.sequence(), 1);
    }

    #[test]
    fn test_builder_build_checked_missing_hash() {
        let result = CheckpointBuilder::new()
            .sequence(1)
            .timestamp(1000)
            .build_checked();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CheckpointError::MissingHash);
    }

    // ==================== CheckpointData Tests ====================

    #[test]
    fn test_checkpoint_data_size() {
        assert_eq!(std::mem::size_of::<CheckpointData>(), CHECKPOINT_DATA_SIZE);
    }

    #[test]
    fn test_checkpoint_data_new() {
        let hash = [0xAB; 32];
        let data = CheckpointData::new(1, 1000, hash);

        // Copy fields to avoid packed struct reference
        let seq = data.sequence;
        let ts = data.timestamp;
        let h = data.state_hash;

        assert_eq!(seq, 1);
        assert_eq!(ts, 1000);
        assert_eq!(h, hash);
    }

    #[test]
    fn test_checkpoint_data_roundtrip() {
        let hash = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        let original = CheckpointData::new(12345, 9876543210, hash);
        let bytes = original.to_bytes();
        let recovered = CheckpointData::from_bytes(&bytes).unwrap();

        // Copy fields to avoid packed struct reference
        let orig_seq = original.sequence;
        let orig_ts = original.timestamp;
        let orig_hash = original.state_hash;

        let rec_seq = recovered.sequence;
        let rec_ts = recovered.timestamp;
        let rec_hash = recovered.state_hash;

        assert_eq!(orig_seq, rec_seq);
        assert_eq!(orig_ts, rec_ts);
        assert_eq!(orig_hash, rec_hash);
    }

    #[test]
    fn test_checkpoint_data_bytes_layout() {
        let hash = [0xFF; 32];
        let data = CheckpointData::new(0x123456789ABCDEF0, 0x0FEDCBA987654321_u64 as i64, hash);
        let bytes = data.to_bytes();

        // sequence at offset 0 (little-endian)
        assert_eq!(
            u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            0x123456789ABCDEF0
        );

        // timestamp at offset 8 (little-endian)
        assert_eq!(
            i64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            0x0FEDCBA987654321_u64 as i64
        );

        // state_hash at offset 16
        assert_eq!(&bytes[16..48], &hash);
    }

    #[test]
    fn test_checkpoint_data_from_bytes_invalid_size() {
        let result = CheckpointData::from_bytes(&[0u8; 10]);
        assert!(result.is_err());

        if let Err(CheckpointError::InvalidSize { expected, actual }) = result {
            assert_eq!(expected, CHECKPOINT_DATA_SIZE);
            assert_eq!(actual, 10);
        } else {
            panic!("Expected InvalidSize error");
        }
    }

    #[test]
    fn test_checkpoint_data_from_bytes_empty() {
        let result = CheckpointData::from_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_checkpoint_data_hash_only() {
        let hash = [0xAB; 32];
        let data = CheckpointData::new(1, 1000, hash);
        assert_eq!(data.hash_only(), &hash);
    }

    // ==================== Checkpoint Serialization Tests ====================

    #[test]
    fn test_checkpoint_to_bytes() {
        let hash = [0xCD; 32];
        let cp = Checkpoint::new(100, 50000, hash);
        let bytes = cp.to_bytes();

        assert_eq!(bytes.len(), CHECKPOINT_DATA_SIZE);

        // Verify we can recover
        let recovered = Checkpoint::from_bytes(&bytes).unwrap();
        assert_eq!(cp, recovered);
    }

    #[test]
    fn test_checkpoint_from_bytes() {
        let hash = [0xEF; 32];
        let mut bytes = [0u8; CHECKPOINT_DATA_SIZE];
        bytes[0..8].copy_from_slice(&42u64.to_le_bytes());
        bytes[8..16].copy_from_slice(&1234567890i64.to_le_bytes());
        bytes[16..48].copy_from_slice(&hash);

        let cp = Checkpoint::from_bytes(&bytes).unwrap();

        assert_eq!(cp.sequence(), 42);
        assert_eq!(cp.timestamp(), 1234567890);
        assert_eq!(cp.state_hash(), &hash);
    }

    #[test]
    fn test_checkpoint_from_bytes_invalid_size() {
        let result = Checkpoint::from_bytes(&[0u8; 20]);
        assert!(result.is_err());
    }

    #[test]
    fn test_checkpoint_to_data() {
        let hash = [0x11; 32];
        let cp = Checkpoint::new(5, 9999, hash);
        let data = cp.to_data();

        // Copy fields to avoid packed struct reference
        let seq = data.sequence;
        let ts = data.timestamp;
        let h = data.state_hash;

        assert_eq!(seq, 5);
        assert_eq!(ts, 9999);
        assert_eq!(h, hash);
    }

    #[test]
    fn test_checkpoint_from_data() {
        let hash = [0x22; 32];
        let data = CheckpointData::new(7, 8888, hash);
        let cp = Checkpoint::from_data(data);

        assert_eq!(cp.sequence(), 7);
        assert_eq!(cp.timestamp(), 8888);
        assert_eq!(cp.state_hash(), &hash);
    }

    // ==================== Error Tests ====================

    #[test]
    fn test_checkpoint_error_display_invalid_size() {
        let err = CheckpointError::InvalidSize {
            expected: 48,
            actual: 10,
        };
        let msg = err.to_string();
        assert!(msg.contains("48"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_checkpoint_error_display_missing_hash() {
        let err = CheckpointError::MissingHash;
        let msg = err.to_string();
        assert!(msg.contains("Missing"));
    }

    #[test]
    fn test_checkpoint_error_display_verification_failed() {
        let expected = [0xAA; 32];
        let actual = [0xBB; 32];
        let err = CheckpointError::VerificationFailed {
            sequence: 5,
            expected,
            actual,
        };
        let msg = err.to_string();
        assert!(msg.contains("5"));
        assert!(msg.contains("verification failed"));
    }

    // ==================== Edge Case Tests ====================

    #[test]
    fn test_checkpoint_zero_values() {
        let cp = Checkpoint::new(0, 0, [0u8; 32]);
        assert_eq!(cp.sequence(), 0);
        assert_eq!(cp.timestamp(), 0);
        assert_eq!(cp.state_hash(), &[0u8; 32]);
    }

    #[test]
    fn test_checkpoint_max_sequence() {
        let cp = Checkpoint::new(u64::MAX, 0, [0u8; 32]);
        assert_eq!(cp.sequence(), u64::MAX);
    }

    #[test]
    fn test_checkpoint_negative_timestamp() {
        let cp = Checkpoint::new(0, -1000, [0u8; 32]);
        assert_eq!(cp.timestamp(), -1000);
    }

    #[test]
    fn test_checkpoint_min_timestamp() {
        let cp = Checkpoint::new(0, i64::MIN, [0u8; 32]);
        assert_eq!(cp.timestamp(), i64::MIN);
    }

    #[test]
    fn test_checkpoint_max_timestamp() {
        let cp = Checkpoint::new(0, i64::MAX, [0u8; 32]);
        assert_eq!(cp.timestamp(), i64::MAX);
    }

    #[test]
    fn test_checkpoint_debug() {
        let cp = Checkpoint::new(1, 1000, [0xAB; 32]);
        let debug = format!("{:?}", cp);
        assert!(debug.contains("Checkpoint"));
        assert!(debug.contains("1"));
        assert!(debug.contains("1000"));
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_checkpoint_workflow() {
        // Simulate creating a checkpoint from state
        let mut hasher = StateHash::new();
        hasher.update_orderbook(b"bid:100@50000");
        hasher.update_position(b"BTC:0.5");
        let hash = hasher.finalize();

        // Create checkpoint
        let cp = CheckpointBuilder::new()
            .sequence(1)
            .timestamp(1_704_067_200_000_000)
            .hash(hash)
            .build();

        // Serialize
        let bytes = cp.to_bytes();

        // Store in journal (simulated)
        let stored = bytes;

        // Recover during replay
        let recovered = Checkpoint::from_bytes(&stored).unwrap();

        // Verify
        assert!(recovered.verify(&hash));
    }

    #[test]
    fn test_multiple_checkpoints() {
        let checkpoints: Vec<Checkpoint> = (0..10)
            .map(|i| {
                let mut hasher = StateHash::new();
                hasher.update_sequence(i);
                let hash = hasher.finalize();

                CheckpointBuilder::new()
                    .sequence(i)
                    .timestamp(i as i64 * 1000)
                    .hash(hash)
                    .build()
            })
            .collect();

        // Verify ordering
        for i in 1..checkpoints.len() {
            assert!(checkpoints[i].is_after(&checkpoints[i - 1]));
        }

        // Verify all unique
        for (i, cp1) in checkpoints.iter().enumerate() {
            for (j, cp2) in checkpoints.iter().enumerate() {
                if i != j {
                    assert_ne!(cp1.state_hash(), cp2.state_hash());
                }
            }
        }
    }

    #[test]
    fn test_checkpoint_comparison_with_comparator() {
        use super::super::ReplayComparator;

        let hash = StateHash::hash_once(b"state data");
        let cp = Checkpoint::new(1, 1000, hash);

        let mut comparator = ReplayComparator::new();

        // Compare with matching hash
        let result = comparator.compare(cp.sequence(), cp.timestamp(), cp.state_hash(), &hash);
        assert!(matches!(
            result,
            super::super::comparator::CompareResult::Match
        ));

        // Compare with different hash
        let wrong_hash = StateHash::hash_once(b"different data");
        let result = comparator.compare(
            cp.sequence() + 1,
            cp.timestamp(),
            cp.state_hash(),
            &wrong_hash,
        );
        assert!(matches!(
            result,
            super::super::comparator::CompareResult::Mismatch { .. }
        ));
    }
}
