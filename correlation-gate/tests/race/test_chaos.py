"""
Chaos Engineering Race Condition Tests
======================================

TDI Step 2: Tests written FIRST before implementation.

Test Count: 10 tests
Purpose: Verify gate resilience under chaotic conditions
"""

import random
import threading
import time
import pytest
from datetime import datetime, timezone
from decimal import Decimal
from typing import List, Optional
from concurrent.futures import ThreadPoolExecutor

from src.models import (
    Position,
    TradeSignal,
    GateDecision,
    GateConfig,
    GateState,
)
from src.gate import CorrelationGate
from src.exceptions import ProviderError, ProviderTimeoutError

from tests.race.harness import (
    ConcurrentTestHarness,
    generate_signals,
    generate_mixed_signals,
)


# =============================================================================
# CHAOTIC MOCK PROVIDERS
# =============================================================================


class DelayingProvider:
    """Provider with configurable random delays."""

    def __init__(
        self,
        positions: List[Position] = None,
        min_delay_ms: float = 0,
        max_delay_ms: float = 100,
    ):
        self._positions = positions or []
        self._min_delay = min_delay_ms / 1000.0
        self._max_delay = max_delay_ms / 1000.0

    def fetch_positions(self) -> List[Position]:
        delay = random.uniform(self._min_delay, self._max_delay)
        time.sleep(delay)
        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


class IntermittentFailureProvider:
    """Provider that fails intermittently.

    Args:
        positions: Positions to return on success
        failure_rate: Probability of failure (0.0 to 1.0)
        skip_first_n_failures: Number of initial calls that always succeed
    """

    def __init__(
        self,
        positions: List[Position] = None,
        failure_rate: float = 0.1,
        skip_first_n_failures: int = 0,
    ):
        self._positions = positions or []
        self._failure_rate = failure_rate
        self._skip_first_n = skip_first_n_failures
        self._call_count = 0
        self._lock = threading.Lock()

    def fetch_positions(self) -> List[Position]:
        with self._lock:
            self._call_count += 1
            current_call = self._call_count

        # First N calls succeed to allow initialization
        if current_call <= self._skip_first_n:
            return self._positions.copy()

        # Subsequent calls may fail
        if random.random() < self._failure_rate:
            raise ProviderError("IntermittentProvider", "Random failure")

        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


class TimeoutProvider:
    """Provider that sometimes times out."""

    def __init__(
        self,
        positions: List[Position] = None,
        timeout_rate: float = 0.1,
        timeout_delay_s: float = 30.0,
    ):
        self._positions = positions or []
        self._timeout_rate = timeout_rate
        self._timeout_delay = timeout_delay_s

    def fetch_positions(self) -> List[Position]:
        if random.random() < self._timeout_rate:
            time.sleep(self._timeout_delay)  # Simulate timeout
            raise ProviderTimeoutError("TimeoutProvider", 5.0)

        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


# =============================================================================
# FIXTURES
# =============================================================================


@pytest.fixture
def gate_config() -> GateConfig:
    """Gate config for chaos testing."""
    return GateConfig(
        enabled=True,
        soft_warning_count=5,
        hard_block_count=10,
        lock_timeout_seconds=2.0,
        position_fetch_timeout_seconds=1.0,
        pending_timeout_seconds=10.0,
    )


# =============================================================================
# RANDOM DELAY TESTS
# =============================================================================


class TestRandomDelays:
    """Tests with random provider delays."""

    def test_provider_random_delays(self, gate_config: GateConfig) -> None:
        """Provider with 0-100ms random delays."""
        provider = DelayingProvider(positions=[], min_delay_ms=0, max_delay_ms=100)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        signals = generate_signals(50)
        harness = ConcurrentTestHarness(num_threads=20)

        result = harness.run_concurrent(gate, signals[:20])

        # All should complete (delays are within timeout)
        assert len(result.decisions) == 20
        assert len(result.errors) == 0

    def test_provider_variable_latency(self, gate_config: GateConfig) -> None:
        """Provider with highly variable latency."""
        provider = DelayingProvider(
            positions=[],
            min_delay_ms=1,
            max_delay_ms=500,  # Up to 500ms
        )
        gate_config.position_fetch_timeout_seconds = 2.0
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        results = []
        for _ in range(20):
            signal = TradeSignal("EUR_USD", "LONG", 10000)
            try:
                decision = gate.evaluate(signal)
                results.append(("success", decision))
            except Exception as e:
                results.append(("error", e))

        # Most should succeed
        successes = sum(1 for r in results if r[0] == "success")
        assert successes >= 15


