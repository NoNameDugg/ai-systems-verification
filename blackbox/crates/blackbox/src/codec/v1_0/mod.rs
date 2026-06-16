//! Schema version 1.0 codec implementation.
//!
//! This module provides the decoder and encoder for SBE schema v1.0.

mod decoder;
mod encoder;

pub use decoder::Decoder;
pub use encoder::Encoder;

/// Schema version for this codec.
pub const SCHEMA_VERSION: (u8, u8) = (1, 0);
