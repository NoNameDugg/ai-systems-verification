//! Journal reader for sequential replay.
//!
//! The reader provides an iterator interface over journal records
//! with schema-aware decoding and CRC validation.
//!
//! # Architecture
//!
//! ```text
//! JournalReader
//!      |
//!      v
//! MMAP File -----> FileHeader (validated)
//!      |
//!      v
//! SchemaBlock (decompressed)
//!      |
//!      v
//! Record Iterator -----> Record 1 -----> Record 2 -----> ... -----> Footer
//! ```
//!
//! # Performance Targets
//!
//! - Open: < 1ms (including header validation)
//! - Read latency (per record): < 100ns p99
//! - Zero allocations for record headers (payload is cloned)
//!
//! # Example
//!
//! ```ignore
//! use blackbox::journal::JournalReader;
//!
//! let reader = JournalReader::open("session.journal")?;
//!
//! println!("Schema version: {:?}", reader.schema_version());
//! println!("Total records: {}", reader.record_count());
//!
//! for result in reader {
//!     let record = result?;
//!     println!("Record type: {:?}, timestamp: {}", record.record_type(), record.timestamp());
//! }
//! ```

use super::format::{
    compression, FileFooter, FileHeader, HeaderError, RecordHeader, RecordType, SchemaBlock,
    SchemaBlockHeader, FILE_FOOTER_SIZE, FILE_HEADER_SIZE, RECORD_HEADER_SIZE,
    SCHEMA_BLOCK_HEADER_SIZE,
};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// Errors that can occur during journal reading.
#[derive(Debug)]
pub enum ReaderError {
    /// I/O error.
    Io(std::io::Error),
    /// Header validation failed.
    InvalidHeader(HeaderError),
    /// Record is corrupted (CRC mismatch).
    CorruptRecord {
        /// File offset where corrupt record was found.
        offset: u64,
        /// Expected CRC from record header.
        expected_crc: u32,
        /// Actual CRC computed from payload.
        actual_crc: u32,
    },
    /// Unexpected end of file.
    UnexpectedEof {
        /// Number of bytes expected.
        expected: usize,
        /// Number of bytes available.
        available: usize,
    },
    /// File too small.
    FileTooSmall {
        /// Minimum expected file size.
        expected: usize,
        /// Actual file size.
        actual: usize,
    },
    /// Schema decompression failed.
    SchemaDecompress(String),
    /// Invalid schema block.
    InvalidSchemaBlock(HeaderError),
    /// Schema hash mismatch - embedded schema does not match expected hash.
    ///
    /// This error indicates potential file corruption or tampering.
    /// The schema was successfully decompressed but its CRC32 hash
    /// does not match the hash stored in the file header.
    SchemaHashMismatch {
        /// Expected CRC32 hash from file header.
        expected: u32,
        /// Actual CRC32 hash computed from decompressed schema.
        actual: u32,
    },
}

