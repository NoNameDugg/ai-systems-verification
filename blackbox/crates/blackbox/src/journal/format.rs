//! Binary format definitions for journal files.
//!
//! All structures are designed for direct memory mapping with
//! explicit field layouts and no padding surprises.
//!
//! Reference: the format spec, sections 6, 8

use blackbox_types::Timestamp;

/// Magic number for journal files: "BLKBOXJL" in ASCII.
pub const FILE_MAGIC: [u8; 8] = *b"BLKBOXJL";

/// Magic number for schema block: "BLKBOXSC" in ASCII.
pub const SCHEMA_MAGIC: [u8; 8] = *b"BLKBOXSC";

/// Magic number for file footer: "BLKBOXND" in ASCII.
pub const FOOTER_MAGIC: [u8; 8] = *b"BLKBOXND";

/// Current format version major (binary layout changes).
pub const FORMAT_VERSION_MAJOR: u8 = 1;
/// Current format version minor (backward-compatible changes).
pub const FORMAT_VERSION_MINOR: u8 = 2;

/// Current schema version major (SBE message breaking changes).
pub const SCHEMA_VERSION_MAJOR: u8 = 1;
/// Current schema version minor (SBE message append-only changes).
pub const SCHEMA_VERSION_MINOR: u8 = 0;

/// File header size in bytes (the format spec, section 8.1).
pub const FILE_HEADER_SIZE: usize = 128;

/// Record header size in bytes (the format spec, section 6.1).
pub const RECORD_HEADER_SIZE: usize = 24;

/// Schema block header size in bytes (the format spec, section 8.1.1).
pub const SCHEMA_BLOCK_HEADER_SIZE: usize = 32;

/// File footer size in bytes (the format spec, section 8.2).
pub const FILE_FOOTER_SIZE: usize = 32;

/// File header at the start of every journal file.
///
/// Layout (128 bytes) per the format spec, section 8.1:
/// ```text
/// Offset  Size  Field              Description
/// ────────────────────────────────────────────────────────────
/// 0x00    8     magic              "BLKBOXJL" (ASCII)
/// 0x08    1     format_major       Format version major
/// 0x09    1     format_minor       Format version minor
/// 0x0A    1     schema_major       Schema version major (SBE)
/// 0x0B    1     schema_minor       Schema version minor (SBE)
/// 0x0C    4     header_size        128 (v1.2+)
/// 0x10    8     schema_offset      Offset to SchemaBlock
/// 0x18    8     schema_size        Size of SchemaBlock (bytes)
/// 0x20    8     first_record       Offset to first record
/// 0x28    8     session_start      First record timestamp (μs)
/// 0x30    8     session_end        Last record timestamp (updated)
/// 0x38    8     record_count       Number of records (updated)
/// 0x40    2     flags              File flags
/// 0x42    2     reserved_1         Reserved (must be 0)
/// 0x44    4     schema_hash        CRC32 of schema XML
/// 0x48    32    checksum           SHA-256 of header + schema
/// 0x68    24    reserved_2         Reserved for future use
/// ────────────────────────────────────────────────────────────
/// Total: 128 bytes
/// ```
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FileHeader {
    /// Magic number "BLKBOXJL".
    pub magic: [u8; 8],
    /// Format version major (binary layout changes).
    pub format_major: u8,
    /// Format version minor (backward-compatible changes).
    pub format_minor: u8,
    /// Schema version major (SBE message changes - breaking).
    pub schema_major: u8,
    /// Schema version minor (SBE message changes - append-only).
    pub schema_minor: u8,
    /// Header size in bytes (128 for v1.2+).
    pub header_size: u32,
    /// Offset to SchemaBlock from start of file.
    pub schema_offset: u64,
    /// Size of SchemaBlock in bytes.
    pub schema_size: u64,
    /// Offset to first record from start of file.
    pub first_record: u64,
    /// First record timestamp (microseconds since epoch).
    pub session_start: i64,
    /// Last record timestamp (updated on close).
    pub session_end: i64,
    /// Number of records in file (updated on close).
    pub record_count: u64,
    /// File flags (bit field).
    pub flags: u16,
    /// Reserved (must be 0).
    pub reserved_1: u16,
    /// CRC32 hash of embedded schema XML.
    pub schema_hash: u32,
    /// SHA-256 checksum of header + schema.
    pub checksum: [u8; 32],
    /// Reserved for future expansion.
    pub reserved_2: [u8; 24],
}

