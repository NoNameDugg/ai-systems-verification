//! Schema version 1.0 message decoder.
//!
//! This decoder implements the binary layout defined in blackbox-v1.0.xml.
//! All fields are little-endian per the format spec, section 2.1.

use crate::codec::data::*;
use crate::codec::error::DecodeError;
use crate::codec::traits::{read_le, MessageDecoder};

/// Decoder for SBE schema version 1.0.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decoder;

impl Decoder {
    /// Create a new v1.0 decoder.
    pub fn new() -> Self {
        Self
    }

    /// Check buffer size and return error if too small.
    #[inline]
    fn check_size(buf: &[u8], expected: usize) -> Result<(), DecodeError> {
        if buf.len() < expected {
            return Err(DecodeError::BufferTooSmall {
                expected,
                available: buf.len(),
            });
        }
        Ok(())
    }

    /// Read a Symbol from buffer at given offset.
    #[inline]
    fn read_symbol(buf: &[u8], offset: usize) -> Symbol {
        let mut data = [0u8; MAX_SYMBOL_LENGTH];
        data.copy_from_slice(&buf[offset..offset + MAX_SYMBOL_LENGTH]);
        let length = buf[offset + MAX_SYMBOL_LENGTH];
        Symbol::from_raw(data, length)
    }

    /// Read a Sha256 from buffer at given offset.
    #[inline]
    fn read_sha256(buf: &[u8], offset: usize) -> Sha256 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&buf[offset..offset + 32]);
        Sha256::new(bytes)
    }

    /// Read variable-length data from buffer.
    /// Returns (data, bytes_consumed).
    #[inline]
    fn read_var_data(buf: &[u8], offset: usize) -> Result<(Vec<u8>, usize), DecodeError> {
        if buf.len() < offset + 4 {
            return Err(DecodeError::BufferTooSmall {
                expected: offset + 4,
                available: buf.len(),
            });
        }
        let length = read_le::read_u32(&buf[offset..]) as usize;
        if buf.len() < offset + 4 + length {
            return Err(DecodeError::BufferTooSmall {
                expected: offset + 4 + length,
                available: buf.len(),
            });
        }
        Ok((buf[offset + 4..offset + 4 + length].to_vec(), 4 + length))
    }

    /// Read Exchange enum from u8.
    #[inline]
    fn read_exchange(value: u8) -> Result<Exchange, DecodeError> {
        Exchange::from_u8(value).ok_or(DecodeError::InvalidEnumValue {
            enum_type: "Exchange",
            value: value as u64,
        })
    }

    /// Read Side enum from u8.
    #[inline]
    fn read_side(value: u8) -> Result<Side, DecodeError> {
        Side::from_u8(value).ok_or(DecodeError::InvalidEnumValue {
            enum_type: "Side",
            value: value as u64,
        })
    }
}

impl MessageDecoder for Decoder {
    fn schema_version(&self) -> (u8, u8) {
        (1, 0)
    }

    /// Decode SessionStart message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - sessionId: u64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - metadata: varData (4 bytes length + data)
    fn decode_session_start(&self, buf: &[u8]) -> Result<SessionStartData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 1; // 17 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let session_id = read_le::read_u64(&buf[8..]);
        let exchange = Self::read_exchange(buf[16])?;
        let (metadata, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(SessionStartData {
            timestamp,
            session_id,
            exchange,
            metadata,
        })
    }

    /// Decode SessionEnd message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - sessionId: u64 (8 bytes)
    /// - recordCount: u64 (8 bytes)
    /// - finalStateHash: sha256 (32 bytes)
    fn decode_session_end(&self, buf: &[u8]) -> Result<SessionEndData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 8 + 32; // 56 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let session_id = read_le::read_u64(&buf[8..]);
        let record_count = read_le::read_u64(&buf[16..]);
        let final_state_hash = Self::read_sha256(buf, 24);

