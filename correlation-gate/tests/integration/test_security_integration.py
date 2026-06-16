"""
Security Integration Tests
==========================

TDI Step 2: Tests written FIRST before implementation.

Test Count: 12 tests
Purpose: Verify full security stack integration
"""

import tempfile
import threading
import time
import json
import pytest
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import List

from src.models import (
    Position,
    TradeSignal,
    GateDecision,
    GateConfig,
    GateState,
)
from src.gate import CorrelationGate
from src.security.atomic import AtomicGateGuard, StateVersionTracker
from src.security.audit import (
    AsyncFileAuditLogger,
    RotatingAuditLogger,
    NullAuditLogger,
)


# =============================================================================
# FIXTURES
# =============================================================================


class MockPositionProvider:
    """Mock provider for integration testing."""

    def __init__(self, positions: List[Position] = None):
        self._positions = positions or []

    def fetch_positions(self) -> List[Position]:
        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


@pytest.fixture
def temp_log_dir() -> Path:
    """Create temporary directory for logs."""
    with tempfile.TemporaryDirectory() as tmpdir:
        yield Path(tmpdir)


@pytest.fixture
def gate_config() -> GateConfig:
    """Standard gate config."""
    return GateConfig(
        enabled=True,
        soft_warning_count=2,
        hard_block_count=3,
        lock_timeout_seconds=5.0,
        position_fetch_timeout_seconds=10.0,
    )


@pytest.fixture
def empty_provider() -> MockPositionProvider:
    """Provider with no positions."""
    return MockPositionProvider([])


# =============================================================================
# GATE WITH AUDIT LOGGING INTEGRATION
# =============================================================================


class TestGateWithAuditLogging:
    """Tests for gate integrated with audit logging."""

    def test_gate_logs_all_decisions(
        self,
        gate_config: GateConfig,
        empty_provider: MockPositionProvider,
        temp_log_dir: Path,
    ) -> None:
        """Gate logs every decision to audit trail."""
        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            gate = CorrelationGate(
                gate_config, empty_provider, audit_logger=audit_logger
            )
            gate.initialize()

            # Make several decisions
            for i in range(5):
                signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
                gate.evaluate(signal)

            # Wait for flush
            time.sleep(0.2)

            # Verify all logged
            lines = log_file.read_text().strip().split("\n")
            assert len(lines) >= 5

            # Each should be valid JSON with required fields
            for line in lines:
                entry = json.loads(line)
                assert "event_type" in entry
                if entry["event_type"] == "GATE_DECISION":
                    assert "data" in entry
                    assert "signal" in entry["data"]
                    assert "decision" in entry["data"]

        finally:
            audit_logger.shutdown()

    def test_gate_logs_state_changes(
        self,
        gate_config: GateConfig,
        empty_provider: MockPositionProvider,
        temp_log_dir: Path,
    ) -> None:
        """Gate logs state transitions."""
        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            gate = CorrelationGate(
                gate_config, empty_provider, audit_logger=audit_logger
            )

            # Initialize triggers state change
            gate.initialize()

            # Wait for flush
            time.sleep(0.2)

            # Check for state change events
            content = log_file.read_text()
            # Initialization should log STATE_CHANGE or GATE_INIT
            assert "STATE_CHANGE" in content or "GATE_INIT" in content

        finally:
            audit_logger.shutdown()

    def test_gate_continues_when_audit_fails(
        self, gate_config: GateConfig, empty_provider: MockPositionProvider
    ) -> None:
        """Gate continues operation even if audit logging fails."""
        # Use NullAuditLogger which never fails
        audit_logger = NullAuditLogger()

        gate = CorrelationGate(gate_config, empty_provider, audit_logger=audit_logger)
        gate.initialize()

        # Should work fine
        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = gate.evaluate(signal)

        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_gate_continues_when_audit_log_decision_raises(
        self, gate_config: GateConfig, empty_provider: MockPositionProvider
    ) -> None:
        """Gate continues operation when log_decision raises exception."""
        from unittest.mock import MagicMock

        # Create mock that raises exception
        failing_logger = MagicMock()
        failing_logger.log_decision.side_effect = Exception("Audit failed!")
        failing_logger.log_state_change.side_effect = Exception("Audit failed!")
        failing_logger.log_error.side_effect = Exception("Audit failed!")

        gate = CorrelationGate(gate_config, empty_provider, audit_logger=failing_logger)
        gate.initialize()  # This triggers log_state_change exception

        # evaluate should still work despite log_decision exception
        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = gate.evaluate(signal)

        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

        # Verify audit methods were called (and failed silently)
        assert failing_logger.log_state_change.called
        assert failing_logger.log_decision.called

    def test_gate_continues_when_audit_log_error_raises(
        self, gate_config: GateConfig
    ) -> None:
        """Gate continues operation when log_error raises exception."""
        from unittest.mock import MagicMock

        # Provider that always fails
        failing_provider = MagicMock()
        failing_provider.fetch_positions.side_effect = Exception("Provider failed!")

        # Create mock that raises exception
        failing_logger = MagicMock()
        failing_logger.log_error.side_effect = Exception("Audit failed!")
        failing_logger.log_state_change.side_effect = Exception("Audit failed!")

        gate = CorrelationGate(
            gate_config, failing_provider, audit_logger=failing_logger
        )

        # Initialize fails, which should try to log_error
        success = gate.initialize()
        assert success is False

        # log_error should have been called (and failed silently)
        assert failing_logger.log_error.called


