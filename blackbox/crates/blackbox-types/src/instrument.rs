//! Instrument type for trading instruments.
//!
//! The Instrument struct uses fixed-size arrays instead of String
//! to ensure it is Copy and can be passed by value without allocation.

use crate::Exchange;

/// Maximum length for instrument symbol (e.g., "BTC-PERPETUAL").
pub const MAX_SYMBOL_LEN: usize = 32;

/// Maximum length for base currency (e.g., "BTC").
pub const MAX_CURRENCY_LEN: usize = 8;

/// A trading instrument with fixed-size storage.
///
/// This struct is `Copy` to enable zero-allocation passing in hot paths.
/// Symbol and currency are stored as fixed-size byte arrays with length tracking.
///
/// # Example
///
/// ```
/// use blackbox_types::{Instrument, Exchange};
///
/// let inst = Instrument::new(Exchange::Deribit, "BTC-PERPETUAL", "BTC");
/// assert_eq!(inst.exchange(), Exchange::Deribit);
/// assert_eq!(inst.symbol(), "BTC-PERPETUAL");
/// assert_eq!(inst.base_currency(), "BTC");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Instrument {
    /// The exchange this instrument trades on.
    exchange: Exchange,
    /// Symbol stored as fixed-size array.
    symbol: [u8; MAX_SYMBOL_LEN],
    /// Length of the symbol string.
    symbol_len: u8,
    /// Base currency stored as fixed-size array.
    base_currency: [u8; MAX_CURRENCY_LEN],
    /// Length of the base currency string.
    base_currency_len: u8,
}

impl Instrument {
    /// Create a new instrument.
    ///
    /// # Panics
    ///
    /// Panics if symbol exceeds 32 bytes or base_currency exceeds 8 bytes.
    pub fn new(exchange: Exchange, symbol: &str, base_currency: &str) -> Self {
        assert!(
            symbol.len() <= MAX_SYMBOL_LEN,
            "Symbol exceeds maximum length of {} bytes",
            MAX_SYMBOL_LEN
        );
        assert!(
            base_currency.len() <= MAX_CURRENCY_LEN,
            "Base currency exceeds maximum length of {} bytes",
            MAX_CURRENCY_LEN
        );

        let mut symbol_arr = [0u8; MAX_SYMBOL_LEN];
        symbol_arr[..symbol.len()].copy_from_slice(symbol.as_bytes());

        let mut base_currency_arr = [0u8; MAX_CURRENCY_LEN];
        base_currency_arr[..base_currency.len()].copy_from_slice(base_currency.as_bytes());

        Self {
            exchange,
            symbol: symbol_arr,
            symbol_len: symbol.len() as u8,
            base_currency: base_currency_arr,
            base_currency_len: base_currency.len() as u8,
        }
    }

    /// Try to create a new instrument, returning None if strings are too long.
    pub fn try_new(exchange: Exchange, symbol: &str, base_currency: &str) -> Option<Self> {
        if symbol.len() > MAX_SYMBOL_LEN || base_currency.len() > MAX_CURRENCY_LEN {
            return None;
        }

        let mut symbol_arr = [0u8; MAX_SYMBOL_LEN];
        symbol_arr[..symbol.len()].copy_from_slice(symbol.as_bytes());

        let mut base_currency_arr = [0u8; MAX_CURRENCY_LEN];
        base_currency_arr[..base_currency.len()].copy_from_slice(base_currency.as_bytes());

        Some(Self {
            exchange,
            symbol: symbol_arr,
            symbol_len: symbol.len() as u8,
            base_currency: base_currency_arr,
            base_currency_len: base_currency.len() as u8,
        })
    }

    /// Returns the exchange.
    #[inline]
    pub const fn exchange(&self) -> Exchange {
        self.exchange
    }

    /// Returns the symbol as a string slice.
    #[inline]
    pub fn symbol(&self) -> &str {
        // Safe: We only store valid UTF-8 from &str input in new/try_new
        std::str::from_utf8(&self.symbol[..self.symbol_len as usize])
            .expect("Invalid UTF-8 in symbol - should never happen")
    }

