//! Schema embedding and management.
//!
//! This module provides access to the embedded SBE schema and utilities
//! for creating and parsing SchemaBlock structures.
//!
//! ## Build-Time Embedding
//!
//! The schema XML is embedded at compile time via `build.rs`:
//! - `SCHEMA_XML`: The raw SBE XML schema string
//! - `SCHEMA_HASH`: CRC32 hash for integrity verification
//! - `SCHEMA_VERSION_MAJOR/MINOR`: Schema version numbers
//!
//! ## Usage
//!
//! ```ignore
//! use blackbox::journal::schema;
//!
//! // Get embedded schema
//! let xml = schema::embedded_xml();
//! let hash = schema::embedded_hash();
//!
//! // Create schema block for writing
//! let block = schema::create_block(true)?; // compressed
//!
//! // Parse schema block from bytes
//! let (header, xml) = schema::parse_block(&bytes)?;
//! ```

use super::format::{
    compression, SchemaBlock, SchemaBlockHeader, SCHEMA_BLOCK_HEADER_SIZE, SCHEMA_MAGIC,
};

// Include generated constants from build.rs
include!(concat!(env!("OUT_DIR"), "/schema_constants.rs"));

/// Error type for schema operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// Compression error.
    CompressionFailed(String),
    /// Decompression error.
    DecompressionFailed(String),
    /// Hash mismatch.
    HashMismatch {
        /// Expected hash value.
        expected: u32,
        /// Actual computed hash.
        actual: u32,
    },
    /// Invalid schema block header.
    InvalidHeader(String),
    /// Buffer too small.
    BufferTooSmall {
        /// Required buffer size.
        required: usize,
        /// Available buffer size.
        available: usize,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompressionFailed(e) => write!(f, "Schema compression failed: {}", e),
            Self::DecompressionFailed(e) => write!(f, "Schema decompression failed: {}", e),
            Self::HashMismatch { expected, actual } => {
                write!(
                    f,
                    "Schema hash mismatch: expected 0x{:08X}, got 0x{:08X}",
                    expected, actual
                )
            }
            Self::InvalidHeader(msg) => write!(f, "Invalid schema block header: {}", msg),
            Self::BufferTooSmall {
                required,
                available,
            } => {
                write!(
                    f,
                    "Buffer too small: need {} bytes, have {}",
                    required, available
                )
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// Get the embedded schema XML string.
///
/// This returns the SBE XML schema that was embedded at compile time.
/// This is the source of truth for encoding messages in new journal files.
#[inline]
pub fn embedded_xml() -> &'static str {
    SCHEMA_XML
}

/// Get the CRC32 hash of the embedded schema.
///
/// This hash can be used to verify schema integrity.
#[inline]
pub fn embedded_hash() -> u32 {
    SCHEMA_HASH
}

/// Get the embedded schema version (major, minor).
#[inline]
pub fn embedded_version() -> (u8, u8) {
    (SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR)
}

/// Compute CRC32 hash of the given data.
///
/// Uses the same algorithm as build.rs to ensure consistency.
pub fn compute_hash(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

/// Verify that the given XML matches the expected hash.
pub fn verify_hash(xml: &[u8], expected_hash: u32) -> Result<(), SchemaError> {
    let actual = compute_hash(xml);
    if actual != expected_hash {
        return Err(SchemaError::HashMismatch {
            expected: expected_hash,
            actual,
        });
    }
    Ok(())
}

/// Create a SchemaBlock from the embedded schema.
///
/// # Arguments
///
/// * `compress` - If true, compress the XML using Zstd (recommended)
/// * `compression_level` - Zstd compression level (1-21, default 3)
///
/// # Returns
///
/// A SchemaBlock ready to be written to a journal file.
pub fn create_block(compress: bool) -> Result<SchemaBlock, SchemaError> {
    create_block_with_level(compress, 3)
}

/// Create a SchemaBlock with custom compression level.
pub fn create_block_with_level(compress: bool, level: i32) -> Result<SchemaBlock, SchemaError> {
    let xml_bytes = SCHEMA_XML.as_bytes();
    let uncompressed_size = xml_bytes.len() as u32;

    let (data, compressed_size, compression_type) = if compress {
        let compressed = zstd::encode_all(xml_bytes, level)
            .map_err(|e| SchemaError::CompressionFailed(e.to_string()))?;
        let compressed_size = compressed.len() as u32;
        (compressed, compressed_size, compression::ZSTD)
    } else {
        (xml_bytes.to_vec(), uncompressed_size, compression::NONE)
    };

    let header = SchemaBlockHeader::new(uncompressed_size, compressed_size, compression_type);

    Ok(SchemaBlock { header, data })
}

/// Parse a SchemaBlock from raw bytes.
///
/// # Arguments
///
/// * `bytes` - Buffer containing schema block header + data
///
/// # Returns
///
/// A tuple of (SchemaBlockHeader, decompressed XML string)
pub fn parse_block(bytes: &[u8]) -> Result<(SchemaBlockHeader, String), SchemaError> {
    if bytes.len() < SCHEMA_BLOCK_HEADER_SIZE {
        return Err(SchemaError::BufferTooSmall {
            required: SCHEMA_BLOCK_HEADER_SIZE,
            available: bytes.len(),
        });
    }

    // Parse header
    let header_bytes: [u8; SCHEMA_BLOCK_HEADER_SIZE] =
        bytes[..SCHEMA_BLOCK_HEADER_SIZE].try_into().unwrap();
    let header = SchemaBlockHeader::from_bytes(&header_bytes);

    // Validate magic
    // Copy fields to avoid packed struct reference issues
    let magic = header.magic;
    if magic != SCHEMA_MAGIC {
        return Err(SchemaError::InvalidHeader(format!(
            "Invalid magic: expected {:?}, got {:?}",
            SCHEMA_MAGIC, magic
        )));
    }

    // Copy remaining fields
    let xml_compressed = header.xml_compressed;
    let xml_uncompressed = header.xml_uncompressed;
    let compression_type = header.compression;

    let total_size = SCHEMA_BLOCK_HEADER_SIZE + xml_compressed as usize;
    if bytes.len() < total_size {
        return Err(SchemaError::BufferTooSmall {
            required: total_size,
            available: bytes.len(),
        });
    }

    // Extract payload
    let payload = &bytes[SCHEMA_BLOCK_HEADER_SIZE..total_size];

    // Decompress if needed
    let xml_bytes = if compression_type == compression::ZSTD {
        zstd::decode_all(payload).map_err(|e| SchemaError::DecompressionFailed(e.to_string()))?
    } else {
        payload.to_vec()
    };

    // Verify decompressed size
    if xml_bytes.len() != xml_uncompressed as usize {
        return Err(SchemaError::DecompressionFailed(format!(
            "Size mismatch: expected {}, got {}",
            xml_uncompressed,
            xml_bytes.len()
        )));
    }

    // Convert to string
    let xml_string = String::from_utf8(xml_bytes)
        .map_err(|e| SchemaError::InvalidHeader(format!("Invalid UTF-8: {}", e)))?;

    Ok((header, xml_string))
}

/// Serialize a SchemaBlock to bytes for writing.
pub fn serialize_block(block: &SchemaBlock) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(block.total_size());
    bytes.extend_from_slice(&block.header.to_bytes());
    bytes.extend_from_slice(&block.data);
    bytes
}

/// Create a configured FileHeader and SchemaBlock pair for writing a new journal.
///
/// This is a convenience function that:
/// 1. Creates a new FileHeader with current timestamp
/// 2. Creates a SchemaBlock with the embedded schema (optionally compressed)
/// 3. Configures the FileHeader with schema offset, size, and hash
///
/// # Arguments
///
/// * `compress` - If true, compress the schema XML using Zstd
///
/// # Returns
///
/// A tuple of (FileHeader, SchemaBlock) ready to be written to a new journal file.
///
/// # Example
///
/// ```ignore
/// use blackbox::journal::schema;
///
/// let (header, schema_block) = schema::create_journal_header(true)?;
///
/// // Write to file:
/// // 1. Write header.to_bytes()
/// // 2. Write serialize_block(&schema_block)
/// // 3. Write records...
/// ```
pub fn create_journal_header(
    compress: bool,
) -> Result<(super::format::FileHeader, SchemaBlock), SchemaError> {
    let schema_block = create_block(compress)?;
    let schema_hash = embedded_hash();

    let mut header = super::format::FileHeader::new();
    header.with_schema(&schema_block, schema_hash);

    Ok((header, schema_block))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Embedded Schema Tests ====================

    #[test]
    fn test_embedded_xml_not_empty() {
        let xml = embedded_xml();
        assert!(!xml.is_empty(), "Embedded schema XML must not be empty");
    }

    #[test]
    fn test_embedded_xml_is_valid_xml() {
        let xml = embedded_xml();
        assert!(
            xml.starts_with("<?xml"),
            "Embedded schema must be valid XML"
        );
        assert!(
            xml.contains("messageSchema"),
            "Embedded schema must contain SBE message schema"
        );
    }

    #[test]
    fn test_embedded_hash_matches() {
        let xml = embedded_xml();
        let computed = compute_hash(xml.as_bytes());
        let embedded = embedded_hash();

        assert_eq!(computed, embedded, "Computed hash must match embedded hash");
    }

    #[test]
    fn test_embedded_version() {
        let (major, minor) = embedded_version();
        assert_eq!(major, 1, "Schema major version must be 1");
        assert_eq!(minor, 0, "Schema minor version must be 0");
    }

    // ==================== Hash Verification Tests ====================

    #[test]
    fn test_verify_hash_success() {
        let data = b"test data";
        let hash = compute_hash(data);
        assert!(verify_hash(data, hash).is_ok());
    }

    #[test]
    fn test_verify_hash_failure() {
        let data = b"test data";
        let wrong_hash = 0xDEADBEEF;
        let result = verify_hash(data, wrong_hash);
        assert!(matches!(result, Err(SchemaError::HashMismatch { .. })));
    }

    // ==================== Block Creation Tests ====================

    #[test]
    fn test_create_block_uncompressed() {
        let block = create_block(false).expect("Failed to create uncompressed block");

        // Copy fields to avoid packed struct reference issues
        let xml_uncompressed = block.header.xml_uncompressed;
        let xml_compressed = block.header.xml_compressed;
        let compression_type = block.header.compression;

        // Header should have correct magic
        assert_eq!(block.header.magic, SCHEMA_MAGIC);

        // Uncompressed sizes should match
        assert_eq!(xml_uncompressed, xml_compressed);
        assert_eq!(compression_type, compression::NONE);

        // Data should be raw XML
        let xml_str = std::str::from_utf8(&block.data).expect("Data should be valid UTF-8");
        assert_eq!(xml_str, embedded_xml());
    }

    #[test]
    fn test_create_block_compressed() {
        let block = create_block(true).expect("Failed to create compressed block");

        // Copy fields to avoid packed struct reference issues
        let xml_uncompressed = block.header.xml_uncompressed;
        let xml_compressed = block.header.xml_compressed;
        let compression_type = block.header.compression;

        // Header should have correct magic
        assert_eq!(block.header.magic, SCHEMA_MAGIC);

        // Compressed size should be smaller
        assert!(
            xml_compressed < xml_uncompressed,
            "Compressed size ({}) should be less than uncompressed ({})",
            xml_compressed,
            xml_uncompressed
        );
        assert_eq!(compression_type, compression::ZSTD);
    }

    #[test]
    fn test_block_total_size() {
        let block = create_block(true).expect("Failed to create block");
        let xml_compressed = block.header.xml_compressed;

        assert_eq!(
            block.total_size(),
            SCHEMA_BLOCK_HEADER_SIZE + xml_compressed as usize
        );
    }

    // ==================== Block Serialization Tests ====================

    #[test]
    fn test_serialize_block() {
        let block = create_block(true).expect("Failed to create block");
        let bytes = serialize_block(&block);

        assert_eq!(bytes.len(), block.total_size());

        // First 8 bytes should be magic
        assert_eq!(&bytes[0..8], b"BLKBOXSC");
    }

    // ==================== Block Parsing Tests ====================

    #[test]
    fn test_parse_block_compressed() {
        // Create and serialize a block
        let original_block = create_block(true).expect("Failed to create block");
        let bytes = serialize_block(&original_block);

        // Copy fields from original to avoid packed struct reference issues
        let orig_xml_uncompressed = original_block.header.xml_uncompressed;
        let orig_xml_compressed = original_block.header.xml_compressed;

        // Parse it back
        let (parsed_header, parsed_xml) = parse_block(&bytes).expect("Failed to parse block");

        // Copy fields from parsed to avoid packed struct reference issues
        let parsed_xml_uncompressed = parsed_header.xml_uncompressed;
        let parsed_xml_compressed = parsed_header.xml_compressed;
        let parsed_compression = parsed_header.compression;

        // Verify header matches
        assert_eq!(parsed_header.magic, SCHEMA_MAGIC);
        assert_eq!(parsed_xml_uncompressed, orig_xml_uncompressed);
        assert_eq!(parsed_xml_compressed, orig_xml_compressed);
        assert_eq!(parsed_compression, compression::ZSTD);

        // Verify XML matches
        assert_eq!(parsed_xml, embedded_xml());
    }

    #[test]
    fn test_parse_block_uncompressed() {
        // Create and serialize an uncompressed block
        let original_block = create_block(false).expect("Failed to create block");
        let bytes = serialize_block(&original_block);

        // Parse it back
        let (parsed_header, parsed_xml) = parse_block(&bytes).expect("Failed to parse block");

        // Copy field to avoid packed struct reference issues
        let parsed_compression = parsed_header.compression;

        // Verify compression type
        assert_eq!(parsed_compression, compression::NONE);

        // Verify XML matches
        assert_eq!(parsed_xml, embedded_xml());
    }

    #[test]
    fn test_parse_block_invalid_magic() {
        let mut bytes = vec![0u8; 64];
        bytes[0..8].copy_from_slice(b"BADMAGIC");

        let result = parse_block(&bytes);
        assert!(matches!(result, Err(SchemaError::InvalidHeader(_))));
    }

    #[test]
    fn test_parse_block_buffer_too_small() {
        let bytes = vec![0u8; 16]; // Less than header size

        let result = parse_block(&bytes);
        assert!(matches!(result, Err(SchemaError::BufferTooSmall { .. })));
    }

    #[test]
    fn test_parse_block_corrupt_compressed_data() {
        // Create a valid header but with corrupt compressed data
        let header = SchemaBlockHeader::new(1000, 52, compression::ZSTD);
        let mut bytes = header.to_bytes().to_vec();
        // Add random garbage as "compressed" data (52 bytes of garbage)
        for _ in 0..13 {
            bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        }

        let result = parse_block(&bytes);
        assert!(matches!(result, Err(SchemaError::DecompressionFailed(_))));
    }

    #[test]
    fn test_parse_block_payload_too_small() {
        // Create header claiming larger payload than available
        let header = SchemaBlockHeader::new(1000, 500, compression::ZSTD);
        let mut bytes = header.to_bytes().to_vec();
        // Only add 10 bytes, but header claims 500
        bytes.extend_from_slice(&[0u8; 10]);

        let result = parse_block(&bytes);
        assert!(matches!(result, Err(SchemaError::BufferTooSmall { .. })));
    }

    // ==================== Round-Trip Tests ====================

    #[test]
    fn test_roundtrip_compressed() {
        // Create, serialize, parse, verify
        let block = create_block(true).expect("Failed to create block");
        let bytes = serialize_block(&block);
        let (_, xml) = parse_block(&bytes).expect("Failed to parse block");

        assert_eq!(xml, embedded_xml());
    }

    #[test]
    fn test_roundtrip_uncompressed() {
        // Create, serialize, parse, verify
        let block = create_block(false).expect("Failed to create block");
        let bytes = serialize_block(&block);
        let (_, xml) = parse_block(&bytes).expect("Failed to parse block");

        assert_eq!(xml, embedded_xml());
    }

    #[test]
    fn test_roundtrip_hash_verification() {
        let block = create_block(true).expect("Failed to create block");
        let bytes = serialize_block(&block);
        let (_, xml) = parse_block(&bytes).expect("Failed to parse block");

        // Verify hash of parsed XML matches embedded hash
        let hash = compute_hash(xml.as_bytes());
        assert_eq!(hash, embedded_hash());
    }

    // ==================== Compression Level Tests ====================

    #[test]
    fn test_compression_levels() {
        // Low compression (fast)
        let block_low = create_block_with_level(true, 1).expect("Failed with level 1");
        let size_low = block_low.header.xml_compressed;

        // High compression (smaller)
        let block_high = create_block_with_level(true, 19).expect("Failed with level 19");
        let size_high = block_high.header.xml_compressed;

        // Higher compression should produce smaller (or equal) output
        assert!(
            size_high <= size_low,
            "Higher compression level should produce smaller or equal output"
        );

        // Both should round-trip correctly
        let bytes_low = serialize_block(&block_low);
        let (_, xml_low) = parse_block(&bytes_low).expect("Failed to parse low");
        assert_eq!(xml_low, embedded_xml());

        let bytes_high = serialize_block(&block_high);
        let (_, xml_high) = parse_block(&bytes_high).expect("Failed to parse high");
        assert_eq!(xml_high, embedded_xml());
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_create_journal_header_compressed() {
        use super::super::format::FILE_HEADER_SIZE;

        let (header, schema_block) = create_journal_header(true).expect("Failed to create header");

        // Copy fields from header to avoid packed struct reference issues
        let schema_offset = header.schema_offset;
        let schema_size = header.schema_size;
        let first_record = header.first_record;
        let schema_hash = header.schema_hash;

        // Verify header fields are correctly set
        assert_eq!(
            schema_offset, FILE_HEADER_SIZE as u64,
            "Schema offset should be right after header"
        );
        assert_eq!(
            schema_size,
            schema_block.total_size() as u64,
            "Schema size should match block size"
        );
        assert_eq!(
            first_record,
            FILE_HEADER_SIZE as u64 + schema_size,
            "First record should be after header + schema"
        );
        assert_eq!(
            schema_hash,
            embedded_hash(),
            "Schema hash should match embedded hash"
        );

        // Verify schema block can be parsed
        let bytes = serialize_block(&schema_block);
        let (_, xml) = parse_block(&bytes).expect("Failed to parse block");
        assert_eq!(xml, embedded_xml());
    }

    #[test]
    fn test_create_journal_header_uncompressed() {
        let (header, schema_block) = create_journal_header(false).expect("Failed to create header");

        // Copy fields from header
        let schema_size = header.schema_size;
        let compression = schema_block.header.compression;

        // Verify uncompressed
        assert_eq!(compression, compression::NONE);

        // Verify schema block matches expected size
        assert_eq!(schema_size, schema_block.total_size() as u64);
    }

    #[test]
    fn test_file_header_with_schema() {
        use super::super::format::{FileHeader, FILE_HEADER_SIZE};

        let block = create_block(true).expect("Failed to create block");
        let hash = embedded_hash();

        let mut header = FileHeader::new();
        header.with_schema(&block, hash);

        // Copy fields to avoid packed struct issues
        let schema_offset = header.schema_offset;
        let schema_size = header.schema_size;
        let first_record = header.first_record;
        let stored_hash = header.schema_hash;

        assert_eq!(schema_offset, FILE_HEADER_SIZE as u64);
        assert_eq!(schema_size, block.total_size() as u64);
        assert_eq!(first_record, FILE_HEADER_SIZE as u64 + schema_size);
        assert_eq!(stored_hash, hash);
    }

    #[test]
    fn test_file_header_version_methods() {
        use super::super::format::{
            FileHeader, FORMAT_VERSION_MAJOR, FORMAT_VERSION_MINOR, SCHEMA_VERSION_MAJOR,
            SCHEMA_VERSION_MINOR,
        };

        let header = FileHeader::new();

        let (format_major, format_minor) = header.format_version();
        let (schema_major, schema_minor) = header.schema_version();

        assert_eq!(format_major, FORMAT_VERSION_MAJOR);
        assert_eq!(format_minor, FORMAT_VERSION_MINOR);
        assert_eq!(schema_major, SCHEMA_VERSION_MAJOR);
        assert_eq!(schema_minor, SCHEMA_VERSION_MINOR);
    }
}
