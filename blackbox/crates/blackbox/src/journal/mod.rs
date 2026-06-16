//! Journal module - High-speed binary journaling.
//!
//! This module provides the core journaling functionality:
//! - Binary file format with self-describing schema
//! - Zero-allocation writing via MMAP
//! - Sequential reading with iterator interface
//! - Lock-free ring buffer for hot path
//! - **Embedded SBE schema for 10-year readability guarantee**
//!
//! ## File Format
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │         FileHeader (128 bytes)       │
//! ├──────────────────────────────────────┤
//! │         SchemaBlock (variable)       │
//! │    [Zstd-compressed SBE XML]         │
//! ├──────────────────────────────────────┤
//! │         Record 1                     │
//! │    ┌─────────────────────────────┐   │
//! │    │ RecordHeader (24 bytes)     │   │
//! │    │ Payload (variable)          │   │
//! │    └─────────────────────────────┘   │
//! ├──────────────────────────────────────┤
//! │         Record 2                     │
//! ├──────────────────────────────────────┤
//! │         ...                          │
//! ├──────────────────────────────────────┤
//! │         FileFooter (32 bytes)        │
//! └──────────────────────────────────────┘
//! ```
//!
//! ## Schema Embedding
//!
//! Every journal file contains an embedded copy of the SBE schema used to
//! encode its records. This ensures journals remain readable even as the
//! schema evolves over time.
//!
//! ```ignore
//! use blackbox::journal::schema;
//!
//! // Access embedded schema
//! let xml = schema::embedded_xml();
//! let hash = schema::embedded_hash();
//! let (major, minor) = schema::embedded_version();
//!
//! // Create schema block for writing
//! let block = schema::create_block(true)?; // compressed
//! ```

mod format;
mod reader;
mod ring_buffer;
pub mod schema;
mod writer;

pub use format::{
    compression, FileFooter, FileHeader, RecordHeader, RecordType, SchemaBlock, SchemaBlockHeader,
    FILE_FOOTER_SIZE, FILE_HEADER_SIZE, FILE_MAGIC, FOOTER_MAGIC, FORMAT_VERSION_MAJOR,
    FORMAT_VERSION_MINOR, RECORD_HEADER_SIZE, SCHEMA_BLOCK_HEADER_SIZE, SCHEMA_MAGIC,
    SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR,
};
pub use reader::{JournalReader, ReaderConfig, ReaderError, Record};
pub use ring_buffer::{BufferEntry, RingBuffer};
pub use writer::{JournalWriter, WriterConfig, WriterError};