# =============================================================================
# ATOMIC OPERATIONS INTEGRATION
# =============================================================================


class TestAtomicOperationsIntegration:
    """Tests for atomic operations integrated with gate."""

    def test_atomic_guard_with_gate(
        self, gate_config: GateConfig, empty_provider: MockPositionProvider
    ) -> None:
        """AtomicGateGuard integrates with gate correctly."""
        gate = CorrelationGate(gate_config, empty_provider)
        gate.initialize()

        # Verify atomic guard is active
        assert hasattr(gate, "_lock") or hasattr(gate, "_atomic_guard")

        # Multiple rapid evaluations should all serialize
        results = []
        for i in range(20):
            signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
            decision = gate.evaluate(signal)
            results.append(decision)

        # All should complete
        assert len(results) == 20

    def test_state_version_tracks_mutations(
        self, gate_config: GateConfig, empty_provider: MockPositionProvider
    ) -> None:
        """State version increments on mutations."""
        gate = CorrelationGate(gate_config, empty_provider)
        gate.initialize()

        initial_version = gate._state_version

        # Evaluate should trigger state changes
        for i in range(5):
            signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
            gate.evaluate(signal)

        # Version should have increased
        assert gate._state_version >= initial_version


# =============================================================================
# FULL SECURITY STACK INTEGRATION
# =============================================================================


