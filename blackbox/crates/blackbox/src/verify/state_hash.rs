//! State hashing for verification.
//!
//! This module provides deterministic hashing of trading system state
//! for comparing original and replayed sessions. The [`StateHash`] struct
//! is the core building block for checkpoint verification.
//!
//! ## Design Principles
//!
//! 1. **Determinism** - Same inputs always produce same hash
//! 2. **Order sensitivity** - Hash depends on order of updates
//! 3. **Type safety** - Labeled updates prevent hash collisions
//! 4. **Composability** - Multiple hashes can be combined
//!
//! ## Usage
//!
//! ```rust
//! use blackbox::verify::{StateHash, Hashable};
//!
//! // Basic usage
//! let mut hasher = StateHash::new();
//! hasher.update_orderbook(b"bid:100@50000");
//! hasher.update_position(b"BTC:0.5");
//! let hash = hasher.finalize();
//!
//! // Using Hashable trait
//! struct MyState { value: i64 }
//!
//! impl Hashable for MyState {
//!     fn hash_into(&self, hasher: &mut StateHash) {
//!         hasher.update(b"MyState", &self.value.to_le_bytes());
//!     }
//! }
//!
//! let state = MyState { value: 42 };
//! let hash = StateHash::from_hashable(&state);
//! ```

use sha2::{Digest, Sha256};

/// Trait for types that can be hashed into a StateHash.
///
/// Implement this trait to enable deterministic state hashing for
/// your types. The implementation must be deterministic - the same
/// state must always produce the same hash.
///
/// # Example
///
/// ```rust
/// use blackbox::verify::{StateHash, Hashable};
///
/// struct PriceLevel {
///     price: i64,
///     quantity: i64,
/// }
///
/// impl Hashable for PriceLevel {
///     fn hash_into(&self, hasher: &mut StateHash) {
///         hasher.update(b"PRICE", &self.price.to_le_bytes());
///         hasher.update(b"QTY", &self.quantity.to_le_bytes());
///     }
/// }
///
/// let level = PriceLevel { price: 50000, quantity: 100 };
/// let hash = StateHash::from_hashable(&level);
/// ```
pub trait Hashable {
    /// Hash this type's state into the given hasher.
    fn hash_into(&self, hasher: &mut StateHash);

    /// Convenience method to get hash directly.
    fn state_hash(&self) -> [u8; 32]
    where
        Self: Sized,
    {
        StateHash::from_hashable(self)
    }
}

/// A SHA-256 hash of system state for verification.
///
/// StateHash provides deterministic hashing of trading system state
/// for comparing original and replayed sessions. It uses SHA-256 for
/// cryptographic strength and produces 32-byte hashes.
///
/// # Determinism
///
/// StateHash is designed for deterministic operation:
/// - Same sequence of updates produces same hash
/// - Order of updates affects the final hash
/// - Each update type has a unique prefix to prevent collisions
///
/// # Example
///
/// ```
/// use blackbox::verify::StateHash;
///
/// let mut hasher = StateHash::new();
/// hasher.update_orderbook(b"bid:100@50000,ask:100@50001");
/// hasher.update_position(b"BTC:0.5");
/// let hash = hasher.finalize();
/// ```
pub struct StateHash {
    hasher: Sha256,
    update_count: u64,
}

