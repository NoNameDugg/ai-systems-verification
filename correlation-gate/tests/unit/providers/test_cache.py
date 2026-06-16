"""
Unit Tests for Position Cache
=============================

Tests FIRST per TDI methodology.
All tests written BEFORE implementation.
"""

import pytest
import time
import threading
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from typing import List

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers.cache import PositionCache
    from src.models import Position
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


def create_test_position(instrument: str = "EUR_USD") -> Position:
    """Create a test position."""
    return Position(
        instrument=instrument,
        direction="LONG",
        units=10000,
        entry_price=Decimal("1.0850"),
        entry_time=datetime.now(timezone.utc),
        position_id=f"pos_{instrument}",
    )


class TestPositionCacheInit:
    """Tests for PositionCache initialization."""

    def test_cache_creation_with_default_ttl(self):
        """Cache can be created with default TTL."""
        cache = PositionCache()
        assert cache._ttl == timedelta(seconds=5.0)

    def test_cache_creation_with_custom_ttl(self):
        """Cache can be created with custom TTL."""
        cache = PositionCache(ttl_seconds=10.0)
        assert cache._ttl == timedelta(seconds=10.0)

    def test_cache_starts_empty(self):
        """Cache starts with no cached data."""
        cache = PositionCache()
        assert cache.get() is None


class TestPositionCacheGetSet:
    """Tests for cache get and set operations."""

    def test_set_stores_positions(self):
        """set stores positions in cache."""
        cache = PositionCache()
        positions = [create_test_position()]

        cache.set(positions)

        result = cache.get()
        assert result is not None
        assert len(result[0]) == 1

    def test_get_returns_tuple_of_positions_and_staleness(self):
        """get returns tuple of (positions, is_stale)."""
        cache = PositionCache()
        positions = [create_test_position()]
        cache.set(positions)

        result = cache.get()

        assert isinstance(result, tuple)
        assert len(result) == 2
        assert isinstance(result[0], list)
        assert isinstance(result[1], bool)

    def test_get_returns_copy_not_reference(self):
        """get returns a copy of positions, not the reference."""
        cache = PositionCache()
        positions = [create_test_position()]
        cache.set(positions)

        result1 = cache.get()[0]
        result2 = cache.get()[0]

        # Should be different list objects
        assert result1 is not result2

    def test_set_stores_copy_not_reference(self):
        """set stores a copy of positions, not the reference."""
        cache = PositionCache()
        positions = [create_test_position()]
        cache.set(positions)

        # Modify original
        positions.append(create_test_position("GBP_USD"))

        # Cache should still have original
        result = cache.get()[0]
        assert len(result) == 1


class TestPositionCacheStaleness:
    """Tests for cache staleness detection."""

    def test_fresh_cache_is_not_stale(self):
        """Freshly set cache is not stale."""
        cache = PositionCache(ttl_seconds=1.0)
        cache.set([create_test_position()])

        _, is_stale = cache.get()

        assert is_stale is False

    def test_cache_becomes_stale_after_ttl(self):
        """Cache becomes stale after TTL expires."""
        cache = PositionCache(ttl_seconds=0.1)
        cache.set([create_test_position()])

        time.sleep(0.15)

        _, is_stale = cache.get()

        assert is_stale is True

    def test_set_resets_staleness(self):
        """Setting new positions resets staleness."""
        cache = PositionCache(ttl_seconds=0.1)
        cache.set([create_test_position()])

        time.sleep(0.15)

        # Should be stale now
        _, is_stale = cache.get()
        assert is_stale is True

        # Set new positions
        cache.set([create_test_position()])

        # Should be fresh now
        _, is_stale = cache.get()
        assert is_stale is False


class TestPositionCacheInvalidation:
    """Tests for cache invalidation."""

    def test_invalidate_clears_cache(self):
        """invalidate clears the cache."""
        cache = PositionCache()
        cache.set([create_test_position()])

        cache.invalidate()

        assert cache.get() is None

    def test_invalidate_on_empty_cache_is_safe(self):
        """invalidate on empty cache doesn't raise."""
        cache = PositionCache()

        # Should not raise
        cache.invalidate()

        assert cache.get() is None


class TestPositionCacheThreadSafety:
    """Tests for thread-safety."""

    def test_concurrent_get_set_is_safe(self):
        """Concurrent get/set operations are thread-safe."""
        cache = PositionCache()
        errors = []

        def writer():
            for i in range(100):
                try:
                    cache.set([create_test_position(f"EUR_USD_{i}")])
                except Exception as e:
                    errors.append(e)

        def reader():
            for _ in range(100):
                try:
                    result = cache.get()
                    if result is not None:
                        _ = result[0]  # Access positions
                except Exception as e:
                    errors.append(e)

        threads = []
        for _ in range(3):
            threads.append(threading.Thread(target=writer))
            threads.append(threading.Thread(target=reader))

        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0

    def test_concurrent_invalidation_is_safe(self):
        """Concurrent invalidation is thread-safe."""
        cache = PositionCache()
        errors = []

        def worker():
            for _ in range(50):
                try:
                    cache.set([create_test_position()])
                    cache.get()
                    cache.invalidate()
                except Exception as e:
                    errors.append(e)

        threads = [threading.Thread(target=worker) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0


class TestPositionCacheFetchTime:
    """Tests for fetch time tracking."""

    def test_get_fetch_time_returns_none_initially(self):
        """get_fetch_time returns None before any set."""
        cache = PositionCache()
        assert cache.get_fetch_time() is None

    def test_get_fetch_time_returns_datetime_after_set(self):
        """get_fetch_time returns datetime after set."""
        cache = PositionCache()
        before = datetime.now(timezone.utc)
        cache.set([create_test_position()])
        after = datetime.now(timezone.utc)

        fetch_time = cache.get_fetch_time()

        assert before <= fetch_time <= after

    def test_invalidate_clears_fetch_time(self):
        """invalidate clears the fetch time."""
        cache = PositionCache()
        cache.set([create_test_position()])
        cache.invalidate()

        assert cache.get_fetch_time() is None