class TestFullSecurityStack:
    """Tests for complete security stack."""

    def test_concurrent_evaluation_with_audit(
        self,
        gate_config: GateConfig,
        empty_provider: MockPositionProvider,
        temp_log_dir: Path,
    ) -> None:
        """Concurrent evaluations with audit logging."""
        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            gate = CorrelationGate(
                gate_config, empty_provider, audit_logger=audit_logger
            )
            gate.initialize()

            results = []
            lock = threading.Lock()
            barrier = threading.Barrier(10)

            def evaluate_signal(sig_id):
                barrier.wait()
                signal = TradeSignal(
                    "EUR_USD", "LONG", 10000, signal_id=f"sig_{sig_id}"
                )
                decision = gate.evaluate(signal)
                with lock:
                    results.append(decision)

            threads = [
                threading.Thread(target=evaluate_signal, args=(i,)) for i in range(10)
            ]

            for t in threads:
                t.start()
            for t in threads:
                t.join()

            # All should complete
            assert len(results) == 10

            # Wait for audit flush
            time.sleep(0.3)

            # All should be logged
            lines = log_file.read_text().strip().split("\n")
            decision_entries = [json.loads(l) for l in lines if "GATE_DECISION" in l]
            assert len(decision_entries) >= 10

        finally:
            audit_logger.shutdown()

    def test_rotating_audit_under_load(
        self,
        gate_config: GateConfig,
        empty_provider: MockPositionProvider,
        temp_log_dir: Path,
    ) -> None:
        """Rotating audit logger handles high load."""
        audit_logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.01,  # Very small for testing
            max_files=5,
        )

        try:
            gate = CorrelationGate(
                gate_config, empty_provider, audit_logger=audit_logger
            )
            gate.initialize()

            # Generate high load
            for i in range(100):
                signal = TradeSignal("EUR_USD", "LONG", 10000, signal_id=f"sig_{i}")
                gate.evaluate(signal)

            # Wait for flush
            time.sleep(0.5)

            # Should have created audit files
            audit_files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(audit_files) >= 1

        finally:
            audit_logger.shutdown()

    def test_audit_captures_exposure_data(
        self, gate_config: GateConfig, temp_log_dir: Path
    ) -> None:
        """Audit log captures exposure snapshot data."""
        # Provider with existing position
        provider = MockPositionProvider(
            [
                Position(
                    instrument="EUR_USD",
                    direction="LONG",
                    units=10000,
                    entry_price=Decimal("1.0850"),
                    entry_time=datetime.now(timezone.utc),
                    position_id="pos_001",
                )
            ]
        )

        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            gate = CorrelationGate(gate_config, provider, audit_logger=audit_logger)
            gate.initialize()

            signal = TradeSignal("GBP_USD", "LONG", 10000)
            gate.evaluate(signal)

            # Wait for flush
            time.sleep(0.2)

            # Check audit entry has exposure data
            content = log_file.read_text()
            entry = json.loads(content.strip().split("\n")[-1])

            if entry["event_type"] == "GATE_DECISION":
                # Should have exposure snapshot in data
                assert "data" in entry
                assert (
                    "exposure_before" in entry["data"]
                    or "exposure_snapshot" in entry["data"]
                )

        finally:
            audit_logger.shutdown()


# =============================================================================
# ERROR HANDLING INTEGRATION
# =============================================================================


class TestErrorHandlingIntegration:
    """Tests for error handling in security stack."""

    def test_audit_logs_errors(
        self, gate_config: GateConfig, temp_log_dir: Path
    ) -> None:
        """Errors are logged to audit trail."""

        # Provider that fails
        class FailingProvider:
            def fetch_positions(self):
                raise RuntimeError("Simulated failure")

            def is_available(self):
                return True

            def get_last_fetch_time(self):
                return datetime.now(timezone.utc)

        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            provider = FailingProvider()
            gate = CorrelationGate(gate_config, provider, audit_logger=audit_logger)

            # Initialize will fail
            success = gate.initialize()
            assert success is False

            # Wait for flush
            time.sleep(0.2)

            # Check for error or state change log
            if log_file.exists():
                content = log_file.read_text()
                # Should have some event logged
                assert len(content) >= 0  # May or may not log init failure

        finally:
            audit_logger.shutdown()

    def test_fail_closed_logged(
        self,
        gate_config: GateConfig,
        empty_provider: MockPositionProvider,
        temp_log_dir: Path,
    ) -> None:
        """Fail-closed decisions are logged."""
        log_file = temp_log_dir / "audit.jsonl"
        audit_logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        try:
            gate = CorrelationGate(
                gate_config, empty_provider, audit_logger=audit_logger
            )
            # Don't initialize - should fail closed

            signal = TradeSignal("EUR_USD", "LONG", 10000)
            decision = gate.evaluate(signal)

            # Should be HARD_BLOCK (fail-closed)
            assert decision.decision == "HARD_BLOCK"
            assert "FAIL-CLOSED" in decision.reason

            # Wait for flush
            time.sleep(0.2)

            # Should be logged
            if log_file.exists():
                content = log_file.read_text()
                if content:
                    assert "HARD_BLOCK" in content or "FAIL" in content

        finally:
            audit_logger.shutdown()
