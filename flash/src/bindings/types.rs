//! Python type wrappers for Flash core types.
//!
//! This module provides PyO3 wrapper types for all core Flash types,
//! enabling seamless Rust ↔ Python interoperability.
//!
//! # Type Mappings
//!
//! | Rust Type | Python Type | Notes |
//! |-----------|-------------|-------|
//! | [`Exchange`] | [`PyExchange`] | Enum with variants |
//! | [`Side`] | [`PySide`] | Enum with variants |
//! | [`MarketEventType`] | [`PyMarketEventType`] | Enum with variants |
//! | [`Instrument`] | [`PyInstrument`] | Struct with properties |
//! | [`PriceLevel`] | [`PyPriceLevel`] | Decimal as string |
//! | [`MarketData`] | [`PyMarketData`] | Sum type with accessors |
//! | [`MarketEvent`] | [`PyMarketEvent`] | Full event wrapper |
//! | [`BookSnapshot`] | [`PyBookSnapshot`] | Snapshot wrapper |
//!
//! # Design Principles
//!
//! 1. **Immutability**: All Python types are immutable (read-only properties)
//! 2. **Precision**: Decimals are exposed as strings to preserve precision
//! 3. **Safety**: Type checking via variant accessors
//! 4. **Pythonic API**: `__repr__`, `__eq__`, `__hash__` where appropriate
//!
//! # Example
//!
//! ```python
//! from astra_flash import PyExchange, PyInstrument, PyPriceLevel
//!
//! # Create instrument
//! inst = PyInstrument("BTC", "USD", PyExchange.Deribit, "BTC-PERPETUAL")
//! print(inst.symbol())  # "BTC/USD"
//!
//! # Access price level
//! level = PyPriceLevel(50000.0, "1.5", 1234567890)
//! print(level.quantity)  # "1.5" (string for precision)
//! print(level.quantity_as_float())  # 1.5 (float for calculations)
//! ```

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

use crate::book::BookSnapshot;
use crate::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side,
};

// =============================================================================
// PYEXCHANGE ENUM
// =============================================================================

/// Python wrapper for the Exchange enum.
///
/// Represents supported exchange identifiers.
///
/// # Variants
///
/// - `Deribit` - Deribit crypto derivatives exchange
/// - `Binance` - Binance spot and futures exchange
/// - `Oanda` - OANDA forex broker
///
/// # Example
///
/// ```python
/// from astra_flash import PyExchange
///
/// exchange = PyExchange.Deribit
/// print(exchange.as_str())  # "deribit"
/// ```
#[pyclass(name = "Exchange", eq, eq_int, hash, frozen)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyExchange {
    /// Deribit crypto derivatives exchange.
    Deribit,
    /// Binance spot and futures exchange.
    Binance,
    /// OANDA forex broker.
    Oanda,
}

#[pymethods]
impl PyExchange {
    /// Returns the lowercase string representation of the exchange.
    ///
    /// # Returns
    ///
    /// String representation (e.g., "deribit", "binance", "oanda")
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deribit => "deribit",
            Self::Binance => "binance",
            Self::Oanda => "oanda",
        }
    }

    fn __repr__(&self) -> String {
        format!("Exchange.{}", self.as_str().to_uppercase())
    }

    fn __str__(&self) -> &'static str {
        self.as_str()
    }
}

impl From<Exchange> for PyExchange {
    fn from(e: Exchange) -> Self {
        match e {
            Exchange::Deribit => Self::Deribit,
            Exchange::Binance => Self::Binance,
            Exchange::Oanda => Self::Oanda,
        }
    }
}

impl From<PyExchange> for Exchange {
    fn from(e: PyExchange) -> Self {
        match e {
            PyExchange::Deribit => Self::Deribit,
            PyExchange::Binance => Self::Binance,
            PyExchange::Oanda => Self::Oanda,
        }
    }
}

// =============================================================================
// PYSIDE ENUM
// =============================================================================

