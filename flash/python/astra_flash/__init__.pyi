"""Type stubs for astra_flash - High-Frequency Market Data Adapter.

This module provides Python bindings for the Flash library, enabling
high-performance market data processing from Rust.

Example:
    >>> from astra_flash import Exchange, Instrument, OrderBook, PriceLevel, Side
    >>> inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
    >>> book = OrderBook(inst)
    >>> book.apply_snapshot(
    ...     bids=[PriceLevel(50000.0, "1.5", 1234567890)],
    ...     asks=[PriceLevel(50100.0, "2.0", 1234567890)],
    ...     timestamp=1234567890
    ... )
    >>> print(book.mid_price)  # 50050.0
"""

from typing import Iterator, List, Literal, Optional, Union

# Module metadata
__version__: str
__name__: str

# =============================================================================
# ENUMS
# =============================================================================

class Exchange:
    """Exchange identifiers for supported trading venues.

    Supported exchanges:
        - Deribit: Cryptocurrency derivatives exchange
        - Binance: Cryptocurrency spot and futures exchange
        - Oanda: Forex trading platform

    Example:
        >>> exchange = Exchange.Deribit
        >>> print(exchange)
        Deribit
    """

    Deribit: Exchange
    Binance: Exchange
    Oanda: Exchange

    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class Side:
    """Order book side (Bid or Ask).

    Attributes:
        Bid: Buy side of the order book (highest prices first)
        Ask: Sell side of the order book (lowest prices first)

    Example:
        >>> side = Side.Bid
        >>> opposite = side.opposite()  # Side.Ask
    """

    Bid: Side
    Ask: Side

    def opposite(self) -> Side:
        """Return the opposite side.

        Returns:
            Side: Bid if self is Ask, Ask if self is Bid
        """
        ...

    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class MarketEventType:
    """Types of market events.

    Event types:
        - Snapshot: Full order book state
        - Delta: Incremental order book update
        - Trade: Trade execution
        - Heartbeat: Connection health check

    Example:
        >>> event_type = MarketEventType.Snapshot
        >>> if event_type == MarketEventType.Snapshot:
        ...     print("Processing full book snapshot")
    """

    Snapshot: MarketEventType
    Delta: MarketEventType
    Trade: MarketEventType
    Heartbeat: MarketEventType

    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

# =============================================================================
# CORE TYPES
# =============================================================================

