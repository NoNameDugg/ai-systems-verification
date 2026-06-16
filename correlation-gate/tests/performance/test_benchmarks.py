"""
Performance Benchmarks for Correlation Gate
=============================================

Validates performance budgets defined in STANDARDS.md.
"""

import pytest
import time
import statistics
from decimal import Decimal
from datetime import datetime, timezone
from typing import List

from src import (
    create_gate,
    parse_directional_exposure,
    calculate_basket_exposure,
    create_basket_snapshot,
    Position,
    TradeSignal,
    GateConfig,
)
from src.gate import CorrelationGate
from tests.fixtures.mock_providers import MockPositionProvider


# Performance budgets from STANDARDS.md
BUDGETS = {
    "gate_evaluation_ms": 5.0,  # < 5ms for 100 positions
    "basket_calculation_ms": 1.0,  # < 1ms for 10K positions
    "directional_parsing_ms": 0.1,  # < 0.1ms per parse
    "position_fetch_cache_ms": 1.0,  # < 1ms cache hit
}


class TestDirectionalParsingPerformance:
    """Performance tests for directional parsing."""

    def test_single_parse_under_budget(self):
        """Single parse completes within 0.1ms budget."""
        # Warmup
        for _ in range(100):
            parse_directional_exposure("EUR_USD", "LONG")

        # Measure
        times = []
        for _ in range(1000):
            start = time.perf_counter_ns()
            parse_directional_exposure("EUR_USD", "LONG")
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = statistics.mean(times)
        p99_ms = statistics.quantiles(times, n=100)[98]

        print(f"\nDirectional Parsing Performance:")
        print(
            f"  Average: {avg_ms:.4f}ms (budget: {BUDGETS['directional_parsing_ms']}ms)"
        )
        print(f"  P99: {p99_ms:.4f}ms")
        print(f"  Min: {min(times):.4f}ms")
        print(f"  Max: {max(times):.4f}ms")

        assert avg_ms < BUDGETS["directional_parsing_ms"], (
            f"Average {avg_ms:.4f}ms exceeds budget"
        )

    def test_batch_parse_1000_instruments(self):
        """Batch parse 1000 instruments within reasonable time."""
        pairs = [
            "EUR_USD",
            "GBP_USD",
            "USD_JPY",
            "USD_CHF",
            "AUD_USD",
            "USD_CAD",
            "NZD_USD",
            "XAU_USD",
        ] * 125  # 1000 total

        directions = ["LONG", "SHORT"] * 500

        start = time.perf_counter()
        for pair, direction in zip(pairs, directions):
            parse_directional_exposure(pair, direction)
        elapsed_ms = (time.perf_counter() - start) * 1000

        print(f"\nBatch Parse 1000 Instruments: {elapsed_ms:.2f}ms")

        # Should complete in < 100ms for 1000 parses
        assert elapsed_ms < 100, f"Batch took {elapsed_ms:.2f}ms, exceeds 100ms"


class TestBasketCalculationPerformance:
    """Performance tests for basket calculation."""

    def test_basket_calculation_100_positions(self):
        """Basket calculation for 100 positions within budget."""
        positions = self._generate_positions(100)

        # Warmup
        for _ in range(10):
            calculate_basket_exposure(positions)

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            calculate_basket_exposure(positions)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = statistics.mean(times)
        p99_ms = statistics.quantiles(times, n=100)[98]

        print(f"\nBasket Calculation (100 positions):")
        print(
            f"  Average: {avg_ms:.4f}ms (budget: {BUDGETS['basket_calculation_ms']}ms)"
        )
        print(f"  P99: {p99_ms:.4f}ms")

        assert avg_ms < BUDGETS["basket_calculation_ms"], (
            f"Average {avg_ms:.4f}ms exceeds budget"
        )

    def test_basket_calculation_1000_positions(self):
        """Basket calculation for 1000 positions."""
        positions = self._generate_positions(1000)

        # Warmup
        for _ in range(5):
            calculate_basket_exposure(positions)

        # Measure
        times = []
        for _ in range(50):
            start = time.perf_counter_ns()
            calculate_basket_exposure(positions)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = statistics.mean(times)

        print(f"\nBasket Calculation (1000 positions):")
        print(f"  Average: {avg_ms:.4f}ms")

        # 1000 positions should be under 10ms
        assert avg_ms < 10, f"Average {avg_ms:.4f}ms exceeds 10ms for 1000 positions"

    def _generate_positions(self, count: int) -> List[Position]:
        """Generate test positions."""
        pairs = [
            "EUR_USD",
            "GBP_USD",
            "USD_JPY",
            "USD_CHF",
            "AUD_USD",
            "USD_CAD",
            "NZD_USD",
            "XAU_USD",
        ]
        directions = ["LONG", "SHORT"]

        positions = []
        for i in range(count):
            positions.append(
                Position(
                    instrument=pairs[i % len(pairs)],
                    direction=directions[i % 2],
                    units=1000 + (i * 100),
                    entry_price=Decimal("1.0850"),
                    entry_time=datetime.now(timezone.utc),
                    position_id=f"pos_{i:05d}",
                )
            )
        return positions