impl std::fmt::Display for ReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::InvalidHeader(e) => write!(f, "Invalid header: {}", e),
            Self::CorruptRecord {
                offset,
                expected_crc,
                actual_crc,
            } => {
                write!(
                    f,
                    "Corrupt record at offset {}: CRC mismatch (expected {:08x}, got {:08x})",
                    offset, expected_crc, actual_crc
                )
            }
            Self::UnexpectedEof {
                expected,
                available,
            } => {
                write!(
                    f,
                    "Unexpected end of file: expected {} bytes, only {} available",
                    expected, available
                )
            }
            Self::FileTooSmall { expected, actual } => {
                write!(
                    f,
                    "File too small: expected at least {} bytes, got {}",
                    expected, actual
                )
            }
            Self::SchemaDecompress(msg) => write!(f, "Schema decompression failed: {}", msg),
            Self::InvalidSchemaBlock(e) => write!(f, "Invalid schema block: {}", e),
            Self::SchemaHashMismatch { expected, actual } => {
                write!(
                    f,
                    "Schema hash mismatch: expected 0x{:08X}, computed 0x{:08X}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for ReaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::InvalidHeader(e) => Some(e),
            Self::InvalidSchemaBlock(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ReaderError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<HeaderError> for ReaderError {
    fn from(e: HeaderError) -> Self {
        Self::InvalidHeader(e)
    }
}

/// A record read from the journal.
#[derive(Debug, Clone)]
pub struct Record {
    /// Record header.
    pub header: RecordHeader,
    /// Payload bytes.
    pub payload: Vec<u8>,
    /// Offset in file where this record was found.
    pub offset: u64,
}

impl Record {
    /// Get the record type.
    pub fn record_type(&self) -> RecordType {
        RecordType::from_u16(self.header.record_type)
    }

    /// Get the timestamp in microseconds.
    pub fn timestamp(&self) -> i64 {
        self.header.timestamp
    }

    /// Get the exchange ID.
    pub fn exchange_id(&self) -> u8 {
        self.header.exchange_id
    }

    /// Get the sequence number.
    pub fn sequence_number(&self) -> u32 {
        self.header.sequence_number
    }

    /// Get the CRC32 checksum.
    pub fn crc32(&self) -> u32 {
        self.header.crc32
    }

    /// Get the flags.
    pub fn flags(&self) -> u8 {
        self.header.flags
    }

    /// Get the payload size.
    pub fn payload_size(&self) -> u32 {
        self.header.payload_size
    }
}

/// Configuration for the journal reader.
#[derive(Debug, Clone)]
pub struct ReaderConfig {
    /// Whether to verify CRC checksums on record payloads.
    pub verify_crc: bool,
    /// Whether to skip corrupt records (instead of returning error).
    pub skip_corrupt: bool,
    /// Whether to verify the schema hash on file open.
    ///
    /// When enabled, the reader computes the CRC32 hash of the decompressed
    /// schema XML and compares it to the hash stored in the file header.
    /// A mismatch indicates potential file corruption or tampering.
    ///
    /// Default: `true`
    pub verify_schema_hash: bool,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            verify_crc: true,
            skip_corrupt: false,
            verify_schema_hash: true,
        }
    }
}

impl ReaderConfig {
    /// Create a lenient config that skips corrupt records.
    ///
    /// This still verifies CRC and schema hash, but skips corrupt records
    /// instead of returning an error.
    pub fn lenient() -> Self {
        Self {
            verify_crc: true,
            skip_corrupt: true,
            verify_schema_hash: true,
        }
    }

    /// Create a fast config that skips all verification.
    ///
    /// This skips both record CRC verification and schema hash verification
    /// for maximum read performance. Use with trusted files only.
    pub fn fast() -> Self {
        Self {
            verify_crc: false,
            skip_corrupt: false,
            verify_schema_hash: false,
        }
    }

    /// Create a config that skips schema hash verification only.
    ///
    /// Useful when reading files that may have been modified but you want
    /// to verify individual record integrity.
    pub fn skip_schema_hash() -> Self {
        Self {
            verify_crc: true,
            skip_corrupt: false,
            verify_schema_hash: false,
        }
    }
}

/// Journal reader for sequential access.
///
/// Provides an iterator interface over records in a journal file.
/// The reader uses memory-mapped I/O for efficient access.
///
/// # Thread Safety
///
/// The reader is NOT thread-safe. Use separate readers for concurrent access.
///
/// # Example
///
/// ```ignore
/// use blackbox::journal::JournalReader;
///
/// let reader = JournalReader::open("session.journal")?;
/// for record in reader {
///     let record = record?;
///     println!("Record type: {:?}", record.record_type());
/// }
/// ```
pub struct JournalReader {
    /// Memory-mapped file.
    mmap: Mmap,
    /// File header.
    header: FileHeader,
    /// Embedded schema XML (decompressed).
    schema_xml: String,
    /// Current read position.
    read_position: usize,
    /// Number of records read.
    records_read: u64,
    /// Whether we've reached the end.
    finished: bool,
    /// Configuration.
    config: ReaderConfig,
}

impl JournalReader {
    /// Open a journal file for reading.
    ///
    /// This validates the file header and extracts the embedded schema.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the journal file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File cannot be opened
    /// - File is too small to contain header
    /// - Header validation fails
    /// - Schema extraction fails
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ReaderError> {
        Self::open_with_config(path, ReaderConfig::default())
    }

    /// Open a journal file with custom configuration.
    pub fn open_with_config<P: AsRef<Path>>(
        path: P,
        config: ReaderConfig,
    ) -> Result<Self, ReaderError> {
        let file = File::open(path)?;

        // Memory-map the file
        let mmap = unsafe { Mmap::map(&file)? };

        // Check minimum file size
        if mmap.len() < FILE_HEADER_SIZE {
            return Err(ReaderError::FileTooSmall {
                expected: FILE_HEADER_SIZE,
                actual: mmap.len(),
            });
        }

        // Read and validate header
        let header_bytes: [u8; FILE_HEADER_SIZE] = mmap[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);
        header.validate()?;

        // Extract schema
        let schema_offset = header.schema_offset as usize;
        let schema_size = header.schema_size as usize;

        if mmap.len() < schema_offset + schema_size {
            return Err(ReaderError::FileTooSmall {
                expected: schema_offset + schema_size,
                actual: mmap.len(),
            });
        }

        let schema_xml = Self::extract_schema(&mmap, schema_offset, schema_size)?;

        // Verify schema hash if configured
        if config.verify_schema_hash {
            let expected_hash = header.schema_hash;
            let actual_hash = crc32fast::hash(schema_xml.as_bytes());
            if actual_hash != expected_hash {
                return Err(ReaderError::SchemaHashMismatch {
                    expected: expected_hash,
                    actual: actual_hash,
                });
            }
        }

        // Get first record position
        let first_record = header.first_record as usize;

        Ok(Self {
            mmap,
            header,
            schema_xml,
            read_position: first_record,
            records_read: 0,
            finished: false,
            config,
        })
    }

    /// Extract and decompress the embedded schema.
    fn extract_schema(mmap: &Mmap, offset: usize, size: usize) -> Result<String, ReaderError> {
        if size < SCHEMA_BLOCK_HEADER_SIZE {
            return Err(ReaderError::FileTooSmall {
                expected: SCHEMA_BLOCK_HEADER_SIZE,
                actual: size,
            });
        }

        // Read schema block header
        let header_bytes: [u8; SCHEMA_BLOCK_HEADER_SIZE] = mmap
            [offset..offset + SCHEMA_BLOCK_HEADER_SIZE]
            .try_into()
            .unwrap();
        let schema_header = SchemaBlockHeader::from_bytes(&header_bytes);
        schema_header
            .validate()
            .map_err(ReaderError::InvalidSchemaBlock)?;

        // Get schema data
        let data_offset = offset + SCHEMA_BLOCK_HEADER_SIZE;
        let compressed_size = schema_header.xml_compressed as usize;
        let uncompressed_size = schema_header.xml_uncompressed as usize;

        if mmap.len() < data_offset + compressed_size {
            return Err(ReaderError::FileTooSmall {
                expected: data_offset + compressed_size,
                actual: mmap.len(),
            });
        }

        let data = &mmap[data_offset..data_offset + compressed_size];

        // Decompress if needed
        let xml_bytes = match schema_header.compression {
            compression::NONE => data.to_vec(),
            compression::ZSTD => {
                let mut decoded = Vec::with_capacity(uncompressed_size);
                let mut decoder = zstd::Decoder::new(data).map_err(|e| {
                    ReaderError::SchemaDecompress(format!("Failed to create decoder: {}", e))
                })?;
                std::io::Read::read_to_end(&mut decoder, &mut decoded).map_err(|e| {
                    ReaderError::SchemaDecompress(format!("Failed to decompress: {}", e))
                })?;
                decoded
            }
            other => {
                return Err(ReaderError::SchemaDecompress(format!(
                    "Unknown compression type: {}",
                    other
                )));
            }
        };

        String::from_utf8(xml_bytes)
            .map_err(|e| ReaderError::SchemaDecompress(format!("Invalid UTF-8: {}", e)))
    }

    /// Get the file header.
    pub fn header(&self) -> &FileHeader {
        &self.header
    }

    /// Get the schema version.
    pub fn schema_version(&self) -> (u8, u8) {
        (self.header.schema_major, self.header.schema_minor)
    }

    /// Get the format version.
    pub fn format_version(&self) -> (u8, u8) {
        (self.header.format_major, self.header.format_minor)
    }

    /// Get the embedded schema XML.
    pub fn schema_xml(&self) -> &str {
        &self.schema_xml
    }

    /// Get the total record count from header.
    ///
    /// Note: This is the count stored in the header, which may be
    /// out of date if the file wasn't closed cleanly.
    pub fn record_count(&self) -> u64 {
        self.header.record_count
    }

    /// Get the number of records read so far.
    pub fn records_read(&self) -> u64 {
        self.records_read
    }

    /// Get the session start timestamp.
    pub fn session_start(&self) -> i64 {
        self.header.session_start
    }

    /// Get the session end timestamp.
    pub fn session_end(&self) -> i64 {
        self.header.session_end
    }

    /// Get the current read position.
    pub fn position(&self) -> usize {
        self.read_position
    }

    /// Check if there are more records to read.
    pub fn has_more(&self) -> bool {
        !self.finished && self.read_position + RECORD_HEADER_SIZE <= self.mmap.len()
    }

    /// Reset to the beginning of records.
    pub fn rewind(&mut self) {
        self.read_position = self.header.first_record as usize;
        self.records_read = 0;
        self.finished = false;
    }

    /// Seek to a specific offset.
    ///
    /// The offset must be at a valid record boundary.
    pub fn seek(&mut self, offset: usize) -> Result<(), ReaderError> {
        let first_record = self.header.first_record as usize;
        if offset < first_record {
            return Err(ReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Cannot seek before first record (offset {} < first_record {})",
                    offset, first_record
                ),
            )));
        }
        if offset > self.mmap.len() {
            return Err(ReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Cannot seek past end of file (offset {} > len {})",
                    offset,
                    self.mmap.len()
                ),
            )));
        }
        self.read_position = offset;
        self.finished = false;
        Ok(())
    }

    /// Read the next record.
    fn read_next(&mut self) -> Option<Result<Record, ReaderError>> {
        if self.finished {
            return None;
        }

        // Check for end of file
        if self.read_position + RECORD_HEADER_SIZE > self.mmap.len() {
            self.finished = true;
            return None;
        }

        // Check for footer magic (indicates clean EOF)
        if self.read_position + 8 <= self.mmap.len() {
            let magic = &self.mmap[self.read_position..self.read_position + 8];
            if magic == super::format::FOOTER_MAGIC {
                self.finished = true;
                return None;
            }
        }

        // Read record header
        let header_end = self.read_position + RECORD_HEADER_SIZE;
        if header_end > self.mmap.len() {
            self.finished = true;
            return Some(Err(ReaderError::UnexpectedEof {
                expected: RECORD_HEADER_SIZE,
                available: self.mmap.len() - self.read_position,
            }));
        }

        let header_bytes: [u8; RECORD_HEADER_SIZE] = self.mmap[self.read_position..header_end]
            .try_into()
            .unwrap();
        let header = RecordHeader::from_bytes(&header_bytes);

        // Copy fields to avoid packed struct issues
        let payload_size = header.payload_size as usize;
        let expected_crc = header.crc32;

        // Read payload
        let payload_start = header_end;
        let payload_end = payload_start + payload_size;

        if payload_end > self.mmap.len() {
            self.finished = true;
            return Some(Err(ReaderError::UnexpectedEof {
                expected: payload_size,
                available: self.mmap.len() - payload_start,
            }));
        }

        let payload = self.mmap[payload_start..payload_end].to_vec();

        // Verify CRC if configured
        if self.config.verify_crc {
            let actual_crc = crc32fast::hash(&payload);
            if actual_crc != expected_crc {
                let offset = self.read_position as u64;
                if self.config.skip_corrupt {
                    // Skip to next potential record
                    self.read_position = payload_end;
                    self.records_read += 1;
                    // Try to read next record
                    return self.read_next();
                }
                self.finished = true;
                return Some(Err(ReaderError::CorruptRecord {
                    offset,
                    expected_crc,
                    actual_crc,
                }));
            }
        }

        let record = Record {
            header,
            payload,
            offset: self.read_position as u64,
        };

        // Advance position
        self.read_position = payload_end;
        self.records_read += 1;

        Some(Ok(record))
    }

    /// Read the file footer if present.
    ///
    /// Returns None if the footer is not present (file not cleanly closed).
    pub fn read_footer(&self) -> Option<FileFooter> {
        // Look for footer at the position after all records
        // This is a best-effort search - we try the recorded position
        // and also scan backwards from the end

        // Try current read position (if we've read all records)
        if self.read_position + FILE_FOOTER_SIZE <= self.mmap.len() {
            let magic = &self.mmap[self.read_position..self.read_position + 8];
            if magic == super::format::FOOTER_MAGIC {
                let footer_bytes: [u8; FILE_FOOTER_SIZE] = self.mmap
                    [self.read_position..self.read_position + FILE_FOOTER_SIZE]
                    .try_into()
                    .unwrap();
                return Some(FileFooter::from_bytes(&footer_bytes));
            }
        }

        // Try scanning from end of file
        if self.mmap.len() >= FILE_FOOTER_SIZE {
            let footer_start = self.mmap.len() - FILE_FOOTER_SIZE;
            let magic = &self.mmap[footer_start..footer_start + 8];
            if magic == super::format::FOOTER_MAGIC {
                let footer_bytes: [u8; FILE_FOOTER_SIZE] = self.mmap
                    [footer_start..footer_start + FILE_FOOTER_SIZE]
                    .try_into()
                    .unwrap();
                return Some(FileFooter::from_bytes(&footer_bytes));
            }
        }

        None
    }

    /// Get the file size.
    pub fn file_size(&self) -> usize {
        self.mmap.len()
    }

    /// Get the schema block for inspection.
    pub fn read_schema_block(&self) -> Result<SchemaBlock, ReaderError> {
        let offset = self.header.schema_offset as usize;
        let size = self.header.schema_size as usize;

        if size < SCHEMA_BLOCK_HEADER_SIZE {
            return Err(ReaderError::FileTooSmall {
                expected: SCHEMA_BLOCK_HEADER_SIZE,
                actual: size,
            });
        }

        let header_bytes: [u8; SCHEMA_BLOCK_HEADER_SIZE] = self.mmap
            [offset..offset + SCHEMA_BLOCK_HEADER_SIZE]
            .try_into()
            .unwrap();
        let header = SchemaBlockHeader::from_bytes(&header_bytes);
        header.validate().map_err(ReaderError::InvalidSchemaBlock)?;

        let data_offset = offset + SCHEMA_BLOCK_HEADER_SIZE;
        let compressed_size = header.xml_compressed as usize;

        Ok(SchemaBlock {
            header,
            data: self.mmap[data_offset..data_offset + compressed_size].to_vec(),
        })
    }
}

