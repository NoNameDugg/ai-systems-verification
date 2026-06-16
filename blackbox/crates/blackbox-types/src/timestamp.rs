//! Microsecond precision timestamp type.

/// A timestamp with microsecond precision.
///
/// Stored as i64 microseconds since Unix epoch (1970-01-01 00:00:00 UTC).
/// This provides a range of approximately +/- 292,000 years from epoch.
///
/// # Why i64?
///
/// - Consistent with Deribit and other exchanges that use microsecond timestamps
/// - Signed to allow representation of dates before 1970 (useful for testing)
/// - 64-bit for sufficient range and precision
///
/// # Example
///
/// ```
/// use blackbox_types::Timestamp;
///
/// let ts = Timestamp::from_micros(1704067200_000_000); // 2024-01-01 00:00:00 UTC
/// assert_eq!(ts.as_micros(), 1704067200_000_000);
/// assert_eq!(ts.as_millis(), 1704067200_000);
/// assert_eq!(ts.as_secs(), 1704067200);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The Unix epoch (1970-01-01 00:00:00 UTC).
    pub const EPOCH: Self = Self(0);

    /// The minimum representable timestamp.
    pub const MIN: Self = Self(i64::MIN);

    /// The maximum representable timestamp.
    pub const MAX: Self = Self(i64::MAX);

    /// Create a timestamp from microseconds since Unix epoch.
    #[inline]
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    /// Create a timestamp from milliseconds since Unix epoch.
    #[inline]
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis * 1_000)
    }

    /// Create a timestamp from seconds since Unix epoch.
    #[inline]
    pub const fn from_secs(secs: i64) -> Self {
        Self(secs * 1_000_000)
    }

    /// Returns the timestamp as microseconds since Unix epoch.
    #[inline]
    pub const fn as_micros(self) -> i64 {
        self.0
    }

    /// Returns the timestamp as milliseconds since Unix epoch.
    #[inline]
    pub const fn as_millis(self) -> i64 {
        self.0 / 1_000
    }

    /// Returns the timestamp as seconds since Unix epoch.
    #[inline]
    pub const fn as_secs(self) -> i64 {
        self.0 / 1_000_000
    }

    /// Returns the duration between two timestamps in microseconds.
    ///
    /// Returns positive if `other` is after `self`, negative otherwise.
    #[inline]
    pub const fn duration_since(self, other: Self) -> i64 {
        self.0 - other.0
    }

    /// Returns true if this timestamp is after the other.
    #[inline]
    pub const fn is_after(self, other: Self) -> bool {
        self.0 > other.0
    }

    /// Returns true if this timestamp is before the other.
    #[inline]
    pub const fn is_before(self, other: Self) -> bool {
        self.0 < other.0
    }

    /// Add microseconds to this timestamp.
    #[inline]
    pub const fn add_micros(self, micros: i64) -> Self {
        Self(self.0.saturating_add(micros))
    }

    /// Subtract microseconds from this timestamp.
    #[inline]
    pub const fn sub_micros(self, micros: i64) -> Self {
        Self(self.0.saturating_sub(micros))
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format as ISO 8601-ish for readability
        let secs = self.0 / 1_000_000;
        let micros = (self.0 % 1_000_000).abs();
        write!(f, "{}.{:06}", secs, micros)
    }
}

impl From<i64> for Timestamp {
    #[inline]
    fn from(micros: i64) -> Self {
        Self::from_micros(micros)
    }
}

impl From<Timestamp> for i64 {
    #[inline]
    fn from(ts: Timestamp) -> Self {
        ts.as_micros()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_conversions() {
        let ts = Timestamp::from_secs(1000);
        assert_eq!(ts.as_secs(), 1000);
        assert_eq!(ts.as_millis(), 1_000_000);
        assert_eq!(ts.as_micros(), 1_000_000_000);
    }

    #[test]
    fn test_timestamp_arithmetic() {
        let t1 = Timestamp::from_micros(1000);
        let t2 = t1.add_micros(500);
        assert_eq!(t2.as_micros(), 1500);

        let t3 = t2.sub_micros(300);
        assert_eq!(t3.as_micros(), 1200);
    }

    #[test]
    fn test_timestamp_comparison() {
        let t1 = Timestamp::from_micros(1000);
        let t2 = Timestamp::from_micros(2000);

        assert!(t2.is_after(t1));
        assert!(t1.is_before(t2));
        assert_eq!(t2.duration_since(t1), 1000);
    }

    #[test]
    fn test_timestamp_size() {
        assert_eq!(std::mem::size_of::<Timestamp>(), 8);
    }

    #[test]
    fn test_timestamp_display() {
        let ts = Timestamp::from_micros(1_704_067_200_123_456);
        let s = ts.to_string();
        assert!(s.contains("1704067200"));
        assert!(s.contains("123456"));
    }
}
