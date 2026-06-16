"""
High-Frequency Race Condition Tests
===================================

TDI Step 2: Tests written FIRST before implementation.

Test Count: 8 tests
Purpose: Verify gate behavior under high-frequency signal load
"""

import threading
import time
import pytest
from datetime import datetime, timezone, timedelta
from decimal import Decimal
from typing import List
from concurrent.futures import ThreadPoolExecutor

from src.models import (
    Position,
    TradeSignal,
    GateDecision,
    GateConfig,
)
from src.gate import CorrelationGate

from tests.race.harness import (
    ConcurrentTestHarness,
    ConcurrentTestResult,
    generate_signals,
    generate_mixed_signals,
)


# =============================================================================
# FIXTURES
# =============================================================================


class MockPositionProvider:
    """Mock provider for testing."""

    def __init__(self, positions: List[Position] = None):
        self._positions = positions or []

    def fetch_positions(self) -> List[Position]:
        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


@pytest.fixture
def gate_config() -> GateConfig:
    """Gate config for high-frequency testing."""
    return GateConfig(
        enabled=True,
        soft_warning_count=5,
        hard_block_count=10,
        lock_timeout_seconds=10.0,
        position_fetch_timeout_seconds=10.0,
        pending_timeout_seconds=5.0,
    )


@pytest.fixture
def empty_provider() -> MockPositionProvider:
    """Provider with no positions."""
    return MockPositionProvider([])


@pytest.fixture
def initialized_gate(
    gate_config: GateConfig, empty_provider: MockPositionProvider
) -> CorrelationGate:
    """Create and initialize gate."""
    gate = CorrelationGate(gate_config, empty_provider)
    gate.initialize()
    return gate


@pytest.fixture
def harness() -> ConcurrentTestHarness:
    """Create test harness."""
    return ConcurrentTestHarness(num_threads=50)


# =============================================================================
# RAPID SEQUENTIAL TESTS
# =============================================================================


class TestRapidSequential:
    """Tests for rapid sequential signal evaluation."""

    def test_rapid_sequential_1000_signals(
        self, initialized_gate: CorrelationGate
    ) -> None:
        """1000 signals in sequence must all be processed correctly."""
        signals = generate_signals(1000)
        harness = ConcurrentTestHarness()

        start = time.perf_counter()
        result = harness.run_rapid_sequential(initialized_gate, signals, delay_ms=0)
        elapsed = time.perf_counter() - start

        # All should complete without errors
        assert len(result.errors) == 0
        assert len(result.decisions) == 1000

        # Should complete in reasonable time (< 5 seconds)
        assert elapsed < 5.0

        # All decisions should be valid
        for d in result.decisions:
            assert d.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_rapid_sequential_evaluation_time(
        self, initialized_gate: CorrelationGate
    ) -> None:
        """Each evaluation should meet performance budget."""
        signals = generate_signals(100)

        evaluation_times = []
        for signal in signals:
            start = time.perf_counter_ns()
            decision = initialized_gate.evaluate(signal)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            evaluation_times.append(elapsed_ms)

        avg_ms = sum(evaluation_times) / len(evaluation_times)
        p99_ms = sorted(evaluation_times)[98]

        # Average should be < 5ms, p99 < 10ms
        assert avg_ms < 5, f"Average {avg_ms}ms exceeds 5ms budget"
        assert p99_ms < 10, f"P99 {p99_ms}ms exceeds 10ms budget"


# =============================================================================
# BURST PATTERN TESTS
# =============================================================================


class TestBurstPattern:
    """Tests for burst-then-quiet patterns."""

    def test_burst_then_quiet(self, initialized_gate: CorrelationGate) -> None:
        """Burst of 50 signals, 1s pause, verify state consistency."""
        signals_burst = generate_signals(50)

        # First burst
        for signal in signals_burst:
            initialized_gate.evaluate(signal)

        # Capture state
        stats_before = initialized_gate.get_statistics()

        # Quiet period
        time.sleep(1.0)

        # Capture state again
        stats_after = initialized_gate.get_statistics()

        # State should be consistent
        assert stats_after["state"] == stats_before["state"]

    def test_alternating_currencies_rapid(
        self, initialized_gate: CorrelationGate
    ) -> None:
        """EUR_USD, GBP_USD, EUR_USD, GBP_USD at 100/sec."""
        signals = []
        for i in range(100):
            instrument = "EUR_USD" if i % 2 == 0 else "GBP_USD"
            signals.append(TradeSignal(instrument, "LONG", 10000))

        start = time.perf_counter()

        for signal in signals:
            initialized_gate.evaluate(signal)

        elapsed = time.perf_counter() - start

        # Should complete in < 1 second (100/sec = 10ms each)
        assert elapsed < 1.0


