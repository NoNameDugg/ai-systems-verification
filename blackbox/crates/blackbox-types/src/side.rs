//! Side enumeration for order direction.

/// Order side (Buy or Sell).
///
/// # Binary Encoding
///
/// | Value | Side |
/// |-------|------|
/// | 0     | Buy  |
/// | 1     | Sell |
///
/// # Example
///
/// ```
/// use blackbox_types::Side;
///
/// let side = Side::Buy;
/// assert_eq!(side.as_u8(), 0);
/// assert_eq!(side.opposite(), Side::Sell);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Side {
    /// Buy order (bid)
    #[default]
    Buy = 0,
    /// Sell order (ask/offer)
    Sell = 1,
}

impl Side {
    /// Convert to u8 for binary serialization.
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Convert from u8 for binary deserialization.
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Buy),
            1 => Some(Self::Sell),
            _ => None,
        }
    }

    /// Returns the opposite side.
    #[inline]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }

    /// Returns true if this is a buy order.
    #[inline]
    pub const fn is_buy(self) -> bool {
        matches!(self, Self::Buy)
    }

    /// Returns true if this is a sell order.
    #[inline]
    pub const fn is_sell(self) -> bool {
        matches!(self, Self::Sell)
    }

    /// Get the display name.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_side_roundtrip() {
        assert_eq!(Side::from_u8(Side::Buy.as_u8()), Some(Side::Buy));
        assert_eq!(Side::from_u8(Side::Sell.as_u8()), Some(Side::Sell));
    }

    #[test]
    fn test_side_opposite() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);
    }

    #[test]
    fn test_side_size() {
        assert_eq!(std::mem::size_of::<Side>(), 1);
    }

    #[test]
    fn test_side_predicates() {
        assert!(Side::Buy.is_buy());
        assert!(!Side::Buy.is_sell());
        assert!(Side::Sell.is_sell());
        assert!(!Side::Sell.is_buy());
    }
}