/// Python wrapper for the Side enum.
///
/// Represents order book side (bid or ask).
///
/// # Variants
///
/// - `Bid` - Buy side (bids)
/// - `Ask` - Sell side (asks)
///
/// # Example
///
/// ```python
/// from astra_flash import PySide
///
/// side = PySide.Bid
/// opposite = side.opposite()  # PySide.Ask
/// ```
#[pyclass(name = "Side", eq, eq_int, hash, frozen)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PySide {
    /// Buy side (bids).
    Bid,
    /// Sell side (asks).
    Ask,
}

#[pymethods]
impl PySide {
    /// Returns the opposite side.
    ///
    /// # Returns
    ///
    /// `PySide.Ask` if self is `Bid`, `PySide.Bid` if self is `Ask`
    #[must_use]
    pub fn opposite(&self) -> Self {
        match self {
            Self::Bid => Self::Ask,
            Self::Ask => Self::Bid,
        }
    }

    /// Returns the lowercase string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bid => "bid",
            Self::Ask => "ask",
        }
    }

    fn __repr__(&self) -> String {
        format!("Side.{}", self.as_str().to_uppercase())
    }

    fn __str__(&self) -> &'static str {
        self.as_str()
    }
}

impl From<Side> for PySide {
    fn from(s: Side) -> Self {
        match s {
            Side::Bid => Self::Bid,
            Side::Ask => Self::Ask,
        }
    }
}

impl From<PySide> for Side {
    fn from(s: PySide) -> Self {
        match s {
            PySide::Bid => Self::Bid,
            PySide::Ask => Self::Ask,
        }
    }
}

// =============================================================================
// PYMARKETEVENTTYPE ENUM
// =============================================================================

/// Python wrapper for the MarketEventType enum.
///
/// Represents the type of market event received from an exchange.
///
/// # Variants
///
/// - `Snapshot` - Full order book snapshot (replaces all data)
/// - `Delta` - Incremental update (delta changes)
/// - `Trade` - Trade execution
/// - `Heartbeat` - Connection heartbeat
///
/// # Example
///
/// ```python
/// from astra_flash import PyMarketEventType
///
/// event_type = PyMarketEventType.Snapshot
/// print(event_type.as_str())  # "snapshot"
/// ```
#[pyclass(name = "MarketEventType", eq, eq_int, hash, frozen)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyMarketEventType {
    /// Full order book snapshot.
    Snapshot,
    /// Incremental update (delta).
    Delta,
    /// Trade execution.
    Trade,
    /// Connection heartbeat.
    Heartbeat,
}

#[pymethods]
impl PyMarketEventType {
    /// Returns the lowercase string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Delta => "delta",
            Self::Trade => "trade",
            Self::Heartbeat => "heartbeat",
        }
    }

    fn __repr__(&self) -> String {
        format!("MarketEventType.{}", self.as_str().to_uppercase())
    }

    fn __str__(&self) -> &'static str {
        self.as_str()
    }
}

impl From<MarketEventType> for PyMarketEventType {
    fn from(t: MarketEventType) -> Self {
        match t {
            MarketEventType::Snapshot => Self::Snapshot,
            MarketEventType::Delta => Self::Delta,
            MarketEventType::Trade => Self::Trade,
            MarketEventType::Heartbeat => Self::Heartbeat,
        }
    }
}

impl From<PyMarketEventType> for MarketEventType {
    fn from(t: PyMarketEventType) -> Self {
        match t {
            PyMarketEventType::Snapshot => Self::Snapshot,
            PyMarketEventType::Delta => Self::Delta,
            PyMarketEventType::Trade => Self::Trade,
            PyMarketEventType::Heartbeat => Self::Heartbeat,
        }
    }
}

// =============================================================================
// PYINSTRUMENT STRUCT
// =============================================================================

