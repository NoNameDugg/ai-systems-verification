"""
Default In-Memory Position Provider
===================================

A minimal, self-contained position provider backed by an in-memory list.

This is the default provider used by the high-level API
(:func:`src.api.create_gate` / :meth:`CorrelationGateAPI.create`) when the
caller does not supply their own provider. It keeps the shipped ``src``
package fully self-contained: no test fixtures or external services are
required to construct a working gate.

For production use, supply a real provider (e.g.
:class:`~src.providers.oanda.OandaPositionProvider`) instead.
"""

import threading
from datetime import datetime, timezone
from typing import List, Optional

from src.models import Position
from src.providers.base import PositionProvider


class InMemoryPositionProvider(PositionProvider):
    """
    Simple in-memory position provider.

    Returns a fixed (but updatable) list of positions. Defaults to an
    empty list, which yields zero exposure. Always reports as available.

    Thread-safe for concurrent reads/updates.

    Example:
        >>> provider = InMemoryPositionProvider()
        >>> provider.fetch_positions()
        []
        >>> provider.set_positions([pos1, pos2])
        >>> len(provider.fetch_positions())
        2
    """

    def __init__(self, positions: Optional[List[Position]] = None) -> None:
        """
        Initialize with an optional list of positions.

        Args:
            positions: Initial open positions. Defaults to an empty list
                (zero exposure).
        """
        self._positions: List[Position] = list(positions) if positions else []
        self._last_fetch: Optional[datetime] = None
        self._lock = threading.RLock()

    def fetch_positions(self) -> List[Position]:
        """Return a copy of the current positions and record the fetch time."""
        with self._lock:
            self._last_fetch = datetime.now(timezone.utc)
            return list(self._positions)

    def is_available(self) -> bool:
        """In-memory provider is always available."""
        return True

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Return the time of the last successful fetch, or None if never fetched."""
        with self._lock:
            return self._last_fetch

    def set_positions(self, positions: List[Position]) -> None:
        """
        Replace the current set of positions.

        Args:
            positions: New list of open positions.
        """
        with self._lock:
            self._positions = list(positions) if positions else []
