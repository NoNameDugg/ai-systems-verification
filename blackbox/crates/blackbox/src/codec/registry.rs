//! Codec registry for version-dispatched decoding.
//!
//! The CodecRegistry maintains a mapping of schema versions to their
//! corresponding decoders and encoders. When reading a journal file,
//! the registry is used to select the appropriate codec based on the
//! schema version stored in the file header.
//!
//! # Example
//!
//! ```ignore
//! use blackbox::codec::CodecRegistry;
//!
//! let registry = CodecRegistry::new();
//!
//! // Get decoder for schema version 1.0
//! if let Some(decoder) = registry.decoder(1, 0) {
//!     let message = decoder.decode_raw_frame(&buffer)?;
//! }
//!
//! // Get the current encoder
//! let encoder = registry.encoder();
//! let size = encoder.encode_raw_frame(&data, &mut buffer)?;
//! ```

use super::error::CodecError;
use super::traits::{MessageDecoder, MessageEncoder};
use super::v1_0;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of codec versions.
///
/// The registry maintains a collection of decoders and encoders for
/// different schema versions. It provides version dispatch functionality
/// to select the appropriate codec when reading journal files.
///
/// # Thread Safety
///
/// The registry is designed to be created once and shared across threads.
/// Both decoders and encoders are wrapped in Arc for efficient sharing.
pub struct CodecRegistry {
    /// Map of (major, minor) -> decoder.
    decoders: HashMap<(u8, u8), Arc<dyn MessageDecoder>>,
    /// Map of (major, minor) -> encoder.
    encoders: HashMap<(u8, u8), Arc<dyn MessageEncoder>>,
    /// Current schema version for encoding new messages.
    current_version: (u8, u8),
}

impl CodecRegistry {
    /// Create a new codec registry with all known versions registered.
    pub fn new() -> Self {
        let mut decoders: HashMap<(u8, u8), Arc<dyn MessageDecoder>> = HashMap::new();
        let mut encoders: HashMap<(u8, u8), Arc<dyn MessageEncoder>> = HashMap::new();

        // Register v1.0 codec
        decoders.insert((1, 0), Arc::new(v1_0::Decoder::new()));
        encoders.insert((1, 0), Arc::new(v1_0::Encoder::new()));

        // Future versions would be registered here:
        // decoders.insert((1, 1), Arc::new(v1_1::Decoder::new()));
        // encoders.insert((1, 1), Arc::new(v1_1::Encoder::new()));

        Self {
            decoders,
            encoders,
            current_version: v1_0::SCHEMA_VERSION,
        }
    }

    /// Get a decoder for the specified schema version.
    ///
    /// Returns `None` if the version is not supported.
    pub fn decoder(&self, major: u8, minor: u8) -> Option<Arc<dyn MessageDecoder>> {
        self.decoders.get(&(major, minor)).cloned()
    }

    /// Get an encoder for the specified schema version.
    ///
    /// Returns `None` if the version is not supported.
    pub fn encoder_for_version(&self, major: u8, minor: u8) -> Option<Arc<dyn MessageEncoder>> {
        self.encoders.get(&(major, minor)).cloned()
    }

    /// Get the current encoder (for the current schema version).
    ///
    /// This always succeeds as the current version is always registered.
    pub fn encoder(&self) -> Arc<dyn MessageEncoder> {
        self.encoders
            .get(&self.current_version)
            .expect("Current version must be registered")
            .clone()
    }

    /// Get the current schema version.
    pub fn current_version(&self) -> (u8, u8) {
        self.current_version
    }

    /// Check if a schema version is supported.
    pub fn is_supported(&self, major: u8, minor: u8) -> bool {
        self.decoders.contains_key(&(major, minor))
    }

    /// Get all supported decoder versions.
    pub fn supported_decoder_versions(&self) -> Vec<(u8, u8)> {
        let mut versions: Vec<_> = self.decoders.keys().copied().collect();
        versions.sort();
        versions
    }

    /// Get all supported encoder versions.
    pub fn supported_encoder_versions(&self) -> Vec<(u8, u8)> {
        let mut versions: Vec<_> = self.encoders.keys().copied().collect();
        versions.sort();
        versions
    }