/// Python wrapper for the Instrument struct.
///
/// Represents a trading instrument identifier with base/quote currencies,
/// exchange, and raw symbol.
///
/// # Properties
///
/// - `base` - Base currency (e.g., "BTC")
/// - `quote` - Quote currency (e.g., "USD")
/// - `exchange` - Exchange this instrument belongs to
/// - `raw_symbol` - Exchange-specific symbol (e.g., "BTC-PERPETUAL")
///
/// # Example
///
/// ```python
/// from astra_flash import PyInstrument, PyExchange
///
/// inst = PyInstrument("BTC", "USD", PyExchange.Deribit, "BTC-PERPETUAL")
/// print(inst.symbol())  # "BTC/USD"
/// print(inst.exchange)  # Exchange.DERIBIT
/// ```
#[pyclass(name = "Instrument", frozen)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PyInstrument {
    /// Base currency (e.g., "BTC").
    base: String,
    /// Quote currency (e.g., "USD").
    quote: String,
    /// Exchange this instrument belongs to.
    exchange: PyExchange,
    /// Exchange-specific raw symbol.
    raw_symbol: String,
}

#[pymethods]
impl PyInstrument {
    /// Creates a new instrument.
    ///
    /// # Arguments
    ///
    /// * `base` - Base currency symbol
    /// * `quote` - Quote currency symbol
    /// * `exchange` - The exchange
    /// * `raw_symbol` - Exchange-specific symbol
    #[new]
    #[must_use]
    pub fn new(base: String, quote: String, exchange: PyExchange, raw_symbol: String) -> Self {
        Self {
            base,
            quote,
            exchange,
            raw_symbol,
        }
    }

    /// Returns the base currency.
    #[getter]
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Returns the quote currency.
    #[getter]
    #[must_use]
    pub fn quote(&self) -> &str {
        &self.quote
    }

    /// Returns the exchange.
    #[getter]
    #[must_use]
    pub fn exchange(&self) -> PyExchange {
        self.exchange
    }

    /// Returns the raw symbol.
    #[getter]
    #[must_use]
    pub fn raw_symbol(&self) -> &str {
        &self.raw_symbol
    }

