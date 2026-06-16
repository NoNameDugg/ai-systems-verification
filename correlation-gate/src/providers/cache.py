"""
Position Cache
==============

Thread-safe caching with TTL for position data.
"""

import threading
from datetime import datetime, timedelta, timezone
from typing import List, Optional, Tuple

from src.models import Position


class PositionCache:
    """
    Thread-safe position cache with TTL.

    Features:
    - Atomic read/write with RLock
    - Staleness detection
    - Memory-bounded (no unbounded growth)
    - Copy-on-read/write to prevent mutation

    Example:
        >>> cache = PositionCache(ttl_seconds=5.0)
        >>> cache.set(positions)
        >>> result = cache.get()
        >>> if result:
        ...     positions, is_stale = result
    """

    def __init__(self, ttl_seconds: float = 5.0) -> None:
        """
        Initialize position cache.

        Args:
            ttl_seconds: Time-to-live for cached data
        """
        self._cache: Optional[List[Position]] = None
        self._fetch_time: Optional[datetime] = None
        self._ttl = timedelta(seconds=ttl_seconds)
        self._lock = threading.RLock()

    def get(self) -> Optional[Tuple[List[Position], bool]]:
        """
        Get cached positions.

        Returns:
            Tuple of (positions, is_stale) or None if cache is empty.
            Positions is a copy, safe to modify.

        Thread Safety:
            This method is thread-safe.
        """
        with self._lock:
            if self._cache is None:
                return None
            is_stale = self._is_stale()
            return (self._cache.copy(), is_stale)

    def set(self, positions: List[Position]) -> None:
        """
        Update cache with new positions.

        Args:
            positions: New position data (copied, not referenced)

        Thread Safety:
            This method is thread-safe.
        """
        with self._lock:
            self._cache = positions.copy()
            self._fetch_time = datetime.now(timezone.utc)

    def invalidate(self) -> None:
        """
        Clear the cache.

        Thread Safety:
            This method is thread-safe.
        """
        with self._lock:
            self._cache = None
            self._fetch_time = None

    def get_fetch_time(self) -> Optional[datetime]:
        """
        Get time of last cache update.

        Returns:
            Datetime when cache was last set, or None if never set.
        """
        with self._lock:
            return self._fetch_time

    def _is_stale(self) -> bool:
        """
        Check if cache has exceeded TTL.

        Returns:
            True if cache is stale or never set.

        Note:
            Must be called while holding lock.
        """
        if self._fetch_time is None:
            return True
        age = datetime.now(timezone.utc) - self._fetch_time
        return age > self._ttl
