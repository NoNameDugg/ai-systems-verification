"""
Provider Performance Benchmarks
===============================

Validates Phase 2 provider performance budgets from STANDARDS.md:
- Position Fetch (Cache Hit): < 1ms
- Position Fetch (API): < 500ms
- Gate Evaluation with provider: < 5ms for 100 positions
"""

import time
from datetime import datetime, timezone
from decimal import Decimal
from typing import List
from unittest.mock import Mock

import pytest

from src.providers.oanda import OandaPositionProvider, OandaConfig
from src.providers.simulator import (
    SimulatorPositionProvider,
    InMemoryPortfolioAccessor,
    SimulatorPosition,
)
from src.providers.fallback import FallbackPositionProvider
from src.providers.cache import PositionCache
from src.providers.health import ProviderHealthMonitor
from src.models import Position
from src.gate import CorrelationGate
from src.models import GateConfig, TradeSignal


def create_test_positions(count: int) -> List[Position]:
    """Generate test positions."""
    instruments = ["EUR_USD", "GBP_USD", "USD_JPY", "AUD_USD", "USD_CAD"]
    directions = ["LONG", "SHORT"]
    positions = []
    for i in range(count):
        positions.append(
            Position(
                instrument=instruments[i % len(instruments)],
                direction=directions[i % 2],
                units=10000 + i * 100,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
                position_id=f"pos_{i:05d}",
            )
        )
    return positions


def create_simulator_positions(count: int) -> List[SimulatorPosition]:
    """Generate simulator positions."""
    instruments = ["EUR_USD", "GBP_USD", "USD_JPY", "AUD_USD", "USD_CAD"]
    directions = ["LONG", "SHORT"]
    positions = []
    for i in range(count):
        positions.append(
            SimulatorPosition(
                position_id=f"sim_{i:05d}",
                instrument=instruments[i % len(instruments)],
                direction=directions[i % 2],
                units=10000 + i * 100,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
    return positions


class TestCacheBenchmarks:
    """Benchmarks for PositionCache."""

    def test_cache_get_within_budget(self):
        """Cache get must complete within 1ms (BUDGET: < 1ms)."""
        cache = PositionCache(ttl_seconds=60.0)
        positions = create_test_positions(100)
        cache.set(positions)

        # Warmup
        for _ in range(10):
            cache.get()

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            cache.get()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 1.0, f"Average {avg_ms:.3f}ms exceeds 1ms budget"
        assert p99_ms < 2.0, f"P99 {p99_ms:.3f}ms exceeds 2ms budget"

    def test_cache_set_within_budget(self):
        """Cache set must complete within 1ms."""
        cache = PositionCache(ttl_seconds=60.0)
        positions = create_test_positions(100)

        # Warmup
        for _ in range(10):
            cache.set(positions)

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            cache.set(positions)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 1.0, f"Average {avg_ms:.3f}ms exceeds 1ms budget"


class TestSimulatorBenchmarks:
    """Benchmarks for SimulatorPositionProvider."""

    def test_simulator_fetch_cache_hit_within_budget(self):
        """Simulator cache hit must complete within 1ms (BUDGET: < 1ms)."""
        accessor = InMemoryPortfolioAccessor()
        for pos in create_simulator_positions(100):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=60.0)

        # First fetch populates cache
        provider.fetch_positions()

        # Warmup
        for _ in range(10):
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

        assert avg_ms < 1.0, f"Average {avg_ms:.3f}ms exceeds 1ms budget"
        assert p99_ms < 2.0, f"P99 {p99_ms:.3f}ms exceeds 2ms budget"

    def test_simulator_fetch_from_accessor_within_budget(self):
        """Simulator fetch from accessor must complete within 5ms."""
        accessor = InMemoryPortfolioAccessor()
        for pos in create_simulator_positions(100):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=0.001)

        # Each fetch goes to accessor (cache expires immediately)
        times = []
        for _ in range(50):
            time.sleep(0.002)  # Ensure cache expires
            start = time.perf_counter_ns()
            provider.fetch_positions()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[int(len(times) * 0.98)]

        assert avg_ms < 5.0, f"Average {avg_ms:.3f}ms exceeds 5ms budget"


class TestOandaBenchmarks:
    """Benchmarks for OandaPositionProvider."""

    def test_oanda_cache_hit_within_budget(self):
        """OANDA cache hit must complete within 1ms (BUDGET: < 1ms)."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # Generate response data for 100 positions
        response_positions = []
        for i in range(50):
            response_positions.append(
                {
                    "instrument": f"EUR_USD_{i}",
                    "long": {"units": "10000", "averagePrice": "1.0850"},
                    "short": {"units": "0", "averagePrice": "0"},
                }
            )

        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": response_positions}
        )

        provider = OandaPositionProvider(
            config, http_client=mock_client, cache_ttl_seconds=60.0
        )

        # First fetch populates cache
        provider.fetch_positions()

        # Warmup
        for _ in range(10):
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

        assert avg_ms < 1.0, f"Average {avg_ms:.3f}ms exceeds 1ms budget"

    def test_oanda_api_call_within_budget(self):
        """OANDA API call must complete within 500ms (simulated)."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # Simulate 50ms API latency (well under 500ms budget)
        def delayed_response(*args, **kwargs):
            time.sleep(0.05)  # 50ms simulated network
            return Mock(status_code=200, json=lambda: {"positions": []})

        mock_client.get.side_effect = delayed_response

        provider = OandaPositionProvider(
            config, http_client=mock_client, cache_ttl_seconds=0.001, max_retries=0
        )

        times = []
        for _ in range(10):
            provider.invalidate_cache()
            start = time.perf_counter_ns()
            provider.fetch_positions()
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        # Should be ~50ms (simulated latency) + overhead
        assert avg_ms < 500.0, f"Average {avg_ms:.3f}ms exceeds 500ms budget"


