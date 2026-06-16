//! Schema version 1.0 message encoder.
//!
//! This encoder implements the binary layout defined in blackbox-v1.0.xml.
//! All fields are little-endian per the format spec, section 2.1.

use crate::codec::data::*;
use crate::codec::error::EncodeError;
use crate::codec::traits::{write_le, MessageEncoder};

/// Encoder for SBE schema version 1.0.
#[derive(Debug, Clone, Copy, Default)]
pub struct Encoder;

impl Encoder {
    /// Create a new v1.0 encoder.
    pub fn new() -> Self {
        Self
    }

    /// Check buffer size and return error if too small.
    #[inline]
    fn check_size(buf: &[u8], required: usize) -> Result<(), EncodeError> {
        if buf.len() < required {
            return Err(EncodeError::BufferTooSmall {
                required,
                available: buf.len(),
            });
        }
        Ok(())
    }

    /// Write a Symbol to buffer at given offset.
    #[inline]
    fn write_symbol(buf: &mut [u8], offset: usize, symbol: &Symbol) {
        buf[offset..offset + MAX_SYMBOL_LENGTH].copy_from_slice(symbol.raw_data());
        buf[offset + MAX_SYMBOL_LENGTH] = symbol.len() as u8;
    }

    /// Write a Sha256 to buffer at given offset.
    #[inline]
    fn write_sha256(buf: &mut [u8], offset: usize, hash: &Sha256) {
        buf[offset..offset + 32].copy_from_slice(hash.as_bytes());
    }

    /// Write variable-length data to buffer.
    /// Returns bytes written.
    #[inline]
    fn write_var_data(buf: &mut [u8], offset: usize, data: &[u8]) -> Result<usize, EncodeError> {
        let required = offset + 4 + data.len();
        if buf.len() < required {
            return Err(EncodeError::BufferTooSmall {
                required,
                available: buf.len(),
            });
        }
        write_le::write_u32(&mut buf[offset..], data.len() as u32);
        buf[offset + 4..offset + 4 + data.len()].copy_from_slice(data);
        Ok(4 + data.len())
    }
}

impl MessageEncoder for Encoder {
    fn schema_version(&self) -> (u8, u8) {
        (1, 0)
    }

    fn encode_session_start(
        &self,
        data: &SessionStartData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 1; // 17 bytes
        let total_size = FIXED_SIZE + 4 + data.metadata.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        write_le::write_u64(&mut buf[8..], data.session_id);
        buf[16] = data.exchange.as_u8();
        Self::write_var_data(buf, FIXED_SIZE, &data.metadata)?;

        Ok(total_size)
    }

    fn encode_session_end(
        &self,
        data: &SessionEndData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 8 + 32; // 56 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        write_le::write_u64(&mut buf[8..], data.session_id);
        write_le::write_u64(&mut buf[16..], data.record_count);
        Self::write_sha256(buf, 24, &data.final_state_hash);

        Ok(FIXED_SIZE)
    }

    fn encode_checkpoint(
        &self,
        data: &CheckpointData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 32 + 32 + 32; // 112 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        write_le::write_u64(&mut buf[8..], data.sequence_number);
        Self::write_sha256(buf, 16, &data.state_hash);
        Self::write_sha256(buf, 48, &data.orderbook_hash);
        Self::write_sha256(buf, 80, &data.position_hash);

        Ok(FIXED_SIZE)
    }

    fn encode_raw_frame(&self, data: &RawFrameData, buf: &mut [u8]) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 4; // 13 bytes
        let total_size = FIXED_SIZE + 4 + data.payload.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        write_le::write_u32(&mut buf[9..], data.sequence_number);
        Self::write_var_data(buf, FIXED_SIZE, &data.payload)?;

