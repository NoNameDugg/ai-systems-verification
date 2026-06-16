//! Codec traits for message encoding and decoding.
//!
//! This module defines the traits that all versioned codecs must implement.
//! These traits provide a common interface for decoding messages regardless
//! of the schema version used to encode them.

use super::data::*;
use super::error::{DecodeError, EncodeError};

/// Trait for decoding SBE messages.
///
/// Each schema version has its own implementation of this trait.
/// The CodecRegistry uses these implementations to decode messages
/// based on the schema version stored in the journal file header.
///
/// # Example
///
/// ```ignore
/// use blackbox::codec::{CodecRegistry, MessageDecoder};
///
/// let registry = CodecRegistry::new();
/// let decoder = registry.get(1, 0).unwrap();
///
/// let raw_frame = decoder.decode_raw_frame(&buffer)?;
/// println!("Timestamp: {}", raw_frame.timestamp);
/// ```
pub trait MessageDecoder: Send + Sync {
    /// Get the schema version this decoder handles.
    fn schema_version(&self) -> (u8, u8);

    /// Decode a SessionStart message (ID: 1).
    fn decode_session_start(&self, buf: &[u8]) -> Result<SessionStartData, DecodeError>;

    /// Decode a SessionEnd message (ID: 2).
    fn decode_session_end(&self, buf: &[u8]) -> Result<SessionEndData, DecodeError>;

    /// Decode a Checkpoint message (ID: 3).
    fn decode_checkpoint(&self, buf: &[u8]) -> Result<CheckpointData, DecodeError>;

    /// Decode a RawFrame message (ID: 256).
    fn decode_raw_frame(&self, buf: &[u8]) -> Result<RawFrameData, DecodeError>;

    /// Decode a QuoteUpdate message (ID: 257).
    fn decode_quote_update(&self, buf: &[u8]) -> Result<QuoteUpdateData, DecodeError>;

    /// Decode a Trade message (ID: 258).
    fn decode_trade(&self, buf: &[u8]) -> Result<TradeData, DecodeError>;

    /// Decode an OrderSubmit message (ID: 512).
    fn decode_order_submit(&self, buf: &[u8]) -> Result<OrderSubmitData, DecodeError>;

    /// Decode an OrderAck message (ID: 513).
    fn decode_order_ack(&self, buf: &[u8]) -> Result<OrderAckData, DecodeError>;

    /// Decode an OrderFill message (ID: 514).
    fn decode_order_fill(&self, buf: &[u8]) -> Result<OrderFillData, DecodeError>;

    /// Decode a StateChange message (ID: 768).
    fn decode_state_change(&self, buf: &[u8]) -> Result<StateChangeData, DecodeError>;

    /// Decode a Signal message (ID: 769).
    fn decode_signal(&self, buf: &[u8]) -> Result<SignalData, DecodeError>;

    /// Decode any message by its type ID.
    ///
    /// This is a convenience method that dispatches to the appropriate
    /// decode method based on the message type ID.
    fn decode_by_id(&self, message_id: u16, buf: &[u8]) -> Result<DecodedMessage, DecodeError> {
        match message_id {
            1 => Ok(DecodedMessage::SessionStart(
                self.decode_session_start(buf)?,
            )),
            2 => Ok(DecodedMessage::SessionEnd(self.decode_session_end(buf)?)),
            3 => Ok(DecodedMessage::Checkpoint(self.decode_checkpoint(buf)?)),
            256 => Ok(DecodedMessage::RawFrame(self.decode_raw_frame(buf)?)),
            257 => Ok(DecodedMessage::QuoteUpdate(self.decode_quote_update(buf)?)),
            258 => Ok(DecodedMessage::Trade(self.decode_trade(buf)?)),
            512 => Ok(DecodedMessage::OrderSubmit(self.decode_order_submit(buf)?)),
            513 => Ok(DecodedMessage::OrderAck(self.decode_order_ack(buf)?)),
            514 => Ok(DecodedMessage::OrderFill(self.decode_order_fill(buf)?)),
            768 => Ok(DecodedMessage::StateChange(self.decode_state_change(buf)?)),
            769 => Ok(DecodedMessage::Signal(self.decode_signal(buf)?)),
            _ => Err(DecodeError::InvalidMessageType(message_id)),
        }
    }
}

/// Trait for encoding SBE messages.
///
/// Each schema version has its own implementation of this trait.
/// The CodecRegistry uses these implementations to encode messages
/// for the current schema version.
///
/// # Example
///
/// ```ignore
/// use blackbox::codec::{CodecRegistry, MessageEncoder};
///
/// let registry = CodecRegistry::new();
/// let encoder = registry.encoder().unwrap();
///
/// let mut buffer = vec![0u8; 1024];
/// let size = encoder.encode_raw_frame(&raw_frame, &mut buffer)?;
/// ```
pub trait MessageEncoder: Send + Sync {
    /// Get the schema version this encoder produces.
    fn schema_version(&self) -> (u8, u8);