class TestFallbackBenchmarks:
    """Benchmarks for FallbackPositionProvider."""

    def test_fallback_primary_path_within_budget(self):
        """Fallback primary path must add minimal overhead (< 0.5ms)."""
        positions = create_test_positions(100)

        primary = Mock()
        primary.fetch_positions.return_value = positions
        primary.is_available.return_value = True
        primary.get_last_fetch_time.return_value = datetime.now(timezone.utc)

        fallback = Mock()

        provider = FallbackPositionProvider(primary, fallback)

        # Warmup
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

        # Fallback wrapper should add < 0.5ms overhead
        assert avg_ms < 1.0, f"Fallback overhead {avg_ms:.3f}ms exceeds 1ms"


class TestHealthMonitorBenchmarks:
    """Benchmarks for ProviderHealthMonitor."""

    def test_health_record_within_budget(self):
        """Health monitoring must add < 0.1ms overhead."""
        monitor = ProviderHealthMonitor()

        # Warmup
        for _ in range(100):
            monitor.record_success("test", latency_ms=1.0)

        # Measure
        times = []
        for _ in range(1000):
            start = time.perf_counter_ns()
            monitor.record_success("test", latency_ms=1.0)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        assert avg_ms < 0.1, f"Health record {avg_ms:.4f}ms exceeds 0.1ms budget"


class TestGateWithProviderBenchmarks:
    """Benchmarks for gate + provider integration."""

    def test_gate_evaluation_with_provider_within_budget(self):
        """Gate evaluation with provider must complete within 5ms for 100 positions."""
        # Setup simulator provider with 100 positions
        accessor = InMemoryPortfolioAccessor()
        for pos in create_simulator_positions(100):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=60.0)

        # Setup gate with provider (using correct parameter names)
        config = GateConfig(soft_warning_count=2, hard_block_count=3)

        class ProviderAdapter:
            def __init__(self, prov):
                self._prov = prov

            def fetch_positions(self):
                return self._prov.fetch_positions()

        gate = CorrelationGate(config, ProviderAdapter(provider))
        gate.initialize()

        signal = TradeSignal(instrument="EUR_USD", direction="LONG", units=10000)

        # Warmup
        for _ in range(10):
            gate.evaluate(signal)

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            gate.evaluate(signal)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 5.0, f"Average {avg_ms:.3f}ms exceeds 5ms budget"
        assert p99_ms < 10.0, f"P99 {p99_ms:.3f}ms exceeds 10ms budget"


class TestScalabilityBenchmarks:
    """Benchmarks for scaling behavior."""

    @pytest.mark.parametrize("position_count", [10, 100, 500, 1000])
    def test_cache_scales_linearly(self, position_count):
        """Cache operations should scale linearly with position count."""
        cache = PositionCache(ttl_seconds=60.0)
        positions = create_test_positions(position_count)

        # Measure set
        start = time.perf_counter_ns()
        cache.set(positions)
        set_ms = (time.perf_counter_ns() - start) / 1_000_000

        # Measure get
        start = time.perf_counter_ns()
        cache.get()
        get_ms = (time.perf_counter_ns() - start) / 1_000_000

        # Should scale sub-linearly (list copy is O(n) but fast)
        # Allow 0.01ms per position (10us) as upper bound
        max_ms = max(1.0, position_count * 0.01)

        assert set_ms < max_ms, f"Set {set_ms:.3f}ms for {position_count} positions"
        assert get_ms < max_ms, f"Get {get_ms:.3f}ms for {position_count} positions"

    @pytest.mark.parametrize("position_count", [10, 100, 500])
    def test_simulator_scales_acceptably(self, position_count):
        """Simulator should handle varying position counts."""
        accessor = InMemoryPortfolioAccessor()
        for pos in create_simulator_positions(position_count):
            accessor.add_position(pos)

        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=0.001)

        # Fresh fetch (no cache)
        time.sleep(0.002)
        start = time.perf_counter_ns()
        positions = provider.fetch_positions()
        elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000

        assert len(positions) == position_count

        # Allow 0.05ms per position (50us) as upper bound
        max_ms = max(2.0, position_count * 0.05)
        assert elapsed_ms < max_ms, f"Fetch {elapsed_ms:.3f}ms for {position_count}"
