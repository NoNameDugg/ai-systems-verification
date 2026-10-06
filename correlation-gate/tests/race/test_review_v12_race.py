"""
External review (2026-10) reproduction — a race test with teeth
================================================================

Finding #9: every test under ``tests/race`` passed with the gate's lock
replaced by a no-op, because they asserted only completion counts, decision
strings being valid, or weak inequalities. None could detect a race.

This test uses the oracle that already existed but had no callers
(``ConcurrentTestHarness.verify_no_over_exposure``): N threads submit
same-direction USD signals at once; at most ``hard_block_count - 1`` may be
approved, and the pending set must never exceed that either.

Committed FAILING (``xfail(strict=True)``) because finding #5 (pending
off-by-one) lets a third approval through even with the lock intact. After the
#5 fix it passes with the lock and fails again when the lock is a no-op — the
mutation that proves it bites (see MUTATIONS.md in the sprint record).
"""

import threading
import time
from datetime import datetime, timezone
from typing import List

import pytest

from src.gate import CorrelationGate
from src.models import GateConfig, Position, TradeSignal
from tests.race.harness import ConcurrentTestHarness


class _SlowEmptyProvider:
    """No positions, but a deliberate fetch delay to widen the race window."""

    def __init__(self, delay_ms: int = 20):
        self._delay_ms = delay_ms

    def fetch_positions(self) -> List[Position]:
        time.sleep(self._delay_ms / 1000.0)
        return []

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


@pytest.mark.xfail(
    strict=True,
    reason="review #5/#9: a third same-basket approval is issued under limit 3",
)
def test_concurrent_same_basket_never_exceeds_limit():
    config = GateConfig(
        soft_warning_count=2,
        hard_block_count=3,
        lock_timeout_seconds=10.0,
        position_fetch_timeout_seconds=10.0,
    )
    gate = CorrelationGate(config, _SlowEmptyProvider(delay_ms=20))
    assert gate.initialize()
    # force a (slow) provider fetch inside every evaluate() so the critical
    # section is wide enough for an unserialised gate to interleave
    gate._last_fetch = None

    n = 20
    signals = [TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"s{i}") for i in range(n)]
    harness = ConcurrentTestHarness(num_threads=n)
    result = harness.run_concurrent(gate, signals)

    assert not result.errors, result.errors
    assert len(result.decisions) == n

    max_approved = config.hard_block_count - 1
    ok, why = harness.verify_no_over_exposure(result.decisions, threshold=max_approved)
    assert ok, why
    assert len(gate.get_pending_ids()) <= max_approved


def test_lock_timeout_waiter_is_hard_blocked_with_reason():
    """A caller that cannot acquire the lock in time gets a fail-closed block."""
    config = GateConfig(lock_timeout_seconds=0.05)
    gate = CorrelationGate(config, _SlowEmptyProvider(delay_ms=0))
    assert gate.initialize()

    holder_has_lock = threading.Event()
    release = threading.Event()

    def hold_lock():
        gate._lock.acquire()
        holder_has_lock.set()
        release.wait(timeout=5.0)
        gate._lock.release()

    t = threading.Thread(target=hold_lock)
    t.start()
    assert holder_has_lock.wait(timeout=2.0)
    try:
        decision = gate.evaluate(TradeSignal("EUR_USD", "LONG", 10000))
    finally:
        release.set()
        t.join(timeout=5.0)

    assert decision.decision == "HARD_BLOCK"
    assert "Lock acquisition timeout" in decision.reason
    assert decision.pending_id is None