impl StateHash {
    /// Create a new state hasher.
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            update_count: 0,
        }
    }

    /// Create a StateHash from a Hashable type.
    pub fn from_hashable<H: Hashable>(hashable: &H) -> [u8; 32] {
        let mut hasher = Self::new();
        hashable.hash_into(&mut hasher);
        hasher.finalize()
    }

    /// Update with orderbook state.
    ///
    /// The data should be a deterministic serialization of the orderbook.
    /// For best results, serialize price levels in sorted order.
    pub fn update_orderbook(&mut self, data: &[u8]) {
        self.hasher.update(b"OB:");
        self.hasher.update(data);
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with position state.
    pub fn update_position(&mut self, data: &[u8]) {
        self.hasher.update(b"POS:");
        self.hasher.update(data);
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with order state.
    pub fn update_orders(&mut self, data: &[u8]) {
        self.hasher.update(b"ORD:");
        self.hasher.update(data);
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with arbitrary labeled data.
    ///
    /// The label provides type discrimination to prevent hash collisions
    /// between different data types with the same byte representation.
    pub fn update(&mut self, label: &[u8], data: &[u8]) {
        self.hasher.update(label);
        self.hasher.update(b":");
        self.hasher.update(data);
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with raw bytes (no label).
    ///
    /// Use this only when you need direct byte hashing without labels.
    /// Prefer `update()` with a label for type safety.
    pub fn update_raw(&mut self, data: &[u8]) {
        self.hasher.update(data);
        self.update_count += 1;
    }

    /// Update with a sequence number for ordering.
    ///
    /// This helps ensure hash uniqueness across checkpoints.
    pub fn update_sequence(&mut self, sequence: u64) {
        self.hasher.update(b"SEQ:");
        self.hasher.update(sequence.to_le_bytes());
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with a timestamp for temporal ordering.
    pub fn update_timestamp(&mut self, timestamp_us: i64) {
        self.hasher.update(b"TS:");
        self.hasher.update(timestamp_us.to_le_bytes());
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with another hash (for combining hashes).
    ///
    /// This enables hierarchical hashing where component hashes
    /// can be combined into a root hash.
    pub fn update_hash(&mut self, hash: &[u8; 32]) {
        self.hasher.update(b"HASH:");
        self.hasher.update(hash);
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with price level data in a deterministic format.
    ///
    /// Price and quantity are hashed as fixed-size little-endian integers.
    pub fn update_price_level(&mut self, price: i64, quantity: i64) {
        self.hasher.update(b"LVL:");
        self.hasher.update(price.to_le_bytes());
        self.hasher.update(quantity.to_le_bytes());
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with bid levels (assumes pre-sorted by price descending).
    pub fn update_bids(&mut self, levels: &[(i64, i64)]) {
        self.hasher.update(b"BIDS:");
        self.hasher.update((levels.len() as u32).to_le_bytes());
        for (price, qty) in levels {
            self.hasher.update(price.to_le_bytes());
            self.hasher.update(qty.to_le_bytes());
        }
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Update with ask levels (assumes pre-sorted by price ascending).
    pub fn update_asks(&mut self, levels: &[(i64, i64)]) {
        self.hasher.update(b"ASKS:");
        self.hasher.update((levels.len() as u32).to_le_bytes());
        for (price, qty) in levels {
            self.hasher.update(price.to_le_bytes());
            self.hasher.update(qty.to_le_bytes());
        }
        self.hasher.update(b"\n");
        self.update_count += 1;
    }

    /// Get the number of updates performed.
    pub fn update_count(&self) -> u64 {
        self.update_count
    }

    /// Finalize and return the hash.
    pub fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }

    /// Compute hash of a single piece of data.
    pub fn hash_once(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Combine multiple hashes into a single root hash.
    ///
    /// This is useful for creating a Merkle-like structure where
    /// component hashes are combined into a final root hash.
    pub fn combine_hashes(hashes: &[[u8; 32]]) -> [u8; 32] {
        let mut hasher = Self::new();
        hasher.hasher.update(b"COMBINE:");
        hasher.hasher.update((hashes.len() as u32).to_le_bytes());
        for hash in hashes {
            hasher.hasher.update(hash);
        }
        hasher.finalize()
    }

    /// Convert a hash to a hex string for display.
    pub fn to_hex(hash: &[u8; 32]) -> String {
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Parse a hash from a hex string.
    pub fn from_hex(hex: &str) -> Option<[u8; 32]> {
        if hex.len() != 64 {
            return None;
        }

        let mut result = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk).ok()?;
            result[i] = u8::from_str_radix(s, 16).ok()?;
        }
        Some(result)
    }
}

impl Default for StateHash {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Hashable for common types

impl Hashable for i64 {
    fn hash_into(&self, hasher: &mut StateHash) {
        hasher.update(b"i64", &self.to_le_bytes());
    }
}

impl Hashable for u64 {
    fn hash_into(&self, hasher: &mut StateHash) {
        hasher.update(b"u64", &self.to_le_bytes());
    }
}

impl Hashable for f64 {
    fn hash_into(&self, hasher: &mut StateHash) {
        hasher.update(b"f64", &self.to_le_bytes());
    }
}

impl Hashable for [u8; 32] {
    fn hash_into(&self, hasher: &mut StateHash) {
        hasher.update_hash(self);
    }
}

impl<T: Hashable> Hashable for Vec<T> {
    fn hash_into(&self, hasher: &mut StateHash) {
        hasher.update(b"VEC_LEN", &(self.len() as u64).to_le_bytes());
        for item in self {
            item.hash_into(hasher);
        }
    }
}

impl<T: Hashable> Hashable for Option<T> {
    fn hash_into(&self, hasher: &mut StateHash) {
        match self {
            Some(v) => {
                hasher.update_raw(&[1u8]);
                v.hash_into(hasher);
            }
            None => {
                hasher.update_raw(&[0u8]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Basic Determinism Tests ====================

    #[test]
    fn test_state_hash_deterministic() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"test");
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"test");
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_state_hash_different_inputs() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"test1");
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"test2");
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_state_hash_order_matters() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"a");
            h.update_position(b"b");
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_position(b"b");
            h.update_orderbook(b"a");
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    // ==================== Update Count Tests ====================

    #[test]
    fn test_update_count_starts_at_zero() {
        let hasher = StateHash::new();
        assert_eq!(hasher.update_count(), 0);
    }

    #[test]
    fn test_update_count_increments() {
        let mut hasher = StateHash::new();
        hasher.update_orderbook(b"a");
        assert_eq!(hasher.update_count(), 1);

        hasher.update_position(b"b");
        assert_eq!(hasher.update_count(), 2);

        hasher.update_orders(b"c");
        assert_eq!(hasher.update_count(), 3);
    }

    #[test]
    fn test_update_count_all_methods() {
        let mut hasher = StateHash::new();
        hasher.update_orderbook(b"a");
        hasher.update_position(b"b");
        hasher.update_orders(b"c");
        hasher.update(b"label", b"d");
        hasher.update_raw(b"e");
        hasher.update_sequence(1);
        hasher.update_timestamp(1000);
        hasher.update_hash(&[0u8; 32]);
        hasher.update_price_level(100, 200);
        hasher.update_bids(&[(100, 10)]);
        hasher.update_asks(&[(101, 10)]);

        assert_eq!(hasher.update_count(), 11);
    }

    // ==================== Sequence & Timestamp Tests ====================

    #[test]
    fn test_sequence_affects_hash() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_sequence(1);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_sequence(2);
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_timestamp_affects_hash() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_timestamp(1000);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_timestamp(2000);
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    // ==================== Price Level Tests ====================

    #[test]
    fn test_price_level_deterministic() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_price_level(50000, 100);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_price_level(50000, 100);
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_price_level_different_price() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_price_level(50000, 100);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_price_level(50001, 100);
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_price_level_different_quantity() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_price_level(50000, 100);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_price_level(50000, 101);
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    // ==================== Bids/Asks Tests ====================

    #[test]
    fn test_bids_deterministic() {
        let levels = vec![(50000, 100), (49999, 200), (49998, 50)];

        let hash1 = {
            let mut h = StateHash::new();
            h.update_bids(&levels);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_bids(&levels);
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_asks_deterministic() {
        let levels = vec![(50001, 100), (50002, 200), (50003, 50)];

        let hash1 = {
            let mut h = StateHash::new();
            h.update_asks(&levels);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_asks(&levels);
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_bids_order_matters() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_bids(&[(50000, 100), (49999, 200)]);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_bids(&[(49999, 200), (50000, 100)]);
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_empty_bids_asks() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update_bids(&[]);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_asks(&[]);
            h.finalize()
        };

        // Empty bids and asks should have different hashes (different labels)
        assert_ne!(hash1, hash2);
    }

    // ==================== Hash Combination Tests ====================

    #[test]
    fn test_update_hash() {
        let inner_hash = StateHash::hash_once(b"inner");

        let hash1 = {
            let mut h = StateHash::new();
            h.update_hash(&inner_hash);
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_hash(&inner_hash);
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_combine_hashes() {
        let hash1 = StateHash::hash_once(b"data1");
        let hash2 = StateHash::hash_once(b"data2");

        let combined1 = StateHash::combine_hashes(&[hash1, hash2]);
        let combined2 = StateHash::combine_hashes(&[hash1, hash2]);

        assert_eq!(combined1, combined2);
    }

    #[test]
    fn test_combine_hashes_order_matters() {
        let hash1 = StateHash::hash_once(b"data1");
        let hash2 = StateHash::hash_once(b"data2");

        let combined1 = StateHash::combine_hashes(&[hash1, hash2]);
        let combined2 = StateHash::combine_hashes(&[hash2, hash1]);

        assert_ne!(combined1, combined2);
    }

    #[test]
    fn test_combine_empty() {
        let combined = StateHash::combine_hashes(&[]);
        // Should still produce a valid hash
        assert_eq!(combined.len(), 32);
    }

    #[test]
    fn test_combine_single() {
        let hash = StateHash::hash_once(b"data");
        let combined = StateHash::combine_hashes(&[hash]);
        // Combined single should be different from the input
        assert_ne!(combined, hash);
    }

    // ==================== Hex Conversion Tests ====================

    #[test]
    fn test_to_hex() {
        let hash = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        let hex = StateHash::to_hex(&hash);
        assert_eq!(
            hex,
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
        );
    }

    #[test]
    fn test_from_hex() {
        let hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let result = StateHash::from_hex(hex);
        assert!(result.is_some());

        let hash = result.unwrap();
        assert_eq!(hash[0], 0x01);
        assert_eq!(hash[31], 0x20);
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = StateHash::hash_once(b"test data");
        let hex = StateHash::to_hex(&original);
        let recovered = StateHash::from_hex(&hex);

        assert_eq!(recovered, Some(original));
    }

    #[test]
    fn test_from_hex_invalid_length() {
        assert_eq!(StateHash::from_hex("0102030405"), None);
        assert_eq!(StateHash::from_hex(""), None);
    }

    #[test]
    fn test_from_hex_invalid_chars() {
        // 'g' is not a valid hex character
        let invalid = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fgg";
        assert_eq!(StateHash::from_hex(invalid), None);
    }

    // ==================== Hashable Trait Tests ====================

    #[test]
    fn test_hashable_i64() {
        let value: i64 = 42;
        let hash = StateHash::from_hashable(&value);
        assert_eq!(hash.len(), 32);

        // Same value should produce same hash
        let hash2 = StateHash::from_hashable(&42i64);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_hashable_u64() {
        let value: u64 = 42;
        let hash = StateHash::from_hashable(&value);
        assert_eq!(hash.len(), 32);

        // i64 and u64 with same bit pattern should have different hashes (different labels)
        let hash_i64 = StateHash::from_hashable(&42i64);
        assert_ne!(hash, hash_i64);
    }

    #[test]
    fn test_hashable_f64() {
        let value: f64 = 123.456;
        let hash = StateHash::from_hashable(&value);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hashable_vec() {
        let vec: Vec<i64> = vec![1, 2, 3];
        let hash = StateHash::from_hashable(&vec);
        assert_eq!(hash.len(), 32);

        // Different vectors should have different hashes
        let vec2: Vec<i64> = vec![1, 2, 4];
        let hash2 = StateHash::from_hashable(&vec2);
        assert_ne!(hash, hash2);
    }

    #[test]
    fn test_hashable_option_some() {
        let value: Option<i64> = Some(42);
        let hash = StateHash::from_hashable(&value);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hashable_option_none() {
        let value: Option<i64> = None;
        let hash = StateHash::from_hashable(&value);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hashable_option_some_vs_none() {
        let some: Option<i64> = Some(0);
        let none: Option<i64> = None;

        let hash_some = StateHash::from_hashable(&some);
        let hash_none = StateHash::from_hashable(&none);

        assert_ne!(hash_some, hash_none);
    }

    #[test]
    fn test_hashable_state_hash_method() {
        let value: i64 = 42;
        let hash1 = StateHash::from_hashable(&value);
        let hash2 = value.state_hash();
        assert_eq!(hash1, hash2);
    }

    // ==================== Custom Hashable Tests ====================

    struct TestStruct {
        a: i64,
        b: i64,
    }

    impl Hashable for TestStruct {
        fn hash_into(&self, hasher: &mut StateHash) {
            hasher.update(b"TestStruct.a", &self.a.to_le_bytes());
            hasher.update(b"TestStruct.b", &self.b.to_le_bytes());
        }
    }

    #[test]
    fn test_custom_hashable() {
        let s1 = TestStruct { a: 1, b: 2 };
        let s2 = TestStruct { a: 1, b: 2 };
        let s3 = TestStruct { a: 1, b: 3 };

        assert_eq!(s1.state_hash(), s2.state_hash());
        assert_ne!(s1.state_hash(), s3.state_hash());
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_update() {
        let hash1 = {
            let mut h = StateHash::new();
            h.update(b"label", b"");
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update(b"label", b"");
            h.finalize()
        };

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_large_data() {
        let large_data = vec![0xABu8; 1024 * 1024]; // 1MB

        let mut h = StateHash::new();
        h.update_orderbook(&large_data);
        let hash = h.finalize();

        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hash_once() {
        let hash1 = StateHash::hash_once(b"test");
        let hash2 = StateHash::hash_once(b"test");
        assert_eq!(hash1, hash2);

        let hash3 = StateHash::hash_once(b"test2");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_default() {
        let h1 = StateHash::new();
        let h2 = StateHash::default();

        // Both should produce same hash when finalized empty
        assert_eq!(h1.finalize(), h2.finalize());
    }

    // ==================== Label Collision Prevention Tests ====================

    #[test]
    fn test_labels_prevent_collision() {
        // Without labels, "OB:a" and "POS:a" as raw bytes might collide
        // With labels, they should be different

        let hash1 = {
            let mut h = StateHash::new();
            h.update_orderbook(b"a");
            h.finalize()
        };

        let hash2 = {
            let mut h = StateHash::new();
            h.update_position(b"a");
            h.finalize()
        };

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_all_update_types_unique() {
        // All update methods should produce unique hashes for empty data
        let hashes: Vec<[u8; 32]> = vec![
            {
                let mut h = StateHash::new();
                h.update_orderbook(b"");
                h.finalize()
            },
            {
                let mut h = StateHash::new();
                h.update_position(b"");
                h.finalize()
            },
            {
                let mut h = StateHash::new();
                h.update_orders(b"");
                h.finalize()
            },
            {
                let mut h = StateHash::new();
                h.update(b"custom", b"");
                h.finalize()
            },
        ];

        // All should be unique
        for (i, h1) in hashes.iter().enumerate() {
            for (j, h2) in hashes.iter().enumerate() {
                if i != j {
                    assert_ne!(h1, h2, "Hash {} and {} should be different", i, j);
                }
            }
        }
    }
}
