"""
DEC-062: Handshake Protocol Unit Tests
======================================

Tests for the synchronize_with_engine() method that eliminates zombie slots.

Tests verify:
1. SyncReport correctly reports purged and retained IDs
2. SyncResult status reflects SUCCESS/FAILED appropriately
3. Zombie slots (pending IDs unknown to engine) are purged
4. Active slots (pending IDs known to engine) are retained
5. Gate transitions to FAILED state on sync exception
"""

import pytest
from datetime import datetime, timezone, timedelta
from decimal import Decimal

from src.gate import CorrelationGate
from src.models import (
    GateConfig,
    GateState,
    TradeSignal,
    SyncReport,
    SyncResult,
)

# Import mock provider from fixtures
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "fixtures"))
from mock_providers import MockPositionProvider


# =============================================================================
# FIXTURES
# =============================================================================


@pytest.fixture
def default_config() -> GateConfig:
    """Standard gate configuration for testing."""
    return GateConfig(
        enabled=True,
        soft_warning_count=2,
        hard_block_count=3,
        pending_timeout_seconds=30.0,
    )


@pytest.fixture
def mock_provider() -> MockPositionProvider:
    """Empty position provider."""
    return MockPositionProvider(positions=[])


@pytest.fixture
def initialized_gate(default_config, mock_provider) -> CorrelationGate:
    """Gate that has been initialized and is in READY state."""
    gate = CorrelationGate(default_config, mock_provider)
    assert gate.initialize()
    assert gate.state == GateState.READY
    return gate


# =============================================================================
# SYNC REPORT TESTS
# =============================================================================


class TestSyncReport:
    """Tests for SyncReport dataclass."""

    def test_sync_report_creation(self):
        """SyncReport can be created with required fields."""
        report = SyncReport(
            purged_ids=["zombie_1", "zombie_2"],
            retained_ids=["active_1"],
        )
        assert report.purged_ids == ["zombie_1", "zombie_2"]
        assert report.retained_ids == ["active_1"]
        assert isinstance(report.sync_timestamp, datetime)

    def test_sync_report_empty_lists(self):
        """SyncReport handles empty lists."""
        report = SyncReport(purged_ids=[], retained_ids=[])
        assert len(report.purged_ids) == 0
        assert len(report.retained_ids) == 0

    def test_sync_report_timestamp_auto_generated(self):
        """SyncReport auto-generates timestamp if not provided."""
        before = datetime.now(timezone.utc)
        report = SyncReport(purged_ids=[], retained_ids=[])
        after = datetime.now(timezone.utc)
        assert before <= report.sync_timestamp <= after


# =============================================================================
# SYNC RESULT TESTS
# =============================================================================


class TestSyncResult:
    """Tests for SyncResult dataclass."""

    def test_sync_result_success(self):
        """SyncResult with SUCCESS status."""
        report = SyncReport(purged_ids=[], retained_ids=[])
        result = SyncResult(status="SUCCESS", report=report)
        assert result.status == "SUCCESS"
        assert result.report == report

    def test_sync_result_failed(self):
        """SyncResult with FAILED status."""
        report = SyncReport(purged_ids=[], retained_ids=[])
        result = SyncResult(status="FAILED", report=report)
        assert result.status == "FAILED"

    def test_sync_result_invalid_status_raises(self):
        """SyncResult raises on invalid status."""
        report = SyncReport(purged_ids=[], retained_ids=[])
        with pytest.raises(ValueError) as exc_info:
            SyncResult(status="INVALID", report=report)
        assert "SUCCESS or FAILED" in str(exc_info.value)


# =============================================================================
# SYNCHRONIZE WITH ENGINE TESTS
# =============================================================================