class Instrument:
    """Trading instrument identifier.

    Represents a tradeable instrument on a specific exchange.

    Args:
        base: Base currency (e.g., "BTC", "ETH")
        quote: Quote currency (e.g., "USD", "USDT")
        exchange: Exchange where the instrument trades
        raw_symbol: Exchange-specific symbol (e.g., "BTC-PERPETUAL", "BTCUSDT")

    Example:
        >>> inst = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
        >>> print(inst.symbol())  # "BTC/USD"
        >>> print(inst.raw_symbol)  # "BTC-PERPETUAL"
    """

    def __init__(
        self,
        base: str,
        quote: str,
        exchange: Exchange,
        raw_symbol: str,
    ) -> None: ...
    @property
    def base(self) -> str:
        """Base currency code."""
        ...

    @property
    def quote(self) -> str:
        """Quote currency code."""
        ...

    @property
    def exchange(self) -> Exchange:
        """Exchange identifier."""
        ...

    @property
    def raw_symbol(self) -> str:
        """Exchange-specific symbol."""
        ...

    def symbol(self) -> str:
        """Get normalized symbol in BASE/QUOTE format.

        Returns:
            str: Symbol like "BTC/USD"
        """
        ...

    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class PriceLevel:
    """A single price level in the order book.

    Represents a price point with quantity and optional order count.

    Args:
        price: Price value as float
        quantity: Quantity as string (preserves decimal precision)
        timestamp: Microseconds since epoch
        order_count: Optional number of orders at this level

    Example:
        >>> level = PriceLevel(50000.0, "1.5", 1234567890)
        >>> print(f"Price: {level.price}, Qty: {level.quantity}")
    """

    def __init__(
        self,
        price: float,
        quantity: str,
        timestamp: int,
        order_count: Optional[int] = None,
    ) -> None: ...
    @property
    def price(self) -> float:
        """Price value."""
        ...

    @property
    def quantity(self) -> str:
        """Quantity as string (preserves decimal precision)."""
        ...

    @property
    def timestamp(self) -> int:
        """Timestamp in microseconds since epoch."""
        ...

    @property
    def order_count(self) -> Optional[int]:
        """Number of orders at this level (if available)."""
        ...

    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class MarketData:
    """Market data payload from an event.

    Contains either order book data, trade data, or heartbeat data.
    Use the `data_type` property to determine the payload type.

    Example:
        >>> if data.data_type == "book":
        ...     for bid in data.bids:
        ...         print(f"Bid: {bid.price} @ {bid.quantity}")
    """

    @property
    def data_type(self) -> Literal["book", "trade", "heartbeat"]:
        """Type of market data."""
        ...

    @property
    def bids(self) -> Optional[List[PriceLevel]]:
        """Bid price levels (book data only)."""
        ...

    @property
    def asks(self) -> Optional[List[PriceLevel]]:
        """Ask price levels (book data only)."""
        ...

    @property
    def trade_price(self) -> Optional[float]:
        """Trade price (trade data only)."""
        ...

    @property
    def trade_quantity(self) -> Optional[str]:
        """Trade quantity (trade data only)."""
        ...

    @property
    def trade_side(self) -> Optional[Side]:
        """Trade aggressor side (trade data only)."""
        ...

    @property
    def trade_id(self) -> Optional[str]:
        """Exchange trade ID (trade data only)."""
        ...

    @property
    def exchange_time(self) -> Optional[int]:
        """Exchange timestamp (heartbeat data only)."""
        ...

    def __repr__(self) -> str: ...

class MarketEvent:
    """Complete market event with metadata.

    Contains the event type, instrument, timestamps, and market data payload.

    Example:
        >>> latency = event.latency_micros()
        >>> print(f"Event latency: {latency}us")
    """

    @property
    def event_type(self) -> MarketEventType:
        """Type of market event."""
        ...

    @property
    def instrument(self) -> Instrument:
        """Instrument this event relates to."""
        ...

    @property
    def timestamp(self) -> int:
        """Exchange timestamp in microseconds."""
        ...

    @property
    def local_timestamp(self) -> int:
        """Local receive timestamp in microseconds."""
        ...

    @property
    def sequence(self) -> Optional[int]:
        """Exchange sequence number (if available)."""
        ...

    @property
    def data(self) -> MarketData:
        """Market data payload."""
        ...

    def latency_micros(self) -> int:
        """Calculate latency between exchange and local timestamp.

        Returns:
            int: Latency in microseconds
        """
        ...

    def __repr__(self) -> str: ...

