"""
Mock Position Providers for Testing
====================================

Test fixtures for simulating various position provider behaviors.
"""

import time
from datetime import datetime, timezone
from decimal import Decimal
from typing import List, Optional, Any
from abc import ABC, abstractmethod


class PositionProviderBase(ABC):
    """Abstract base for position providers (mirrors src interface)."""

    @abstractmethod
    def fetch_positions(self) -> List[Any]:
        """Fetch current open positions."""
        pass

    @abstractmethod
    def is_available(self) -> bool:
        """Check if provider is available."""
        pass

    @abstractmethod
    def get_last_fetch_time(self) -> Optional[datetime]:
        """Get time of last successful fetch."""
        pass


class MockPositionProvider(PositionProviderBase):
    """
    Mock provider that returns pre-configured positions.

    Usage:
        provider = MockPositionProvider(positions=[pos1, pos2])
        positions = provider.fetch_positions()
    """

    def __init__(self, positions: List[Any]) -> None:
        """
        Initialize with list of positions to return.

        Args:
            positions: List of Position objects to return from fetch
        """
        self._positions = positions
        self._last_fetch: Optional[datetime] = None

    def fetch_positions(self) -> List[Any]:
        """Return configured positions."""
        self._last_fetch = datetime.now(timezone.utc)
        return self._positions.copy()

    def is_available(self) -> bool:
        """Always available."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Return last fetch time."""
        return self._last_fetch

    def set_positions(self, positions: List[Any]) -> None:
        """Update positions for dynamic test scenarios."""
        self._positions = positions


class FailingPositionProvider(PositionProviderBase):
    """
    Provider that always raises an exception.

    Used to test error handling paths.
    """

    def __init__(self, exception: Optional[Exception] = None) -> None:
        """
        Initialize with optional custom exception.

        Args:
            exception: Exception to raise, defaults to RuntimeError
        """
        self._exception = exception or RuntimeError("Provider unavailable")

    def fetch_positions(self) -> List[Any]:
        """Always raises configured exception."""
        raise self._exception

    def is_available(self) -> bool:
        """Always unavailable."""
        return False

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Never fetched successfully."""
        return None


class SlowPositionProvider(PositionProviderBase):
    """
    Provider that introduces artificial delay.

    Used to test timeout scenarios and concurrent access.
    """

    def __init__(self, delay_seconds: float, positions: List[Any]) -> None:
        """
        Initialize with delay and positions.

        Args:
            delay_seconds: Seconds to delay before returning
            positions: Positions to return after delay
        """
        self._delay = delay_seconds
        self._positions = positions
        self._last_fetch: Optional[datetime] = None

    def fetch_positions(self) -> List[Any]:
        """Return positions after configured delay."""
        time.sleep(self._delay)
        self._last_fetch = datetime.now(timezone.utc)
        return self._positions.copy()

    def is_available(self) -> bool:
        """Available but slow."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Return last fetch time."""
        return self._last_fetch


class TimeoutPositionProvider(PositionProviderBase):
    """
    Provider that simulates timeout by sleeping longer than timeout.

    Used to test timeout handling in gate evaluation.
    """

    def __init__(self, timeout_seconds: float) -> None:
        """
        Initialize with timeout duration.

        Args:
            timeout_seconds: How long to sleep (should exceed gate timeout)
        """
        self._timeout = timeout_seconds

    def fetch_positions(self) -> List[Any]:
        """Sleep to simulate timeout, never returns normally."""
        time.sleep(self._timeout)
        return []  # If we get here, timeout wasn't enforced

    def is_available(self) -> bool:
        """Appears available."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Never completes successfully."""
        return None


class ExceptionPositionProvider(PositionProviderBase):
    """
    Provider that raises a specific exception type.

    Used to test exception handling for various error types.
    """

    def __init__(self, exception: Exception) -> None:
        """
        Initialize with specific exception to raise.

        Args:
            exception: The exact exception instance to raise
        """
        self._exception = exception

    def fetch_positions(self) -> List[Any]:
        """Raise configured exception."""
        raise self._exception

    def is_available(self) -> bool:
        """Appears available but will fail."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Never fetched successfully."""
        return None


class StatefulPositionProvider(PositionProviderBase):
    """
    Provider that can change state during test execution.

    Used to test dynamic scenarios like position changes during evaluation.
    """

    def __init__(self) -> None:
        """Initialize with empty state."""
        self._positions: List[Any] = []
        self._fail_next = False
        self._delay_next = 0.0
        self._last_fetch: Optional[datetime] = None

    def fetch_positions(self) -> List[Any]:
        """Return positions with optional fail/delay."""
        if self._fail_next:
            self._fail_next = False
            raise RuntimeError("Intentional failure")

        if self._delay_next > 0:
            time.sleep(self._delay_next)
            self._delay_next = 0.0

        self._last_fetch = datetime.now(timezone.utc)
        return self._positions.copy()

    def is_available(self) -> bool:
        """Check configured availability."""
        return not self._fail_next

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Return last fetch time."""
        return self._last_fetch

    def set_positions(self, positions: List[Any]) -> None:
        """Update positions."""
        self._positions = positions

    def set_fail_next(self) -> None:
        """Configure next fetch to fail."""
        self._fail_next = True

    def set_delay_next(self, seconds: float) -> None:
        """Configure next fetch to delay."""
        self._delay_next = seconds


class MockPriceProvider:
    """
    Mock price provider for XAU and other instruments.

    Used to test dynamic notional calculations.
    """

    def __init__(self, prices: Optional[dict] = None) -> None:
        """
        Initialize with optional price dictionary.

        Args:
            prices: Dict of instrument -> Decimal price
        """
        self._prices = prices or {
            "XAU_USD": Decimal("2000.00"),
            "EUR_USD": Decimal("1.0850"),
            "GBP_USD": Decimal("1.2650"),
            "USD_JPY": Decimal("149.50"),
            "USD_CHF": Decimal("0.8750"),
            "AUD_USD": Decimal("0.6550"),
            "USD_CAD": Decimal("1.3650"),
            "NZD_USD": Decimal("0.6100"),
        }
        self._fail_next = False

    def get_price(self, instrument: str) -> Any:
        """Get price for instrument."""
        if self._fail_next:
            self._fail_next = False
            raise RuntimeError("Price fetch failed")

        if instrument not in self._prices:
            raise KeyError(f"Unknown instrument: {instrument}")

        # Return mock PriceSnapshot-like object
        return MockPriceSnapshot(
            instrument=instrument, mid_price=self._prices[instrument]
        )

    def set_price(self, instrument: str, price: Decimal) -> None:
        """Update price for instrument."""
        self._prices[instrument] = price

    def set_fail_next(self) -> None:
        """Configure next fetch to fail."""
        self._fail_next = True


class MockPriceSnapshot:
    """Mock price snapshot for testing."""

    def __init__(self, instrument: str, mid_price: Decimal, age_ms: int = 0) -> None:
        self.instrument = instrument
        self.mid_price = mid_price
        self.bid = mid_price - Decimal("0.0001")
        self.ask = mid_price + Decimal("0.0001")
        self.timestamp = datetime.now(timezone.utc)
        self._age_ms = age_ms

    @property
    def age_ms(self) -> int:
        """Return configured age."""
        return self._age_ms

    @property
    def is_stale(self) -> bool:
        """Stale if > 5 seconds old."""
        return self._age_ms > 5000
