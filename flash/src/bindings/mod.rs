//! Python Bindings module - PyO3 interface for Flash.
//!
//! This module provides Python bindings for the Flash library:
//!
//! - [`types`] - Core type conversions (Rust ↔ Python)
//! - [`orderbook`] - OrderBook Python API
//! - [`stream`] - Redis Stream consumer interface
//!
//! # Phase 5 Implementation
//!
//! This module is implemented in Phase 5 of the TDI roadmap:
//!
//! - Part 5.1: Core Type Conversions ✓
//! - Part 5.2: OrderBook Python API ✓
//! - Part 5.3: Async Stream Interface ✓
//! - Part 5.4: Package Distribution (maturin)
//!
//! # Python Usage
//!
//! ```python
//! from astra_flash import Exchange, Instrument, PriceLevel, Side, OrderBook
//!
//! # Create instrument
//! inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
//! print(inst.symbol())  # "BTC/USD"
//!
//! # Create order book
//! book = OrderBook(inst)
//!
//! # Apply snapshot
//! book.apply_snapshot(
//!     bids=[PriceLevel(50000.0, "1.5", 1234567890)],
//!     asks=[PriceLevel(50100.0, "2.0", 1234567890)],
//!     timestamp=1234567890
//! )
//!
//! # Query
//! print(book.best_bid)   # PriceLevel
//! print(book.mid_price)  # 50050.0
//! print(book.spread)     # 100.0
//! ```
//!
//! # Build Commands
//!
//! ```bash
//! # Development
//! maturin develop --features python
//!
//! # Release wheel
//! maturin build --release --features python
//! ```
//!
//! # Type Mappings
//!
//! | Rust Type | Python Type | Notes |
//! |-----------|-------------|-------|
//! | `Exchange` | `Exchange` | Enum |
//! | `Side` | `Side` | Enum with `opposite()` |
//! | `MarketEventType` | `MarketEventType` | Enum |
//! | `Instrument` | `Instrument` | Struct |
//! | `PriceLevel` | `PriceLevel` | Decimal as string |
//! | `MarketData` | `MarketData` | Sum type with accessors |
//! | `MarketEvent` | `MarketEvent` | Full event |
//! | `BookSnapshot` | `BookSnapshot` | Snapshot |
//! | `OrderBook` | `OrderBook` | Thread-safe order book |
//! | `OrderBookConfig` | `OrderBookConfig` | Book configuration |
//! | `OrderBookStats` | `OrderBookStats` | Book statistics |
//! | `FlashClient` | `FlashClient` | Redis stream consumer |
//! | `StreamConfig` | `StreamConfig` | Stream subscription config |
//! | `StreamIterator` | `StreamIterator` | Event iterator |
//! | `FlashClientStats` | `FlashClientStats` | Consumer statistics |

// Only compile when python feature is enabled
#![cfg(feature = "python")]

// =============================================================================
// SUBMODULES
// =============================================================================

/// Core type wrappers for Python (Part 5.1).
pub mod types;

/// OrderBook Python API (Part 5.2).
pub mod orderbook;

/// Redis Stream consumer interface (Part 5.3).
pub mod stream;

// =============================================================================
// RE-EXPORTS
// =============================================================================

pub use types::{
    PyBookSnapshot, PyExchange, PyInstrument, PyMarketData, PyMarketEvent, PyMarketEventType,
    PyPriceLevel, PySide,
};

pub use orderbook::{PyOrderBook, PyOrderBookConfig, PyOrderBookStats};

pub use stream::{PyFlashClient, PyFlashClientStats, PyStreamConfig, PyStreamIterator};

// =============================================================================
// PYMODULE DEFINITION
// =============================================================================

use pyo3::prelude::*;

/// Flash Python module.
///
/// Provides Python bindings for high-frequency market data processing.
///
/// # Types
///
/// - `Exchange` - Exchange identifiers (Deribit, Binance, Oanda)
/// - `Side` - Order book side (Bid, Ask)
/// - `MarketEventType` - Event types (Snapshot, Delta, Trade, Heartbeat)
/// - `Instrument` - Trading instrument
/// - `PriceLevel` - Price level with quantity
/// - `MarketData` - Market data payload
/// - `MarketEvent` - Complete market event
/// - `BookSnapshot` - Order book snapshot
/// - `OrderBook` - Thread-safe order book
/// - `OrderBookConfig` - Order book configuration
/// - `OrderBookStats` - Order book statistics
#[pymodule]
fn astra_flash(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Add version info
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__name__", "astra_flash")?;

    // Add enums (Part 5.1)
    m.add_class::<PyExchange>()?;
    m.add_class::<PySide>()?;
    m.add_class::<PyMarketEventType>()?;

    // Add structs (Part 5.1)
    m.add_class::<PyInstrument>()?;
    m.add_class::<PyPriceLevel>()?;
    m.add_class::<PyMarketData>()?;
    m.add_class::<PyMarketEvent>()?;
    m.add_class::<PyBookSnapshot>()?;

    // Add OrderBook types (Part 5.2)
    m.add_class::<PyOrderBook>()?;
    m.add_class::<PyOrderBookConfig>()?;
    m.add_class::<PyOrderBookStats>()?;

    // Add Stream types (Part 5.3)
    m.add_class::<PyFlashClient>()?;
    m.add_class::<PyStreamConfig>()?;
    m.add_class::<PyStreamIterator>()?;
    m.add_class::<PyFlashClientStats>()?;

    Ok(())
}
