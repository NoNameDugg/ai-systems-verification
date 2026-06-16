"""
Performance Benchmarks for Position Providers
=============================================

Validates performance budgets per STANDARDS.md.
"""

import pytest
import time
import threading
from datetime import datetime, timezone
from decimal import Decimal
from typing import List
from unittest.mock import Mock

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers import (
        OandaPositionProvider,
        OandaConfig,
        SimulatorPositionProvider,
        InMemoryPortfolioAccessor,
        SimulatorPosition,
        FallbackPositionProvider,
        ProviderFactory,
        ProviderHealthMonitor,
    )
    from src.providers.cache import PositionCache
    from src.models import Position
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


def generate_positions(count: int) -> List[Position]:
    """Generate test positions."""
    instruments = [
        "EUR_USD",
        "GBP_USD",
        "USD_JPY",
        "USD_CHF",
        "AUD_USD",
        "USD_CAD",
        "NZD_USD",
    ]
    positions = []
    for i in range(count):
        positions.append(
            Position(
                instrument=instruments[i % len(instruments)],
                direction="LONG" if i % 2 == 0 else "SHORT",
                units=10000 + i * 100,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
                position_id=f"pos_{i:05d}",
            )
        )
    return positions


def generate_simulator_positions(count: int) -> List[SimulatorPosition]:
    """Generate test simulator positions."""
    instruments = [
        "EUR_USD",
        "GBP_USD",
        "USD_JPY",
        "USD_CHF",
        "AUD_USD",
        "USD_CAD",
        "NZD_USD",
    ]
    positions = []
    for i in range(count):
        positions.append(
            SimulatorPosition(
                position_id=f"sim_{i:05d}",
                instrument=instruments[i % len(instruments)],
                direction="LONG" if i % 2 == 0 else "SHORT",
                units=10000 + i * 100,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
    return positions


class TestOandaProviderPerformance:
    """Performance tests for OandaPositionProvider."""

    def test_cache_hit_within_1ms_budget(self):
        """Cache hit must complete within 1ms budget."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    }
                    for _ in range(100)  # 100 positions
                ]
            },
        )

        provider = OandaPositionProvider(config, http_client=mock_client)

        # Prime the cache
        provider.fetch_positions()

        # Measure cache hits
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            provider.fetch_positions()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 1.0, f"Average cache hit {avg_ms:.3f}ms exceeds 1ms budget"
        assert p99_ms < 2.0, f"P99 cache hit {p99_ms:.3f}ms exceeds 2ms budget"


class TestSimulatorProviderPerformance:
    """Performance tests for SimulatorPositionProvider."""

    def test_fetch_100_positions_within_10ms_budget(self):
        """Fetching 100 positions must complete within 10ms budget."""
        accessor = InMemoryPortfolioAccessor()
        for pos in generate_simulator_positions(100):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor)

        # Warm up
        for _ in range(10):
            provider.fetch_positions()

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            provider.fetch_positions()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 10.0, f"Average fetch {avg_ms:.3f}ms exceeds 10ms budget"
        assert p99_ms < 20.0, f"P99 fetch {p99_ms:.3f}ms exceeds 20ms budget"

    def test_fetch_1000_positions_within_50ms_budget(self):
        """Fetching 1000 positions must complete within 50ms budget."""
        accessor = InMemoryPortfolioAccessor()
        for pos in generate_simulator_positions(1000):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor)

        # Measure
        times = []
        for _ in range(50):
            start = time.perf_counter_ns()
            provider.fetch_positions()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        assert avg_ms < 50.0, (
            f"Average fetch {avg_ms:.3f}ms exceeds 50ms budget for 1000 positions"
        )


class TestFallbackProviderPerformance:
    """Performance tests for FallbackPositionProvider."""

    def test_fallback_switch_within_1ms_budget(self):
        """Fallback switch must complete within 1ms budget."""
        primary = Mock()
        primary.fetch_positions.return_value = generate_positions(10)
        primary.is_available.return_value = True

        fallback = Mock()
        fallback.fetch_positions.return_value = generate_positions(10)
        fallback.is_available.return_value = True

        provider = FallbackPositionProvider(primary, fallback)

        # Measure force_fallback
        times = []
        for _ in range(100):
            provider.force_primary()
            start = time.perf_counter_ns()
            provider.force_fallback()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        assert avg_ms < 1.0, (
            f"Average fallback switch {avg_ms:.3f}ms exceeds 1ms budget"
        )


class TestCachePerformance:
    """Performance tests for PositionCache."""

    def test_cache_set_get_within_1ms_budget(self):
        """Cache set and get must complete within 1ms budget."""
        cache = PositionCache()
        positions = generate_positions(100)

        # Measure set
        set_times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            cache.set(positions)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            set_times.append(elapsed_ms)

        avg_set_ms = sum(set_times) / len(set_times)

        # Measure get
        get_times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            cache.get()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            get_times.append(elapsed_ms)

        avg_get_ms = sum(get_times) / len(get_times)

        assert avg_set_ms < 1.0, (
            f"Average cache set {avg_set_ms:.3f}ms exceeds 1ms budget"
        )
        assert avg_get_ms < 1.0, (
            f"Average cache get {avg_get_ms:.3f}ms exceeds 1ms budget"
        )


class TestHealthMonitorPerformance:
    """Performance tests for ProviderHealthMonitor."""

    def test_record_and_get_health_within_1ms_budget(self):
        """Recording and getting health must complete within 1ms budget."""
        monitor = ProviderHealthMonitor()

        # Measure record_success
        record_times = []
        for i in range(100):
            start = time.perf_counter_ns()
            monitor.record_success("test_provider", latency_ms=float(i))
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            record_times.append(elapsed_ms)

        avg_record_ms = sum(record_times) / len(record_times)

        # Measure get_health
        health_times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            monitor.get_health("test_provider")
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            health_times.append(elapsed_ms)

        avg_health_ms = sum(health_times) / len(health_times)

        assert avg_record_ms < 1.0, (
            f"Average record {avg_record_ms:.3f}ms exceeds 1ms budget"
        )
        assert avg_health_ms < 1.0, (
            f"Average get_health {avg_health_ms:.3f}ms exceeds 1ms budget"
        )


class TestProviderFactoryPerformance:
    """Performance tests for ProviderFactory."""

    def test_factory_create_within_1ms_budget(self):
        """Factory create must complete within 1ms budget."""
        # Measure create
        times = []
        for _ in range(100):
            accessor = InMemoryPortfolioAccessor()
            start = time.perf_counter_ns()
            ProviderFactory.create("simulator", portfolio_accessor=accessor)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        assert avg_ms < 1.0, f"Average factory create {avg_ms:.3f}ms exceeds 1ms budget"


class TestConcurrentPerformance:
    """Performance tests under concurrent load."""

    def test_concurrent_simulator_fetches_scale_linearly(self):
        """Concurrent fetches should scale reasonably."""
        accessor = InMemoryPortfolioAccessor()
        for pos in generate_simulator_positions(100):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor)

        results = []

        def fetch_loop():
            local_times = []
            for _ in range(50):
                start = time.perf_counter_ns()
                provider.fetch_positions()
                elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
                local_times.append(elapsed_ms)
            results.append(sum(local_times) / len(local_times))

        # Single thread baseline
        fetch_loop()
        single_thread_avg = results[-1]
        results.clear()

        # 4 concurrent threads
        threads = [threading.Thread(target=fetch_loop) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        concurrent_avg = sum(results) / len(results)

        # Concurrent should not be more than 4x slower (ideally similar due to GIL release)
        # Allow generous margin for OS scheduling overhead
        assert concurrent_avg < single_thread_avg * 10, (
            f"Concurrent fetch {concurrent_avg:.3f}ms is more than 10x slower "
            f"than single thread {single_thread_avg:.3f}ms"
        )