    /// Returns the base currency as a string slice.
    #[inline]
    pub fn base_currency(&self) -> &str {
        // Safe: We only store valid UTF-8 from &str input in new/try_new
        std::str::from_utf8(&self.base_currency[..self.base_currency_len as usize])
            .expect("Invalid UTF-8 in base_currency - should never happen")
    }

    /// Returns the raw symbol bytes (for binary serialization).
    #[inline]
    pub const fn symbol_bytes(&self) -> &[u8; MAX_SYMBOL_LEN] {
        &self.symbol
    }

    /// Returns the symbol length.
    #[inline]
    pub const fn symbol_len(&self) -> u8 {
        self.symbol_len
    }

    /// Returns the raw base currency bytes (for binary serialization).
    #[inline]
    pub const fn base_currency_bytes(&self) -> &[u8; MAX_CURRENCY_LEN] {
        &self.base_currency
    }

    /// Returns the base currency length.
    #[inline]
    pub const fn base_currency_len(&self) -> u8 {
        self.base_currency_len
    }
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            exchange: Exchange::Unknown,
            symbol: [0u8; MAX_SYMBOL_LEN],
            symbol_len: 0,
            base_currency: [0u8; MAX_CURRENCY_LEN],
            base_currency_len: 0,
        }
    }
}

impl std::fmt::Debug for Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Instrument")
            .field("exchange", &self.exchange)
            .field("symbol", &self.symbol())
            .field("base_currency", &self.base_currency())
            .finish()
    }
}

impl std::fmt::Display for Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.exchange, self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_creation() {
        let inst = Instrument::new(Exchange::Deribit, "BTC-PERPETUAL", "BTC");
        assert_eq!(inst.exchange(), Exchange::Deribit);
        assert_eq!(inst.symbol(), "BTC-PERPETUAL");
        assert_eq!(inst.base_currency(), "BTC");
    }

    #[test]
    fn test_instrument_is_copy() {
        let inst = Instrument::new(Exchange::Deribit, "BTC-PERPETUAL", "BTC");
        let copy = inst; // Copy, not move
        assert_eq!(inst.symbol(), copy.symbol()); // Both still valid
    }

    #[test]
    fn test_instrument_size() {
        // Should be relatively compact
        let size = std::mem::size_of::<Instrument>();
        assert!(
            size <= 48,
            "Instrument size {} exceeds expected 48 bytes",
            size
        );
    }

    #[test]
    fn test_instrument_try_new_success() {
        let inst = Instrument::try_new(Exchange::Binance, "BTCUSDT", "BTC");
        assert!(inst.is_some());
    }

    #[test]
    fn test_instrument_try_new_too_long() {
        let long_symbol = "A".repeat(33);
        let inst = Instrument::try_new(Exchange::Binance, &long_symbol, "BTC");
        assert!(inst.is_none());
    }

    #[test]
    #[should_panic]
    fn test_instrument_new_panics_on_long_symbol() {
        let long_symbol = "A".repeat(33);
        Instrument::new(Exchange::Binance, &long_symbol, "BTC");
    }

    #[test]
    fn test_instrument_display() {
        let inst = Instrument::new(Exchange::Deribit, "ETH-PERPETUAL", "ETH");
        assert_eq!(inst.to_string(), "Deribit:ETH-PERPETUAL");
    }

    #[test]
    fn test_instrument_equality() {
        let inst1 = Instrument::new(Exchange::Deribit, "BTC-PERPETUAL", "BTC");
        let inst2 = Instrument::new(Exchange::Deribit, "BTC-PERPETUAL", "BTC");
        let inst3 = Instrument::new(Exchange::Deribit, "ETH-PERPETUAL", "ETH");

        assert_eq!(inst1, inst2);
        assert_ne!(inst1, inst3);
    }
}