    /// Returns the unified symbol format "BASE/QUOTE".
    #[must_use]
    pub fn symbol(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    fn __repr__(&self) -> String {
        format!(
            "Instrument(base='{}', quote='{}', exchange={}, raw_symbol='{}')",
            self.base,
            self.quote,
            self.exchange.__repr__(),
            self.raw_symbol
        )
    }

    fn __str__(&self) -> String {
        format!("{}:{}", self.exchange.as_str(), self.raw_symbol)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.base == other.base
            && self.quote == other.quote
            && self.exchange == other.exchange
            && self.raw_symbol == other.raw_symbol
    }

    fn __hash__(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl From<Instrument> for PyInstrument {
    fn from(i: Instrument) -> Self {
        Self {
            base: i.base,
            quote: i.quote,
            exchange: i.exchange.into(),
            raw_symbol: i.raw_symbol,
        }
    }
}

impl From<PyInstrument> for Instrument {
    fn from(i: PyInstrument) -> Self {
        Self {
            base: i.base,
            quote: i.quote,
            exchange: i.exchange.into(),
            raw_symbol: i.raw_symbol,
        }
    }
}

// =============================================================================
// PYPRICELEVEL STRUCT
// =============================================================================

/// Python wrapper for the PriceLevel struct.
///
/// Represents a single price level in an order book. The quantity is stored
/// as a string to preserve decimal precision.
///
/// # Properties
///
/// - `price` - Price of this level (f64)
/// - `quantity` - Quantity as string (for precision)
/// - `order_count` - Number of orders (L3 data, optional)
/// - `timestamp` - Exchange timestamp in microseconds
///
/// # Example
///
/// ```python
/// from astra_flash import PyPriceLevel
///
/// level = PyPriceLevel(50000.0, "1.5", 1234567890)
/// print(level.price)  # 50000.0
/// print(level.quantity)  # "1.5"
/// print(level.quantity_as_float())  # 1.5
/// ```
#[pyclass(name = "PriceLevel", frozen)]
#[derive(Debug, Clone)]
pub struct PyPriceLevel {
    /// Price of this level.
    price: f64,
    /// Quantity as string (Decimal for precision).
    quantity: String,
    /// Number of orders (L3 data).
    order_count: Option<u32>,
    /// Exchange timestamp in microseconds.
    timestamp: i64,
}

#[pymethods]
impl PyPriceLevel {
    /// Creates a new price level.
    ///
    /// # Arguments
    ///
    /// * `price` - The price of this level
    /// * `quantity` - Total quantity as string
    /// * `timestamp` - Exchange timestamp in microseconds
    #[new]
    #[pyo3(signature = (price, quantity, timestamp, order_count=None))]
    #[must_use]
    pub fn new(price: f64, quantity: String, timestamp: i64, order_count: Option<u32>) -> Self {
        Self {
            price,
            quantity,
            order_count,
            timestamp,
        }
    }

    /// Returns the price.
    #[getter]
    #[must_use]
    pub fn price(&self) -> f64 {
        self.price
    }

    /// Returns the quantity as a string (preserves precision).
    #[getter]
    #[must_use]
    pub fn quantity(&self) -> &str {
        &self.quantity
    }

    /// Returns the order count (L3 data).
    #[getter]
    #[must_use]
    pub fn order_count(&self) -> Option<u32> {
        self.order_count
    }

    /// Returns the timestamp in microseconds.
    #[getter]
    #[must_use]
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// Returns the quantity as a float.
    ///
    /// Note: This may lose precision for very precise decimals.
    /// Use `quantity` property for full precision.
    #[must_use]
    pub fn quantity_as_float(&self) -> f64 {
        self.quantity.parse().unwrap_or(0.0)
    }

    /// Returns true if the quantity is zero (level should be removed).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quantity == "0"
            || self.quantity == "0.0"
            || self.quantity.parse::<f64>().map_or(true, |q| q == 0.0)
    }

    fn __repr__(&self) -> String {
        format!(
            "PriceLevel(price={}, quantity='{}', timestamp={})",
            self.price, self.quantity, self.timestamp
        )
    }

    fn __str__(&self) -> String {
        format!("{} @ {}", self.quantity, self.price)
    }
}

impl From<PriceLevel> for PyPriceLevel {
    fn from(l: PriceLevel) -> Self {
        Self {
            price: l.price,
            quantity: l.quantity.to_string(),
            order_count: l.order_count,
            timestamp: l.timestamp,
        }
    }
}

impl From<PyPriceLevel> for PriceLevel {
    fn from(l: PyPriceLevel) -> Self {
        use rust_decimal::Decimal;
        use std::str::FromStr;

        Self {
            price: l.price,
            quantity: Decimal::from_str(&l.quantity).unwrap_or_default(),
            order_count: l.order_count,
            timestamp: l.timestamp,
        }
    }
}

// =============================================================================
// PYMARKETDATA ENUM
// =============================================================================

/// Python wrapper for the MarketData enum.
///
/// Represents market data payload in a market event. Use type checking methods
/// and accessors to safely access variant data.
///
/// # Methods
///
/// - `is_book()` - Check if this is book data
/// - `is_trade()` - Check if this is trade data
/// - `is_heartbeat()` - Check if this is heartbeat data
/// - `as_book()` - Get book data (bids, asks)
/// - `as_trade()` - Get trade data (price, quantity, side, trade_id)
/// - `as_heartbeat()` - Get heartbeat data (exchange_time)
///
/// # Example
///
/// ```python
/// from astra_flash import PyMarketData
///
/// data = get_market_data()  # From somewhere
/// if data.is_book():
///     bids, asks = data.as_book()
///     print(f"Best bid: {bids[0].price}")
/// elif data.is_trade():
///     price, qty, side, trade_id = data.as_trade()
///     print(f"Trade: {qty} @ {price}")
/// ```
#[pyclass(name = "MarketData", frozen)]
#[derive(Debug, Clone)]
pub struct PyMarketData {
    /// Internal storage for the data variant.
    inner: MarketDataInner,
}

/// Internal representation of market data.
#[derive(Debug, Clone)]
enum MarketDataInner {
    Book {
        bids: Vec<PyPriceLevel>,
        asks: Vec<PyPriceLevel>,
    },
    Trade {
        price: f64,
        quantity: String,
        side: PySide,
        trade_id: Option<String>,
    },
    Heartbeat {
        exchange_time: i64,
    },
}

#[pymethods]
impl PyMarketData {
    /// Returns true if this is book data.
    #[must_use]
    pub fn is_book(&self) -> bool {
        matches!(self.inner, MarketDataInner::Book { .. })
    }

    /// Returns true if this is trade data.
    #[must_use]
    pub fn is_trade(&self) -> bool {
        matches!(self.inner, MarketDataInner::Trade { .. })
    }

    /// Returns true if this is heartbeat data.
    #[must_use]
    pub fn is_heartbeat(&self) -> bool {
        matches!(self.inner, MarketDataInner::Heartbeat { .. })
    }

    /// Returns book data (bids, asks).
    ///
    /// # Returns
    ///
    /// Tuple of (bids, asks) where each is a list of PyPriceLevel.
    ///
    /// # Errors
    ///
    /// Raises TypeError if this is not book data.
    pub fn as_book(&self) -> PyResult<(Vec<PyPriceLevel>, Vec<PyPriceLevel>)> {
        match &self.inner {
            MarketDataInner::Book { bids, asks } => Ok((bids.clone(), asks.clone())),
            _ => Err(PyTypeError::new_err("Not book data")),
        }
    }

    /// Returns trade data (price, quantity, side, trade_id).
    ///
    /// # Returns
    ///
    /// Tuple of (price, quantity, side, trade_id).
    ///
    /// # Errors
    ///
    /// Raises TypeError if this is not trade data.
    pub fn as_trade(&self) -> PyResult<(f64, String, PySide, Option<String>)> {
        match &self.inner {
            MarketDataInner::Trade {
                price,
                quantity,
                side,
                trade_id,
            } => Ok((*price, quantity.clone(), *side, trade_id.clone())),
            _ => Err(PyTypeError::new_err("Not trade data")),
        }
    }

    /// Returns heartbeat data (exchange_time).
    ///
    /// # Returns
    ///
    /// Exchange timestamp in microseconds.
    ///
    /// # Errors
    ///
    /// Raises TypeError if this is not heartbeat data.
    pub fn as_heartbeat(&self) -> PyResult<i64> {
        match &self.inner {
            MarketDataInner::Heartbeat { exchange_time } => Ok(*exchange_time),
            _ => Err(PyTypeError::new_err("Not heartbeat data")),
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            MarketDataInner::Book { bids, asks } => {
                format!("MarketData.Book(bids={}, asks={})", bids.len(), asks.len())
            },
            MarketDataInner::Trade {
                price,
                quantity,
                side,
                ..
            } => {
                format!(
                    "MarketData.Trade(price={}, quantity='{}', side={})",
                    price,
                    quantity,
                    side.__repr__()
                )
            },
            MarketDataInner::Heartbeat { exchange_time } => {
                format!("MarketData.Heartbeat(exchange_time={})", exchange_time)
            },
        }
    }
}

impl From<MarketData> for PyMarketData {
    fn from(d: MarketData) -> Self {
        let inner = match d {
            MarketData::Book { bids, asks } => MarketDataInner::Book {
                bids: bids.into_iter().map(|l| l.into()).collect(),
                asks: asks.into_iter().map(|l| l.into()).collect(),
            },
            MarketData::Trade {
                price,
                quantity,
                side,
                trade_id,
            } => MarketDataInner::Trade {
                price,
                quantity: quantity.to_string(),
                side: side.into(),
                trade_id,
            },
            MarketData::Heartbeat { exchange_time } => MarketDataInner::Heartbeat { exchange_time },
        };
        Self { inner }
    }
}

impl From<PyMarketData> for MarketData {
    fn from(d: PyMarketData) -> Self {
        use rust_decimal::Decimal;
        use std::str::FromStr;

        match d.inner {
            MarketDataInner::Book { bids, asks } => Self::Book {
                bids: bids.into_iter().map(|l| l.into()).collect(),
                asks: asks.into_iter().map(|l| l.into()).collect(),
            },
            MarketDataInner::Trade {
                price,
                quantity,
                side,
                trade_id,
            } => Self::Trade {
                price,
                quantity: Decimal::from_str(&quantity).unwrap_or_default(),
                side: side.into(),
                trade_id,
            },
            MarketDataInner::Heartbeat { exchange_time } => Self::Heartbeat { exchange_time },
        }
    }
}

// =============================================================================
// PYMARKETEVENT STRUCT
// =============================================================================

/// Python wrapper for the MarketEvent struct.
///
/// Represents a unified market event from any exchange.
///
/// # Properties
///
/// - `event_type` - Type of market event
/// - `instrument` - Instrument this event is for
/// - `timestamp` - Exchange timestamp in microseconds
/// - `local_timestamp` - Local receipt timestamp in microseconds
/// - `sequence` - Exchange sequence number (for gap detection)
/// - `data` - Event data payload
///
/// # Example
///
/// ```python
/// from astra_flash import PyMarketEvent
///
/// event = get_event()  # From somewhere
/// print(event.event_type)  # MarketEventType.SNAPSHOT
/// print(event.latency_micros())  # 1000 (1ms)
/// ```
#[pyclass(name = "MarketEvent", frozen)]
#[derive(Debug, Clone)]
pub struct PyMarketEvent {
    /// Type of market event.
    event_type: PyMarketEventType,
    /// Instrument this event is for.
    instrument: PyInstrument,
    /// Exchange timestamp in microseconds.
    timestamp: i64,
    /// Local receipt timestamp in microseconds.
    local_timestamp: i64,
    /// Exchange sequence number.
    sequence: Option<u64>,
    /// Event data payload.
    data: PyMarketData,
}

#[pymethods]
impl PyMarketEvent {
    /// Returns the event type.
    #[getter]
    #[must_use]
    pub fn event_type(&self) -> PyMarketEventType {
        self.event_type
    }

    /// Returns the instrument.
    #[getter]
    #[must_use]
    pub fn instrument(&self) -> PyInstrument {
        self.instrument.clone()
    }

    /// Returns the exchange timestamp in microseconds.
    #[getter]
    #[must_use]
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// Returns the local receipt timestamp in microseconds.
    #[getter]
    #[must_use]
    pub fn local_timestamp(&self) -> i64 {
        self.local_timestamp
    }

    /// Returns the exchange sequence number.
    #[getter]
    #[must_use]
    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    /// Returns the event data payload.
    #[getter]
    #[must_use]
    pub fn data(&self) -> PyMarketData {
        self.data.clone()
    }

    /// Returns the latency between exchange time and local time in microseconds.
    #[must_use]
    pub fn latency_micros(&self) -> i64 {
        self.local_timestamp - self.timestamp
    }

    fn __repr__(&self) -> String {
        format!(
            "MarketEvent(type={}, instrument={}, timestamp={}, latency_us={})",
            self.event_type.__repr__(),
            self.instrument.__str__(),
            self.timestamp,
            self.latency_micros()
        )
    }
}

impl From<MarketEvent> for PyMarketEvent {
    fn from(e: MarketEvent) -> Self {
        Self {
            event_type: e.event_type.into(),
            instrument: e.instrument.into(),
            timestamp: e.timestamp,
            local_timestamp: e.local_timestamp,
            sequence: e.sequence,
            data: e.data.into(),
        }
    }
}

impl From<PyMarketEvent> for MarketEvent {
    fn from(e: PyMarketEvent) -> Self {
        Self {
            event_type: e.event_type.into(),
            instrument: e.instrument.into(),
            timestamp: e.timestamp,
            local_timestamp: e.local_timestamp,
            sequence: e.sequence,
            data: e.data.into(),
        }
    }
}

// =============================================================================
// PYBOOKSNAPSHOT STRUCT
// =============================================================================

/// Python wrapper for the BookSnapshot struct.
///
/// Represents a serializable order book snapshot with bids, asks, and metadata.
///
/// # Properties
///
/// - `instrument` - Instrument this snapshot is for
/// - `timestamp` - Exchange timestamp in microseconds
/// - `bids` - List of bid price levels
/// - `asks` - List of ask price levels
///
/// # Example
///
/// ```python
/// from astra_flash import PyBookSnapshot
///
/// snapshot = get_snapshot()  # From somewhere
/// print(snapshot.best_bid())  # PyPriceLevel or None
/// print(snapshot.mid_price())  # 50050.0 or None
/// print(snapshot.spread())  # 100.0 or None
/// ```
#[pyclass(name = "BookSnapshot", frozen)]
#[derive(Debug, Clone)]
pub struct PyBookSnapshot {
    /// Instrument this snapshot is for.
    instrument: PyInstrument,
    /// Exchange timestamp in microseconds.
    timestamp: i64,
    /// Bid price levels.
    bids: Vec<PyPriceLevel>,
    /// Ask price levels.
    asks: Vec<PyPriceLevel>,
}

#[pymethods]
impl PyBookSnapshot {
    /// Returns the instrument.
    #[getter]
    #[must_use]
    pub fn instrument(&self) -> PyInstrument {
        self.instrument.clone()
    }

    /// Returns the timestamp in microseconds.
    #[getter]
    #[must_use]
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// Returns the bid price levels.
    #[getter]
    #[must_use]
    pub fn bids(&self) -> Vec<PyPriceLevel> {
        self.bids.clone()
    }

    /// Returns the ask price levels.
    #[getter]
    #[must_use]
    pub fn asks(&self) -> Vec<PyPriceLevel> {
        self.asks.clone()
    }

    /// Returns the best bid (highest price bid).
    ///
    /// # Returns
    ///
    /// The best bid PyPriceLevel, or None if no bids.
    #[must_use]
    pub fn best_bid(&self) -> Option<PyPriceLevel> {
        self.bids.first().cloned()
    }

    /// Returns the best ask (lowest price ask).
    ///
    /// # Returns
    ///
    /// The best ask PyPriceLevel, or None if no asks.
    #[must_use]
    pub fn best_ask(&self) -> Option<PyPriceLevel> {
        self.asks.first().cloned()
    }

    /// Returns the mid price ((best_bid + best_ask) / 2).
    ///
    /// # Returns
    ///
    /// The mid price, or None if either side is empty.
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2.0),
            _ => None,
        }
    }

    /// Returns the bid-ask spread (best_ask - best_bid).
    ///
    /// # Returns
    ///
    /// The spread, or None if either side is empty.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "BookSnapshot(instrument={}, bids={}, asks={}, timestamp={})",
            self.instrument.__str__(),
            self.bids.len(),
            self.asks.len(),
            self.timestamp
        )
    }
}

