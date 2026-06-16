//! Version-independent data structures for decoded messages.
//!
//! These structures represent the logical content of SBE messages
//! and are independent of the wire format. Decoders for all schema
//! versions produce these same types.

use std::fmt;

/// Maximum length for symbol strings.
pub const MAX_SYMBOL_LENGTH: usize = 32;

/// Maximum length for variable-length data fields.
pub const MAX_VAR_DATA_LENGTH: usize = 64 * 1024; // 64KB

/// Fixed-length symbol with embedded length.
///
/// Matches the SBE schema `symbol` composite type:
/// - 32 bytes of character data (right-padded with zeros)
/// - 1 byte length indicator
#[derive(Clone, Copy)]
pub struct Symbol {
    /// Symbol data (null-padded).
    data: [u8; MAX_SYMBOL_LENGTH],
    /// Actual length of the symbol.
    length: u8,
}

impl Symbol {
    /// Create a new symbol from a string.
    ///
    /// Truncates to 32 characters if longer.
    pub fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_SYMBOL_LENGTH);
        let mut data = [0u8; MAX_SYMBOL_LENGTH];
        data[..len].copy_from_slice(&bytes[..len]);
        Self {
            data,
            length: len as u8,
        }
    }

    /// Create a symbol from raw bytes and length.
    pub fn from_raw(data: [u8; MAX_SYMBOL_LENGTH], length: u8) -> Self {
        Self { data, length }
    }

    /// Get the symbol as a string slice.
    pub fn as_str(&self) -> &str {
        // Safe because we only store valid UTF-8 or truncate to valid boundary
        let len = self.length as usize;
        std::str::from_utf8(&self.data[..len]).unwrap_or("")
    }

    /// Get the raw data bytes.
    pub fn raw_data(&self) -> &[u8; MAX_SYMBOL_LENGTH] {
        &self.data
    }

    /// Get the length.
    pub fn len(&self) -> usize {
        self.length as usize
    }

    /// Check if the symbol is empty.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl Default for Symbol {
    fn default() -> Self {
        Self {
            data: [0u8; MAX_SYMBOL_LENGTH],
            length: 0,
        }
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol(\"{}\")", self.as_str())
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Symbol {}

impl From<&str> for Symbol {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Symbol {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

/// Exchange identifier.
///
/// Maps to the SBE `Exchange` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Exchange {
    /// Unknown or unspecified exchange.
    #[default]
    Unknown = 0,
    /// Deribit exchange.
    Deribit = 1,
    /// Binance exchange.
    Binance = 2,
    /// Bybit exchange.
    Bybit = 3,
    /// OKX exchange.
    OKX = 4,
}

impl Exchange {
    /// Convert from u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Deribit),
            2 => Some(Self::Binance),
            3 => Some(Self::Bybit),
            4 => Some(Self::OKX),
            _ => None,
        }
    }

    /// Convert to u8 value.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::Deribit => write!(f, "Deribit"),
            Self::Binance => write!(f, "Binance"),
            Self::Bybit => write!(f, "Bybit"),
            Self::OKX => write!(f, "OKX"),
        }
    }
}

/// Order side.
///
/// Maps to the SBE `Side` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Side {
    /// Buy side.
    #[default]
    Buy = 0,
    /// Sell side.
    Sell = 1,
}

impl Side {
    /// Convert from u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Buy),
            1 => Some(Self::Sell),
            _ => None,
        }
    }

    /// Convert to u8 value.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "Buy"),
            Self::Sell => write!(f, "Sell"),
        }
    }
}

/// SHA-256 hash (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Sha256(pub [u8; 32]);

impl Sha256 {
    /// Create a new SHA-256 hash from bytes.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create a zeroed hash.
    pub fn zeroed() -> Self {
        Self([0u8; 32])
    }

    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256(")?;
        for byte in &self.0[..4] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...)")
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

// ==================== Message Data Structures ====================

/// Session start message data (ID: 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Session identifier.
    pub session_id: u64,
    /// Exchange.
    pub exchange: Exchange,
    /// Optional metadata (JSON or custom format).
    pub metadata: Vec<u8>,
}

/// Session end message data (ID: 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEndData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Session identifier.
    pub session_id: u64,
    /// Total records in session.
    pub record_count: u64,
    /// Final state hash.
    pub final_state_hash: Sha256,
}

