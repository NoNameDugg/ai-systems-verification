//! Versioned codec system for SBE message encoding/decoding.
//!
//! This module provides a version-dispatched codec system that enables
//! reading journal files with different schema versions. Every journal
//! file contains its schema version in the header, and the `CodecRegistry`
//! selects the appropriate decoder at runtime.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                     VERSIONED CODEC ARCHITECTURE                     │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                      │
//! │  1. OPEN JOURNAL FILE                                               │
//! │     ┌────────────────────────────────────────────────────┐          │
//! │     │ Read FileHeader                                     │          │
//! │     │ Extract: schema_version = (1, 0)                   │          │
//! │     │ Extract: embedded schema XML (if needed)           │          │
//! │     └────────────────────────────────────────────────────┘          │
//! │                           │                                          │
//! │                           ▼                                          │
//! │  2. SELECT CODEC VERSION                                            │
//! │     ┌────────────────────────────────────────────────────┐          │
//! │     │ CodecRegistry::decoder_with_fallback(1, 0)         │          │
//! │     │   → Returns v1_0::Decoder                          │          │
//! │     └────────────────────────────────────────────────────┘          │
//! │                           │                                          │
//! │                           ▼                                          │
//! │  3. DECODE RECORDS                                                  │
//! │     ┌────────────────────────────────────────────────────┐          │
//! │     │ Use selected decoder for ALL records in this file  │          │
//! │     │ Decoder matches file version, not current version  │          │
//! │     └────────────────────────────────────────────────────┘          │
//! │                                                                      │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Schema Versioning
//!
//! Schema versions follow semantic versioning:
//!
//! - **MAJOR version**: Breaking changes (field reordering, type changes)
//! - **MINOR version**: Backward-compatible changes (new fields at end)
//!
//! The codec system handles version compatibility:
//!
//! | Writer | Reader | Compatible? | Notes |
//! |--------|--------|-------------|-------|
//! | 1.0    | 1.0    | ✅ Yes      | Exact match |
//! | 1.0    | 1.1    | ✅ Yes      | New fields get defaults |
//! | 1.1    | 1.0    | ✅ Yes      | Extra fields ignored |
//! | 1.0    | 2.0    | ⚠️ Degraded | Dynamic decode fallback |
//! | 2.0    | 1.0    | ❌ No       | Major version mismatch |
//!
//! # Usage
//!
//! ## Decoding Messages
//!
//! ```ignore
//! use blackbox::codec::{CodecRegistry, MessageDecoder};
//!
//! let registry = CodecRegistry::new();
//!
//! // Get decoder for specific version (from journal header)
//! let decoder = registry.decoder_with_fallback(1, 0)?;
//!
//! // Decode a raw frame
//! let raw_frame = decoder.decode_raw_frame(&buffer)?;
//! println!("Timestamp: {}", raw_frame.timestamp);
//!
//! // Decode by message ID
//! let message = decoder.decode_by_id(256, &buffer)?;
//! ```
//!
//! ## Encoding Messages
//!
//! ```ignore
//! use blackbox::codec::{CodecRegistry, MessageEncoder, RawFrameData, Exchange};
//!
//! let registry = CodecRegistry::new();
//! let encoder = registry.encoder();
//!
//! let data = RawFrameData {
//!     timestamp: 1704067200000000,
//!     exchange: Exchange::Deribit,
//!     sequence_number: 1,
//!     payload: b"websocket frame".to_vec(),
//! };
//!
//! let mut buffer = vec![0u8; 1024];
//! let size = encoder.encode_raw_frame(&data, &mut buffer)?;
//! ```
//!
//! ## Global Registry
//!
//! For convenience, a global registry instance is available:
//!
//! ```ignore
//! use blackbox::codec::global_registry;
//!
//! let decoder = global_registry().decoder(1, 0).unwrap();
//! ```

pub mod data;
pub mod error;
pub mod registry;
pub mod traits;
pub mod v1_0;

// Re-export commonly used types
pub use data::{
    CheckpointData, DecodedMessage, Exchange, OrderAckData, OrderFillData, OrderSubmitData,
    QuoteUpdateData, RawFrameData, SessionEndData, SessionStartData, Sha256, Side, SignalData,
    StateChangeData, Symbol, TradeData, MAX_SYMBOL_LENGTH, MAX_VAR_DATA_LENGTH,
};
pub use error::{CodecError, DecodeError, EncodeError};
pub use registry::{global_registry, CodecRegistry};
pub use traits::{MessageDecoder, MessageEncoder};