impl Iterator for JournalReader {
    type Item = Result<Record, ReaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.read_next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{JournalWriter, WriterConfig};
    use std::fs;

    // ==================== Test Helpers ====================

    fn create_test_path() -> std::path::PathBuf {
        let id: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let path = std::env::temp_dir().join(format!("test_reader_{}.journal", id));
        let _ = fs::remove_file(&path);
        path
    }

    fn create_test_journal(records: usize) -> std::path::PathBuf {
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        for i in 0..records {
            let payload = format!("payload {}", i);
            writer
                .write(RecordType::RawFrame, (i % 255) as u8, payload.as_bytes())
                .expect("Failed to write");
        }

        writer.close().expect("Failed to close");
        path
    }

    // ==================== Open Tests ====================

    #[test]
    fn test_open_valid_journal() {
        let path = create_test_journal(0);
        let reader = JournalReader::open(&path).expect("Failed to open");

        assert!(!reader.schema_xml().is_empty());
        let (major, minor) = reader.format_version();
        assert_eq!(major, 1);
        assert_eq!(minor, 2);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_open_with_config() {
        let path = create_test_journal(0);
        let config = ReaderConfig::fast();
        let reader = JournalReader::open_with_config(&path, config).expect("Failed to open");

        assert!(!reader.config.verify_crc);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_open_nonexistent_file() {
        let result = JournalReader::open("nonexistent_file.journal");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_invalid_magic() {
        let path = create_test_path();
        fs::write(&path, b"BADMAGIC" /* ... and more bytes */).unwrap();
        // Pad to header size
        let mut data = b"BADMAGIC".to_vec();
        data.resize(FILE_HEADER_SIZE, 0);
        fs::write(&path, &data).unwrap();

        let result = JournalReader::open(&path);
        assert!(result.is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_open_file_too_small() {
        let path = create_test_path();
        fs::write(&path, b"tiny").unwrap();

        let result = JournalReader::open(&path);
        assert!(matches!(result, Err(ReaderError::FileTooSmall { .. })));

        let _ = fs::remove_file(&path);
    }

    // ==================== Header Tests ====================

    #[test]
    fn test_header_access() {
        let path = create_test_journal(5);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let header = reader.header();
        assert_eq!(&header.magic, b"BLKBOXJL");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_version() {
        let path = create_test_journal(0);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let (major, minor) = reader.schema_version();
        assert_eq!(major, 1);
        assert_eq!(minor, 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_xml_not_empty() {
        let path = create_test_journal(0);
        let reader = JournalReader::open(&path).expect("Failed to open");

        assert!(!reader.schema_xml().is_empty());
        assert!(reader.schema_xml().contains("xml"));

        let _ = fs::remove_file(&path);
    }

    // ==================== Record Reading Tests ====================

    #[test]
    fn test_read_single_record() {
        let path = create_test_journal(1);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        let record = reader
            .next()
            .expect("Should have record")
            .expect("Record should be valid");

        assert_eq!(record.record_type(), RecordType::RawFrame);
        assert_eq!(record.exchange_id(), 0);
        assert_eq!(&record.payload, b"payload 0");

        // No more records
        assert!(reader.next().is_none());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_multiple_records() {
        let path = create_test_journal(10);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 10);
        for (i, record) in records.iter().enumerate() {
            let expected_payload = format!("payload {}", i);
            assert_eq!(record.payload, expected_payload.as_bytes());
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_empty_journal() {
        let path = create_test_journal(0);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        // Should immediately reach end (footer)
        assert!(reader.next().is_none());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_records_with_various_types() {
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        writer.write(RecordType::SessionStart, 0, b"start").unwrap();
        writer.write(RecordType::RawFrame, 1, b"frame1").unwrap();
        writer.write(RecordType::QuoteUpdate, 1, b"quote").unwrap();
        writer.write(RecordType::OrderSubmit, 2, b"order").unwrap();
        writer.write(RecordType::SessionEnd, 0, b"end").unwrap();

        writer.close().expect("Failed to close");

        let reader = JournalReader::open(&path).expect("Failed to open");
        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 5);
        assert_eq!(records[0].record_type(), RecordType::SessionStart);
        assert_eq!(records[1].record_type(), RecordType::RawFrame);
        assert_eq!(records[2].record_type(), RecordType::QuoteUpdate);
        assert_eq!(records[3].record_type(), RecordType::OrderSubmit);
        assert_eq!(records[4].record_type(), RecordType::SessionEnd);

        let _ = fs::remove_file(&path);
    }

    // ==================== Record Field Tests ====================

    #[test]
    fn test_record_timestamp() {
        let path = create_test_journal(1);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        let record = reader.next().unwrap().unwrap();

        // Timestamp should be recent (within last hour)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        let diff = now - record.timestamp();
        assert!((0..3_600_000_000).contains(&diff)); // Within 1 hour

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_record_sequence_numbers() {
        let path = create_test_journal(5);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();

        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.sequence_number(), i as u32);
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_record_exchange_id() {
        let path = create_test_journal(10);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();

        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.exchange_id(), (i % 255) as u8);
        }

        let _ = fs::remove_file(&path);
    }

    // ==================== CRC Verification Tests ====================

    #[test]
    fn test_crc_verification_pass() {
        let path = create_test_journal(5);
        let config = ReaderConfig::default();
        assert!(config.verify_crc);

        let reader = JournalReader::open_with_config(&path, config).expect("Failed to open");

        // All records should pass CRC check
        for result in reader {
            assert!(result.is_ok());
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_crc_skip_disabled() {
        let path = create_test_journal(1);
        let config = ReaderConfig::default();
        assert!(!config.skip_corrupt);

        let reader = JournalReader::open_with_config(&path, config).expect("Failed to open");

        // Should read normally
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);

        let _ = fs::remove_file(&path);
    }

    // ==================== Position and Seek Tests ====================

    #[test]
    fn test_position_tracking() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        let initial_pos = reader.position();
        let _ = reader.next();
        let after_one = reader.position();

        assert!(after_one > initial_pos);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_records_read_count() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        assert_eq!(reader.records_read(), 0);

        let _ = reader.next();
        assert_eq!(reader.records_read(), 1);

        let _ = reader.next();
        assert_eq!(reader.records_read(), 2);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_rewind() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        // Read all
        while reader.next().is_some() {}
        assert_eq!(reader.records_read(), 5);

        // Rewind
        reader.rewind();
        assert_eq!(reader.records_read(), 0);
        assert!(reader.has_more());

        // Read again
        let record = reader.next().unwrap().unwrap();
        assert_eq!(&record.payload, b"payload 0");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_seek() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        // Read first record
        let record1 = reader.next().unwrap().unwrap();
        let offset1 = record1.offset;

        // Read second record
        let record2 = reader.next().unwrap().unwrap();
        let offset2 = record2.offset;

        // Seek back to first record
        reader.seek(offset1 as usize).expect("Seek failed");
        let record1_again = reader.next().unwrap().unwrap();
        assert_eq!(record1.payload, record1_again.payload);

        // Seek to second record
        reader.seek(offset2 as usize).expect("Seek failed");
        let record2_again = reader.next().unwrap().unwrap();
        assert_eq!(record2.payload, record2_again.payload);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_seek_invalid() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        // Seek before first record
        let result = reader.seek(0);
        assert!(result.is_err());

        // Seek past end
        let result = reader.seek(reader.file_size() + 1000);
        assert!(result.is_err());

        let _ = fs::remove_file(&path);
    }

    // ==================== Footer Tests ====================

    #[test]
    fn test_read_footer_present() {
        let path = create_test_journal(5);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        // Read all records
        while reader.next().is_some() {}

        // Read footer
        let footer = reader.read_footer().expect("Footer should be present");
        // Copy field to avoid packed struct reference issues
        let total_records = footer.total_records;
        assert_eq!(total_records, 5);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_footer_total_records() {
        let path = create_test_journal(100);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        while reader.next().is_some() {}

        let footer = reader.read_footer().expect("Footer should be present");
        // Copy field to avoid packed struct reference issues
        let total_records = footer.total_records;
        assert_eq!(total_records, 100);

        let _ = fs::remove_file(&path);
    }

    // ==================== has_more Tests ====================

    #[test]
    fn test_has_more_initially_true() {
        let path = create_test_journal(5);
        let reader = JournalReader::open(&path).expect("Failed to open");

        assert!(reader.has_more());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_has_more_after_reading_all() {
        let path = create_test_journal(3);
        let mut reader = JournalReader::open(&path).expect("Failed to open");

        while reader.next().is_some() {}

        assert!(!reader.has_more());

        let _ = fs::remove_file(&path);
    }

    // ==================== Schema Block Tests ====================

    #[test]
    fn test_read_schema_block() {
        let path = create_test_journal(0);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let schema_block = reader
            .read_schema_block()
            .expect("Should read schema block");
        assert_eq!(&schema_block.header.magic, b"BLKBOXSC");

        let _ = fs::remove_file(&path);
    }

    // ==================== Large Journal Tests ====================

    #[test]
    fn test_read_large_journal() {
        let path = create_test_path();
        // Use larger ring buffer for high throughput test
        let config = WriterConfig {
            ring_buffer_capacity: 2048,
            file_size: 4 * 1024 * 1024,
            compress_schema: true,
            sync_on_close: false,
            prefault_pages: false,
        };
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        for i in 0..1000 {
            let payload = format!("payload {}", i);
            writer
                .write(RecordType::RawFrame, (i % 255) as u8, payload.as_bytes())
                .expect("Failed to write");
        }

        writer.close().expect("Failed to close");

        let reader = JournalReader::open(&path).expect("Failed to open");

        let count = reader.filter_map(|r| r.ok()).count();
        assert_eq!(count, 1000);

        let _ = fs::remove_file(&path);
    }

    // ==================== Different Payload Sizes ====================

    #[test]
    fn test_read_various_payload_sizes() {
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        let sizes = [0, 1, 10, 100, 1000, 10000];
        for &size in &sizes {
            let payload = vec![0x42u8; size];
            writer.write(RecordType::RawFrame, 1, &payload).unwrap();
        }

        writer.close().expect("Failed to close");

        let reader = JournalReader::open(&path).expect("Failed to open");
        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), sizes.len());
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.payload.len(), sizes[i]);
        }

        let _ = fs::remove_file(&path);
    }

    // ==================== Config Tests ====================

    #[test]
    fn test_reader_config_default() {
        let config = ReaderConfig::default();
        assert!(config.verify_crc);
        assert!(!config.skip_corrupt);
        assert!(config.verify_schema_hash);
    }

    #[test]
    fn test_reader_config_lenient() {
        let config = ReaderConfig::lenient();
        assert!(config.verify_crc);
        assert!(config.skip_corrupt);
        assert!(config.verify_schema_hash);
    }

    #[test]
    fn test_reader_config_fast() {
        let config = ReaderConfig::fast();
        assert!(!config.verify_crc);
        assert!(!config.skip_corrupt);
        assert!(!config.verify_schema_hash);
    }

    #[test]
    fn test_reader_config_skip_schema_hash() {
        let config = ReaderConfig::skip_schema_hash();
        assert!(config.verify_crc);
        assert!(!config.skip_corrupt);
        assert!(!config.verify_schema_hash);
    }

    // ==================== Schema Hash Verification Tests ====================

    #[test]
    fn test_schema_hash_verification_pass() {
        // Normal journal files should pass hash verification
        let path = create_test_journal(1);
        let config = ReaderConfig::default();
        assert!(config.verify_schema_hash);

        let result = JournalReader::open_with_config(&path, config);
        assert!(
            result.is_ok(),
            "Valid journal should pass schema hash verification"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_verification_disabled() {
        // When disabled, should not verify (useful for modified files)
        let path = create_test_journal(1);
        let config = ReaderConfig::fast();
        assert!(!config.verify_schema_hash);

        let result = JournalReader::open_with_config(&path, config);
        assert!(result.is_ok());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_verification_skip_schema_hash_config() {
        // Test the skip_schema_hash config specifically
        let path = create_test_journal(1);
        let config = ReaderConfig::skip_schema_hash();

        let result = JournalReader::open_with_config(&path, config);
        assert!(result.is_ok());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_mismatch_detected() {
        // Create a journal with a corrupted schema hash
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        writer.write(RecordType::RawFrame, 1, b"test").unwrap();
        writer.close().expect("Failed to close");

        // Read the file, corrupt the schema hash, and write back
        let mut data = fs::read(&path).expect("Failed to read file");

        // schema_hash is at offset 0x44 (68) in the header - 4 bytes
        // Corrupt it by XORing with a value
        data[0x44] ^= 0xFF;
        data[0x45] ^= 0xFF;
        data[0x46] ^= 0xFF;
        data[0x47] ^= 0xFF;

        fs::write(&path, &data).expect("Failed to write corrupted file");

        // Try to open with verification enabled
        let result = JournalReader::open(&path);
        assert!(
            matches!(result, Err(ReaderError::SchemaHashMismatch { .. })),
            "Expected SchemaHashMismatch error"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_mismatch_error_values() {
        // Test that the error contains correct expected/actual values
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        writer.write(RecordType::RawFrame, 1, b"test").unwrap();
        writer.close().expect("Failed to close");

        // Read the original hash before corruption
        let data = fs::read(&path).expect("Failed to read file");
        let original_hash = u32::from_le_bytes([data[0x44], data[0x45], data[0x46], data[0x47]]);

        // Corrupt the schema hash
        let mut data = data;
        data[0x44] = 0xDE;
        data[0x45] = 0xAD;
        data[0x46] = 0xBE;
        data[0x47] = 0xEF;

        fs::write(&path, &data).expect("Failed to write corrupted file");

        let result = JournalReader::open(&path);
        match result {
            Err(ReaderError::SchemaHashMismatch { expected, actual }) => {
                assert_eq!(expected, 0xEFBEADDE, "Expected the corrupted hash value");
                assert_eq!(
                    actual, original_hash,
                    "Actual should be the real computed hash"
                );
            }
            Ok(_) => panic!("Expected SchemaHashMismatch error, got Ok"),
            Err(e) => panic!("Expected SchemaHashMismatch, got: {}", e),
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_mismatch_can_bypass_with_config() {
        // Create a journal with a corrupted schema hash
        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        writer.write(RecordType::RawFrame, 1, b"test").unwrap();
        writer.close().expect("Failed to close");

        // Corrupt the schema hash
        let mut data = fs::read(&path).expect("Failed to read file");
        data[0x44] ^= 0xFF;
        fs::write(&path, &data).expect("Failed to write corrupted file");

        // Should fail with default config
        let result = JournalReader::open(&path);
        assert!(matches!(
            result,
            Err(ReaderError::SchemaHashMismatch { .. })
        ));

        // Should succeed with verification disabled
        let config = ReaderConfig::skip_schema_hash();
        let result = JournalReader::open_with_config(&path, config);
        assert!(result.is_ok(), "Should open with verification disabled");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_display() {
        let err = ReaderError::SchemaHashMismatch {
            expected: 0x12345678,
            actual: 0xDEADBEEF,
        };
        let msg = err.to_string();
        assert!(msg.contains("0x12345678"), "Should contain expected hash");
        assert!(msg.contains("0xDEADBEEF"), "Should contain actual hash");
    }

    // ==================== Stats Tests ====================

    #[test]
    fn test_session_timestamps() {
        let path = create_test_journal(5);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let start = reader.session_start();
        let end = reader.session_end();

        // Start should be non-zero
        assert!(start > 0);

        // End should be >= start (if file was closed cleanly)
        assert!(end >= start || end == 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_file_size() {
        let path = create_test_journal(10);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let size = reader.file_size();

        // Should be at least header + schema + some records
        assert!(size > FILE_HEADER_SIZE + SCHEMA_BLOCK_HEADER_SIZE);

        let _ = fs::remove_file(&path);
    }

    // ==================== Cross-Version Compatibility Tests (T1.16) ====================
    //
    // These tests verify the 10-year guarantee: journals written today must be
    // readable by future versions of the code.

    #[test]
    fn test_schema_version_matches_current() {
        // Verify that journals are written with the current schema version
        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let (major, minor) = reader.schema_version();
        assert_eq!(major, 1, "Schema major version should be 1");
        assert_eq!(minor, 0, "Schema minor version should be 0");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_format_version_matches_current() {
        // Verify format version in written journals
        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let (major, minor) = reader.format_version();
        assert_eq!(major, 1, "Format major version should be 1");
        assert_eq!(minor, 2, "Format minor version should be 2");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_codec_registry_provides_decoder_for_journal() {
        // Verify that CodecRegistry can provide a decoder for the journal's schema version
        use crate::codec::global_registry;

        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let (major, minor) = reader.schema_version();
        let registry = global_registry();

        // Should get exact match decoder
        let decoder = registry.decoder(major, minor);
        assert!(
            decoder.is_some(),
            "Registry should have decoder for v{}.{}",
            major,
            minor
        );

        let decoder = decoder.unwrap();
        assert_eq!(decoder.schema_version(), (major, minor));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_codec_registry_fallback_for_future_minor_version() {
        // Simulate reading a journal from a future minor version
        use crate::codec::global_registry;

        let registry = global_registry();

        // Request v1.5 (future minor version) - should fall back to v1.0
        let result = registry.decoder_with_fallback(1, 5);
        assert!(result.is_ok(), "Should fall back for future minor version");

        let decoder = result.unwrap();
        // Falls back to highest available v1.x (which is v1.0)
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    #[test]
    fn test_codec_registry_rejects_future_major_version() {
        // Major version changes are breaking - should fail
        use crate::codec::{global_registry, CodecError};

        let registry = global_registry();

        // Request v2.0 (future major version) - should fail
        let result = registry.decoder_with_fallback(2, 0);
        assert!(matches!(
            result,
            Err(CodecError::UnsupportedVersion { major: 2, minor: 0 })
        ));
    }

    #[test]
    fn test_decode_raw_frame_from_journal() {
        // End-to-end test: write journal, read records, decode with codec
        use crate::codec::global_registry;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write a raw frame record
        let test_payload = b"{\"type\":\"quote\",\"bid\":100.5}";
        writer.write(RecordType::RawFrame, 1, test_payload).unwrap();
        writer.close().expect("Failed to close");

        // Read and decode
        let mut reader = JournalReader::open(&path).expect("Failed to open");
        let (major, minor) = reader.schema_version();

        let registry = global_registry();
        let _decoder = registry
            .decoder_with_fallback(major, minor)
            .expect("Should get decoder");

        let record = reader.next().unwrap().unwrap();
        assert_eq!(record.record_type(), RecordType::RawFrame);

        // The payload in the record is the raw data we wrote
        assert_eq!(record.payload, test_payload);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_embedded_schema_is_sbe_xml() {
        // Verify the embedded schema is valid SBE XML
        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let schema = reader.schema_xml();

        // Should contain SBE XML markers
        assert!(schema.contains("sbe:messageSchema"), "Should be SBE schema");
        assert!(
            schema.contains("package=\"blackbox\""),
            "Should have correct package"
        );
        assert!(
            schema.contains("byteOrder=\"littleEndian\""),
            "Should specify byte order"
        );

        // Should contain message definitions
        assert!(schema.contains("RawFrame"), "Should have RawFrame message");
        assert!(
            schema.contains("SessionStart"),
            "Should have SessionStart message"
        );
        assert!(
            schema.contains("OrderSubmit"),
            "Should have OrderSubmit message"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_embedded_schema_matches_writer_schema() {
        // The embedded schema should match what the writer used
        use crate::journal::schema::embedded_xml;

        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let embedded = reader.schema_xml();
        let expected = embedded_xml();

        // Must match exactly
        assert_eq!(
            embedded, expected,
            "Embedded schema must match writer's schema"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_ten_year_guarantee_scenario() {
        // Simulate the 10-year guarantee scenario:
        // 1. Write a journal with current schema
        // 2. Verify it contains all information needed for future reading
        // 3. Verify the CodecRegistry can select the appropriate decoder
        use crate::codec::global_registry;
        use crate::journal::schema::embedded_xml;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write various record types
        writer
            .write(RecordType::SessionStart, 0, b"session_meta")
            .unwrap();
        writer
            .write(RecordType::RawFrame, 1, b"quote_data")
            .unwrap();
        writer
            .write(RecordType::QuoteUpdate, 1, b"quote_update")
            .unwrap();
        writer
            .write(RecordType::OrderSubmit, 2, b"order_data")
            .unwrap();
        writer
            .write(RecordType::SessionEnd, 0, b"end_meta")
            .unwrap();
        writer.close().expect("Failed to close");

        // Read back and verify self-describing capability
        let reader = JournalReader::open(&path).expect("Failed to open");

        // Step 1: Extract version info
        let (schema_major, schema_minor) = reader.schema_version();
        let (_format_major, _format_minor) = reader.format_version();

        // Step 2: Extract embedded schema (the key to 10-year guarantee)
        let embedded_schema = reader.schema_xml();

        // Step 3: Verify schema is complete
        assert!(!embedded_schema.is_empty(), "Schema must be embedded");
        assert_eq!(embedded_schema, embedded_xml(), "Schema must be exact copy");

        // Step 4: Verify CodecRegistry can provide decoder
        let registry = global_registry();
        let decoder_result = registry.decoder_with_fallback(schema_major, schema_minor);
        assert!(
            decoder_result.is_ok(),
            "Registry must provide decoder for embedded version"
        );

        // Step 5: Read all records
        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 5, "Should read all 5 records");

        // Step 6: Verify record types preserved
        assert_eq!(records[0].record_type(), RecordType::SessionStart);
        assert_eq!(records[1].record_type(), RecordType::RawFrame);
        assert_eq!(records[2].record_type(), RecordType::QuoteUpdate);
        assert_eq!(records[3].record_type(), RecordType::OrderSubmit);
        assert_eq!(records[4].record_type(), RecordType::SessionEnd);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_version_info_in_header() {
        // Verify all version information is correctly stored in header
        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let header = reader.header();

        // Check magic
        assert_eq!(&header.magic, b"BLKBOXJL");

        // Check format version
        assert_eq!(header.format_major, 1);
        assert_eq!(header.format_minor, 2);

        // Check schema version
        assert_eq!(header.schema_major, 1);
        assert_eq!(header.schema_minor, 0);

        // Check header size
        let header_size = header.header_size;
        assert_eq!(header_size, 128);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_block_accessible() {
        // Verify schema block can be read separately
        let path = create_test_journal(1);
        let reader = JournalReader::open(&path).expect("Failed to open");

        let schema_block = reader
            .read_schema_block()
            .expect("Should read schema block");

        // Verify schema block header
        assert_eq!(&schema_block.header.magic, b"BLKBOXSC");
        let xml_uncompressed = schema_block.header.xml_uncompressed;
        assert!(xml_uncompressed > 0, "Should have uncompressed size");

        // Verify data can be decompressed to the schema
        // Note: schema_block.data contains compressed data, we use schema_xml() for decompressed
        let decompressed_schema = reader.schema_xml();
        assert!(decompressed_schema.contains("sbe:messageSchema"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_multiple_record_types_decodable() {
        // Verify all record types are written with proper type IDs
        use crate::codec::global_registry;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write all record types
        writer.write(RecordType::SessionStart, 0, b"start").unwrap();
        writer.write(RecordType::SessionEnd, 0, b"end").unwrap();
        writer.write(RecordType::Checkpoint, 0, b"check").unwrap();
        writer.write(RecordType::RawFrame, 1, b"frame").unwrap();
        writer.write(RecordType::QuoteUpdate, 1, b"quote").unwrap();
        writer.write(RecordType::Trade, 1, b"trade").unwrap();
        writer.write(RecordType::OrderSubmit, 1, b"submit").unwrap();
        writer.write(RecordType::OrderAck, 1, b"ack").unwrap();
        writer.write(RecordType::OrderFill, 1, b"fill").unwrap();
        writer.write(RecordType::StateChange, 0, b"state").unwrap();
        writer.write(RecordType::Signal, 0, b"signal").unwrap();
        writer.close().expect("Failed to close");

        let reader = JournalReader::open(&path).expect("Failed to open");
        let (major, minor) = reader.schema_version();

        let registry = global_registry();
        let _decoder = registry
            .decoder_with_fallback(major, minor)
            .expect("Should get decoder");

        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 11);

        // Verify each record type has correct type ID
        let expected_types = [
            RecordType::SessionStart,
            RecordType::SessionEnd,
            RecordType::Checkpoint,
            RecordType::RawFrame,
            RecordType::QuoteUpdate,
            RecordType::Trade,
            RecordType::OrderSubmit,
            RecordType::OrderAck,
            RecordType::OrderFill,
            RecordType::StateChange,
            RecordType::Signal,
        ];

        for (i, record) in records.iter().enumerate() {
            assert_eq!(
                record.record_type(),
                expected_types[i],
                "Record {} type mismatch",
                i
            );
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_schema_hash_consistent_with_version() {
        // Verify schema hash is computed from the embedded schema
        use crate::journal::schema::embedded_xml;

        let path = create_test_journal(1);

        // Read the file and extract schema hash from header
        let data = fs::read(&path).expect("Failed to read file");
        let stored_hash = u32::from_le_bytes([data[0x44], data[0x45], data[0x46], data[0x47]]);

        // Compute expected hash from current schema
        let expected_hash = crc32fast::hash(embedded_xml().as_bytes());

        assert_eq!(
            stored_hash, expected_hash,
            "Stored hash should match computed hash"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_cross_version_forward_compatibility() {
        // Simulate forward compatibility: old data, new reader
        // This tests that the append-only schema evolution rule works
        use crate::codec::global_registry;

        let path = create_test_journal(5);
        let reader = JournalReader::open(&path).expect("Failed to open");

        // Current version is v1.0
        let (major, minor) = reader.schema_version();
        assert_eq!((major, minor), (1, 0));

        // Simulate a future reader requesting v1.1 decoder but getting v1.0
        let registry = global_registry();

        // Future v1.1 reader would fall back to v1.0 for our file
        let decoder = registry
            .decoder_with_fallback(1, 1)
            .expect("Should fall back to v1.0");
        assert_eq!(decoder.schema_version(), (1, 0));

        // All records should still be readable
        let records: Vec<Record> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 5);

        let _ = fs::remove_file(&path);
    }
}