impl FileHeader {
    /// Create a new file header with current timestamp.
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);

        Self::new_with_session_start(now)
    }

    /// Create a new file header with a specific timestamp.
    ///
    /// This is used for deterministic testing and replay scenarios
    /// where the session start time must be controlled.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The session start timestamp
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::journal::FileHeader;
    /// use blackbox_types::Timestamp;
    ///
    /// let ts = Timestamp::from_micros(1_704_067_200_000_000);
    /// let header = FileHeader::new_with_timestamp(ts);
    /// // Copy field to avoid packed struct reference
    /// let session_start = header.session_start;
    /// assert_eq!(session_start, 1_704_067_200_000_000);
    /// ```
    pub fn new_with_timestamp(timestamp: Timestamp) -> Self {
        Self::new_with_session_start(timestamp.as_micros())
    }

    /// Internal constructor with session start value.
    fn new_with_session_start(session_start: i64) -> Self {
        Self {
            magic: FILE_MAGIC,
            format_major: FORMAT_VERSION_MAJOR,
            format_minor: FORMAT_VERSION_MINOR,
            schema_major: SCHEMA_VERSION_MAJOR,
            schema_minor: SCHEMA_VERSION_MINOR,
            header_size: FILE_HEADER_SIZE as u32,
            schema_offset: FILE_HEADER_SIZE as u64,
            schema_size: 0,
            first_record: 0,
            session_start,
            session_end: 0,
            record_count: 0,
            flags: 0,
            reserved_1: 0,
            schema_hash: 0,
            checksum: [0u8; 32],
            reserved_2: [0u8; 24],
        }
    }

    /// Validate the header magic and version.
    pub fn validate(&self) -> Result<(), HeaderError> {
        if self.magic != FILE_MAGIC {
            return Err(HeaderError::InvalidMagic);
        }
        if self.format_major > FORMAT_VERSION_MAJOR {
            return Err(HeaderError::UnsupportedVersion {
                major: self.format_major,
                minor: self.format_minor,
            });
        }
        Ok(())
    }

    /// Convert to bytes for writing.
    pub fn to_bytes(&self) -> [u8; FILE_HEADER_SIZE] {
        // SAFETY: FileHeader is repr(C, packed) with known size
        unsafe { std::mem::transmute_copy(self) }
    }

    /// Parse from bytes.
    pub fn from_bytes(bytes: &[u8; FILE_HEADER_SIZE]) -> Self {
        // SAFETY: FileHeader is repr(C, packed) with known size
        unsafe { std::ptr::read(bytes.as_ptr() as *const Self) }
    }

    /// Configure the header with schema block information.
    ///
    /// This sets the schema offset, size, hash, and first record offset
    /// based on the provided schema block.
    ///
    /// # Arguments
    ///
    /// * `schema_block` - The schema block that will follow this header
    /// * `schema_hash` - CRC32 hash of the uncompressed schema XML
    pub fn with_schema(&mut self, schema_block: &super::SchemaBlock, schema_hash: u32) {
        self.schema_offset = FILE_HEADER_SIZE as u64;
        self.schema_size = schema_block.total_size() as u64;
        self.first_record = FILE_HEADER_SIZE as u64 + self.schema_size;
        self.schema_hash = schema_hash;
    }

    /// Get the schema version as a tuple.
    pub fn schema_version(&self) -> (u8, u8) {
        (self.schema_major, self.schema_minor)
    }

    /// Get the format version as a tuple.
    pub fn format_version(&self) -> (u8, u8) {
        (self.format_major, self.format_minor)
    }
}

