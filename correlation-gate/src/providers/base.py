"""
Position Provider Base Classes
==============================

Abstract interfaces for position providers.
"""

from abc import ABC, abstractmethod
from datetime import datetime
from typing import List, Optional

from src.models import Position


class PositionProvider(ABC):
    """
    Abstract interface for position providers.

    Implementations fetch positions from various sources:
    - OANDA REST API (production)
    - MarketSim portfolio (simulation)
    - Mock providers (testing)

    All implementations must be:
    - Thread-safe
    - Fail-closed (errors don't return stale data silently)
    - Timeout-aware

    Example:
        >>> provider = OandaPositionProvider(config)
        >>> positions = provider.fetch_positions()
        >>> print(f"Found {len(positions)} positions")
    """

    @abstractmethod
    def fetch_positions(self) -> List[Position]:
        """
        Fetch current open positions.

        Returns:
            List of Position objects representing current open positions.
            Empty list if no positions are open.

        Raises:
            ProviderError: On non-recoverable errors
            ProviderTimeoutError: If operation times out
            ProviderAuthError: If authentication fails

        Performance:
            API calls: < 500ms
            Cache hit: < 1ms
        """
        raise NotImplementedError

    @abstractmethod
    def is_available(self) -> bool:
        """
        Check if provider is available.

        Returns:
            True if provider can fetch positions, False otherwise.
            This is a quick health check, not a full connectivity test.
        """
        raise NotImplementedError

    @abstractmethod
    def get_last_fetch_time(self) -> Optional[datetime]:
        """
        Get time of last successful fetch.

        Returns:
            Datetime of last successful fetch, or None if never fetched.
        """
        raise NotImplementedError