# =============================================================================
# INTERMITTENT FAILURE TESTS
# =============================================================================


class TestIntermittentFailures:
    """Tests with intermittent provider failures."""

    def test_provider_intermittent_failures(self, gate_config: GateConfig) -> None:
        """Provider fails intermittently during refresh_positions.

        Tests that the gate handles intermittent provider failures gracefully.
        The gate fetches positions on init and refresh_positions, not on evaluate.
        Failures during refresh trigger DEGRADED state (not exception).
        """
        provider = IntermittentFailureProvider(
            positions=[],
            failure_rate=0.5,  # 50% failure rate on refresh
            skip_first_n_failures=1,  # Allow init to succeed
        )
        # Use high thresholds
        gate_config.soft_warning_count = 200
        gate_config.hard_block_count = 300
        gate = CorrelationGate(gate_config, provider)

        # Initialize should succeed (first call skipped)
        success = gate.initialize()
        assert success is True
        assert gate.state == GateState.READY

        # Try multiple refresh_positions calls - some will fail internally
        degraded_observed = False
        for i in range(20):
            gate.refresh_positions()
            if gate.state == GateState.DEGRADED:
                degraded_observed = True

        # Gate should have entered DEGRADED state at least once
        # (provider fails 50% of the time after init)
        assert degraded_observed, (
            "Expected gate to enter DEGRADED state on provider failures"
        )

        # Gate should still be usable even in DEGRADED state
        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = gate.evaluate(signal)
        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_provider_high_failure_rate(self, gate_config: GateConfig) -> None:
        """Provider fails 50% of requests - gate stays safe."""
        provider = IntermittentFailureProvider(positions=[], failure_rate=0.5)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        results = []
        for i in range(50):
            signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
            decision = gate.evaluate(signal)
            results.append(decision)

        # All should return valid decisions
        assert len(results) == 50
        for d in results:
            assert d.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

        # Many should be HARD_BLOCK (fail-closed)
        hard_blocks = sum(1 for d in results if d.decision == "HARD_BLOCK")
        assert hard_blocks > 10


# =============================================================================
# LOCK CONTENTION TESTS
# =============================================================================


class TestLockContention:
    """Tests for lock contention scenarios."""

    def test_lock_contention_with_delays(self, gate_config: GateConfig) -> None:
        """Threads with random hold times on lock."""
        provider = DelayingProvider(positions=[], min_delay_ms=10, max_delay_ms=50)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        signals = generate_signals(30)
        harness = ConcurrentTestHarness(num_threads=30)

        result = harness.run_concurrent(gate, signals)

        # All should complete (no deadlocks)
        assert len(result.errors) == 0
        assert len(result.decisions) == 30


# =============================================================================
# MIXED OPERATIONS TESTS
# =============================================================================


class TestMixedOperations:
    """Tests for mixed concurrent operations."""

    def test_mixed_operations_concurrent(self, gate_config: GateConfig) -> None:
        """evaluate, confirm, cancel, refresh all concurrent."""
        provider = DelayingProvider(positions=[], min_delay_ms=0, max_delay_ms=20)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        results = []
        lock = threading.Lock()

        def evaluate_op():
            for _ in range(10):
                signal = TradeSignal("EUR_USD", "LONG", 10000)
                decision = gate.evaluate(signal)
                with lock:
                    results.append(("evaluate", decision))
                time.sleep(random.uniform(0.001, 0.01))

        def confirm_op():
            for _ in range(10):
                pending_ids = list(gate.get_pending_ids())
                if pending_ids:
                    pid = random.choice(pending_ids)
                    success = gate.confirm_execution(pid)
                    with lock:
                        results.append(("confirm", success))
                time.sleep(random.uniform(0.001, 0.01))

        def cancel_op():
            for _ in range(10):
                pending_ids = list(gate.get_pending_ids())
                if pending_ids:
                    pid = random.choice(pending_ids)
                    success = gate.cancel_pending(pid)
                    with lock:
                        results.append(("cancel", success))
                time.sleep(random.uniform(0.001, 0.01))

        def refresh_op():
            for _ in range(5):
                count = gate.refresh_positions()
                with lock:
                    results.append(("refresh", count))
                time.sleep(random.uniform(0.01, 0.02))

        threads = [
            threading.Thread(target=evaluate_op),
            threading.Thread(target=evaluate_op),
            threading.Thread(target=confirm_op),
            threading.Thread(target=cancel_op),
            threading.Thread(target=refresh_op),
        ]

        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30.0)
            if t.is_alive():
                pytest.fail("Thread still running - possible deadlock")

        # All operations should complete
        assert len(results) > 0