impl Default for FileHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// Schema block header containing embedded SBE XML metadata.
///
/// Layout (32 bytes) per the format spec, section 8.1.1:
/// ```text
/// Offset  Size  Field              Description
/// ────────────────────────────────────────────────────────────
/// 0x00    8     magic              "BLKBOXSC" (ASCII)
/// 0x08    4     xml_uncompressed   Uncompressed XML size
/// 0x0C    4     xml_compressed     Compressed size (or same if uncompressed)
/// 0x10    1     compression        0 = none, 1 = zstd
/// 0x11    15    reserved           Reserved (must be 0)
/// ────────────────────────────────────────────────────────────
/// Header: 32 bytes, Payload: variable (follows header)
/// ```
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SchemaBlockHeader {
    /// Magic number "BLKBOXSC".
    pub magic: [u8; 8],
    /// Size of uncompressed XML data.
    pub xml_uncompressed: u32,
    /// Size of compressed data (same as uncompressed if no compression).
    pub xml_compressed: u32,
    /// Compression type (0=none, 1=zstd).
    pub compression: u8,
    /// Reserved for future use.
    pub reserved: [u8; 15],
}

/// Compression type constants.
pub mod compression {
    /// No compression.
    pub const NONE: u8 = 0;
    /// Zstandard compression.
    pub const ZSTD: u8 = 1;
}

impl SchemaBlockHeader {
    /// Create a new schema block header.
    pub fn new(uncompressed_size: u32, compressed_size: u32, compression: u8) -> Self {
        Self {
            magic: SCHEMA_MAGIC,
            xml_uncompressed: uncompressed_size,
            xml_compressed: compressed_size,
            compression,
            reserved: [0u8; 15],
        }
    }

    /// Validate the schema block header.
    pub fn validate(&self) -> Result<(), HeaderError> {
        if self.magic != SCHEMA_MAGIC {
            return Err(HeaderError::InvalidSchemaMagic);
        }
        Ok(())
    }

    /// Convert to bytes for writing.
    pub fn to_bytes(&self) -> [u8; SCHEMA_BLOCK_HEADER_SIZE] {
        // SAFETY: SchemaBlockHeader is repr(C, packed) with known size
        unsafe { std::mem::transmute_copy(self) }
    }

    /// Parse from bytes.
    pub fn from_bytes(bytes: &[u8; SCHEMA_BLOCK_HEADER_SIZE]) -> Self {
        // SAFETY: SchemaBlockHeader is repr(C, packed) with known size
        unsafe { std::ptr::read(bytes.as_ptr() as *const Self) }
    }
}

/// Schema block (header + data).
pub struct SchemaBlock {
    /// Header with metadata.
    pub header: SchemaBlockHeader,
    /// Schema data (optionally compressed).
    pub data: Vec<u8>,
}

impl SchemaBlock {
    /// Total size of this schema block in bytes (header + data).
    pub fn total_size(&self) -> usize {
        SCHEMA_BLOCK_HEADER_SIZE + self.data.len()
    }
}

/// File footer written when journal is closed cleanly.
///
/// Layout (32 bytes) per the format spec, section 8.2:
/// ```text
/// Offset  Size  Field            Description
/// ──────────────────────────────────────────────────────
/// 0x00    8     magic            "BLKBOXND" (ASCII)
/// 0x08    8     total_records    Final record count
/// 0x10    8     total_bytes      Total bytes written
/// 0x18    8     final_timestamp  Last record timestamp
/// ──────────────────────────────────────────────────────
/// Total: 32 bytes
/// ```
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FileFooter {
    /// Magic number "BLKBOXND".
    pub magic: [u8; 8],
    /// Final record count.
    pub total_records: u64,
    /// Total bytes written (including header, schema, footer).
    pub total_bytes: u64,
    /// Last record timestamp (microseconds since epoch).
    pub final_timestamp: i64,
}

impl FileFooter {
    /// Create a new file footer.
    pub fn new(total_records: u64, total_bytes: u64, final_timestamp: i64) -> Self {
        Self {
            magic: FOOTER_MAGIC,
            total_records,
            total_bytes,
            final_timestamp,
        }
    }

    /// Validate the footer magic.
    pub fn validate(&self) -> Result<(), HeaderError> {
        if self.magic != FOOTER_MAGIC {
            return Err(HeaderError::InvalidFooterMagic);
        }
        Ok(())
    }

    /// Convert to bytes for writing.
    pub fn to_bytes(&self) -> [u8; FILE_FOOTER_SIZE] {
        // SAFETY: FileFooter is repr(C, packed) with known size
        unsafe { std::mem::transmute_copy(self) }
    }