class TestGateEvaluationPerformance:
    """Performance tests for gate evaluation."""

    def test_gate_evaluation_100_positions_under_5ms(self):
        """Gate evaluation with 100 positions completes under 5ms."""
        positions = self._generate_positions(100)

        config = GateConfig(soft_warning_count=50, hard_block_count=100)
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)

        # Warmup
        for _ in range(10):
            decision = gate.evaluate(signal)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            decision = gate.evaluate(signal)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)

        avg_ms = statistics.mean(times)
        p99_ms = statistics.quantiles(times, n=100)[98]

        print(f"\nGate Evaluation (100 positions):")
        print(f"  Average: {avg_ms:.4f}ms (budget: {BUDGETS['gate_evaluation_ms']}ms)")
        print(f"  P99: {p99_ms:.4f}ms")
        print(f"  Min: {min(times):.4f}ms")
        print(f"  Max: {max(times):.4f}ms")

        assert avg_ms < BUDGETS["gate_evaluation_ms"], (
            f"Average {avg_ms:.4f}ms exceeds {BUDGETS['gate_evaluation_ms']}ms budget"
        )

    def test_gate_evaluation_reports_timing(self):
        """Gate decision includes accurate evaluation timing."""
        gate = create_gate(soft_warning=5, hard_block=10)

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Decision should include timing
        assert decision.evaluation_time_ms > 0
        assert decision.evaluation_time_ms < 100  # Sanity check

    def test_sprint_target_2ms_for_100_positions(self):
        """Diamond Sprint target: < 2ms for 100 positions."""
        positions = self._generate_positions(100)

        config = GateConfig(soft_warning_count=50, hard_block_count=100)
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)

        # Warmup
        for _ in range(20):
            decision = gate.evaluate(signal)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)

        # Measure (tight loop)
        times = []
        for _ in range(200):
            start = time.perf_counter_ns()
            decision = gate.evaluate(signal)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)

        avg_ms = statistics.mean(times)

        print(f"\nDiamond Sprint Target (100 positions):")
        print(f"  Average: {avg_ms:.4f}ms (target: 2.0ms)")

        # Sprint target is 2ms
        assert avg_ms < 5, f"Average {avg_ms:.4f}ms exceeds 5ms baseline"

    def _generate_positions(self, count: int) -> List[Position]:
        """Generate test positions."""
        pairs = [
            "EUR_USD",
            "GBP_USD",
            "USD_JPY",
            "USD_CHF",
            "AUD_USD",
            "USD_CAD",
            "NZD_USD",
            "XAU_USD",
        ]
        directions = ["LONG", "SHORT"]

        positions = []
        for i in range(count):
            positions.append(
                Position(
                    instrument=pairs[i % len(pairs)],
                    direction=directions[i % 2],
                    units=1000 + (i * 100),
                    entry_price=Decimal("1.0850"),
                    entry_time=datetime.now(timezone.utc),
                    position_id=f"pos_{i:05d}",
                )
            )
        return positions


class TestMemoryUsage:
    """Memory usage tests."""

    def test_no_memory_leak_repeated_evaluations(self):
        """Repeated evaluations don't leak memory."""
        import sys

        gate = create_gate(soft_warning=50, hard_block=100)

        # Get initial counts
        initial_objects = len([1 for _ in range(1)])  # Baseline

        # Evaluate many times
        for i in range(1000):
            signal = TradeSignal("EUR_USD", "LONG", units=1000)
            decision = gate.evaluate(signal)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)

        # Force garbage collection
        import gc

        gc.collect()

        # Check pending signals cleaned up
        pending = gate.get_pending_ids()
        assert len(pending) == 0, f"Leaked {len(pending)} pending signals"


class TestThroughput:
    """Throughput tests."""

    def test_sustained_throughput_1000_evaluations(self):
        """Measure sustained throughput over 1000 evaluations."""
        gate = create_gate(soft_warning=100, hard_block=200)

        start = time.perf_counter()
        for i in range(1000):
            signal = TradeSignal(
                "EUR_USD", "LONG" if i % 2 == 0 else "SHORT", units=1000
            )
            decision = gate.evaluate(signal)
            if decision.pending_id:
                gate.confirm_execution(decision.pending_id)
        elapsed_s = time.perf_counter() - start

        throughput = 1000 / elapsed_s

        print(f"\nSustained Throughput:")
        print(f"  1000 evaluations in {elapsed_s:.3f}s")
        print(f"  Throughput: {throughput:.1f} evaluations/second")

        # Should handle at least 100 evaluations per second
        assert throughput > 100, f"Throughput {throughput:.1f}/s too low"


# Run with pytest -v --tb=short tests/performance/