class BookSnapshot:
    """Order book snapshot with bid and ask levels.

    Represents a point-in-time state of an order book.

    Args:
        instrument: The trading instrument
        timestamp: Snapshot timestamp in microseconds
        bids: List of bid price levels (highest first)
        asks: List of ask price levels (lowest first)

    Example:
        >>> snapshot = book.snapshot(depth=10)
        >>> print(f"Mid price: {snapshot.mid_price()}")
        >>> print(f"Spread: {snapshot.spread()}")
    """

    def __init__(
        self,
        instrument: Instrument,
        timestamp: int,
        bids: List[PriceLevel],
        asks: List[PriceLevel],
    ) -> None: ...
    @property
    def instrument(self) -> Instrument:
        """The trading instrument."""
        ...

    @property
    def timestamp(self) -> int:
        """Snapshot timestamp in microseconds."""
        ...

    @property
    def bids(self) -> List[PriceLevel]:
        """Bid price levels (highest first)."""
        ...

    @property
    def asks(self) -> List[PriceLevel]:
        """Ask price levels (lowest first)."""
        ...

    def best_bid(self) -> Optional[PriceLevel]:
        """Get the best (highest) bid level.

        Returns:
            Optional[PriceLevel]: Best bid or None if no bids
        """
        ...

    def best_ask(self) -> Optional[PriceLevel]:
        """Get the best (lowest) ask level.

        Returns:
            Optional[PriceLevel]: Best ask or None if no asks
        """
        ...

    def mid_price(self) -> Optional[float]:
        """Calculate the mid price.

        Returns:
            Optional[float]: (best_bid + best_ask) / 2, or None if empty
        """
        ...

    def spread(self) -> Optional[float]:
        """Calculate the bid-ask spread.

        Returns:
            Optional[float]: best_ask - best_bid, or None if empty
        """
        ...

    def __repr__(self) -> str: ...

# =============================================================================
# ORDER BOOK TYPES
# =============================================================================

class OrderBookConfig:
    """Configuration for order book behavior.

    Args:
        max_depth: Maximum levels for snapshot serialization (default: 50)
        max_levels: Hard limit on price levels per side (default: 100)
        track_orders: Track individual orders (L3 data, default: False)
        auto_prune: Remove zero-quantity levels (default: True)

    Example:
        >>> config = OrderBookConfig(max_depth=25, max_levels=50)
        >>> book = OrderBook(instrument, config)
    """

    def __init__(
        self,
        max_depth: int = 50,
        max_levels: int = 100,
        track_orders: bool = False,
        auto_prune: bool = True,
    ) -> None: ...
    @property
    def max_depth(self) -> int:
        """Maximum levels for snapshot serialization."""
        ...

    @property
    def max_levels(self) -> int:
        """Hard limit on price levels per side."""
        ...

    @property
    def track_orders(self) -> bool:
        """Whether to track individual orders."""
        ...

    @property
    def auto_prune(self) -> bool:
        """Whether to remove zero-quantity levels."""
        ...

    def __repr__(self) -> str: ...

class OrderBookStats:
    """Statistics about order book operations.

    Tracks update counts, timestamps, and current state.

    Example:
        >>> stats = book.stats()
        >>> print(f"Updates: {stats.update_count}")
        >>> print(f"Bid levels: {stats.bid_levels}")
    """

    @property
    def update_count(self) -> int:
        """Total number of updates applied."""
        ...

    @property
    def snapshot_count(self) -> int:
        """Number of snapshots applied."""
        ...

    @property
    def delta_count(self) -> int:
        """Number of delta updates applied."""
        ...

    @property
    def last_update_timestamp(self) -> int:
        """Timestamp of last update in microseconds."""
        ...

    @property
    def bid_levels(self) -> int:
        """Current number of bid price levels."""
        ...

    @property
    def ask_levels(self) -> int:
        """Current number of ask price levels."""
        ...

    def __repr__(self) -> str: ...

