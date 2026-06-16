//! Exchange enumeration for supported trading venues.

/// Supported cryptocurrency exchanges.
///
/// Each variant has a fixed u8 value for binary serialization.
/// These values MUST NOT change once assigned (append-only).
///
/// # Binary Encoding
///
/// | Value | Exchange |
/// |-------|----------|
/// | 0     | Unknown  |
/// | 1     | Deribit  |
/// | 2     | Binance  |
/// | 3     | Bybit    |
/// | 4     | OKX      |
///
/// # Example
///
/// ```
/// use blackbox_types::Exchange;
///
/// let exchange = Exchange::Deribit;
/// assert_eq!(exchange.as_u8(), 1);
/// assert_eq!(Exchange::from_u8(1), Some(Exchange::Deribit));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Exchange {
    /// Unknown or unspecified exchange
    #[default]
    Unknown = 0,
    /// Deribit - Options and Futures
    Deribit = 1,
    /// Binance - Spot and Futures
    Binance = 2,
    /// Bybit - Derivatives
    Bybit = 3,
    /// OKX - Multi-asset
    OKX = 4,
}

impl Exchange {
    /// Convert to u8 for binary serialization.
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Convert from u8 for binary deserialization.
    ///
    /// Returns `None` for unknown values to support forward compatibility.
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Deribit),
            2 => Some(Self::Binance),
            3 => Some(Self::Bybit),
            4 => Some(Self::OKX),
            _ => None,
        }
    }

    /// Get the display name of the exchange.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Deribit => "Deribit",
            Self::Binance => "Binance",
            Self::Bybit => "Bybit",
            Self::OKX => "OKX",
        }
    }

    /// Returns all known exchanges (excluding Unknown).
    pub const fn all() -> &'static [Exchange] {
        &[
            Exchange::Deribit,
            Exchange::Binance,
            Exchange::Bybit,
            Exchange::OKX,
        ]
    }
}

impl std::fmt::Display for Exchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_roundtrip() {
        for &exchange in Exchange::all() {
            let value = exchange.as_u8();
            let recovered = Exchange::from_u8(value);
            assert_eq!(recovered, Some(exchange));
        }
    }

    #[test]
    fn test_exchange_unknown_value() {
        assert_eq!(Exchange::from_u8(255), None);
        assert_eq!(Exchange::from_u8(100), None);
    }

    #[test]
    fn test_exchange_size() {
        assert_eq!(std::mem::size_of::<Exchange>(), 1);
    }

    #[test]
    fn test_exchange_default() {
        assert_eq!(Exchange::default(), Exchange::Unknown);
    }
}
