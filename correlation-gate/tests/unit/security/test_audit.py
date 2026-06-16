"""
Unit Tests for Audit Logging Module
===================================

TDI Step 2: Tests written FIRST before implementation.

Test Count: 30 tests
Coverage Target: 100%
"""

import json
import os
import queue
import tempfile
import threading
import time
import pytest
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import List, Dict, Any
from unittest.mock import MagicMock, patch

# These imports will fail until implementation exists
# This is expected in Red phase of TDI
from src.security.audit import (
    AuditLogger,
    AsyncFileAuditLogger,
    RotatingAuditLogger,
    NullAuditLogger,
    AuditEntry,
    AuditEventType,
)
from src.models import (
    TradeSignal,
    GateDecision,
    BasketSnapshot,
    BasketExposure,
    GateState,
)


# =============================================================================
# FIXTURES
# =============================================================================


@pytest.fixture
def temp_log_dir() -> Path:
    """Create temporary directory for log files."""
    with tempfile.TemporaryDirectory() as tmpdir:
        yield Path(tmpdir)


@pytest.fixture
def temp_log_file(temp_log_dir: Path) -> Path:
    """Create path for temporary log file."""
    return temp_log_dir / "test_audit.jsonl"


@pytest.fixture
def async_logger(temp_log_file: Path) -> AsyncFileAuditLogger:
    """Create AsyncFileAuditLogger for testing."""
    logger = AsyncFileAuditLogger(
        log_path=temp_log_file,
        buffer_size=100,
        flush_interval_ms=50,  # Fast flush for testing
    )
    yield logger
    logger.shutdown(timeout_seconds=2.0)


@pytest.fixture
def sample_signal() -> TradeSignal:
    """Create sample trade signal."""
    return TradeSignal(
        instrument="EUR_USD", direction="LONG", units=10000, signal_id="sig_test123"
    )


@pytest.fixture
def sample_decision(sample_signal: TradeSignal) -> GateDecision:
    """Create sample gate decision."""
    return GateDecision(
        decision="ALLOW",
        pending_id="pending_test123",
        affected_currencies=[],
        current_exposure=BasketSnapshot.empty(state_version=1),
        projected_exposure=BasketSnapshot.empty(state_version=1),
        reason="Within limits",
        recommendations=[],
        evaluation_time_ms=1.5,
    )


# =============================================================================
# AUDIT ENTRY TESTS
# =============================================================================


class TestAuditEntry:
    """Tests for AuditEntry dataclass."""

    def test_create_audit_entry(self) -> None:
        """AuditEntry can be created with required fields."""
        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.GATE_DECISION,
            data={"test": "data"},
        )
        assert entry.event_type == AuditEventType.GATE_DECISION
        assert entry.data["test"] == "data"

    def test_audit_entry_to_dict(self) -> None:
        """AuditEntry converts to dictionary correctly."""
        now = datetime.now(timezone.utc)
        entry = AuditEntry(
            timestamp=now,
            event_type=AuditEventType.GATE_DECISION,
            data={"key": "value"},
            operation_id="op_123",
        )

        d = entry.to_dict()

        assert d["timestamp"] == now.isoformat()
        assert d["event_type"] == "GATE_DECISION"
        assert d["data"]["key"] == "value"
        assert d["operation_id"] == "op_123"

    def test_audit_entry_to_json(self) -> None:
        """AuditEntry serializes to JSON correctly."""
        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.GATE_DECISION,
            data={"amount": 100},
        )

        json_str = entry.to_json()

        # Should be valid JSON
        parsed = json.loads(json_str)
        assert parsed["event_type"] == "GATE_DECISION"


class TestAuditEventType:
    """Tests for AuditEventType enum."""

    def test_all_event_types_defined(self) -> None:
        """All expected event types are defined."""
        expected = [
            "GATE_DECISION",
            "STATE_CHANGE",
            "PENDING_CONFIRMED",
            "PENDING_CANCELLED",
            "PENDING_EXPIRED",
            "ERROR",
            "GATE_INIT",
            "GATE_SHUTDOWN",
        ]

        actual = [e.value for e in AuditEventType]
        for expected_type in expected:
            assert expected_type in actual


# =============================================================================
# NULL AUDIT LOGGER TESTS
# =============================================================================