        Ok(SessionEndData {
            timestamp,
            session_id,
            record_count,
            final_state_hash,
        })
    }

    /// Decode Checkpoint message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - sequenceNumber: u64 (8 bytes)
    /// - stateHash: sha256 (32 bytes)
    /// - orderbookHash: sha256 (32 bytes)
    /// - positionHash: sha256 (32 bytes)
    fn decode_checkpoint(&self, buf: &[u8]) -> Result<CheckpointData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 8 + 32 + 32 + 32; // 112 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let sequence_number = read_le::read_u64(&buf[8..]);
        let state_hash = Self::read_sha256(buf, 16);
        let orderbook_hash = Self::read_sha256(buf, 48);
        let position_hash = Self::read_sha256(buf, 80);

        Ok(CheckpointData {
            timestamp,
            sequence_number,
            state_hash,
            orderbook_hash,
            position_hash,
        })
    }

    /// Decode RawFrame message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - sequenceNumber: u32 (4 bytes)
    /// - payload: varData (4 bytes length + data)
    fn decode_raw_frame(&self, buf: &[u8]) -> Result<RawFrameData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 4; // 13 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let sequence_number = read_le::read_u32(&buf[9..]);
        let (payload, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(RawFrameData {
            timestamp,
            exchange,
            sequence_number,
            payload,
        })
    }

    /// Decode QuoteUpdate message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - symbol: symbol (33 bytes)
    /// - bidPrice: i64 (8 bytes)
    /// - bidSize: i64 (8 bytes)
    /// - askPrice: i64 (8 bytes)
    /// - askSize: i64 (8 bytes)
    fn decode_quote_update(&self, buf: &[u8]) -> Result<QuoteUpdateData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 8 + 8 + 8 + 8; // 74 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let symbol = Self::read_symbol(buf, 9);
        let bid_price = read_le::read_i64(&buf[42..]);
        let bid_size = read_le::read_i64(&buf[50..]);
        let ask_price = read_le::read_i64(&buf[58..]);
        let ask_size = read_le::read_i64(&buf[66..]);

        Ok(QuoteUpdateData {
            timestamp,
            exchange,
            symbol,
            bid_price,
            bid_size,
            ask_price,
            ask_size,
        })
    }

    /// Decode Trade message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - symbol: symbol (33 bytes)
    /// - price: i64 (8 bytes)
    /// - size: i64 (8 bytes)
    /// - side: u8 (1 byte)
    /// - tradeId: varData (4 bytes length + data)
    fn decode_trade(&self, buf: &[u8]) -> Result<TradeData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 8 + 8 + 1; // 59 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let symbol = Self::read_symbol(buf, 9);
        let price = read_le::read_i64(&buf[42..]);
        let size = read_le::read_i64(&buf[50..]);
        let side = Self::read_side(buf[58])?;
        let (trade_id, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(TradeData {
            timestamp,
            exchange,
            symbol,
            price,
            size,
            side,
            trade_id,
        })
    }

    /// Decode OrderSubmit message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - symbol: symbol (33 bytes)
    /// - side: u8 (1 byte)
    /// - price: i64 (8 bytes)
    /// - size: i64 (8 bytes)
    /// - clientOrderId: varData (4 bytes length + data)
    fn decode_order_submit(&self, buf: &[u8]) -> Result<OrderSubmitData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 33 + 1 + 8 + 8; // 59 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let symbol = Self::read_symbol(buf, 9);
        let side = Self::read_side(buf[42])?;
        let price = read_le::read_i64(&buf[43..]);
        let size = read_le::read_i64(&buf[51..]);
        let (client_order_id, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(OrderSubmitData {
            timestamp,
            exchange,
            symbol,
            side,
            price,
            size,
            client_order_id,
        })
    }

    /// Decode OrderAck message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - clientOrderId: varData (4 bytes length + data)
    /// - exchangeOrderId: varData (4 bytes length + data)
    fn decode_order_ack(&self, buf: &[u8]) -> Result<OrderAckData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1; // 9 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let (client_order_id, consumed1) = Self::read_var_data(buf, FIXED_SIZE)?;
        let (exchange_order_id, _) = Self::read_var_data(buf, FIXED_SIZE + consumed1)?;

        Ok(OrderAckData {
            timestamp,
            exchange,
            client_order_id,
            exchange_order_id,
        })
    }

    /// Decode OrderFill message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - exchange: u8 (1 byte)
    /// - fillPrice: i64 (8 bytes)
    /// - fillSize: i64 (8 bytes)
    /// - remainingSize: i64 (8 bytes)
    /// - exchangeOrderId: varData (4 bytes length + data)
    fn decode_order_fill(&self, buf: &[u8]) -> Result<OrderFillData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 1 + 8 + 8 + 8; // 33 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let exchange = Self::read_exchange(buf[8])?;
        let fill_price = read_le::read_i64(&buf[9..]);
        let fill_size = read_le::read_i64(&buf[17..]);
        let remaining_size = read_le::read_i64(&buf[25..]);
        let (exchange_order_id, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(OrderFillData {
            timestamp,
            exchange,
            fill_price,
            fill_size,
            remaining_size,
            exchange_order_id,
        })
    }

    /// Decode StateChange message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - eventType: u16 (2 bytes)
    /// - stateData: varData (4 bytes length + data)
    fn decode_state_change(&self, buf: &[u8]) -> Result<StateChangeData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 2; // 10 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let event_type = read_le::read_u16(&buf[8..]);
        let (state_data, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(StateChangeData {
            timestamp,
            event_type,
            state_data,
        })
    }

    /// Decode Signal message.
    ///
    /// Layout:
    /// - timestamp: i64 (8 bytes)
    /// - signalType: u16 (2 bytes)
    /// - exchange: u8 (1 byte)
    /// - symbol: symbol (33 bytes)
    /// - side: u8 (1 byte)
    /// - strength: i64 (8 bytes)
    /// - metadata: varData (4 bytes length + data)
    fn decode_signal(&self, buf: &[u8]) -> Result<SignalData, DecodeError> {
        const FIXED_SIZE: usize = 8 + 2 + 1 + 33 + 1 + 8; // 53 bytes
        Self::check_size(buf, FIXED_SIZE)?;

        let timestamp = read_le::read_i64(&buf[0..]);
        let signal_type = read_le::read_u16(&buf[8..]);
        let exchange = Self::read_exchange(buf[10])?;
        let symbol = Self::read_symbol(buf, 11);
        let side = Self::read_side(buf[44])?;
        let strength = read_le::read_i64(&buf[45..]);
        let (metadata, _) = Self::read_var_data(buf, FIXED_SIZE)?;

        Ok(SignalData {
            timestamp,
            signal_type,
            exchange,
            symbol,
            side,
            strength,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::traits::write_le;

    fn make_decoder() -> Decoder {
        Decoder::new()
    }

    // ==================== Schema Version Test ====================

    #[test]
    fn test_schema_version() {
        let decoder = make_decoder();
        assert_eq!(decoder.schema_version(), (1, 0));
    }

    // ==================== RawFrame Tests ====================

    #[test]
    fn test_decode_raw_frame_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::Deribit.as_u8();
        // sequenceNumber: u32
        write_le::write_u32(&mut buf[9..], 42);
        // payload length: u32
        write_le::write_u32(&mut buf[13..], 12);
        // payload data
        buf[17..29].copy_from_slice(b"test payload");

        let result = decoder.decode_raw_frame(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.exchange, Exchange::Deribit);
        assert_eq!(result.sequence_number, 42);
        assert_eq!(result.payload, b"test payload");
    }

    #[test]
    fn test_decode_raw_frame_buffer_too_small() {
        let decoder = make_decoder();
        let buf = vec![0u8; 10]; // Too small

        let result = decoder.decode_raw_frame(&buf);
        assert!(matches!(result, Err(DecodeError::BufferTooSmall { .. })));
    }

    #[test]
    fn test_decode_raw_frame_invalid_exchange() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        write_le::write_i64(&mut buf[0..], 0);
        buf[8] = 99; // Invalid exchange
        write_le::write_u32(&mut buf[9..], 0);
        write_le::write_u32(&mut buf[13..], 0);

        let result = decoder.decode_raw_frame(&buf);
        assert!(matches!(result, Err(DecodeError::InvalidEnumValue { .. })));
    }

    // ==================== QuoteUpdate Tests ====================

    #[test]
    fn test_decode_quote_update_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::Binance.as_u8();
        // symbol: 32 bytes data + 1 byte length
        buf[9..17].copy_from_slice(b"BTC-USDT");
        buf[9 + 32] = 8; // length
                         // prices and sizes
        write_le::write_i64(&mut buf[42..], 50000_00000000);
        write_le::write_i64(&mut buf[50..], 1_00000000);
        write_le::write_i64(&mut buf[58..], 50001_00000000);
        write_le::write_i64(&mut buf[66..], 2_00000000);

        let result = decoder.decode_quote_update(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.exchange, Exchange::Binance);
        assert_eq!(result.symbol.as_str(), "BTC-USDT");
        assert_eq!(result.bid_price, 50000_00000000);
        assert_eq!(result.ask_price, 50001_00000000);
    }

    // ==================== OrderSubmit Tests ====================

    #[test]
    fn test_decode_order_submit_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::Deribit.as_u8();
        // symbol: 32 bytes data + 1 byte length
        buf[9..17].copy_from_slice(b"ETH-PERP");
        buf[9 + 32] = 8; // length
                         // side: u8
        buf[42] = Side::Buy.as_u8();
        // price: i64
        write_le::write_i64(&mut buf[43..], 2500_00000000);
        // size: i64
        write_le::write_i64(&mut buf[51..], 10_00000000);
        // clientOrderId length
        write_le::write_u32(&mut buf[59..], 9);
        // clientOrderId data
        buf[63..72].copy_from_slice(b"order-123");

        let result = decoder.decode_order_submit(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.exchange, Exchange::Deribit);
        assert_eq!(result.symbol.as_str(), "ETH-PERP");
        assert_eq!(result.side, Side::Buy);
        assert_eq!(result.price, 2500_00000000);
        assert_eq!(result.size, 10_00000000);
        assert_eq!(result.client_order_id, b"order-123");
    }

    #[test]
    fn test_decode_order_submit_invalid_side() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        write_le::write_i64(&mut buf[0..], 0);
        buf[8] = Exchange::Unknown.as_u8();
        buf[9 + 32] = 0;
        buf[42] = 99; // Invalid side
        write_le::write_u32(&mut buf[59..], 0);

        let result = decoder.decode_order_submit(&buf);
        assert!(matches!(result, Err(DecodeError::InvalidEnumValue { .. })));
    }

    // ==================== SessionStart Tests ====================

    #[test]
    fn test_decode_session_start_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // sessionId: u64
        write_le::write_u64(&mut buf[8..], 12345678);
        // exchange: u8
        buf[16] = Exchange::Binance.as_u8();
        // metadata length
        write_le::write_u32(&mut buf[17..], 5);
        // metadata
        buf[21..26].copy_from_slice(b"hello");

        let result = decoder.decode_session_start(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.session_id, 12345678);
        assert_eq!(result.exchange, Exchange::Binance);
        assert_eq!(result.metadata, b"hello");
    }

    // ==================== SessionEnd Tests ====================

    #[test]
    fn test_decode_session_end_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // sessionId: u64
        write_le::write_u64(&mut buf[8..], 12345678);
        // recordCount: u64
        write_le::write_u64(&mut buf[16..], 1000);
        // finalStateHash: 32 bytes
        buf[24..56].fill(0xAB);

        let result = decoder.decode_session_end(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.session_id, 12345678);
        assert_eq!(result.record_count, 1000);
        assert_eq!(result.final_state_hash.as_bytes()[0], 0xAB);
    }

    // ==================== Checkpoint Tests ====================

    #[test]
    fn test_decode_checkpoint_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // sequenceNumber: u64
        write_le::write_u64(&mut buf[8..], 500);
        // stateHash: 32 bytes
        buf[16..48].fill(0x11);
        // orderbookHash: 32 bytes
        buf[48..80].fill(0x22);
        // positionHash: 32 bytes
        buf[80..112].fill(0x33);

        let result = decoder.decode_checkpoint(&buf).unwrap();
        assert_eq!(result.timestamp, 1_704_067_200_000_000);
        assert_eq!(result.sequence_number, 500);
        assert_eq!(result.state_hash.as_bytes()[0], 0x11);
        assert_eq!(result.orderbook_hash.as_bytes()[0], 0x22);
        assert_eq!(result.position_hash.as_bytes()[0], 0x33);
    }

    // ==================== Trade Tests ====================

    #[test]
    fn test_decode_trade_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::OKX.as_u8();
        // symbol: 32 bytes data + 1 byte length
        buf[9..16].copy_from_slice(b"SOL-USD");
        buf[9 + 32] = 7;
        // price: i64
        write_le::write_i64(&mut buf[42..], 100_00000000);
        // size: i64
        write_le::write_i64(&mut buf[50..], 5_00000000);
        // side: u8
        buf[58] = Side::Sell.as_u8();
        // tradeId length
        write_le::write_u32(&mut buf[59..], 7);
        // tradeId
        buf[63..70].copy_from_slice(b"trade01");

        let result = decoder.decode_trade(&buf).unwrap();
        assert_eq!(result.exchange, Exchange::OKX);
        assert_eq!(result.symbol.as_str(), "SOL-USD");
        assert_eq!(result.price, 100_00000000);
        assert_eq!(result.size, 5_00000000);
        assert_eq!(result.side, Side::Sell);
        assert_eq!(result.trade_id, b"trade01");
    }

    // ==================== OrderAck Tests ====================

    #[test]
    fn test_decode_order_ack_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::Bybit.as_u8();
        // clientOrderId length
        write_le::write_u32(&mut buf[9..], 6);
        // clientOrderId
        buf[13..19].copy_from_slice(b"cli123");
        // exchangeOrderId length
        write_le::write_u32(&mut buf[19..], 8);
        // exchangeOrderId
        buf[23..31].copy_from_slice(b"exch5678");

        let result = decoder.decode_order_ack(&buf).unwrap();
        assert_eq!(result.exchange, Exchange::Bybit);
        assert_eq!(result.client_order_id, b"cli123");
        assert_eq!(result.exchange_order_id, b"exch5678");
    }

    // ==================== OrderFill Tests ====================

    #[test]
    fn test_decode_order_fill_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // exchange: u8
        buf[8] = Exchange::Deribit.as_u8();
        // fillPrice: i64
        write_le::write_i64(&mut buf[9..], 50000_00000000);
        // fillSize: i64
        write_le::write_i64(&mut buf[17..], 1_00000000);
        // remainingSize: i64
        write_le::write_i64(&mut buf[25..], 9_00000000);
        // exchangeOrderId length
        write_le::write_u32(&mut buf[33..], 4);
        // exchangeOrderId
        buf[37..41].copy_from_slice(b"ex01");

        let result = decoder.decode_order_fill(&buf).unwrap();
        assert_eq!(result.fill_price, 50000_00000000);
        assert_eq!(result.fill_size, 1_00000000);
        assert_eq!(result.remaining_size, 9_00000000);
        assert_eq!(result.exchange_order_id, b"ex01");
    }

    // ==================== StateChange Tests ====================

    #[test]
    fn test_decode_state_change_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // eventType: u16
        write_le::write_u16(&mut buf[8..], 0x0301);
        // stateData length
        write_le::write_u32(&mut buf[10..], 10);
        // stateData
        buf[14..24].copy_from_slice(b"state_data");

        let result = decoder.decode_state_change(&buf).unwrap();
        assert_eq!(result.event_type, 0x0301);
        assert_eq!(result.state_data, b"state_data");
    }

    // ==================== Signal Tests ====================

    #[test]
    fn test_decode_signal_success() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 128];

        // timestamp: i64
        write_le::write_i64(&mut buf[0..], 1_704_067_200_000_000);
        // signalType: u16
        write_le::write_u16(&mut buf[8..], 0x0001);
        // exchange: u8
        buf[10] = Exchange::Binance.as_u8();
        // symbol: 32 bytes data + 1 byte length
        buf[11..18].copy_from_slice(b"BTC-USD");
        buf[11 + 32] = 7;
        // side: u8
        buf[44] = Side::Buy.as_u8();
        // strength: i64
        write_le::write_i64(&mut buf[45..], 85_00000000);
        // metadata length
        write_le::write_u32(&mut buf[53..], 4);
        // metadata
        buf[57..61].copy_from_slice(b"meta");

        let result = decoder.decode_signal(&buf).unwrap();
        assert_eq!(result.signal_type, 0x0001);
        assert_eq!(result.exchange, Exchange::Binance);
        assert_eq!(result.symbol.as_str(), "BTC-USD");
        assert_eq!(result.side, Side::Buy);
        assert_eq!(result.strength, 85_00000000);
        assert_eq!(result.metadata, b"meta");
    }

    // ==================== decode_by_id Tests ====================

    #[test]
    fn test_decode_by_id_raw_frame() {
        let decoder = make_decoder();
        let mut buf = vec![0u8; 64];

        write_le::write_i64(&mut buf[0..], 12345);
        buf[8] = Exchange::Unknown.as_u8();
        write_le::write_u32(&mut buf[9..], 1);
        write_le::write_u32(&mut buf[13..], 0);

        let result = decoder.decode_by_id(256, &buf).unwrap();
        assert!(matches!(result, DecodedMessage::RawFrame(_)));
        assert_eq!(result.timestamp(), 12345);
        assert_eq!(result.message_id(), 256);
    }

    #[test]
    fn test_decode_by_id_invalid() {
        let decoder = make_decoder();
        let buf = vec![0u8; 64];

        let result = decoder.decode_by_id(0x9999, &buf);
        assert!(matches!(
            result,
            Err(DecodeError::InvalidMessageType(0x9999))
        ));
    }
}