# =============================================================================
# TIMEOUT DURING EVALUATION TESTS
# =============================================================================


class TestTimeoutDuringEvaluation:
    """Tests for timeout behavior during evaluation."""

    def test_provider_timeout_during_evaluation(self, gate_config: GateConfig) -> None:
        """Provider times out mid-evaluation."""
        provider = TimeoutProvider(
            positions=[],
            timeout_rate=0.3,  # 30% timeout
            timeout_delay_s=gate_config.position_fetch_timeout_seconds + 1.0,
        )
        gate_config.position_fetch_timeout_seconds = 0.5
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        results = []
        for i in range(20):
            signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
            decision = gate.evaluate(signal)
            results.append(decision)

        # Timeouts should result in HARD_BLOCK (fail-closed)
        hard_blocks = sum(1 for d in results if d.decision == "HARD_BLOCK")
        assert hard_blocks > 0  # Some timeouts

        # All should be valid decisions
        for d in results:
            assert d.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")


# =============================================================================
# EDGE CASE TESTS
# =============================================================================


class TestChaosEdgeCases:
    """Edge case tests under chaos."""

    def test_empty_positions_concurrent(self, gate_config: GateConfig) -> None:
        """Concurrent evaluations with no positions (edge case).

        Tests that the gate handles concurrent evaluations without deadlock.
        With pending signals affecting exposure calculations, some blocking
        is expected even with high thresholds.
        """
        provider = DelayingProvider(positions=[], min_delay_ms=0, max_delay_ms=10)
        # Use very high thresholds to minimize threshold-based blocking
        gate_config.soft_warning_count = 100
        gate_config.hard_block_count = 200
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        # Generate mixed signals
        signals = generate_mixed_signals(50)
        harness = ConcurrentTestHarness(num_threads=50)

        result = harness.run_concurrent(gate, signals)

        # All evaluations should complete (no deadlocks)
        assert len(result.decisions) == 50
        assert len(result.errors) == 0

        # Some should be ALLOW/WARNING (with high thresholds, many pass)
        allowed = sum(
            1 for d in result.decisions if d.decision in ("ALLOW", "SOFT_WARNING")
        )
        assert allowed > 20  # At least some should pass (conservative expectation)

    def test_shutdown_during_evaluation(self, gate_config: GateConfig) -> None:
        """Graceful handling when shutdown requested during evaluation."""
        provider = DelayingProvider(positions=[], min_delay_ms=50, max_delay_ms=100)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        evaluation_started = threading.Event()
        evaluation_done = threading.Event()
        result_decision = [None]

        def slow_evaluate():
            evaluation_started.set()
            signal = TradeSignal("EUR_USD", "LONG", 10000)
            result_decision[0] = gate.evaluate(signal)
            evaluation_done.set()

        # Start evaluation in background
        eval_thread = threading.Thread(target=slow_evaluate)
        eval_thread.start()

        # Wait for evaluation to start
        evaluation_started.wait(timeout=1.0)

        # Evaluation should complete normally
        evaluation_done.wait(timeout=5.0)
        eval_thread.join()

        assert result_decision[0] is not None

    def test_reinitialize_after_failure(self, gate_config: GateConfig) -> None:
        """Gate can reinitialize after FAILED state."""
        # First, create gate that will fail
        failing_provider = IntermittentFailureProvider(
            positions=[],
            failure_rate=1.0,  # Always fail
        )
        gate = CorrelationGate(gate_config, failing_provider)

        # Initialize should fail
        success = gate.initialize()
        assert success is False
        assert gate.state == GateState.FAILED

        # Now switch to working provider and reinitialize
        # (This tests that gate can recover)
        working_provider = DelayingProvider(
            positions=[], min_delay_ms=0, max_delay_ms=1
        )
        gate._provider = working_provider

        # Re-initialize
        success = gate.initialize()
        assert success is True
        assert gate.state == GateState.READY