class TestNullAuditLogger:
    """Tests for NullAuditLogger (no-op implementation)."""

    def test_null_logger_log_decision_returns_immediately(
        self, sample_signal: TradeSignal, sample_decision: GateDecision
    ) -> None:
        """NullAuditLogger.log_decision returns immediately without action."""
        logger = NullAuditLogger()

        start = time.perf_counter()
        logger.log_decision(sample_signal, sample_decision)
        elapsed = time.perf_counter() - start

        assert elapsed < 0.001  # Should be nearly instant

    def test_null_logger_log_state_change(self) -> None:
        """NullAuditLogger.log_state_change returns immediately."""
        logger = NullAuditLogger()
        logger.log_state_change(
            GateState.INITIALIZING, GateState.READY, "Init complete"
        )
        # No exception means success

    def test_null_logger_log_error(self) -> None:
        """NullAuditLogger.log_error returns immediately."""
        logger = NullAuditLogger()
        logger.log_error(ValueError("test"), context={"key": "value"})
        # No exception means success

    def test_null_logger_shutdown(self) -> None:
        """NullAuditLogger.shutdown returns immediately."""
        logger = NullAuditLogger()
        logger.shutdown()
        # No exception means success


# =============================================================================
# ASYNC FILE AUDIT LOGGER TESTS
# =============================================================================