class TestSynchronizeWithEngine:
    """Tests for gate.synchronize_with_engine() method."""

    def test_sync_empty_gate_empty_engine(self, initialized_gate):
        """Sync with no pending on either side returns empty SUCCESS."""
        result = initialized_gate.synchronize_with_engine([])

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 0
        assert len(result.report.retained_ids) == 0

    def test_sync_purges_zombies(self, initialized_gate):
        """Pending IDs not in engine's list are purged."""
        # Add some pending signals to the gate directly
        signal1 = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        signal2 = TradeSignal(instrument="GBP_USD", direction="SHORT", units=2000)

        # Evaluate to create pending entries
        decision1 = initialized_gate.evaluate(signal1)
        decision2 = initialized_gate.evaluate(signal2)

        assert decision1.pending_id is not None
        assert decision2.pending_id is not None

        # Verify gate has 2 pending
        assert len(initialized_gate.get_pending_ids()) == 2

        # Engine only knows about one of them
        engine_active_ids = [decision1.pending_id]

        result = initialized_gate.synchronize_with_engine(engine_active_ids)

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 1
        assert decision2.pending_id in result.report.purged_ids
        assert len(result.report.retained_ids) == 1
        assert decision1.pending_id in result.report.retained_ids

        # Verify gate now has only 1 pending
        assert len(initialized_gate.get_pending_ids()) == 1
        assert decision1.pending_id in initialized_gate.get_pending_ids()

    def test_sync_retains_all_active(self, initialized_gate):
        """All pending IDs known to engine are retained."""
        # Create pending signals
        signal1 = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        signal2 = TradeSignal(instrument="USD_JPY", direction="LONG", units=2000)

        decision1 = initialized_gate.evaluate(signal1)
        decision2 = initialized_gate.evaluate(signal2)

        # Engine knows about both
        engine_active_ids = [decision1.pending_id, decision2.pending_id]

        result = initialized_gate.synchronize_with_engine(engine_active_ids)

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 0
        assert len(result.report.retained_ids) == 2
        assert len(initialized_gate.get_pending_ids()) == 2

    def test_sync_purges_all_zombies(self, initialized_gate):
        """All pending IDs not in engine's list are purged."""
        # Create pending signals
        signal1 = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        signal2 = TradeSignal(instrument="GBP_USD", direction="SHORT", units=2000)
        signal3 = TradeSignal(instrument="USD_JPY", direction="LONG", units=3000)

        initialized_gate.evaluate(signal1)
        initialized_gate.evaluate(signal2)
        initialized_gate.evaluate(signal3)

        assert len(initialized_gate.get_pending_ids()) == 3

        # Engine knows about none of them (crash scenario)
        result = initialized_gate.synchronize_with_engine([])

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 3
        assert len(result.report.retained_ids) == 0
        assert len(initialized_gate.get_pending_ids()) == 0

    def test_sync_handles_engine_ids_not_in_gate(self, initialized_gate):
        """Engine IDs not in gate are ignored (not an error)."""
        # Engine has IDs that gate doesn't know about
        # (this could happen if gate was restarted)
        result = initialized_gate.synchronize_with_engine(
            ["unknown_id_1", "unknown_id_2"]
        )

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 0
        assert len(result.report.retained_ids) == 0

    def test_sync_mixed_scenario(self, initialized_gate):
        """Mixed scenario: some retained, some purged, some unknown."""
        # Create 3 pending signals in gate
        signal1 = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        signal2 = TradeSignal(instrument="GBP_USD", direction="SHORT", units=2000)
        signal3 = TradeSignal(instrument="USD_JPY", direction="LONG", units=3000)

        decision1 = initialized_gate.evaluate(signal1)
        decision2 = initialized_gate.evaluate(signal2)
        decision3 = initialized_gate.evaluate(signal3)

        # Engine knows about decision1 and decision3, plus an unknown ID
        engine_ids = [
            decision1.pending_id,
            decision3.pending_id,
            "unknown_from_old_session",
        ]

        result = initialized_gate.synchronize_with_engine(engine_ids)

        assert result.status == "SUCCESS"
        # decision2 should be purged
        assert len(result.report.purged_ids) == 1
        assert decision2.pending_id in result.report.purged_ids
        # decision1 and decision3 should be retained
        assert len(result.report.retained_ids) == 2
        assert decision1.pending_id in result.report.retained_ids
        assert decision3.pending_id in result.report.retained_ids

    def test_sync_gate_remains_ready(self, initialized_gate):
        """Gate remains in READY state after successful sync."""
        signal = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        initialized_gate.evaluate(signal)

        result = initialized_gate.synchronize_with_engine([])

        assert result.status == "SUCCESS"
        assert initialized_gate.state == GateState.READY