        Ok(total_size)
    }

    fn encode_quote_update(
        &self,
        data: &QuoteUpdateData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 8 + 8 + 8 + 8; // 74 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        Self::write_symbol(buf, 9, &data.symbol);
        write_le::write_i64(&mut buf[42..], data.bid_price);
        write_le::write_i64(&mut buf[50..], data.bid_size);
        write_le::write_i64(&mut buf[58..], data.ask_price);
        write_le::write_i64(&mut buf[66..], data.ask_size);

        Ok(FIXED_SIZE)
    }

    fn encode_trade(&self, data: &TradeData, buf: &mut [u8]) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 8 + 8 + 1; // 59 bytes
        let total_size = FIXED_SIZE + 4 + data.trade_id.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        Self::write_symbol(buf, 9, &data.symbol);
        write_le::write_i64(&mut buf[42..], data.price);
        write_le::write_i64(&mut buf[50..], data.size);
        buf[58] = data.side.as_u8();
        Self::write_var_data(buf, FIXED_SIZE, &data.trade_id)?;

        Ok(total_size)
    }

    fn encode_order_submit(
        &self,
        data: &OrderSubmitData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 1 + 8 + 8; // 59 bytes
        let total_size = FIXED_SIZE + 4 + data.client_order_id.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        Self::write_symbol(buf, 9, &data.symbol);
        buf[42] = data.side.as_u8();
        write_le::write_i64(&mut buf[43..], data.price);
        write_le::write_i64(&mut buf[51..], data.size);
        Self::write_var_data(buf, FIXED_SIZE, &data.client_order_id)?;

        Ok(total_size)
    }

    fn encode_order_ack(&self, data: &OrderAckData, buf: &mut [u8]) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1; // 9 bytes
        let total_size =
            FIXED_SIZE + 4 + data.client_order_id.len() + 4 + data.exchange_order_id.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        let consumed1 = Self::write_var_data(buf, FIXED_SIZE, &data.client_order_id)?;
        Self::write_var_data(buf, FIXED_SIZE + consumed1, &data.exchange_order_id)?;

        Ok(total_size)
    }

    fn encode_order_fill(
        &self,
        data: &OrderFillData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 8 + 8 + 8; // 33 bytes
        let total_size = FIXED_SIZE + 4 + data.exchange_order_id.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        buf[8] = data.exchange.as_u8();
        write_le::write_i64(&mut buf[9..], data.fill_price);
        write_le::write_i64(&mut buf[17..], data.fill_size);
        write_le::write_i64(&mut buf[25..], data.remaining_size);
        Self::write_var_data(buf, FIXED_SIZE, &data.exchange_order_id)?;

        Ok(total_size)
    }

    fn encode_state_change(
        &self,
        data: &StateChangeData,
        buf: &mut [u8],
    ) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 2; // 10 bytes
        let total_size = FIXED_SIZE + 4 + data.state_data.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        write_le::write_u16(&mut buf[8..], data.event_type);
        Self::write_var_data(buf, FIXED_SIZE, &data.state_data)?;

        Ok(total_size)
    }

    fn encode_signal(&self, data: &SignalData, buf: &mut [u8]) -> Result<usize, EncodeError> {
        const FIXED_SIZE: usize = 8 + 2 + 1 + 33 + 1 + 8; // 53 bytes
        let total_size = FIXED_SIZE + 4 + data.metadata.len();
        Self::check_size(buf, total_size)?;

        write_le::write_i64(&mut buf[0..], data.timestamp);
        write_le::write_u16(&mut buf[8..], data.signal_type);
        buf[10] = data.exchange.as_u8();
        Self::write_symbol(buf, 11, &data.symbol);
        buf[44] = data.side.as_u8();
        write_le::write_i64(&mut buf[45..], data.strength);
        Self::write_var_data(buf, FIXED_SIZE, &data.metadata)?;

        Ok(total_size)
    }

    fn encoded_size(&self, message: &DecodedMessage) -> usize {
        match message {
            DecodedMessage::SessionStart(d) => 17 + 4 + d.metadata.len(),
            DecodedMessage::SessionEnd(_) => 56,
            DecodedMessage::Checkpoint(_) => 112,
            DecodedMessage::RawFrame(d) => 13 + 4 + d.payload.len(),
            DecodedMessage::QuoteUpdate(_) => 74,
            DecodedMessage::Trade(d) => 59 + 4 + d.trade_id.len(),
            DecodedMessage::OrderSubmit(d) => 59 + 4 + d.client_order_id.len(),
            DecodedMessage::OrderAck(d) => {
                9 + 4 + d.client_order_id.len() + 4 + d.exchange_order_id.len()
            }
            DecodedMessage::OrderFill(d) => 33 + 4 + d.exchange_order_id.len(),
            DecodedMessage::StateChange(d) => 10 + 4 + d.state_data.len(),
            DecodedMessage::Signal(d) => 53 + 4 + d.metadata.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::traits::MessageDecoder;
    use crate::codec::v1_0::Decoder;

    fn make_encoder() -> Encoder {
        Encoder::new()
    }

    fn make_decoder() -> Decoder {
        Decoder::new()
    }

    // ==================== Schema Version Test ====================

    #[test]
    fn test_schema_version() {
        let encoder = make_encoder();
        assert_eq!(encoder.schema_version(), (1, 0));
    }

    // ==================== RawFrame Roundtrip Tests ====================

    #[test]
    fn test_raw_frame_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = RawFrameData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            sequence_number: 42,
            payload: b"test payload data".to_vec(),
        };

        let mut buf = vec![0u8; 256];
        let size = encoder.encode_raw_frame(&original, &mut buf).unwrap();
        let decoded = decoder.decode_raw_frame(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.sequence_number, original.sequence_number);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_raw_frame_empty_payload() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = RawFrameData {
            timestamp: 0,
            exchange: Exchange::Unknown,
            sequence_number: 0,
            payload: vec![],
        };

        let mut buf = vec![0u8; 64];
        let size = encoder.encode_raw_frame(&original, &mut buf).unwrap();
        let decoded = decoder.decode_raw_frame(&buf[..size]).unwrap();

        assert!(decoded.payload.is_empty());
    }

    // ==================== QuoteUpdate Roundtrip Tests ====================

    #[test]
    fn test_quote_update_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = QuoteUpdateData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Binance,
            symbol: Symbol::new("BTC-USDT"),
            bid_price: 50000_00000000,
            bid_size: 1_50000000,
            ask_price: 50001_00000000,
            ask_size: 2_25000000,
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_quote_update(&original, &mut buf).unwrap();
        let decoded = decoder.decode_quote_update(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.symbol, original.symbol);
        assert_eq!(decoded.bid_price, original.bid_price);
        assert_eq!(decoded.bid_size, original.bid_size);
        assert_eq!(decoded.ask_price, original.ask_price);
        assert_eq!(decoded.ask_size, original.ask_size);
    }

    // ==================== OrderSubmit Roundtrip Tests ====================

    #[test]
    fn test_order_submit_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = OrderSubmitData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            symbol: Symbol::new("ETH-PERP"),
            side: Side::Buy,
            price: 2500_00000000,
            size: 10_00000000,
            client_order_id: b"order-abc-123".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_order_submit(&original, &mut buf).unwrap();
        let decoded = decoder.decode_order_submit(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.symbol, original.symbol);
        assert_eq!(decoded.side, original.side);
        assert_eq!(decoded.price, original.price);
        assert_eq!(decoded.size, original.size);
        assert_eq!(decoded.client_order_id, original.client_order_id);
    }

    // ==================== SessionStart Roundtrip Tests ====================

    #[test]
    fn test_session_start_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = SessionStartData {
            timestamp: 1_704_067_200_000_000,
            session_id: 12345678901234,
            exchange: Exchange::Binance,
            metadata: b"session metadata".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_session_start(&original, &mut buf).unwrap();
        let decoded = decoder.decode_session_start(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.session_id, original.session_id);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.metadata, original.metadata);
    }

    // ==================== SessionEnd Roundtrip Tests ====================

    #[test]
    fn test_session_end_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = SessionEndData {
            timestamp: 1_704_067_200_000_000,
            session_id: 12345678901234,
            record_count: 50000,
            final_state_hash: Sha256::new([0xAB; 32]),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_session_end(&original, &mut buf).unwrap();
        let decoded = decoder.decode_session_end(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.session_id, original.session_id);
        assert_eq!(decoded.record_count, original.record_count);
        assert_eq!(decoded.final_state_hash, original.final_state_hash);
    }

    // ==================== Checkpoint Roundtrip Tests ====================

    #[test]
    fn test_checkpoint_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = CheckpointData {
            timestamp: 1_704_067_200_000_000,
            sequence_number: 10000,
            state_hash: Sha256::new([0x11; 32]),
            orderbook_hash: Sha256::new([0x22; 32]),
            position_hash: Sha256::new([0x33; 32]),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_checkpoint(&original, &mut buf).unwrap();
        let decoded = decoder.decode_checkpoint(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.sequence_number, original.sequence_number);
        assert_eq!(decoded.state_hash, original.state_hash);
        assert_eq!(decoded.orderbook_hash, original.orderbook_hash);
        assert_eq!(decoded.position_hash, original.position_hash);
    }

    // ==================== Trade Roundtrip Tests ====================

    #[test]
    fn test_trade_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = TradeData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::OKX,
            symbol: Symbol::new("SOL-USDT"),
            price: 100_50000000,
            size: 25_00000000,
            side: Side::Sell,
            trade_id: b"trade-xyz-789".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_trade(&original, &mut buf).unwrap();
        let decoded = decoder.decode_trade(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.symbol, original.symbol);
        assert_eq!(decoded.price, original.price);
        assert_eq!(decoded.size, original.size);
        assert_eq!(decoded.side, original.side);
        assert_eq!(decoded.trade_id, original.trade_id);
    }

    // ==================== OrderAck Roundtrip Tests ====================

    #[test]
    fn test_order_ack_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = OrderAckData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Bybit,
            client_order_id: b"client-order-123".to_vec(),
            exchange_order_id: b"exchange-order-456".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_order_ack(&original, &mut buf).unwrap();
        let decoded = decoder.decode_order_ack(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.client_order_id, original.client_order_id);
        assert_eq!(decoded.exchange_order_id, original.exchange_order_id);
    }

    // ==================== OrderFill Roundtrip Tests ====================

    #[test]
    fn test_order_fill_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = OrderFillData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            fill_price: 50000_00000000,
            fill_size: 1_00000000,
            remaining_size: 9_00000000,
            exchange_order_id: b"exch-fill-001".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_order_fill(&original, &mut buf).unwrap();
        let decoded = decoder.decode_order_fill(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.fill_price, original.fill_price);
        assert_eq!(decoded.fill_size, original.fill_size);
        assert_eq!(decoded.remaining_size, original.remaining_size);
        assert_eq!(decoded.exchange_order_id, original.exchange_order_id);
    }

    // ==================== StateChange Roundtrip Tests ====================

    #[test]
    fn test_state_change_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = StateChangeData {
            timestamp: 1_704_067_200_000_000,
            event_type: 0x0301,
            state_data: b"serialized state data".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_state_change(&original, &mut buf).unwrap();
        let decoded = decoder.decode_state_change(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.event_type, original.event_type);
        assert_eq!(decoded.state_data, original.state_data);
    }

    // ==================== Signal Roundtrip Tests ====================

    #[test]
    fn test_signal_roundtrip() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = SignalData {
            timestamp: 1_704_067_200_000_000,
            signal_type: 0x0001,
            exchange: Exchange::Binance,
            symbol: Symbol::new("ETH-BTC"),
            side: Side::Buy,
            strength: 85_00000000,
            metadata: b"signal metadata".to_vec(),
        };

        let mut buf = vec![0u8; 128];
        let size = encoder.encode_signal(&original, &mut buf).unwrap();
        let decoded = decoder.decode_signal(&buf[..size]).unwrap();

        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.signal_type, original.signal_type);
        assert_eq!(decoded.exchange, original.exchange);
        assert_eq!(decoded.symbol, original.symbol);
        assert_eq!(decoded.side, original.side);
        assert_eq!(decoded.strength, original.strength);
        assert_eq!(decoded.metadata, original.metadata);
    }

    // ==================== encoded_size Tests ====================

    #[test]
    fn test_encoded_size_raw_frame() {
        let encoder = make_encoder();
        let data = RawFrameData {
            timestamp: 0,
            exchange: Exchange::Unknown,
            sequence_number: 0,
            payload: b"payload".to_vec(),
        };
        let message = DecodedMessage::RawFrame(data.clone());

        let mut buf = vec![0u8; 256];
        let actual_size = encoder.encode_raw_frame(&data, &mut buf).unwrap();
        let predicted_size = encoder.encoded_size(&message);

        assert_eq!(actual_size, predicted_size);
    }

    #[test]
    fn test_encoded_size_all_types() {
        let encoder = make_encoder();

        // Test each message type
        let messages = vec![
            DecodedMessage::SessionStart(SessionStartData {
                timestamp: 0,
                session_id: 0,
                exchange: Exchange::Unknown,
                metadata: b"meta".to_vec(),
            }),
            DecodedMessage::SessionEnd(SessionEndData {
                timestamp: 0,
                session_id: 0,
                record_count: 0,
                final_state_hash: Sha256::zeroed(),
            }),
            DecodedMessage::Checkpoint(CheckpointData {
                timestamp: 0,
                sequence_number: 0,
                state_hash: Sha256::zeroed(),
                orderbook_hash: Sha256::zeroed(),
                position_hash: Sha256::zeroed(),
            }),
            DecodedMessage::RawFrame(RawFrameData {
                timestamp: 0,
                exchange: Exchange::Unknown,
                sequence_number: 0,
                payload: b"test".to_vec(),
            }),
            DecodedMessage::QuoteUpdate(QuoteUpdateData {
                timestamp: 0,
                exchange: Exchange::Unknown,
                symbol: Symbol::new("TEST"),
                bid_price: 0,
                bid_size: 0,
                ask_price: 0,
                ask_size: 0,
            }),
        ];

        let mut buf = vec![0u8; 512];
        for message in messages {
            let predicted = encoder.encoded_size(&message);
            let actual = encoder.encode(&message, &mut buf).unwrap();
            assert_eq!(actual, predicted, "Size mismatch for {:?}", message);
        }
    }

    // ==================== Error Tests ====================

    #[test]
    fn test_encode_buffer_too_small() {
        let encoder = make_encoder();
        let data = RawFrameData {
            timestamp: 0,
            exchange: Exchange::Unknown,
            sequence_number: 0,
            payload: b"large payload".to_vec(),
        };

        let mut buf = vec![0u8; 10]; // Too small
        let result = encoder.encode_raw_frame(&data, &mut buf);
        assert!(matches!(result, Err(EncodeError::BufferTooSmall { .. })));
    }

    // ==================== Generic Encode Tests ====================

    #[test]
    fn test_encode_generic() {
        let encoder = make_encoder();
        let decoder = make_decoder();

        let original = DecodedMessage::RawFrame(RawFrameData {
            timestamp: 12345,
            exchange: Exchange::Deribit,
            sequence_number: 99,
            payload: b"generic test".to_vec(),
        });

        let mut buf = vec![0u8; 256];
        let size = encoder.encode(&original, &mut buf).unwrap();
        let decoded = decoder.decode_by_id(256, &buf[..size]).unwrap();

        assert_eq!(decoded.timestamp(), 12345);
    }
}