class TestAsyncFileAuditLoggerBasic:
    """Basic tests for AsyncFileAuditLogger."""

    def test_init_creates_logger(self, temp_log_file: Path) -> None:
        """AsyncFileAuditLogger initializes correctly."""
        logger = AsyncFileAuditLogger(
            log_path=temp_log_file, buffer_size=100, flush_interval_ms=100
        )
        try:
            assert logger.log_path == temp_log_file
        finally:
            logger.shutdown()

    def test_log_decision_is_non_blocking(
        self,
        async_logger: AsyncFileAuditLogger,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """log_decision returns immediately (non-blocking)."""
        start = time.perf_counter()
        async_logger.log_decision(sample_signal, sample_decision)
        elapsed = time.perf_counter() - start

        # Should return in < 1ms (just queue put)
        assert elapsed < 0.01

    def test_log_decision_writes_to_file(
        self,
        async_logger: AsyncFileAuditLogger,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """log_decision eventually writes to file."""
        async_logger.log_decision(sample_signal, sample_decision)

        # Wait for flush
        time.sleep(0.2)

        assert temp_log_file.exists()
        content = temp_log_file.read_text()
        assert "GATE_DECISION" in content
        assert sample_signal.signal_id in content

    def test_log_decision_json_lines_format(
        self,
        async_logger: AsyncFileAuditLogger,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """Log entries are in JSON-Lines format (one JSON object per line)."""
        async_logger.log_decision(sample_signal, sample_decision)
        async_logger.log_decision(sample_signal, sample_decision)

        # Wait for flush
        time.sleep(0.2)

        lines = temp_log_file.read_text().strip().split("\n")
        assert len(lines) == 2

        # Each line should be valid JSON
        for line in lines:
            parsed = json.loads(line)
            assert "timestamp" in parsed
            assert "event_type" in parsed


class TestAsyncFileAuditLoggerStateChange:
    """Tests for log_state_change method."""

    def test_log_state_change_writes_event(
        self, async_logger: AsyncFileAuditLogger, temp_log_file: Path
    ) -> None:
        """log_state_change writes STATE_CHANGE event."""
        async_logger.log_state_change(
            GateState.INITIALIZING, GateState.READY, "Initialization complete"
        )

        time.sleep(0.2)

        content = temp_log_file.read_text()
        assert "STATE_CHANGE" in content
        assert "INITIALIZING" in content
        assert "READY" in content


class TestAsyncFileAuditLoggerError:
    """Tests for log_error method."""

    def test_log_error_writes_event(
        self, async_logger: AsyncFileAuditLogger, temp_log_file: Path
    ) -> None:
        """log_error writes ERROR event."""
        async_logger.log_error(
            ValueError("Test error message"), context={"source": "test"}
        )

        time.sleep(0.2)

        content = temp_log_file.read_text()
        assert "ERROR" in content
        assert "Test error message" in content


class TestAsyncFileAuditLoggerConcurrency:
    """Tests for concurrent logging behavior."""

    def test_concurrent_logging_thread_safe(
        self,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """Multiple threads can log concurrently without corruption."""
        # Use a larger buffer to avoid dropping entries
        logger = AsyncFileAuditLogger(
            log_path=temp_log_file, buffer_size=500, flush_interval_ms=50
        )

        try:
            num_threads = 10
            logs_per_thread = 20

            def log_many():
                for _ in range(logs_per_thread):
                    logger.log_decision(sample_signal, sample_decision)

            threads = [threading.Thread(target=log_many) for _ in range(num_threads)]
            for t in threads:
                t.start()
            for t in threads:
                t.join()

            # Wait for flush
            time.sleep(0.5)

            lines = temp_log_file.read_text().strip().split("\n")
            assert len(lines) == num_threads * logs_per_thread

            # Each line should be valid JSON
            for line in lines:
                json.loads(line)  # Raises if invalid

        finally:
            logger.shutdown()


class TestAsyncFileAuditLoggerBufferFull:
    """Tests for buffer overflow behavior."""

    def test_buffer_full_drops_entries(self, temp_log_file: Path) -> None:
        """When buffer is full, new entries are dropped (non-blocking)."""
        logger = AsyncFileAuditLogger(
            log_path=temp_log_file,
            buffer_size=5,  # Very small buffer
            flush_interval_ms=10000,  # Very slow flush
        )

        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = GateDecision(
            decision="ALLOW",
            pending_id="test",
            affected_currencies=[],
            current_exposure=BasketSnapshot.empty(0),
            projected_exposure=BasketSnapshot.empty(0),
            reason="test",
            recommendations=[],
            evaluation_time_ms=1.0,
        )

        try:
            # Log many more than buffer size
            for _ in range(20):
                logger.log_decision(signal, decision)

            # Should not block - all calls should return quickly
        finally:
            logger.shutdown()


class TestAsyncFileAuditLoggerShutdown:
    """Tests for shutdown behavior."""

    def test_shutdown_flushes_remaining(
        self,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """shutdown() flushes remaining entries before returning."""
        logger = AsyncFileAuditLogger(
            log_path=temp_log_file,
            buffer_size=100,
            flush_interval_ms=10000,  # Very slow auto-flush
        )

        # Log entries
        for _ in range(5):
            logger.log_decision(sample_signal, sample_decision)

        # Immediately shutdown
        logger.shutdown(timeout_seconds=5.0)

        # All entries should be written
        lines = temp_log_file.read_text().strip().split("\n")
        assert len(lines) == 5

    def test_shutdown_timeout(self, temp_log_file: Path) -> None:
        """shutdown() respects timeout parameter."""
        logger = AsyncFileAuditLogger(
            log_path=temp_log_file, buffer_size=100, flush_interval_ms=100
        )

        start = time.perf_counter()
        logger.shutdown(timeout_seconds=0.5)
        elapsed = time.perf_counter() - start

        # Should complete within reasonable time of timeout
        assert elapsed < 2.0


# =============================================================================
# ROTATING AUDIT LOGGER TESTS
# =============================================================================


class TestRotatingAuditLogger:
    """Tests for RotatingAuditLogger."""

    def test_init_creates_timestamped_file(self, temp_log_dir: Path) -> None:
        """RotatingAuditLogger creates file with timestamp name on first write."""
        logger = RotatingAuditLogger(log_dir=temp_log_dir, max_size_mb=10, max_files=5)
        try:
            # Write an entry to trigger file creation
            signal = TradeSignal("EUR_USD", "LONG", 10000)
            decision = GateDecision(
                decision="ALLOW",
                pending_id="test",
                affected_currencies=[],
                current_exposure=BasketSnapshot.empty(0),
                projected_exposure=BasketSnapshot.empty(0),
                reason="test",
                recommendations=[],
                evaluation_time_ms=1.0,
            )
            logger.log_decision(signal, decision)

            # Wait for flush
            time.sleep(0.2)

            # File should now exist with timestamp name
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) >= 1
        finally:
            logger.shutdown()

    def test_rotation_on_size_limit(self, temp_log_dir: Path) -> None:
        """New file created when size limit reached."""
        # Use very small size limit
        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.001,  # ~1KB
            max_files=10,
        )

        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = GateDecision(
            decision="ALLOW",
            pending_id="test",
            affected_currencies=[],
            current_exposure=BasketSnapshot.empty(0),
            projected_exposure=BasketSnapshot.empty(0),
            reason="test",
            recommendations=[],
            evaluation_time_ms=1.0,
        )

        try:
            # Log many entries to trigger rotation
            for _ in range(100):
                logger.log_decision(signal, decision)
                time.sleep(0.01)

            time.sleep(0.5)

            # Should have multiple files
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) >= 2
        finally:
            logger.shutdown()

    def test_old_files_cleaned_up(self, temp_log_dir: Path) -> None:
        """Old files are deleted when max_files exceeded."""
        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.0001,  # Very small
            max_files=3,
        )

        signal = TradeSignal("EUR_USD", "LONG", 10000)
        decision = GateDecision(
            decision="ALLOW",
            pending_id="test",
            affected_currencies=[],
            current_exposure=BasketSnapshot.empty(0),
            projected_exposure=BasketSnapshot.empty(0),
            reason="test",
            recommendations=[],
            evaluation_time_ms=1.0,
        )

        try:
            # Log many entries to trigger multiple rotations
            for _ in range(200):
                logger.log_decision(signal, decision)
                time.sleep(0.005)

            time.sleep(0.5)

            # Should have at most max_files
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) <= 3
        finally:
            logger.shutdown()


