"""Flash - High-Frequency Market Data Adapter.

This module provides Python bindings for the Flash library,
enabling high-performance market data processing from Rust.

Quick Start:
    >>> from astra_flash import Exchange, Instrument, OrderBook, PriceLevel, Side
    >>> inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
    >>> book = OrderBook(inst)
    >>> book.apply_snapshot(
    ...     bids=[PriceLevel(50000.0, "1.5", 1234567890)],
    ...     asks=[PriceLevel(50100.0, "2.0", 1234567890)],
    ...     timestamp=1234567890
    ... )
    >>> print(book.mid_price)  # 50050.0

Available Classes:
    Enums:
        - Exchange: Trading venue identifiers (Deribit, Binance, Oanda)
        - Side: Order book side (Bid, Ask)
        - MarketEventType: Event types (Snapshot, Delta, Trade, Heartbeat)

    Core Types:
        - Instrument: Trading instrument identifier
        - PriceLevel: Price level with quantity
        - MarketData: Market data payload
        - MarketEvent: Complete market event
        - BookSnapshot: Order book snapshot

    Order Book:
        - OrderBook: Thread-safe L2 order book
        - OrderBookConfig: Order book configuration
        - OrderBookStats: Order book statistics

    Streaming:
        - FlashClient: Redis stream consumer client
        - StreamConfig: Stream subscription configuration
        - StreamIterator: Event iterator
        - FlashClientStats: Consumer statistics

Note:
    This is a stub file for IDE support. The actual implementation
    is provided by the compiled Rust extension module.
"""

# The actual module is implemented in Rust via PyO3.
# This file exists for IDE support and documentation.
# Imports will work when the Rust extension is built.

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    # Type-checking imports for IDE support
    from astra_flash import (
        BookSnapshot,
        Exchange,
        FlashClient,
        FlashClientStats,
        Instrument,
        MarketData,
        MarketEvent,
        MarketEventType,
        OrderBook,
        OrderBookConfig,
        OrderBookStats,
        PriceLevel,
        Side,
        StreamConfig,
        StreamIterator,
    )

    __all__ = [
        # Module info
        "__version__",
        "__name__",
        # Enums
        "Exchange",
        "Side",
        "MarketEventType",
        # Core types
        "Instrument",
        "PriceLevel",
        "MarketData",
        "MarketEvent",
        "BookSnapshot",
        # Order book
        "OrderBook",
        "OrderBookConfig",
        "OrderBookStats",
        # Streaming
        "FlashClient",
        "StreamConfig",
        "StreamIterator",
        "FlashClientStats",
    ]