    /// Get a decoder for a version, with fallback logic.
    ///
    /// This method implements the version selection rules from the design notes:
    /// 1. Exact match: Use the decoder for the exact version
    /// 2. Same major, higher minor: Use the highest available minor version
    /// 3. Different major: Return error (not supported)
    ///
    /// # Arguments
    ///
    /// * `major` - Major schema version
    /// * `minor` - Minor schema version
    ///
    /// # Returns
    ///
    /// The decoder to use, or an error if no compatible decoder exists.
    pub fn decoder_with_fallback(
        &self,
        major: u8,
        minor: u8,
    ) -> Result<Arc<dyn MessageDecoder>, CodecError> {
        // Try exact match first
        if let Some(decoder) = self.decoders.get(&(major, minor)) {
            return Ok(decoder.clone());
        }

        // Find the highest minor version for the same major
        let mut best_match: Option<(u8, Arc<dyn MessageDecoder>)> = None;
        for ((maj, min), decoder) in &self.decoders {
            if *maj == major && *min <= minor {
                match &best_match {
                    None => best_match = Some((*min, decoder.clone())),
                    Some((best_min, _)) if min > best_min => {
                        best_match = Some((*min, decoder.clone()));
                    }
                    _ => {}
                }
            }
        }

        if let Some((_, decoder)) = best_match {
            return Ok(decoder);
        }

        // No compatible decoder found
        Err(CodecError::UnsupportedVersion { major, minor })
    }

    /// Attempt to create a dynamic decoder from embedded schema XML.
    ///
    /// This is the fallback for completely unknown versions.
    /// Currently returns an error, but could be extended to parse
    /// the schema XML and generate a decoder at runtime.
    pub fn dynamic_decoder(
        &self,
        _schema_xml: &str,
    ) -> Result<Arc<dyn MessageDecoder>, CodecError> {
        // TODO: Implement dynamic schema parsing for the 10-year guarantee
        // This would parse the embedded SBE XML and create a generic decoder
        Err(CodecError::SchemaParseError(
            "Dynamic schema decoding not yet implemented".to_string(),
        ))
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CodecRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodecRegistry")
            .field("current_version", &self.current_version)
            .field("decoder_versions", &self.supported_decoder_versions())
            .field("encoder_versions", &self.supported_encoder_versions())
            .finish()
    }
}