impl From<BookSnapshot> for PyBookSnapshot {
    fn from(s: BookSnapshot) -> Self {
        Self {
            instrument: s.instrument.into(),
            timestamp: s.timestamp,
            bids: s.bids.into_iter().map(|l| l.into()).collect(),
            asks: s.asks.into_iter().map(|l| l.into()).collect(),
        }
    }
}

impl From<PyBookSnapshot> for BookSnapshot {
    fn from(s: PyBookSnapshot) -> Self {
        Self {
            instrument: s.instrument.into(),
            timestamp: s.timestamp,
            bids: s.bids.into_iter().map(|l| l.into()).collect(),
            asks: s.asks.into_iter().map(|l| l.into()).collect(),
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_pyexchange_roundtrip() {
        for exchange in [Exchange::Deribit, Exchange::Binance, Exchange::Oanda] {
            let py: PyExchange = exchange.into();
            let back: Exchange = py.into();
            assert_eq!(exchange, back);
        }
    }

    #[test]
    fn test_pyside_roundtrip() {
        for side in [Side::Bid, Side::Ask] {
            let py: PySide = side.into();
            let back: Side = py.into();
            assert_eq!(side, back);
        }
    }

    #[test]
    fn test_pyside_opposite() {
        assert_eq!(PySide::Bid.opposite(), PySide::Ask);
        assert_eq!(PySide::Ask.opposite(), PySide::Bid);
    }

    #[test]
    fn test_pymarket_event_type_roundtrip() {
        for t in [
            MarketEventType::Snapshot,
            MarketEventType::Delta,
            MarketEventType::Trade,
            MarketEventType::Heartbeat,
        ] {
            let py: PyMarketEventType = t.into();
            let back: MarketEventType = py.into();
            assert_eq!(t, back);
        }
    }

    #[test]
    fn test_pyinstrument_conversion() {
        let rust = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
        let py: PyInstrument = rust.clone().into();

        assert_eq!(py.base(), "BTC");
        assert_eq!(py.quote(), "USD");
        assert_eq!(py.exchange(), PyExchange::Deribit);
        assert_eq!(py.raw_symbol(), "BTC-PERPETUAL");
        assert_eq!(py.symbol(), "BTC/USD");
    }

    #[test]
    fn test_pyprice_level_conversion() {
        let rust = PriceLevel::new(50000.0, dec!(1.5), 1234567890);
        let py: PyPriceLevel = rust.clone().into();

        assert!((py.price() - 50000.0).abs() < f64::EPSILON);
        assert_eq!(py.quantity(), "1.5");
        assert_eq!(py.timestamp(), 1234567890);
        assert!(py.order_count().is_none());
        assert!(!py.is_empty());
    }

    #[test]
    fn test_pyprice_level_is_empty() {
        let level = PriceLevel::new(100.0, dec!(0), 1234567890);
        let py: PyPriceLevel = level.into();
        assert!(py.is_empty());
    }

    #[test]
    fn test_pymarket_data_book() {
        let rust = MarketData::Book {
            bids: vec![PriceLevel::new(100.0, dec!(1), 123)],
            asks: vec![PriceLevel::new(101.0, dec!(2), 123)],
        };
        let py: PyMarketData = rust.into();

        assert!(py.is_book());
        assert!(!py.is_trade());
        assert!(!py.is_heartbeat());

        let (bids, asks) = py.as_book().unwrap();
        assert_eq!(bids.len(), 1);
        assert_eq!(asks.len(), 1);
    }

    #[test]
    fn test_pymarket_data_trade() {
        let rust = MarketData::Trade {
            price: 50000.0,
            quantity: dec!(1.5),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        };
        let py: PyMarketData = rust.into();

        assert!(!py.is_book());
        assert!(py.is_trade());
        assert!(!py.is_heartbeat());

        let (price, qty, side, tid) = py.as_trade().unwrap();
        assert!((price - 50000.0).abs() < f64::EPSILON);
        assert_eq!(qty, "1.5");
        assert_eq!(side, PySide::Bid);
        assert_eq!(tid, Some("trade_123".to_string()));
    }

    #[test]
    fn test_pymarket_data_heartbeat() {
        let rust = MarketData::Heartbeat {
            exchange_time: 1234567890,
        };
        let py: PyMarketData = rust.into();

        assert!(!py.is_book());
        assert!(!py.is_trade());
        assert!(py.is_heartbeat());

        let time = py.as_heartbeat().unwrap();
        assert_eq!(time, 1234567890);
    }

    #[test]
    fn test_decimal_precision_preserved() {
        let precise = dec!(1.23456789012345678901234567);
        let level = PriceLevel::new(100.0, precise, 123);
        let py: PyPriceLevel = level.into();

        // String must preserve full precision
        let qty_str = py.quantity();
        let parsed: rust_decimal::Decimal = qty_str.parse().unwrap();
        assert_eq!(parsed, precise);
    }
}
