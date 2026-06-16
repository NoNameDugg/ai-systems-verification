"""
Concurrent Test Harness for Race Condition Testing
===================================================

Provides infrastructure for testing concurrent behavior.
"""

import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from datetime import datetime, timezone
from decimal import Decimal
from typing import List, Dict, Any, Callable, Optional, Tuple

from src.models import (
    Position,
    TradeSignal,
    GateDecision,
    GateConfig,
    BasketSnapshot,
)


@dataclass
class ConcurrentTestResult:
    """
    Result of concurrent test execution.

    Attributes:
        decisions: List of all decisions in completion order
        timing: Timing information
        errors: Any errors that occurred
        thread_ids: Thread IDs that executed each decision
    """

    decisions: List[GateDecision] = field(default_factory=list)
    timing: Dict[str, float] = field(default_factory=dict)
    errors: List[Exception] = field(default_factory=list)
    thread_ids: List[int] = field(default_factory=list)
    execution_order: List[str] = field(default_factory=list)


class ConcurrentTestHarness:
    """
    Test harness for race condition testing.

    Provides:
    - Thread pool management
    - Barrier synchronization
    - Result collection
    - Timing analysis
    """

    def __init__(self, num_threads: int = 10) -> None:
        """
        Initialize test harness.

        Args:
            num_threads: Number of concurrent threads
        """
        self._num_threads = num_threads
        self._results_lock = threading.Lock()

    def run_concurrent(
        self,
        gate: Any,  # CorrelationGate
        signals: List[TradeSignal],
        synchronized_start: bool = True,
    ) -> ConcurrentTestResult:
        """
        Run signals concurrently against gate.

        Args:
            gate: Gate to test
            signals: Signals to evaluate (one per thread)
            synchronized_start: If True, all threads start together

        Returns:
            ConcurrentTestResult with all decisions and timing
        """
        result = ConcurrentTestResult()
        num_threads = len(signals)
        barrier = threading.Barrier(num_threads) if synchronized_start else None

        def evaluate_signal(signal: TradeSignal) -> Tuple[GateDecision, int]:
            if barrier:
                barrier.wait()

            thread_id = threading.get_ident()
            decision = gate.evaluate(signal)
            return decision, thread_id

        start_time = time.perf_counter()

        with ThreadPoolExecutor(max_workers=num_threads) as executor:
            futures = [executor.submit(evaluate_signal, signal) for signal in signals]

            for future in as_completed(futures):
                try:
                    decision, thread_id = future.result()
                    with self._results_lock:
                        result.decisions.append(decision)
                        result.thread_ids.append(thread_id)
                except Exception as e:
                    with self._results_lock:
                        result.errors.append(e)

        result.timing["total_seconds"] = time.perf_counter() - start_time
        return result

    def run_rapid_sequential(
        self, gate: Any, signals: List[TradeSignal], delay_ms: float = 0
    ) -> ConcurrentTestResult:
        """
        Run signals in rapid sequence from single thread.

        Args:
            gate: Gate to test
            signals: Signals to evaluate
            delay_ms: Delay between signals in milliseconds

        Returns:
            ConcurrentTestResult with all decisions
        """
        result = ConcurrentTestResult()
        delay_s = delay_ms / 1000.0

        start_time = time.perf_counter()

        for signal in signals:
            try:
                decision = gate.evaluate(signal)
                result.decisions.append(decision)
            except Exception as e:
                result.errors.append(e)

            if delay_s > 0:
                time.sleep(delay_s)

        result.timing["total_seconds"] = time.perf_counter() - start_time
        return result

    def verify_no_over_exposure(
        self, decisions: List[GateDecision], threshold: int
    ) -> Tuple[bool, Optional[str]]:
        """
        Verify that concurrent decisions did not exceed threshold.

        Counts how many ALLOW/SOFT_WARNING decisions were made.
        If more than threshold, something bypassed the gate.

        Args:
            decisions: List of gate decisions
            threshold: Maximum expected ALLOW/WARNING count

        Returns:
            Tuple of (passed, failure_reason)
        """
        allowed_count = sum(
            1 for d in decisions if d.decision in ("ALLOW", "SOFT_WARNING")
        )

        if allowed_count > threshold:
            return (
                False,
                f"Over-exposure: {allowed_count} allowed, threshold {threshold}",
            )

        return True, None

    def verify_serialization(
        self, execution_order: List[Tuple[str, str]]
    ) -> Tuple[bool, Optional[str]]:
        """
        Verify operations were serialized (no interleaving).

        Args:
            execution_order: List of (operation_id, event) tuples
                            where event is "start" or "end"

        Returns:
            Tuple of (passed, failure_reason)
        """
        stack = []

        for op_id, event in execution_order:
            if event == "start":
                if stack and stack[-1] != op_id:
                    return (
                        False,
                        f"Interleaving detected: {op_id} started while {stack[-1]} running",
                    )
                stack.append(op_id)
            elif event == "end":
                if not stack or stack[-1] != op_id:
                    return False, f"Mismatched end for {op_id}"
                stack.pop()

        return True, None


def generate_signals(
    count: int, instrument: str = "EUR_USD", direction: str = "LONG", units: int = 10000
) -> List[TradeSignal]:
    """
    Generate multiple trade signals.

    Args:
        count: Number of signals to generate
        instrument: Trading pair
        direction: Trade direction
        units: Position size

    Returns:
        List of TradeSignal objects
    """
    return [
        TradeSignal(
            instrument=instrument,
            direction=direction,
            units=units,
            signal_id=f"sig_test_{i:04d}",
        )
        for i in range(count)
    ]


def generate_mixed_signals(count: int) -> List[TradeSignal]:
    """
    Generate mixed signals (different instruments/directions).

    Args:
        count: Number of signals

    Returns:
        List of TradeSignal objects
    """
    instruments = ["EUR_USD", "GBP_USD", "USD_JPY", "AUD_USD"]
    directions = ["LONG", "SHORT"]

    signals = []
    for i in range(count):
        signals.append(
            TradeSignal(
                instrument=instruments[i % len(instruments)],
                direction=directions[i % len(directions)],
                units=10000,
                signal_id=f"sig_mixed_{i:04d}",
            )
        )

    return signals