    /// Encode a SessionStart message (ID: 1).
    ///
    /// Returns the number of bytes written.
    fn encode_session_start(
        &self,
        data: &SessionStartData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode a SessionEnd message (ID: 2).
    ///
    /// Returns the number of bytes written.
    fn encode_session_end(
        &self,
        data: &SessionEndData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode a Checkpoint message (ID: 3).
    ///
    /// Returns the number of bytes written.
    fn encode_checkpoint(
        &self,
        data: &CheckpointData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode a RawFrame message (ID: 256).
    ///
    /// Returns the number of bytes written.
    fn encode_raw_frame(&self, data: &RawFrameData, buf: &mut [u8]) -> Result<usize, EncodeError>;

    /// Encode a QuoteUpdate message (ID: 257).
    ///
    /// Returns the number of bytes written.
    fn encode_quote_update(
        &self,
        data: &QuoteUpdateData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode a Trade message (ID: 258).
    ///
    /// Returns the number of bytes written.
    fn encode_trade(&self, data: &TradeData, buf: &mut [u8]) -> Result<usize, EncodeError>;

    /// Encode an OrderSubmit message (ID: 512).
    ///
    /// Returns the number of bytes written.
    fn encode_order_submit(
        &self,
        data: &OrderSubmitData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode an OrderAck message (ID: 513).
    ///
    /// Returns the number of bytes written.
    fn encode_order_ack(&self, data: &OrderAckData, buf: &mut [u8]) -> Result<usize, EncodeError>;

    /// Encode an OrderFill message (ID: 514).
    ///
    /// Returns the number of bytes written.
    fn encode_order_fill(&self, data: &OrderFillData, buf: &mut [u8])
        -> Result<usize, EncodeError>;

    /// Encode a StateChange message (ID: 768).
    ///
    /// Returns the number of bytes written.
    fn encode_state_change(
        &self,
        data: &StateChangeData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError>;

    /// Encode a Signal message (ID: 769).
    ///
    /// Returns the number of bytes written.
    fn encode_signal(&self, data: &SignalData, buf: &mut [u8]) -> Result<usize, EncodeError>;

    /// Encode any message.
    ///
    /// This is a convenience method that dispatches to the appropriate
    /// encode method based on the message variant.
    fn encode(&self, message: &DecodedMessage, buf: &mut [u8]) -> Result<usize, EncodeError> {
        match message {
            DecodedMessage::SessionStart(d) => self.encode_session_start(d, buf),
            DecodedMessage::SessionEnd(d) => self.encode_session_end(d, buf),
            DecodedMessage::Checkpoint(d) => self.encode_checkpoint(d, buf),
            DecodedMessage::RawFrame(d) => self.encode_raw_frame(d, buf),
            DecodedMessage::QuoteUpdate(d) => self.encode_quote_update(d, buf),
            DecodedMessage::Trade(d) => self.encode_trade(d, buf),
            DecodedMessage::OrderSubmit(d) => self.encode_order_submit(d, buf),
            DecodedMessage::OrderAck(d) => self.encode_order_ack(d, buf),
            DecodedMessage::OrderFill(d) => self.encode_order_fill(d, buf),
            DecodedMessage::StateChange(d) => self.encode_state_change(d, buf),
            DecodedMessage::Signal(d) => self.encode_signal(d, buf),
        }
    }

    /// Calculate the encoded size of a message.
    ///
    /// Returns the exact number of bytes that will be written when encoding.
    fn encoded_size(&self, message: &DecodedMessage) -> usize;
}

/// Helper functions for reading little-endian values from byte slices.
pub mod read_le {
    /// Read a u8 from a byte slice.
    #[inline]
    pub fn read_u8(buf: &[u8]) -> u8 {
        buf[0]
    }

    /// Read a u16 from a byte slice (little-endian).
    #[inline]
    pub fn read_u16(buf: &[u8]) -> u16 {
        u16::from_le_bytes([buf[0], buf[1]])
    }

    /// Read a u32 from a byte slice (little-endian).
    #[inline]
    pub fn read_u32(buf: &[u8]) -> u32 {
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
    }

    /// Read a u64 from a byte slice (little-endian).
    #[inline]
    pub fn read_u64(buf: &[u8]) -> u64 {
        u64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ])
    }

    /// Read an i64 from a byte slice (little-endian).
    #[inline]
    pub fn read_i64(buf: &[u8]) -> i64 {
        i64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ])
    }
}

/// Helper functions for writing little-endian values to byte slices.
pub mod write_le {
    /// Write a u8 to a byte slice.
    #[inline]
    pub fn write_u8(buf: &mut [u8], value: u8) {
        buf[0] = value;
    }

    /// Write a u16 to a byte slice (little-endian).
    #[inline]
    pub fn write_u16(buf: &mut [u8], value: u16) {
        let bytes = value.to_le_bytes();
        buf[0] = bytes[0];
        buf[1] = bytes[1];
    }

    /// Write a u32 to a byte slice (little-endian).
    #[inline]
    pub fn write_u32(buf: &mut [u8], value: u32) {
        let bytes = value.to_le_bytes();
        buf[..4].copy_from_slice(&bytes);
    }

    /// Write a u64 to a byte slice (little-endian).
    #[inline]
    pub fn write_u64(buf: &mut [u8], value: u64) {
        let bytes = value.to_le_bytes();
        buf[..8].copy_from_slice(&bytes);
    }

    /// Write an i64 to a byte slice (little-endian).
    #[inline]
    pub fn write_i64(buf: &mut [u8], value: i64) {
        let bytes = value.to_le_bytes();
        buf[..8].copy_from_slice(&bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Read LE Tests ====================

    #[test]
    fn test_read_u8() {
        let buf = [0x42];
        assert_eq!(read_le::read_u8(&buf), 0x42);
    }

    #[test]
    fn test_read_u16() {
        let buf = [0x34, 0x12]; // 0x1234 in LE
        assert_eq!(read_le::read_u16(&buf), 0x1234);
    }

    #[test]
    fn test_read_u32() {
        let buf = [0x78, 0x56, 0x34, 0x12]; // 0x12345678 in LE
        assert_eq!(read_le::read_u32(&buf), 0x12345678);
    }

    #[test]
    fn test_read_u64() {
        let buf = [0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01];
        assert_eq!(read_le::read_u64(&buf), 0x0123456789ABCDEF);
    }

    #[test]
    fn test_read_i64_positive() {
        let buf = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(read_le::read_i64(&buf), 0x0100000000000000);
    }

    #[test]
    fn test_read_i64_negative() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_le::read_i64(&buf), -1);
    }

    // ==================== Write LE Tests ====================

    #[test]
    fn test_write_u8() {
        let mut buf = [0u8];
        write_le::write_u8(&mut buf, 0xAB);
        assert_eq!(buf, [0xAB]);
    }

    #[test]
    fn test_write_u16() {
        let mut buf = [0u8; 2];
        write_le::write_u16(&mut buf, 0x1234);
        assert_eq!(buf, [0x34, 0x12]);
    }

    #[test]
    fn test_write_u32() {
        let mut buf = [0u8; 4];
        write_le::write_u32(&mut buf, 0x12345678);
        assert_eq!(buf, [0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_write_u64() {
        let mut buf = [0u8; 8];
        write_le::write_u64(&mut buf, 0x0123456789ABCDEF);
        assert_eq!(buf, [0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01]);
    }

    #[test]
    fn test_write_i64() {
        let mut buf = [0u8; 8];
        write_le::write_i64(&mut buf, -1);
        assert_eq!(buf, [0xFF; 8]);
    }

    // ==================== Roundtrip Tests ====================

    #[test]
    fn test_u16_roundtrip() {
        for value in [0u16, 1, 0x1234, 0xFFFF] {
            let mut buf = [0u8; 2];
            write_le::write_u16(&mut buf, value);
            assert_eq!(read_le::read_u16(&buf), value);
        }
    }

    #[test]
    fn test_u32_roundtrip() {
        for value in [0u32, 1, 0x12345678, 0xFFFFFFFF] {
            let mut buf = [0u8; 4];
            write_le::write_u32(&mut buf, value);
            assert_eq!(read_le::read_u32(&buf), value);
        }
    }

    #[test]
    fn test_u64_roundtrip() {
        for value in [0u64, 1, 0x0123456789ABCDEF, 0xFFFFFFFFFFFFFFFF] {
            let mut buf = [0u8; 8];
            write_le::write_u64(&mut buf, value);
            assert_eq!(read_le::read_u64(&buf), value);
        }
    }

    #[test]
    fn test_i64_roundtrip() {
        for value in [0i64, 1, -1, i64::MIN, i64::MAX, 0x0123456789ABCDEF] {
            let mut buf = [0u8; 8];
            write_le::write_i64(&mut buf, value);
            assert_eq!(read_le::read_i64(&buf), value);
        }
    }
}