    /// Parse from bytes.
    pub fn from_bytes(bytes: &[u8; FILE_FOOTER_SIZE]) -> Self {
        // SAFETY: FileFooter is repr(C, packed) with known size
        unsafe { std::ptr::read(bytes.as_ptr() as *const Self) }
    }
}

/// Record header preceding each record.
///
/// Layout (24 bytes):
/// ```text
/// Offset  Size  Field
/// 0x00    8     timestamp (microseconds)
/// 0x08    4     payload_size
/// 0x0C    2     record_type
/// 0x0E    1     exchange_id
/// 0x0F    1     flags
/// 0x10    4     sequence_number
/// 0x14    4     crc32 (of payload)
/// ```
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RecordHeader {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Size of payload in bytes.
    pub payload_size: u32,
    /// Record type identifier.
    pub record_type: u16,
    /// Exchange identifier.
    pub exchange_id: u8,
    /// Flags (bit field).
    pub flags: u8,
    /// Monotonic sequence number within file.
    pub sequence_number: u32,
    /// CRC32 of payload bytes.
    pub crc32: u32,
}

impl RecordHeader {
    /// Create a new record header.
    pub fn new(
        timestamp: Timestamp,
        record_type: RecordType,
        exchange_id: u8,
        payload_size: u32,
        sequence_number: u32,
    ) -> Self {
        Self {
            timestamp: timestamp.as_micros(),
            payload_size,
            record_type: record_type.as_u16(),
            exchange_id,
            flags: 0,
            sequence_number,
            crc32: 0,
        }
    }

    /// Convert to bytes for writing.
    pub fn to_bytes(&self) -> [u8; RECORD_HEADER_SIZE] {
        // SAFETY: RecordHeader is repr(C, packed) with known size
        unsafe { std::mem::transmute_copy(self) }
    }

    /// Parse from bytes.
    pub fn from_bytes(bytes: &[u8; RECORD_HEADER_SIZE]) -> Self {
        // SAFETY: RecordHeader is repr(C, packed) with known size
        unsafe { std::ptr::read(bytes.as_ptr() as *const Self) }
    }
}

/// Record type identifiers.
///
/// Values are assigned in ranges:
/// - 0x0000-0x00FF: System records
/// - 0x0100-0x01FF: Market data
/// - 0x0200-0x02FF: Orders
/// - 0x0300-0x03FF: Internal state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum RecordType {
    /// Unknown record type.
    Unknown = 0x0000,
    /// Session start marker.
    SessionStart = 0x0001,
    /// Session end marker.
    SessionEnd = 0x0002,
    /// Checkpoint with state hash.
    Checkpoint = 0x0003,

    /// Raw WebSocket frame (ingress).
    RawFrame = 0x0100,
    /// Parsed quote update.
    QuoteUpdate = 0x0101,
    /// Parsed trade.
    Trade = 0x0102,
    /// Order book snapshot.
    BookSnapshot = 0x0103,

    /// Order submission (egress).
    OrderSubmit = 0x0200,
    /// Order acknowledgment.
    OrderAck = 0x0201,
    /// Order fill.
    OrderFill = 0x0202,
    /// Order cancel.
    OrderCancel = 0x0203,

    /// Internal state change.
    StateChange = 0x0300,
    /// Signal generated.
    Signal = 0x0301,
}

impl RecordType {
    /// Convert to u16 for serialization.
    #[inline]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Parse from u16.
    pub const fn from_u16(value: u16) -> Self {
        match value {
            0x0000 => Self::Unknown,
            0x0001 => Self::SessionStart,
            0x0002 => Self::SessionEnd,
            0x0003 => Self::Checkpoint,
            0x0100 => Self::RawFrame,
            0x0101 => Self::QuoteUpdate,
            0x0102 => Self::Trade,
            0x0103 => Self::BookSnapshot,
            0x0200 => Self::OrderSubmit,
            0x0201 => Self::OrderAck,
            0x0202 => Self::OrderFill,
            0x0203 => Self::OrderCancel,
            0x0300 => Self::StateChange,
            0x0301 => Self::Signal,
            _ => Self::Unknown,
        }
    }
}