# =============================================================================
# PENDING EXPIRY TESTS
# =============================================================================


class TestPendingExpiry:
    """Tests for pending signal expiry under load."""

    def test_pending_expiry_under_high_load(
        self, gate_config: GateConfig, empty_provider: MockPositionProvider
    ) -> None:
        """Pending signals expire correctly during high load."""
        gate_config.pending_timeout_seconds = 0.5  # Very short timeout
        gate = CorrelationGate(gate_config, empty_provider)
        gate.initialize()

        # Generate pending signals
        for i in range(10):
            signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"expire_{i}")
            gate.evaluate(signal)

        initial_pending = len(gate.get_pending_ids())
        assert initial_pending == 10

        # Wait for expiry + some margin
        time.sleep(0.7)

        # High-frequency evaluation to trigger purge
        for i in range(50):
            signal = TradeSignal("GBP_USD", "LONG", 10000, signal_id=f"new_{i}")
            gate.evaluate(signal)

        # Original pending should be expired
        remaining_ids = gate.get_pending_ids()
        original_remaining = [pid for pid in remaining_ids if "expire_" in pid]
        assert len(original_remaining) == 0


# =============================================================================
# STATE CONSISTENCY TESTS
# =============================================================================


class TestStateConsistency:
    """Tests for state consistency after high-frequency operations."""

    def test_state_consistency_after_burst(
        self, initialized_gate: CorrelationGate
    ) -> None:
        """State is consistent after high-frequency burst."""
        # Capture initial state
        initial_stats = initialized_gate.get_statistics()
        initial_version = initial_stats["state_version"]

        # High-frequency burst
        signals = generate_signals(500)
        for signal in signals:
            initialized_gate.evaluate(signal)

        # Confirm some pending
        pending_ids = list(initialized_gate.get_pending_ids())
        for pid in pending_ids[:100]:  # Confirm first 100
            initialized_gate.confirm_execution(pid)

        # Cancel some
        pending_ids = list(initialized_gate.get_pending_ids())
        for pid in pending_ids[:50]:  # Cancel next 50
            initialized_gate.cancel_pending(pid)

        # Final stats
        final_stats = initialized_gate.get_statistics()

        # State should be valid
        assert final_stats["state"] in ["READY", "DEGRADED"]

        # Version should have increased
        assert final_stats["state_version"] >= initial_version

    def test_no_deadlock_under_load(self, initialized_gate: CorrelationGate) -> None:
        """Gate doesn't deadlock under concurrent load."""
        num_threads = 20
        operations_per_thread = 50
        completed = []
        lock = threading.Lock()

        def run_operations():
            for i in range(operations_per_thread):
                try:
                    signal = TradeSignal("EUR_USD", "LONG", 10000)
                    decision = initialized_gate.evaluate(signal)

                    if decision.pending_id and i % 3 == 0:
                        initialized_gate.confirm_execution(decision.pending_id)
                    elif decision.pending_id and i % 3 == 1:
                        initialized_gate.cancel_pending(decision.pending_id)

                    with lock:
                        completed.append(True)
                except Exception as e:
                    with lock:
                        completed.append(False)

        threads = [threading.Thread(target=run_operations) for _ in range(num_threads)]
        for t in threads:
            t.start()

        # Wait with timeout to detect deadlock
        for t in threads:
            t.join(timeout=30.0)
            if t.is_alive():
                pytest.fail("Thread still running - possible deadlock")

        # All operations should complete
        assert len(completed) == num_threads * operations_per_thread
        assert all(completed)


# =============================================================================
# THROUGHPUT TESTS
# =============================================================================


class TestThroughput:
    """Tests for gate throughput."""

    def test_throughput_100_ops_per_second(
        self, initialized_gate: CorrelationGate
    ) -> None:
        """Gate can handle at least 100 operations per second."""
        signals = generate_signals(100)

        start = time.perf_counter()
        for signal in signals:
            initialized_gate.evaluate(signal)
        elapsed = time.perf_counter() - start

        ops_per_second = 100 / elapsed
        assert ops_per_second >= 100, f"Throughput {ops_per_second:.1f} ops/s below 100"

    def test_concurrent_throughput(self, initialized_gate: CorrelationGate) -> None:
        """Gate throughput under concurrent load."""
        harness = ConcurrentTestHarness(num_threads=50)
        signals = generate_signals(200)

        start = time.perf_counter()
        result = harness.run_concurrent(initialized_gate, signals)
        elapsed = time.perf_counter() - start

        # Should complete in reasonable time
        assert elapsed < 5.0

        # All should complete
        assert len(result.decisions) == 200
        assert len(result.errors) == 0