class TestSynchronizeFailureModes:
    """Tests for synchronize_with_engine failure scenarios."""

    def test_sync_fails_sets_gate_failed(self, default_config, mock_provider):
        """Exception during sync sets gate to FAILED state."""
        gate = CorrelationGate(default_config, mock_provider)
        gate.initialize()

        # Monkey-patch _pending to raise on iteration
        class FailingDict(dict):
            def keys(self):
                raise RuntimeError("Simulated failure during sync")

        gate._pending = FailingDict()

        result = gate.synchronize_with_engine(["some_id"])

        assert result.status == "FAILED"
        assert gate.state == GateState.FAILED

    def test_failed_sync_returns_empty_report(self, default_config, mock_provider):
        """Failed sync returns empty report."""
        gate = CorrelationGate(default_config, mock_provider)
        gate.initialize()

        # Cause failure
        class FailingDict(dict):
            def keys(self):
                raise RuntimeError("Simulated failure")

        gate._pending = FailingDict()

        result = gate.synchronize_with_engine([])

        assert result.status == "FAILED"
        assert len(result.report.purged_ids) == 0
        assert len(result.report.retained_ids) == 0


# =============================================================================
# CRASH RECOVERY SCENARIO TESTS
# =============================================================================


class TestCrashRecoveryScenarios:
    """Tests simulating engine crash and restart scenarios."""

    def test_engine_restart_with_persisted_state(self, initialized_gate):
        """
        Simulate: Engine crashes after getting pending_id, restarts with persisted state.
        Gate should retain the pending slot.
        """
        # Engine gets a pending_id
        signal = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        decision = initialized_gate.evaluate(signal)
        pending_id = decision.pending_id

        # Simulate engine restart - engine loads persisted pending_id
        persisted_ids = [pending_id]

        # Engine performs handshake at startup
        result = initialized_gate.synchronize_with_engine(persisted_ids)

        assert result.status == "SUCCESS"
        assert len(result.report.retained_ids) == 1
        assert pending_id in result.report.retained_ids
        assert pending_id in initialized_gate.get_pending_ids()

    def test_engine_restart_without_persistence(self, initialized_gate):
        """
        Simulate: Engine crashes and restarts without persisted state.
        Gate should purge all zombie slots.
        """
        # Engine creates multiple pending signals
        signal1 = TradeSignal(instrument="EUR_USD", direction="LONG", units=1000)
        signal2 = TradeSignal(instrument="GBP_USD", direction="SHORT", units=2000)

        initialized_gate.evaluate(signal1)
        initialized_gate.evaluate(signal2)

        assert len(initialized_gate.get_pending_ids()) == 2

        # Engine restarts with no persisted state
        result = initialized_gate.synchronize_with_engine([])

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 2
        assert len(initialized_gate.get_pending_ids()) == 0

    def test_partial_persistence_recovery(self, initialized_gate):
        """
        Simulate: Engine crashes with partial state persistence.
        Only persisted IDs should be retained.
        """
        # Create 3 pending signals
        signals = [
            TradeSignal(instrument="EUR_USD", direction="LONG", units=1000),
            TradeSignal(instrument="GBP_USD", direction="SHORT", units=2000),
            TradeSignal(instrument="USD_JPY", direction="LONG", units=3000),
        ]

        decisions = [initialized_gate.evaluate(s) for s in signals]
        pending_ids = [d.pending_id for d in decisions]

        # Simulate: Only first signal's state was persisted before crash
        persisted = [pending_ids[0]]

        result = initialized_gate.synchronize_with_engine(persisted)

        assert result.status == "SUCCESS"
        assert len(result.report.purged_ids) == 2
        assert len(result.report.retained_ids) == 1
        assert pending_ids[0] in result.report.retained_ids
        assert pending_ids[1] in result.report.purged_ids
        assert pending_ids[2] in result.report.purged_ids