class OrderBook:
    """Thread-safe L2 order book with microsecond performance.

    Maintains bid and ask price levels with O(log n) updates
    and O(1) best price lookups.

    Args:
        instrument: The trading instrument
        config: Optional configuration (uses defaults if None)

    Example:
        >>> book = OrderBook(instrument)
        >>> book.apply_snapshot(bids, asks, timestamp)
        >>> print(f"Mid: {book.mid_price}, Spread: {book.spread}")
    """

    def __init__(
        self,
        instrument: Instrument,
        config: Optional[OrderBookConfig] = None,
    ) -> None: ...
    @property
    def instrument(self) -> Instrument:
        """The trading instrument."""
        ...

    @property
    def best_bid(self) -> Optional[PriceLevel]:
        """Best (highest) bid price level."""
        ...

    @property
    def best_ask(self) -> Optional[PriceLevel]:
        """Best (lowest) ask price level."""
        ...

    @property
    def mid_price(self) -> Optional[float]:
        """Mid price: (best_bid + best_ask) / 2."""
        ...

    @property
    def spread(self) -> Optional[float]:
        """Bid-ask spread in price units."""
        ...

    @property
    def spread_bps(self) -> Optional[float]:
        """Bid-ask spread in basis points."""
        ...

    def apply_snapshot(
        self,
        bids: List[PriceLevel],
        asks: List[PriceLevel],
        timestamp: int,
    ) -> None:
        """Apply a full order book snapshot.

        Replaces all existing price levels with the new data.

        Args:
            bids: Bid price levels (highest first)
            asks: Ask price levels (lowest first)
            timestamp: Snapshot timestamp in microseconds
        """
        ...

    def apply_delta(
        self,
        side: Side,
        levels: List[PriceLevel],
        timestamp: int,
    ) -> None:
        """Apply an incremental update to one side.

        Levels with quantity > 0 are inserted/updated.
        Levels with quantity = 0 are removed (if auto_prune enabled).

        Args:
            side: Which side to update (Bid or Ask)
            levels: Price levels to apply
            timestamp: Update timestamp in microseconds
        """
        ...

    def top_bids(self, n: int = 10) -> List[PriceLevel]:
        """Get top N bid levels.

        Args:
            n: Number of levels to return

        Returns:
            List[PriceLevel]: Best bid levels (highest first)
        """
        ...

    def top_asks(self, n: int = 10) -> List[PriceLevel]:
        """Get top N ask levels.

        Args:
            n: Number of levels to return

        Returns:
            List[PriceLevel]: Best ask levels (lowest first)
        """
        ...

    def snapshot(self, depth: Optional[int] = None) -> BookSnapshot:
        """Create a snapshot of the current book state.

        Args:
            depth: Optional depth limit (uses config.max_depth if None)

        Returns:
            BookSnapshot: Current book state
        """
        ...

    def stats(self) -> OrderBookStats:
        """Get current order book statistics.

        Returns:
            OrderBookStats: Update counts and level counts
        """
        ...

    def clear(self) -> None:
        """Clear all price levels from the book."""
        ...

    def __repr__(self) -> str: ...

# =============================================================================
# STREAM TYPES
# =============================================================================

class StreamConfig:
    """Configuration for Redis stream subscription.

    Args:
        topics: List of Redis stream keys to subscribe to
        format: Serialization format ("bincode" or "json")
        start_id: Starting stream ID ("$" for new, "0" for beginning)
        block_ms: Blocking timeout in milliseconds
        count: Maximum messages per read
        group_name: Optional consumer group name
        consumer_name: Optional consumer name within group

    Example:
        >>> config = StreamConfig(
        ...     topics=["market_data.deribit.btc_usd.book"],
        ...     format="bincode",
        ...     start_id="$",
        ...     block_ms=5000,
        ...     count=100
        ... )
    """

    def __init__(
        self,
        topics: List[str],
        format: str = "bincode",
        start_id: str = "$",
        block_ms: int = 5000,
        count: int = 100,
        group_name: Optional[str] = None,
        consumer_name: Optional[str] = None,
    ) -> None: ...
    def topics(self) -> List[str]:
        """Get the list of subscribed topics."""
        ...

    def format(self) -> str:
        """Get the serialization format."""
        ...

    def start_id(self) -> str:
        """Get the starting stream ID."""
        ...

    def block_ms(self) -> int:
        """Get the blocking timeout in milliseconds."""
        ...

    def count(self) -> int:
        """Get the maximum messages per read."""
        ...

    def group_name(self) -> Optional[str]:
        """Get the consumer group name."""
        ...

    def consumer_name(self) -> Optional[str]:
        """Get the consumer name."""
        ...

    @staticmethod
    def validate_topics(topics: List[str]) -> None:
        """Validate that topics list is non-empty.

        Args:
            topics: List of topics to validate

        Raises:
            ValueError: If topics list is empty
        """
        ...