/// Header validation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// File header magic number mismatch.
    InvalidMagic,
    /// Schema block magic number mismatch.
    InvalidSchemaMagic,
    /// File footer magic number mismatch.
    InvalidFooterMagic,
    /// Version not supported.
    UnsupportedVersion { major: u8, minor: u8 },
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "Invalid file header magic number"),
            Self::InvalidSchemaMagic => write!(f, "Invalid schema block magic number"),
            Self::InvalidFooterMagic => write!(f, "Invalid file footer magic number"),
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "Unsupported version: {}.{}", major, minor)
            }
        }
    }
}

impl std::error::Error for HeaderError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Size Tests ====================

    #[test]
    fn test_file_header_size() {
        assert_eq!(
            std::mem::size_of::<FileHeader>(),
            FILE_HEADER_SIZE,
            "FileHeader must be exactly {} bytes per the format spec, section 8.1",
            FILE_HEADER_SIZE
        );
    }

    #[test]
    fn test_schema_block_header_size() {
        assert_eq!(
            std::mem::size_of::<SchemaBlockHeader>(),
            SCHEMA_BLOCK_HEADER_SIZE,
            "SchemaBlockHeader must be exactly {} bytes per the format spec, section 8.1.1",
            SCHEMA_BLOCK_HEADER_SIZE
        );
    }

    #[test]
    fn test_record_header_size() {
        assert_eq!(
            std::mem::size_of::<RecordHeader>(),
            RECORD_HEADER_SIZE,
            "RecordHeader must be exactly {} bytes per the format spec, section 6.1",
            RECORD_HEADER_SIZE
        );
    }

    #[test]
    fn test_file_footer_size() {
        assert_eq!(
            std::mem::size_of::<FileFooter>(),
            FILE_FOOTER_SIZE,
            "FileFooter must be exactly {} bytes per the format spec, section 8.2",
            FILE_FOOTER_SIZE
        );
    }

    // ==================== FileHeader Tests ====================

    #[test]
    fn test_file_header_roundtrip() {
        let header = FileHeader::new();
        let bytes = header.to_bytes();
        let recovered = FileHeader::from_bytes(&bytes);

        // Copy fields to avoid packed struct reference issues
        let magic = recovered.magic;
        let format_major = recovered.format_major;
        let format_minor = recovered.format_minor;
        let header_size = recovered.header_size;

        assert_eq!(magic, FILE_MAGIC);
        assert_eq!(format_major, FORMAT_VERSION_MAJOR);
        assert_eq!(format_minor, FORMAT_VERSION_MINOR);
        assert_eq!(header_size, FILE_HEADER_SIZE as u32);
    }

    // ==================== Clock Injection Tests (T3.5) ====================

    #[test]
    fn test_file_header_new_with_timestamp() {
        let ts = Timestamp::from_micros(1_704_067_200_000_000);
        let header = FileHeader::new_with_timestamp(ts);

        let session_start = header.session_start;
        assert_eq!(session_start, 1_704_067_200_000_000);
    }

    #[test]
    fn test_file_header_new_with_timestamp_epoch() {
        let header = FileHeader::new_with_timestamp(Timestamp::EPOCH);

        let session_start = header.session_start;
        assert_eq!(session_start, 0);
    }

    #[test]
    fn test_file_header_new_with_timestamp_min() {
        let header = FileHeader::new_with_timestamp(Timestamp::MIN);

        let session_start = header.session_start;
        assert_eq!(session_start, Timestamp::MIN.as_micros());
    }

    #[test]
    fn test_file_header_new_with_timestamp_max() {
        let header = FileHeader::new_with_timestamp(Timestamp::MAX);

        let session_start = header.session_start;
        assert_eq!(session_start, Timestamp::MAX.as_micros());
    }

    #[test]
    fn test_file_header_new_with_timestamp_deterministic() {
        let ts = Timestamp::from_micros(1234567890);

        let header1 = FileHeader::new_with_timestamp(ts);
        let header2 = FileHeader::new_with_timestamp(ts);

        // Copy fields to avoid packed struct reference issues
        let session_start1 = header1.session_start;
        let session_start2 = header2.session_start;

        // Both headers should have identical session_start
        assert_eq!(session_start1, session_start2);
        assert_eq!(session_start1, 1234567890);
    }

    #[test]
    fn test_file_header_new_with_timestamp_roundtrip() {
        let ts = Timestamp::from_micros(9876543210);
        let header = FileHeader::new_with_timestamp(ts);
        let bytes = header.to_bytes();
        let recovered = FileHeader::from_bytes(&bytes);

        let session_start = recovered.session_start;
        assert_eq!(session_start, 9876543210);
    }

    #[test]
    fn test_file_header_new_with_timestamp_validates() {
        let ts = Timestamp::from_micros(1000);
        let header = FileHeader::new_with_timestamp(ts);

        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_file_header_new_with_timestamp_other_fields_default() {
        let ts = Timestamp::from_micros(5000);
        let header = FileHeader::new_with_timestamp(ts);

        // Verify other fields are set correctly
        let magic = header.magic;
        let format_major = header.format_major;
        let format_minor = header.format_minor;
        let record_count = header.record_count;
        let session_end = header.session_end;

        assert_eq!(magic, FILE_MAGIC);
        assert_eq!(format_major, FORMAT_VERSION_MAJOR);
        assert_eq!(format_minor, FORMAT_VERSION_MINOR);
        assert_eq!(record_count, 0);
        assert_eq!(session_end, 0);
    }

    #[test]
    fn test_file_header_validation() {
        let header = FileHeader::new();
        assert!(header.validate().is_ok());

        let mut bad_magic = header;
        bad_magic.magic = *b"BADMAGIC";
        assert!(matches!(
            bad_magic.validate(),
            Err(HeaderError::InvalidMagic)
        ));
    }

    // ==================== SchemaBlockHeader Tests ====================

    #[test]
    fn test_schema_block_header_roundtrip() {
        let header = SchemaBlockHeader::new(4096, 1024, compression::ZSTD);
        let bytes = header.to_bytes();
        let recovered = SchemaBlockHeader::from_bytes(&bytes);

        // Copy fields to avoid packed struct reference issues
        let magic = recovered.magic;
        let xml_uncompressed = recovered.xml_uncompressed;
        let xml_compressed = recovered.xml_compressed;
        let comp = recovered.compression;

        assert_eq!(magic, SCHEMA_MAGIC);
        assert_eq!(xml_uncompressed, 4096);
        assert_eq!(xml_compressed, 1024);
        assert_eq!(comp, compression::ZSTD);
    }

    #[test]
    fn test_schema_block_header_validation() {
        let header = SchemaBlockHeader::new(1000, 500, compression::NONE);
        assert!(header.validate().is_ok());

        let mut bad_magic = header;
        bad_magic.magic = *b"BADSCHMA";
        assert!(matches!(
            bad_magic.validate(),
            Err(HeaderError::InvalidSchemaMagic)
        ));
    }

    // ==================== FileFooter Tests ====================

    #[test]
    fn test_file_footer_roundtrip() {
        let footer = FileFooter::new(1000, 1024 * 1024, 1_704_067_200_000_000);
        let bytes = footer.to_bytes();
        let recovered = FileFooter::from_bytes(&bytes);

        // Copy fields to avoid packed struct reference issues
        let magic = recovered.magic;
        let total_records = recovered.total_records;
        let total_bytes = recovered.total_bytes;
        let final_timestamp = recovered.final_timestamp;

        assert_eq!(magic, FOOTER_MAGIC);
        assert_eq!(total_records, 1000);
        assert_eq!(total_bytes, 1024 * 1024);
        assert_eq!(final_timestamp, 1_704_067_200_000_000);
    }

    #[test]
    fn test_file_footer_validation() {
        let footer = FileFooter::new(100, 1000, 0);
        assert!(footer.validate().is_ok());

        let mut bad_magic = footer;
        bad_magic.magic = *b"BADEND__";
        assert!(matches!(
            bad_magic.validate(),
            Err(HeaderError::InvalidFooterMagic)
        ));
    }

    // ==================== RecordHeader Tests ====================

    #[test]
    fn test_record_header_roundtrip() {
        let header = RecordHeader::new(
            Timestamp::from_micros(1_704_067_200_000_000),
            RecordType::RawFrame,
            1,
            256,
            42,
        );
        let bytes = header.to_bytes();
        let recovered = RecordHeader::from_bytes(&bytes);

        // Copy fields to avoid packed struct reference issues
        let timestamp = recovered.timestamp;
        let record_type = recovered.record_type;
        let payload_size = recovered.payload_size;
        let sequence_number = recovered.sequence_number;

        assert_eq!(timestamp, 1_704_067_200_000_000);
        assert_eq!(record_type, RecordType::RawFrame.as_u16());
        assert_eq!(payload_size, 256);
        assert_eq!(sequence_number, 42);
    }

    #[test]
    fn test_record_type_roundtrip() {
        for rt in [
            RecordType::SessionStart,
            RecordType::RawFrame,
            RecordType::OrderSubmit,
            RecordType::StateChange,
        ] {
            assert_eq!(RecordType::from_u16(rt.as_u16()), rt);
        }
    }

    // ==================== Binary Layout Tests ====================

    #[test]
    fn test_file_header_field_offsets() {
        // Verify critical field offsets match the format spec, section 8.1
        let header = FileHeader::new();
        let bytes = header.to_bytes();

        // magic at 0x00
        assert_eq!(&bytes[0x00..0x08], b"BLKBOXJL");

        // format_major at 0x08
        assert_eq!(bytes[0x08], FORMAT_VERSION_MAJOR);

        // format_minor at 0x09
        assert_eq!(bytes[0x09], FORMAT_VERSION_MINOR);

        // schema_major at 0x0A
        assert_eq!(bytes[0x0A], SCHEMA_VERSION_MAJOR);

        // schema_minor at 0x0B
        assert_eq!(bytes[0x0B], SCHEMA_VERSION_MINOR);

        // header_size at 0x0C (u32 LE)
        let header_size = u32::from_le_bytes([bytes[0x0C], bytes[0x0D], bytes[0x0E], bytes[0x0F]]);
        assert_eq!(header_size, FILE_HEADER_SIZE as u32);
    }

    #[test]
    fn test_schema_block_header_field_offsets() {
        // Verify critical field offsets match the format spec, section 8.1.1
        let header = SchemaBlockHeader::new(4096, 1024, compression::ZSTD);
        let bytes = header.to_bytes();

        // magic at 0x00
        assert_eq!(&bytes[0x00..0x08], b"BLKBOXSC");

        // xml_uncompressed at 0x08 (u32 LE)
        let uncompressed = u32::from_le_bytes([bytes[0x08], bytes[0x09], bytes[0x0A], bytes[0x0B]]);
        assert_eq!(uncompressed, 4096);

        // xml_compressed at 0x0C (u32 LE)
        let compressed = u32::from_le_bytes([bytes[0x0C], bytes[0x0D], bytes[0x0E], bytes[0x0F]]);
        assert_eq!(compressed, 1024);

        // compression at 0x10
        assert_eq!(bytes[0x10], compression::ZSTD);
    }

    #[test]
    fn test_record_header_field_offsets() {
        // Verify critical field offsets match the format spec, section 6.1
        let header = RecordHeader::new(
            Timestamp::from_micros(0x123456789ABCDEF0_u64 as i64),
            RecordType::RawFrame,
            0xAB,
            0xDEADBEEF,
            0x12345678,
        );
        let bytes = header.to_bytes();

        // timestamp at 0x00 (i64 LE)
        let ts = i64::from_le_bytes(bytes[0x00..0x08].try_into().unwrap());
        assert_eq!(ts, 0x123456789ABCDEF0_u64 as i64);

        // payload_size at 0x08 (u32 LE)
        let payload = u32::from_le_bytes(bytes[0x08..0x0C].try_into().unwrap());
        assert_eq!(payload, 0xDEADBEEF);

        // record_type at 0x0C (u16 LE)
        let rtype = u16::from_le_bytes(bytes[0x0C..0x0E].try_into().unwrap());
        assert_eq!(rtype, RecordType::RawFrame.as_u16());

        // exchange_id at 0x0E
        assert_eq!(bytes[0x0E], 0xAB);

        // flags at 0x0F
        assert_eq!(bytes[0x0F], 0);

        // sequence_number at 0x10 (u32 LE)
        let seq = u32::from_le_bytes(bytes[0x10..0x14].try_into().unwrap());
        assert_eq!(seq, 0x12345678);
    }
}
