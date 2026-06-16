"""
Simulator Position Provider
===========================

Position provider for MarketSim integration.
"""

import logging
import threading
import uuid
from abc import ABC, abstractmethod
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal
from typing import Callable, Dict, List, Optional

from src.models import Position
from src.providers.base import PositionProvider
from src.providers.cache import PositionCache


logger = logging.getLogger(__name__)


@dataclass
class SimulatorPosition:
    """Simulator position data structure."""

    position_id: str
    instrument: str
    direction: str  # "LONG" or "SHORT"
    units: int
    entry_price: Decimal
    entry_time: datetime
    unrealized_pnl: Decimal = Decimal("0")
    margin_used: Decimal = Decimal("0")


@dataclass
class PositionChangeEvent:
    """Position change notification."""

    event_type: str  # "OPENED", "CLOSED", "MODIFIED"
    position: SimulatorPosition
    timestamp: datetime


class PortfolioAccessor(ABC):
    """
    Abstract interface for accessing simulator portfolio.

    This decouples the provider from MarketSim internals.
    """

    @abstractmethod
    def get_open_positions(self) -> List[SimulatorPosition]:
        """Get all open positions from simulator."""
        raise NotImplementedError

    @abstractmethod
    def get_position_by_id(self, position_id: str) -> Optional[SimulatorPosition]:
        """Get specific position by ID."""
        raise NotImplementedError

    @abstractmethod
    def add_change_listener(
        self, listener: Callable[[PositionChangeEvent], None]
    ) -> str:
        """Add position change listener. Returns listener ID."""
        raise NotImplementedError

    @abstractmethod
    def remove_change_listener(self, listener_id: str) -> bool:
        """Remove position change listener."""
        raise NotImplementedError


class InMemoryPortfolioAccessor(PortfolioAccessor):
    """
    In-memory portfolio accessor for testing.

    Provides a simple implementation that doesn't require MarketSim.
    """

    def __init__(self) -> None:
        """Initialize with empty portfolio."""
        self._positions: Dict[str, SimulatorPosition] = {}
        self._listeners: Dict[str, Callable[[PositionChangeEvent], None]] = {}
        self._lock = threading.RLock()

    def get_open_positions(self) -> List[SimulatorPosition]:
        """Get all open positions."""
        with self._lock:
            return list(self._positions.values())

    def get_position_by_id(self, position_id: str) -> Optional[SimulatorPosition]:
        """Get specific position by ID."""
        with self._lock:
            return self._positions.get(position_id)

    def add_position(self, position: SimulatorPosition) -> None:
        """
        Add position to portfolio.

        Args:
            position: Position to add
        """
        with self._lock:
            self._positions[position.position_id] = position
            self._notify_listeners(
                PositionChangeEvent(
                    event_type="OPENED",
                    position=position,
                    timestamp=datetime.now(timezone.utc),
                )
            )

    def remove_position(self, position_id: str) -> None:
        """
        Remove position from portfolio.

        Args:
            position_id: ID of position to remove
        """
        with self._lock:
            if position_id in self._positions:
                position = self._positions.pop(position_id)
                self._notify_listeners(
                    PositionChangeEvent(
                        event_type="CLOSED",
                        position=position,
                        timestamp=datetime.now(timezone.utc),
                    )
                )

    def add_change_listener(
        self, listener: Callable[[PositionChangeEvent], None]
    ) -> str:
        """Add position change listener."""
        with self._lock:
            listener_id = str(uuid.uuid4())
            self._listeners[listener_id] = listener
            return listener_id

    def remove_change_listener(self, listener_id: str) -> bool:
        """Remove position change listener."""
        with self._lock:
            if listener_id in self._listeners:
                del self._listeners[listener_id]
                return True
            return False

    def _notify_listeners(self, event: PositionChangeEvent) -> None:
        """Notify all listeners of position change."""
        listeners = list(self._listeners.values())
        for listener in listeners:
            try:
                listener(event)
            except Exception as e:
                logger.warning(f"Listener error: {e}")