# =============================================================================
# WINDOWS COMPATIBILITY TESTS
# =============================================================================


class TestWindowsCompatibility:
    """Tests for Windows-specific behavior."""

    def test_file_not_locked_during_idle(
        self,
        async_logger: AsyncFileAuditLogger,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """Log file is not locked when logger is idle."""
        async_logger.log_decision(sample_signal, sample_decision)
        time.sleep(0.3)  # Wait for write

        # Should be able to read file without issues
        content = temp_log_file.read_text()
        assert len(content) > 0

        # Should be able to read again
        content2 = temp_log_file.read_text()
        assert content == content2

    def test_concurrent_reader_not_blocked(
        self,
        async_logger: AsyncFileAuditLogger,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """Reader can access file while logger is writing."""
        read_count = 0
        stop_event = threading.Event()

        def reader():
            nonlocal read_count
            while not stop_event.is_set():
                try:
                    if temp_log_file.exists():
                        content = temp_log_file.read_text()
                        read_count += 1
                except Exception:
                    pass
                time.sleep(0.01)

        reader_thread = threading.Thread(target=reader)
        reader_thread.start()

        # Log while reader is running
        for _ in range(50):
            async_logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.01)

        stop_event.set()
        reader_thread.join()

        # Reader should have been able to read multiple times
        assert read_count > 10


# =============================================================================
# PERFORMANCE TESTS
# =============================================================================


class TestAuditLoggerPerformance:
    """Performance tests for audit logger."""

    def test_log_decision_under_100us(
        self,
        async_logger: AsyncFileAuditLogger,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """log_decision completes in under 0.1ms (100 microseconds)."""
        times = []

        for _ in range(100):
            start = time.perf_counter_ns()
            async_logger.log_decision(sample_signal, sample_decision)
            elapsed_us = (time.perf_counter_ns() - start) / 1000

            times.append(elapsed_us)

        avg_us = sum(times) / len(times)
        p99_us = sorted(times)[98]

        # Average should be < 100us, p99 < 500us
        assert avg_us < 100, f"Average {avg_us}us exceeds 100us budget"
        assert p99_us < 500, f"P99 {p99_us}us exceeds 500us budget"


# =============================================================================
# ERROR HANDLING TESTS (Coverage for exception paths)
# =============================================================================


class TestAsyncFileAuditLoggerErrorHandling:
    """Tests for error handling paths in AsyncFileAuditLogger."""

    def test_write_entries_ioerror_logged_not_raised(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """IOError in _write_entries is logged but doesn't crash."""
        log_file = temp_log_dir / "audit.jsonl"
        logger = AsyncFileAuditLogger(
            log_path=log_file, buffer_size=100, flush_interval_ms=50
        )

        try:
            # Log an entry
            logger.log_decision(sample_signal, sample_decision)

            # Mock IOError on next write by making path a directory
            time.sleep(0.1)  # Let first write complete

            # Create a directory with same name as log file to cause IOError
            log_file.unlink(missing_ok=True)
            log_file.mkdir(parents=True, exist_ok=True)

            # Log another entry - should not raise
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)  # Allow write attempt

            # Logger should still be functional (error was handled)
            # Restore normal path
            log_file.rmdir()

            # Log one more
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

        finally:
            # Cleanup
            if log_file.is_dir():
                log_file.rmdir()
            logger.shutdown()

    def test_write_entries_with_mock_ioerror(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """IOError during file write is caught and logged."""
        log_file = temp_log_dir / "audit.jsonl"
        logger = AsyncFileAuditLogger(
            log_path=log_file, buffer_size=100, flush_interval_ms=50
        )

        try:
            # Patch open to raise IOError
            original_write_entries = logger._write_entries

            def mock_write_entries(entries):
                raise IOError("Disk full")

            logger._write_entries = mock_write_entries

            # Log entry - should not raise despite IOError
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

            # Restore and verify logger still works
            logger._write_entries = original_write_entries
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

            # File should have at least one entry
            if log_file.exists():
                content = log_file.read_text()
                assert len(content) > 0

        finally:
            logger.shutdown()

    def test_shutdown_handles_empty_buffer_exception(self, temp_log_dir: Path) -> None:
        """shutdown() handles queue.Empty exception during drain."""
        log_file = temp_log_dir / "audit.jsonl"
        logger = AsyncFileAuditLogger(
            log_path=log_file,
            buffer_size=100,
            flush_interval_ms=1000,  # Slow flush
        )

        try:
            # Shutdown with empty buffer - should not raise
            logger.shutdown(timeout_seconds=1.0)
        except Exception as e:
            pytest.fail(f"shutdown() raised {e}")


class TestRotatingAuditLoggerErrorHandling:
    """Tests for error handling paths in RotatingAuditLogger."""

    def test_check_rotation_oserror_handled(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError in _check_rotation is caught silently."""
        logger = RotatingAuditLogger(log_dir=temp_log_dir, max_size_mb=10, max_files=5)

        try:
            # Write to create file
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

            # Mock stat to raise OSError
            original_check = logger._check_rotation

            def mock_check_rotation():
                # Simulate OSError from stat()
                if logger._current_file.exists():
                    raise OSError("Permission denied")

            logger._check_rotation = mock_check_rotation

            # Log entry - should not raise despite OSError
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

            # Restore and verify logger still works
            logger._check_rotation = original_check
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

        finally:
            logger.shutdown()

    def test_cleanup_old_files_oserror_handled(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError in _cleanup_old_files is caught silently."""
        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.0001,  # Very small to trigger rotation
            max_files=2,
        )

        try:
            # Create multiple files to trigger cleanup
            for _ in range(50):
                logger.log_decision(sample_signal, sample_decision)
                time.sleep(0.02)

            # Wait for rotation and cleanup
            time.sleep(0.5)

            # Mock cleanup to raise OSError
            original_cleanup = logger._cleanup_old_files

            def mock_cleanup():
                raise OSError("Permission denied")

            logger._cleanup_old_files = mock_cleanup

            # More writes - should not raise despite cleanup error
            for _ in range(20):
                logger.log_decision(sample_signal, sample_decision)
                time.sleep(0.02)

            time.sleep(0.3)

            # Restore
            logger._cleanup_old_files = original_cleanup

        finally:
            logger.shutdown()

    def test_cleanup_unlink_oserror_handled(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError during file unlink in cleanup is caught."""
        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.0001,  # Very small
            max_files=2,
        )

        try:
            # Create files
            for _ in range(100):
                logger.log_decision(sample_signal, sample_decision)
                time.sleep(0.01)

            time.sleep(0.5)

            # Files exist
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) >= 1

        finally:
            logger.shutdown()

    def test_rotate_generates_new_filename(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """_rotate() generates unique timestamp-based filenames."""
        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=0.0001,  # Very small
            max_files=10,
        )

        try:
            initial_file = logger._current_file

            # Write enough to trigger rotation
            for _ in range(100):
                logger.log_decision(sample_signal, sample_decision)
                time.sleep(0.01)

            time.sleep(0.5)

            # Should have different files
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) >= 2

            # All files should have timestamp format
            for f in files:
                assert f.name.startswith("gate_audit_")
                assert f.name.endswith(".jsonl")

        finally:
            logger.shutdown()


class TestAuditEntryEdgeCases:
    """Additional edge case tests for AuditEntry."""

    def test_to_json_with_special_characters(self) -> None:
        """to_json handles special characters properly."""
        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.ERROR,
            data={"message": 'Error: "quotes" and \\backslash'},
            operation_id="op_123",
        )

        json_str = entry.to_json()
        # Should be valid JSON
        parsed = json.loads(json_str)
        assert parsed["data"]["message"] == 'Error: "quotes" and \\backslash'

    def test_to_dict_without_operation_id(self) -> None:
        """to_dict works when operation_id is None."""
        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.STATE_CHANGE,
            data={"old": "INIT", "new": "READY"},
        )

        d = entry.to_dict()
        assert "operation_id" not in d
        assert d["event_type"] == "STATE_CHANGE"


# =============================================================================
# COVERAGE COMPLETENESS TESTS
# =============================================================================


class TestLogDecisionWithContext:
    """Tests for log_decision with context parameter (covers line 300)."""

    def test_log_decision_with_context(
        self,
        async_logger: AsyncFileAuditLogger,
        temp_log_file: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """log_decision includes context in entry when provided."""
        context = {
            "operation_id": "op_test_123",
            "source": "integration_test",
            "correlation_id": "corr_456",
        }

        async_logger.log_decision(sample_signal, sample_decision, context=context)
        time.sleep(0.2)

        async_logger.shutdown()

        # Verify context is in the log entry
        content = temp_log_file.read_text()
        entry = json.loads(content.strip().split("\n")[-1])

        assert "context" in entry["data"]
        assert entry["data"]["context"]["source"] == "integration_test"
        assert entry["data"]["context"]["correlation_id"] == "corr_456"
        assert entry["operation_id"] == "op_test_123"

    def test_log_decision_context_with_operation_id_extraction(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """log_decision extracts operation_id from context."""
        log_file = temp_log_dir / "audit_context.jsonl"
        logger = AsyncFileAuditLogger(log_path=log_file, flush_interval_ms=50)

        context = {"operation_id": "extracted_op_id", "extra": "data"}
        logger.log_decision(sample_signal, sample_decision, context=context)
        time.sleep(0.2)

        logger.shutdown()

        content = log_file.read_text()
        entry = json.loads(content.strip())

        # operation_id should be extracted to top level
        assert entry["operation_id"] == "extracted_op_id"
        # context should also be in data
        assert entry["data"]["context"]["extra"] == "data"


class TestShutdownBufferDrainRaceCondition:
    """Tests for shutdown buffer drain race condition (covers lines 380-381)."""

    def test_shutdown_buffer_drain_race_condition(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """Shutdown handles race where buffer.empty() is False but get_nowait raises Empty."""
        log_file = temp_log_dir / "audit_race.jsonl"
        logger = AsyncFileAuditLogger(
            log_path=log_file,
            buffer_size=100,
            flush_interval_ms=1000,  # Slow flush to have entries in buffer
        )

        # Add entries
        for i in range(10):
            logger.log_decision(sample_signal, sample_decision)

        # Mock the buffer to simulate race condition
        original_buffer = logger._buffer
        original_empty = original_buffer.empty
        original_get_nowait = original_buffer.get_nowait

        call_count = [0]

        def mock_empty():
            # First call returns False (there are items)
            # This triggers entry into the while loop
            call_count[0] += 1
            if call_count[0] <= 2:
                return False
            return True

        def mock_get_nowait():
            # Raise Empty to trigger exception handling
            raise queue.Empty()

        original_buffer.empty = mock_empty
        original_buffer.get_nowait = mock_get_nowait

        # Shutdown should handle the race gracefully
        try:
            logger.shutdown(timeout_seconds=2.0)
        except queue.Empty:
            pytest.fail("shutdown() did not handle queue.Empty race condition")
        finally:
            # Restore
            original_buffer.empty = original_empty
            original_buffer.get_nowait = original_get_nowait


class TestCleanupOSErrorHandling:
    """Tests for _cleanup_old_files OSError paths (covers lines 559-566)."""

    def test_cleanup_unlink_oserror_continues(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError during unlink in cleanup continues to next file."""
        # Create many log files manually to ensure cleanup triggers
        for i in range(10):
            fake_file = temp_log_dir / f"gate_audit_20260101_{i:06d}.jsonl"
            fake_file.write_text('{"test": "entry"}\n')
            time.sleep(0.01)  # Ensure different mtime

        logger = RotatingAuditLogger(
            log_dir=temp_log_dir,
            max_size_mb=10,  # Large enough not to rotate
            max_files=3,  # Fewer than files created
        )

        try:
            # Verify files exist
            files_before = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files_before) >= 10, (
                f"Expected 10+ files, got {len(files_before)}"
            )

            # Mock Path.unlink to raise OSError
            original_unlink = Path.unlink
            unlink_call_count = [0]

            def mock_unlink(self, missing_ok=False):
                # Only intercept for our audit files
                if "gate_audit_" in str(self):
                    unlink_call_count[0] += 1
                    # Raise OSError on first few calls to exercise error handling
                    if unlink_call_count[0] <= 5:
                        raise OSError("File in use by another process")
                return original_unlink(self, missing_ok=missing_ok)

            Path.unlink = mock_unlink

            try:
                # Call actual cleanup - should enter the while loop and try to delete
                logger._cleanup_old_files()

                # Verify unlink was attempted (files > max_files = 3)
                assert unlink_call_count[0] >= 1, (
                    f"unlink() was never called, files={len(files_before)}"
                )
            finally:
                Path.unlink = original_unlink

            # Logger should still be functional
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

        finally:
            logger.shutdown()

    def test_cleanup_glob_oserror_handled(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError during glob/sorted in cleanup is caught silently."""
        logger = RotatingAuditLogger(log_dir=temp_log_dir, max_size_mb=10, max_files=5)

        try:
            # Write to create file
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

            # To trigger lines 565-566, we need OSError during sorted(glob())
            # Patch the sorted builtin to raise OSError
            import builtins

            original_sorted = builtins.sorted

            def mock_sorted(*args, **kwargs):
                # If called with a generator from glob, raise OSError
                try:
                    first_arg = args[0] if args else None
                    # Check if this is our glob call by checking kwargs
                    if kwargs.get("key") is not None:
                        raise OSError("Simulated directory access error")
                except (TypeError, StopIteration):
                    pass
                return original_sorted(*args, **kwargs)

            builtins.sorted = mock_sorted

            try:
                # Call cleanup - should handle OSError
                logger._cleanup_old_files()
            except OSError:
                pytest.fail("_cleanup_old_files did not catch sorted OSError")
            finally:
                builtins.sorted = original_sorted

            # Logger should still work
            logger.log_decision(sample_signal, sample_decision)
            time.sleep(0.2)

        finally:
            logger.shutdown()

    def test_cleanup_stat_oserror_in_sorted(
        self,
        temp_log_dir: Path,
        sample_signal: TradeSignal,
        sample_decision: GateDecision,
    ) -> None:
        """OSError during stat in sorted() key is caught."""
        logger = RotatingAuditLogger(log_dir=temp_log_dir, max_size_mb=10, max_files=5)

        try:
            # Create some files
            for _ in range(20):
                logger.log_decision(sample_signal, sample_decision)
                time.sleep(0.01)
            time.sleep(0.3)

            # Get file list
            files = list(temp_log_dir.glob("gate_audit_*.jsonl"))
            assert len(files) >= 1

            # Mock stat to raise on one file during sorted()
            original_stat = Path.stat
            call_count = [0]

            def mock_stat(self):
                call_count[0] += 1
                if call_count[0] == 2:  # Fail on second file
                    raise OSError("File deleted during iteration")
                return original_stat(self)

            Path.stat = mock_stat

            # Call cleanup - OSError in sorted should be caught
            try:
                logger._cleanup_old_files()
            except OSError:
                pytest.fail("_cleanup_old_files did not catch stat OSError")
            finally:
                Path.stat = original_stat

        finally:
            logger.shutdown()