/// Checkpoint message data (ID: 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Sequence number.
    pub sequence_number: u64,
    /// Overall state hash.
    pub state_hash: Sha256,
    /// Order book hash.
    pub orderbook_hash: Sha256,
    /// Position hash.
    pub position_hash: Sha256,
}

/// Raw WebSocket frame data (ID: 256).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrameData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Source exchange.
    pub exchange: Exchange,
    /// Sequence number within session.
    pub sequence_number: u32,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
}

/// Quote update data (ID: 257).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteUpdateData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Source exchange.
    pub exchange: Exchange,
    /// Trading symbol.
    pub symbol: Symbol,
    /// Best bid price (fixed-point, e.g., price * 1e8).
    pub bid_price: i64,
    /// Best bid size (fixed-point).
    pub bid_size: i64,
    /// Best ask price (fixed-point).
    pub ask_price: i64,
    /// Best ask size (fixed-point).
    pub ask_size: i64,
}

/// Trade data (ID: 258).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Source exchange.
    pub exchange: Exchange,
    /// Trading symbol.
    pub symbol: Symbol,
    /// Trade price (fixed-point).
    pub price: i64,
    /// Trade size (fixed-point).
    pub size: i64,
    /// Trade side (aggressor side).
    pub side: Side,
    /// Exchange trade ID.
    pub trade_id: Vec<u8>,
}

/// Order submit data (ID: 512).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSubmitData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Target exchange.
    pub exchange: Exchange,
    /// Trading symbol.
    pub symbol: Symbol,
    /// Order side.
    pub side: Side,
    /// Order price (fixed-point).
    pub price: i64,
    /// Order size (fixed-point).
    pub size: i64,
    /// Client order ID.
    pub client_order_id: Vec<u8>,
}

/// Order acknowledgment data (ID: 513).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderAckData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Exchange.
    pub exchange: Exchange,
    /// Client order ID.
    pub client_order_id: Vec<u8>,
    /// Exchange order ID.
    pub exchange_order_id: Vec<u8>,
}

/// Order fill data (ID: 514).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFillData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Exchange.
    pub exchange: Exchange,
    /// Fill price (fixed-point).
    pub fill_price: i64,
    /// Fill size (fixed-point).
    pub fill_size: i64,
    /// Remaining size (fixed-point).
    pub remaining_size: i64,
    /// Exchange order ID.
    pub exchange_order_id: Vec<u8>,
}

/// State change data (ID: 768).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChangeData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Event type identifier.
    pub event_type: u16,
    /// Serialized state data.
    pub state_data: Vec<u8>,
}

/// Signal data (ID: 769).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalData {
    /// Timestamp in microseconds since epoch.
    pub timestamp: i64,
    /// Signal type identifier.
    pub signal_type: u16,
    /// Target exchange.
    pub exchange: Exchange,
    /// Trading symbol.
    pub symbol: Symbol,
    /// Signal side.
    pub side: Side,
    /// Signal strength (fixed-point).
    pub strength: i64,
    /// Additional metadata.
    pub metadata: Vec<u8>,
}

/// Union of all decoded message types.
#[derive(Debug, Clone)]
pub enum DecodedMessage {
    /// Session start marker.
    SessionStart(SessionStartData),
    /// Session end marker.
    SessionEnd(SessionEndData),
    /// Checkpoint.
    Checkpoint(CheckpointData),
    /// Raw WebSocket frame.
    RawFrame(RawFrameData),
    /// Quote update.
    QuoteUpdate(QuoteUpdateData),
    /// Trade.
    Trade(TradeData),
    /// Order submission.
    OrderSubmit(OrderSubmitData),
    /// Order acknowledgment.
    OrderAck(OrderAckData),
    /// Order fill.
    OrderFill(OrderFillData),
    /// State change.
    StateChange(StateChangeData),
    /// Trading signal.
    Signal(SignalData),
}