class SimulatorPositionProvider(PositionProvider):
    """
    Position provider for MarketSim simulator.

    Features:
    - Direct integration with simulator portfolio
    - Support for mock data injection
    - Position change notifications
    - Thread-safe operations

    Example:
        >>> accessor = InMemoryPortfolioAccessor()
        >>> provider = SimulatorPositionProvider(accessor)
        >>> positions = provider.fetch_positions()
    """

    def __init__(
        self, portfolio_accessor: PortfolioAccessor, cache_ttl_seconds: float = 1.0
    ) -> None:
        """
        Initialize simulator provider.

        Args:
            portfolio_accessor: Interface to simulator portfolio
            cache_ttl_seconds: Cache TTL (shorter for simulation)
        """
        self._accessor = portfolio_accessor
        self._cache_ttl_seconds = cache_ttl_seconds
        self._cache = PositionCache(ttl_seconds=cache_ttl_seconds)
        self._last_fetch_time: Optional[datetime] = None
        self._injected_positions: Optional[List[Position]] = None
        self._subscriptions: Dict[str, str] = {}  # our_id -> accessor_id
        self._change_callbacks: Dict[str, Callable[[List[Position]], None]] = {}
        self._lock = threading.RLock()

    def fetch_positions(self) -> List[Position]:
        """
        Fetch current positions from simulator.

        Returns:
            List of Position objects
        """
        with self._lock:
            # Check for injected positions first
            if self._injected_positions is not None:
                return self._injected_positions.copy()

            # Check cache
            cached = self._cache.get()
            if cached is not None:
                positions, is_stale = cached
                if not is_stale:
                    return positions

            # Fetch from accessor
            sim_positions = self._accessor.get_open_positions()
            positions = [self._convert_position(sp) for sp in sim_positions]

            self._cache.set(positions)
            self._last_fetch_time = datetime.now(timezone.utc)
            return positions

    def is_available(self) -> bool:
        """Simulator is always available."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Get time of last successful fetch."""
        return self._last_fetch_time

    def inject_positions(self, positions: List[Position]) -> None:
        """
        Inject mock positions for testing.

        Args:
            positions: Positions to inject (overrides portfolio)
        """
        with self._lock:
            self._injected_positions = positions.copy()

    def clear_injection(self) -> None:
        """Clear injected positions, return to using portfolio."""
        with self._lock:
            self._injected_positions = None

    def subscribe_changes(self, callback: Callable[[List[Position]], None]) -> str:
        """
        Subscribe to position change notifications.

        Args:
            callback: Function called with updated positions on change

        Returns:
            Subscription ID for unsubscribe
        """
        with self._lock:
            sub_id = str(uuid.uuid4())
            self._change_callbacks[sub_id] = callback

            # Set up accessor listener if not already done
            if not self._subscriptions:
                accessor_id = self._accessor.add_change_listener(
                    self._on_position_change
                )
                self._subscriptions[sub_id] = accessor_id
            else:
                # Reuse existing accessor listener
                self._subscriptions[sub_id] = list(self._subscriptions.values())[0]

            return sub_id

    def unsubscribe_changes(self, subscription_id: str) -> bool:
        """
        Unsubscribe from position changes.

        Args:
            subscription_id: ID from subscribe_changes

        Returns:
            True if unsubscribed, False if not found
        """
        with self._lock:
            if subscription_id not in self._change_callbacks:
                return False

            del self._change_callbacks[subscription_id]

            # Remove accessor listener if no more subscribers
            if subscription_id in self._subscriptions:
                accessor_id = self._subscriptions.pop(subscription_id)
                if not self._subscriptions:
                    self._accessor.remove_change_listener(accessor_id)

            return True

    def _on_position_change(self, event: PositionChangeEvent) -> None:
        """Handle position change from accessor."""
        # Invalidate cache
        self._cache.invalidate()

        # Notify subscribers
        with self._lock:
            callbacks = list(self._change_callbacks.values())

        positions = self.fetch_positions()
        for callback in callbacks:
            try:
                callback(positions)
            except Exception as e:
                logger.warning(f"Subscriber callback error: {e}")

    def _convert_position(self, sim_pos: SimulatorPosition) -> Position:
        """Convert SimulatorPosition to Position."""
        return Position(
            instrument=sim_pos.instrument,
            direction=sim_pos.direction,
            units=sim_pos.units,
            entry_price=sim_pos.entry_price,
            entry_time=sim_pos.entry_time,
            position_id=sim_pos.position_id,
        )