/// Global codec registry instance.
///
/// This provides a convenient way to access the registry without
/// creating a new instance each time.
pub fn global_registry() -> &'static CodecRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(CodecRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::data::{Exchange, RawFrameData};

    // ==================== Registry Creation Tests ====================

    #[test]
    fn test_registry_new() {
        let registry = CodecRegistry::new();
        assert!(registry.is_supported(1, 0));
        assert!(!registry.is_supported(2, 0));
        assert!(!registry.is_supported(1, 1));
    }

    #[test]
    fn test_registry_default() {
        let registry = CodecRegistry::default();
        assert!(registry.is_supported(1, 0));
    }

    #[test]
    fn test_registry_current_version() {
        let registry = CodecRegistry::new();
        assert_eq!(registry.current_version(), (1, 0));
    }

    // ==================== Decoder Tests ====================

    #[test]
    fn test_decoder_v1_0() {
        let registry = CodecRegistry::new();
        let decoder = registry.decoder(1, 0);
        assert!(decoder.is_some());
        assert_eq!(decoder.unwrap().schema_version(), (1, 0));
    }

    #[test]
    fn test_decoder_unsupported() {
        let registry = CodecRegistry::new();
        assert!(registry.decoder(2, 0).is_none());
        assert!(registry.decoder(1, 1).is_none());
        assert!(registry.decoder(0, 0).is_none());
    }

    // ==================== Encoder Tests ====================

    #[test]
    fn test_encoder_current() {
        let registry = CodecRegistry::new();
        let encoder = registry.encoder();
        assert_eq!(encoder.schema_version(), (1, 0));
    }

    #[test]
    fn test_encoder_for_version() {
        let registry = CodecRegistry::new();
        let encoder = registry.encoder_for_version(1, 0);
        assert!(encoder.is_some());
        assert_eq!(encoder.unwrap().schema_version(), (1, 0));
    }

    #[test]
    fn test_encoder_for_version_unsupported() {
        let registry = CodecRegistry::new();
        assert!(registry.encoder_for_version(2, 0).is_none());
    }

    // ==================== Supported Versions Tests ====================

    #[test]
    fn test_supported_decoder_versions() {
        let registry = CodecRegistry::new();
        let versions = registry.supported_decoder_versions();
        assert_eq!(versions, vec![(1, 0)]);
    }

    #[test]
    fn test_supported_encoder_versions() {
        let registry = CodecRegistry::new();
        let versions = registry.supported_encoder_versions();
        assert_eq!(versions, vec![(1, 0)]);
    }

    // ==================== Decoder with Fallback Tests ====================

    #[test]
    fn test_decoder_with_fallback_exact_match() {
        let registry = CodecRegistry::new();
        let decoder = registry.decoder_with_fallback(1, 0).unwrap();
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    #[test]
    fn test_decoder_with_fallback_higher_minor() {
        let registry = CodecRegistry::new();
        // Request v1.1, should fall back to v1.0
        let decoder = registry.decoder_with_fallback(1, 1).unwrap();
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    #[test]
    fn test_decoder_with_fallback_much_higher_minor() {
        let registry = CodecRegistry::new();
        // Request v1.99, should still fall back to v1.0
        let decoder = registry.decoder_with_fallback(1, 99).unwrap();
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    #[test]
    fn test_decoder_with_fallback_different_major() {
        let registry = CodecRegistry::new();
        // Request v2.0, should fail (no compatible decoder)
        let result = registry.decoder_with_fallback(2, 0);
        assert!(matches!(
            result,
            Err(CodecError::UnsupportedVersion { major: 2, minor: 0 })
        ));
    }

    #[test]
    fn test_decoder_with_fallback_lower_major() {
        let registry = CodecRegistry::new();
        // Request v0.1, should fail (no v0.x decoders)
        let result = registry.decoder_with_fallback(0, 1);
        assert!(matches!(
            result,
            Err(CodecError::UnsupportedVersion { major: 0, minor: 1 })
        ));
    }

    // ==================== Dynamic Decoder Tests ====================

    #[test]
    fn test_dynamic_decoder_not_implemented() {
        let registry = CodecRegistry::new();
        let result = registry.dynamic_decoder("<schema/>");
        assert!(matches!(result, Err(CodecError::SchemaParseError(_))));
    }

    // ==================== Global Registry Tests ====================

    #[test]
    fn test_global_registry() {
        let registry = global_registry();
        assert!(registry.is_supported(1, 0));
    }

    #[test]
    fn test_global_registry_same_instance() {
        let r1 = global_registry();
        let r2 = global_registry();
        assert!(std::ptr::eq(r1, r2));
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_encode_decode_roundtrip_via_registry() {
        let registry = CodecRegistry::new();

        let original = RawFrameData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            sequence_number: 42,
            payload: b"test payload".to_vec(),
        };

        // Encode with current encoder
        let encoder = registry.encoder();
        let mut buf = vec![0u8; 256];
        let size = encoder.encode_raw_frame(&original, &mut buf).unwrap();

        // Decode with v1.0 decoder
        let decoder = registry.decoder(1, 0).unwrap();
        let decoded = decoder.decode_raw_frame(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.sequence_number, original.sequence_number);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_version_fallback_workflow() {
        let registry = CodecRegistry::new();

        // Simulate reading a journal with schema v1.5 (future version)
        let journal_schema_version = (1, 5);

        // Get decoder with fallback
        let decoder = registry
            .decoder_with_fallback(journal_schema_version.0, journal_schema_version.1)
            .expect("Should fall back to v1.0");

        // Should get v1.0 decoder
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    // ==================== Debug Tests ====================

    #[test]
    fn test_registry_debug() {
        let registry = CodecRegistry::new();
        let debug = format!("{:?}", registry);
        assert!(debug.contains("CodecRegistry"));
        assert!(debug.contains("current_version"));
        assert!(debug.contains("(1, 0)"));
    }
}