class FlashClientStats:
    """Statistics for FlashClient operations.

    Tracks message counts, byte counts, errors, and latency.

    Example:
        >>> stats = client.stats()
        >>> print(f"Messages: {stats.messages_received()}")
        >>> print(f"Avg latency: {stats.avg_latency_us()}us")
    """

    def __init__(self) -> None: ...
    def messages_received(self) -> int:
        """Total messages received."""
        ...

    def bytes_deserialized(self) -> int:
        """Total bytes deserialized."""
        ...

    def deserialize_errors(self) -> int:
        """Number of deserialization errors."""
        ...

    def connection_errors(self) -> int:
        """Number of connection errors."""
        ...

    def avg_latency_us(self) -> float:
        """Average processing latency in microseconds."""
        ...

    def record_message(self, bytes: int) -> None:
        """Record a received message.

        Args:
            bytes: Size of the message in bytes
        """
        ...

    def record_error(self) -> None:
        """Record an error occurrence."""
        ...

    def update_latency(self, latency_us: float) -> None:
        """Update the latency moving average.

        Args:
            latency_us: Latency in microseconds
        """
        ...

    def reset(self) -> None:
        """Reset all statistics to zero."""
        ...

class StreamIterator:
    """Iterator for streaming market events from Redis.

    Implements the iterator protocol for consuming market events.
    The iterator blocks waiting for new events up to the configured timeout.

    Example:
        >>> for event in client.subscribe(config):
        ...     print(f"Received: {event.event_type}")
        ...     if not iterator.is_active():
        ...         break
    """

    def is_active(self) -> bool:
        """Check if the iterator is still active.

        Returns:
            bool: True if active, False if stopped
        """
        ...

    def stop(self) -> None:
        """Stop the iterator.

        Safe to call multiple times. After stopping, the iterator
        will return no more events.
        """
        ...

    def config(self) -> StreamConfig:
        """Get the subscription configuration.

        Returns:
            StreamConfig: The configuration used for this subscription
        """
        ...

    def topic_count(self) -> int:
        """Get the number of subscribed topics.

        Returns:
            int: Number of topics being consumed
        """
        ...

    def received_count(self) -> int:
        """Get the number of messages received.

        Returns:
            int: Total messages received by this iterator
        """
        ...

    def __iter__(self) -> Iterator[MarketEvent]: ...
    def __next__(self) -> MarketEvent: ...

class FlashClient:
    """Redis stream consumer client for market data.

    Connects to Redis and subscribes to market data streams.
    Supports both simple subscriptions and consumer groups.

    Args:
        redis_url: Redis connection URL (e.g., "redis://localhost:6379")

    Example:
        >>> client = FlashClient("redis://localhost:6379")
        >>> for event in client.subscribe_one("market_data.deribit.btc_usd.book"):
        ...     print(f"Mid price: {event.data.bids[0].price if event.data.bids else 'N/A'}")
    """

    def __init__(self, redis_url: str) -> None: ...
    def subscribe(self, config: StreamConfig) -> StreamIterator:
        """Subscribe to streams with full configuration.

        Args:
            config: Stream subscription configuration

        Returns:
            StreamIterator: Iterator for consuming events
        """
        ...

    def subscribe_one(self, topic: str) -> StreamIterator:
        """Subscribe to a single stream with defaults.

        Convenience method for simple single-topic subscriptions.

        Args:
            topic: Redis stream key to subscribe to

        Returns:
            StreamIterator: Iterator for consuming events
        """
        ...

    def stats(self) -> FlashClientStats:
        """Get client statistics.

        Returns:
            FlashClientStats: Current statistics
        """
        ...

    def close(self) -> None:
        """Close the client and stop all iterators.

        Stops all active iterators created by this client.
        """
        ...