impl DecodedMessage {
    /// Get the timestamp from any message type.
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::SessionStart(d) => d.timestamp,
            Self::SessionEnd(d) => d.timestamp,
            Self::Checkpoint(d) => d.timestamp,
            Self::RawFrame(d) => d.timestamp,
            Self::QuoteUpdate(d) => d.timestamp,
            Self::Trade(d) => d.timestamp,
            Self::OrderSubmit(d) => d.timestamp,
            Self::OrderAck(d) => d.timestamp,
            Self::OrderFill(d) => d.timestamp,
            Self::StateChange(d) => d.timestamp,
            Self::Signal(d) => d.timestamp,
        }
    }

    /// Get the SBE message ID for this message type.
    pub fn message_id(&self) -> u16 {
        match self {
            Self::SessionStart(_) => 1,
            Self::SessionEnd(_) => 2,
            Self::Checkpoint(_) => 3,
            Self::RawFrame(_) => 256,
            Self::QuoteUpdate(_) => 257,
            Self::Trade(_) => 258,
            Self::OrderSubmit(_) => 512,
            Self::OrderAck(_) => 513,
            Self::OrderFill(_) => 514,
            Self::StateChange(_) => 768,
            Self::Signal(_) => 769,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Symbol Tests ====================

    #[test]
    fn test_symbol_new() {
        let sym = Symbol::new("BTC-USD");
        assert_eq!(sym.as_str(), "BTC-USD");
        assert_eq!(sym.len(), 7);
        assert!(!sym.is_empty());
    }

    #[test]
    fn test_symbol_empty() {
        let sym = Symbol::default();
        assert_eq!(sym.as_str(), "");
        assert_eq!(sym.len(), 0);
        assert!(sym.is_empty());
    }

    #[test]
    fn test_symbol_truncation() {
        let long_name = "THIS_IS_A_VERY_LONG_SYMBOL_NAME_THAT_EXCEEDS_32_CHARS";
        let sym = Symbol::new(long_name);
        assert_eq!(sym.len(), MAX_SYMBOL_LENGTH);
        assert!(sym.as_str().len() <= MAX_SYMBOL_LENGTH);
    }

    #[test]
    fn test_symbol_from_str() {
        let sym: Symbol = "ETH-USD".into();
        assert_eq!(sym.as_str(), "ETH-USD");
    }

    #[test]
    fn test_symbol_equality() {
        let s1 = Symbol::new("BTC-USD");
        let s2 = Symbol::new("BTC-USD");
        let s3 = Symbol::new("ETH-USD");
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_symbol_display() {
        let sym = Symbol::new("SOL-USD");
        assert_eq!(format!("{}", sym), "SOL-USD");
    }

    #[test]
    fn test_symbol_from_raw() {
        let mut data = [0u8; MAX_SYMBOL_LENGTH];
        data[0..6].copy_from_slice(b"XRP-BT");
        let sym = Symbol::from_raw(data, 6);
        assert_eq!(sym.as_str(), "XRP-BT");
    }

    // ==================== Exchange Tests ====================

    #[test]
    fn test_exchange_from_u8() {
        assert_eq!(Exchange::from_u8(0), Some(Exchange::Unknown));
        assert_eq!(Exchange::from_u8(1), Some(Exchange::Deribit));
        assert_eq!(Exchange::from_u8(2), Some(Exchange::Binance));
        assert_eq!(Exchange::from_u8(3), Some(Exchange::Bybit));
        assert_eq!(Exchange::from_u8(4), Some(Exchange::OKX));
        assert_eq!(Exchange::from_u8(5), None);
        assert_eq!(Exchange::from_u8(255), None);
    }

    #[test]
    fn test_exchange_roundtrip() {
        for ex in [
            Exchange::Unknown,
            Exchange::Deribit,
            Exchange::Binance,
            Exchange::Bybit,
            Exchange::OKX,
        ] {
            assert_eq!(Exchange::from_u8(ex.as_u8()), Some(ex));
        }
    }

    #[test]
    fn test_exchange_display() {
        assert_eq!(format!("{}", Exchange::Deribit), "Deribit");
        assert_eq!(format!("{}", Exchange::Binance), "Binance");
    }

    // ==================== Side Tests ====================

    #[test]
    fn test_side_from_u8() {
        assert_eq!(Side::from_u8(0), Some(Side::Buy));
        assert_eq!(Side::from_u8(1), Some(Side::Sell));
        assert_eq!(Side::from_u8(2), None);
    }

    #[test]
    fn test_side_roundtrip() {
        assert_eq!(Side::from_u8(Side::Buy.as_u8()), Some(Side::Buy));
        assert_eq!(Side::from_u8(Side::Sell.as_u8()), Some(Side::Sell));
    }

    #[test]
    fn test_side_display() {
        assert_eq!(format!("{}", Side::Buy), "Buy");
        assert_eq!(format!("{}", Side::Sell), "Sell");
    }

    // ==================== Sha256 Tests ====================

    #[test]
    fn test_sha256_new() {
        let bytes = [0xABu8; 32];
        let hash = Sha256::new(bytes);
        assert_eq!(hash.as_bytes(), &bytes);
    }

    #[test]
    fn test_sha256_zeroed() {
        let hash = Sha256::zeroed();
        assert_eq!(hash.as_bytes(), &[0u8; 32]);
    }

    #[test]
    fn test_sha256_display() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xDE;
        bytes[1] = 0xAD;
        bytes[2] = 0xBE;
        bytes[3] = 0xEF;
        let hash = Sha256::new(bytes);
        let display = format!("{}", hash);
        assert!(display.starts_with("deadbeef"));
        assert_eq!(display.len(), 64); // 32 bytes * 2 hex chars
    }

    // ==================== Data Structure Tests ====================

    #[test]
    fn test_raw_frame_data() {
        let data = RawFrameData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            sequence_number: 42,
            payload: b"test payload".to_vec(),
        };
        assert_eq!(data.timestamp, 1_704_067_200_000_000);
        assert_eq!(data.exchange, Exchange::Deribit);
        assert_eq!(data.sequence_number, 42);
        assert_eq!(data.payload, b"test payload");
    }

    #[test]
    fn test_quote_update_data() {
        let data = QuoteUpdateData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Binance,
            symbol: Symbol::new("BTC-USDT"),
            bid_price: 50000_00000000,
            bid_size: 1_00000000,
            ask_price: 50001_00000000,
            ask_size: 2_00000000,
        };
        assert_eq!(data.symbol.as_str(), "BTC-USDT");
        assert_eq!(data.bid_price, 50000_00000000);
    }

    #[test]
    fn test_order_submit_data() {
        let data = OrderSubmitData {
            timestamp: 1_704_067_200_000_000,
            exchange: Exchange::Deribit,
            symbol: Symbol::new("ETH-PERP"),
            side: Side::Buy,
            price: 2500_00000000,
            size: 10_00000000,
            client_order_id: b"order-123".to_vec(),
        };
        assert_eq!(data.side, Side::Buy);
        assert_eq!(data.client_order_id, b"order-123");
    }

    #[test]
    fn test_checkpoint_data() {
        let data = CheckpointData {
            timestamp: 1_704_067_200_000_000,
            sequence_number: 1000,
            state_hash: Sha256::new([0xAA; 32]),
            orderbook_hash: Sha256::new([0xBB; 32]),
            position_hash: Sha256::new([0xCC; 32]),
        };
        assert_eq!(data.sequence_number, 1000);
        assert_eq!(data.state_hash.as_bytes()[0], 0xAA);
    }

    // ==================== DecodedMessage Tests ====================

    #[test]
    fn test_decoded_message_timestamp() {
        let raw_frame = DecodedMessage::RawFrame(RawFrameData {
            timestamp: 12345,
            exchange: Exchange::Unknown,
            sequence_number: 0,
            payload: vec![],
        });
        assert_eq!(raw_frame.timestamp(), 12345);

        let quote = DecodedMessage::QuoteUpdate(QuoteUpdateData {
            timestamp: 67890,
            exchange: Exchange::Binance,
            symbol: Symbol::new("TEST"),
            bid_price: 0,
            bid_size: 0,
            ask_price: 0,
            ask_size: 0,
        });
        assert_eq!(quote.timestamp(), 67890);
    }

    #[test]
    fn test_decoded_message_id() {
        let session_start = DecodedMessage::SessionStart(SessionStartData {
            timestamp: 0,
            session_id: 0,
            exchange: Exchange::Unknown,
            metadata: vec![],
        });
        assert_eq!(session_start.message_id(), 1);

        let raw_frame = DecodedMessage::RawFrame(RawFrameData {
            timestamp: 0,
            exchange: Exchange::Unknown,
            sequence_number: 0,
            payload: vec![],
        });
        assert_eq!(raw_frame.message_id(), 256);

        let order_submit = DecodedMessage::OrderSubmit(OrderSubmitData {
            timestamp: 0,
            exchange: Exchange::Unknown,
            symbol: Symbol::default(),
            side: Side::Buy,
            price: 0,
            size: 0,
            client_order_id: vec![],
        });
        assert_eq!(order_submit.message_id(), 512);
    }
}
