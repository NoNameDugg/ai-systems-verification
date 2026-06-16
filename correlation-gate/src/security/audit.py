"""
Audit Logging for Correlation Gate
===================================

Windows-safe, non-blocking audit trail implementation.

Components:
- AuditLogger: Abstract base for audit loggers
- AsyncFileAuditLogger: Async file-based logger
- RotatingAuditLogger: Size-based rotating logger
- NullAuditLogger: No-op logger for testing
"""

import json
import logging
import queue
import threading
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional

from src.models import TradeSignal, GateDecision, GateState


logger = logging.getLogger(__name__)


class AuditEventType(Enum):
    """
    Types of audit events.

    Each event type captures different gate activity.
    """

    GATE_DECISION = "GATE_DECISION"
    STATE_CHANGE = "STATE_CHANGE"
    PENDING_CONFIRMED = "PENDING_CONFIRMED"
    PENDING_CANCELLED = "PENDING_CANCELLED"
    PENDING_EXPIRED = "PENDING_EXPIRED"
    ERROR = "ERROR"
    GATE_INIT = "GATE_INIT"
    GATE_SHUTDOWN = "GATE_SHUTDOWN"


@dataclass
class AuditEntry:
    """
    Single audit log entry.

    Attributes:
        timestamp: When event occurred
        event_type: Type of event
        data: Event-specific data
        operation_id: Optional correlation ID
    """

    timestamp: datetime
    event_type: AuditEventType
    data: Dict[str, Any]
    operation_id: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """
        Convert to dictionary for JSON serialization.

        Returns:
            Dictionary representation
        """
        result = {
            "timestamp": self.timestamp.isoformat(),
            "event_type": self.event_type.value,
            "data": self.data,
        }
        if self.operation_id:
            result["operation_id"] = self.operation_id
        return result

    def to_json(self) -> str:
        """
        Serialize to JSON string.

        Returns:
            JSON string (single line, no trailing newline)
        """
        return json.dumps(self.to_dict(), separators=(",", ":"), default=str)


class AuditLogger(ABC):
    """
    Abstract base for audit loggers.

    All implementations must be:
    - Thread-safe
    - Non-blocking
    - Windows-compatible

    Subclasses should implement the abstract methods.
    """

    @abstractmethod
    def log_decision(
        self,
        signal: TradeSignal,
        decision: GateDecision,
        context: Optional[Dict[str, Any]] = None,
    ) -> None:
        """
        Log a gate decision.

        MUST be non-blocking.

        Args:
            signal: The evaluated signal
            decision: The gate decision
            context: Optional additional context
        """

    @abstractmethod
    def log_state_change(
        self, old_state: GateState, new_state: GateState, reason: str
    ) -> None:
        """
        Log gate state transition.

        Args:
            old_state: Previous state
            new_state: New state
            reason: Reason for transition
        """

    @abstractmethod
    def log_error(
        self, error: Exception, context: Optional[Dict[str, Any]] = None
    ) -> None:
        """
        Log error with context.

        Args:
            error: The exception
            context: Optional additional context
        """

    @abstractmethod
    def shutdown(self, timeout_seconds: float = 5.0) -> None:
        """
        Graceful shutdown - flush pending entries.

        Args:
            timeout_seconds: Max time to wait for flush
        """


class NullAuditLogger(AuditLogger):
    """
    No-op audit logger for testing.

    All methods return immediately without action.
    Useful for performance testing or when audit is disabled.
    """

    def log_decision(
        self,
        signal: TradeSignal,
        decision: GateDecision,
        context: Optional[Dict[str, Any]] = None,
    ) -> None:
        """No-op decision logging."""
        pass

    def log_state_change(
        self, old_state: GateState, new_state: GateState, reason: str
    ) -> None:
        """No-op state change logging."""
        pass

    def log_error(
        self, error: Exception, context: Optional[Dict[str, Any]] = None
    ) -> None:
        """No-op error logging."""
        pass

    def shutdown(self, timeout_seconds: float = 5.0) -> None:
        """No-op shutdown."""
        pass


class AsyncFileAuditLogger(AuditLogger):
    """
    Async file-based audit logger.

    Windows-Safe Implementation:
    - Background thread handles all file I/O
    - Queue-based buffering
    - Minimal file lock duration
    - JSON-Lines format

    Thread Safety:
    - All public methods are thread-safe
    - Queue operations are atomic

    Performance:
    - log_*() methods: < 0.1ms (queue put)
    - File writes: batched every flush_interval_ms
    """

    def __init__(
        self,
        log_path: Path,
        buffer_size: int = 1000,
        flush_interval_ms: int = 100,
        include_hash_chain: bool = False,
    ) -> None:
        """
        Initialize async audit logger.

        Args:
            log_path: Path to log file
            buffer_size: Max entries to buffer
            flush_interval_ms: How often to flush (milliseconds)
            include_hash_chain: Enable tamper-evident chain (future)
        """
        self._log_path = Path(log_path)
        self._buffer: queue.Queue = queue.Queue(maxsize=buffer_size)
        self._flush_interval = flush_interval_ms / 1000.0
        self._include_hash_chain = include_hash_chain
        self._shutdown_event = threading.Event()
        self._last_hash: Optional[str] = None

        # Start writer thread
        self._writer_thread = threading.Thread(
            target=self._writer_loop, daemon=True, name="AuditLogWriter"
        )
        self._writer_thread.start()

    @property
    def log_path(self) -> Path:
        """Get log file path."""
        return self._log_path

    def log_decision(
        self,
        signal: TradeSignal,
        decision: GateDecision,
        context: Optional[Dict[str, Any]] = None,
    ) -> None:
        """
        Log gate decision (non-blocking).

        Entry is queued for async write. Returns immediately.

        Args:
            signal: The evaluated signal
            decision: The gate decision
            context: Optional additional context
        """
        data = {
            "signal": {
                "signal_id": signal.signal_id,
                "instrument": signal.instrument,
                "direction": signal.direction,
                "units": signal.units,
            },
            "decision": {
                "result": decision.decision,
                "pending_id": decision.pending_id,
                "affected_currencies": decision.affected_currencies,
                "reason": decision.reason,
            },
            "metrics": {"evaluation_time_ms": decision.evaluation_time_ms},
        }

        # Add exposure snapshot if available
        if decision.current_exposure and decision.current_exposure.baskets:
            data["exposure_before"] = {
                currency: {
                    "net_direction": exp.net_direction,
                    "net_count": exp.net_count,
                    "net_notional": str(exp.net_notional),
                }
                for currency, exp in decision.current_exposure.baskets.items()
            }

        if context:
            data["context"] = context

        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.GATE_DECISION,
            data=data,
            operation_id=context.get("operation_id") if context else None,
        )

        self._queue_entry(entry)

    def log_state_change(
        self, old_state: GateState, new_state: GateState, reason: str
    ) -> None:
        """
        Log gate state transition.

        Args:
            old_state: Previous state
            new_state: New state
            reason: Reason for transition
        """
        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.STATE_CHANGE,
            data={
                "old_state": old_state.value
                if isinstance(old_state, GateState)
                else str(old_state),
                "new_state": new_state.value
                if isinstance(new_state, GateState)
                else str(new_state),
                "reason": reason,
            },
        )

        self._queue_entry(entry)

    def log_error(
        self, error: Exception, context: Optional[Dict[str, Any]] = None
    ) -> None:
        """
        Log error with context.

        Args:
            error: The exception
            context: Optional additional context
        """
        data = {"error_type": type(error).__name__, "message": str(error)}

        if context:
            data["context"] = context

        entry = AuditEntry(
            timestamp=datetime.now(timezone.utc),
            event_type=AuditEventType.ERROR,
            data=data,
        )

        self._queue_entry(entry)

    def shutdown(self, timeout_seconds: float = 5.0) -> None:
        """
        Graceful shutdown - flush remaining entries.

        Args:
            timeout_seconds: Max time to wait for flush
        """
        self._shutdown_event.set()
        self._writer_thread.join(timeout=timeout_seconds)

        # Final flush of any remaining entries
        remaining = []
        while not self._buffer.empty():
            try:
                remaining.append(self._buffer.get_nowait())
            except queue.Empty:
                break

        if remaining:
            self._write_entries(remaining)

    def _queue_entry(self, entry: AuditEntry) -> None:
        """
        Queue entry for async write.

        Non-blocking - drops entry if buffer full.

        Args:
            entry: Entry to queue
        """
        try:
            self._buffer.put_nowait(entry)
        except queue.Full:
            # Buffer full - entry is dropped
            # This is acceptable trade-off for non-blocking
            logger.warning("Audit buffer full, entry dropped")

    def _writer_loop(self) -> None:
        """
        Background writer thread.

        Flushes buffer to disk periodically with minimal lock time.
        """
        while not self._shutdown_event.is_set():
            entries_to_write: List[AuditEntry] = []

            # Drain buffer
            try:
                while True:
                    entry = self._buffer.get_nowait()
                    entries_to_write.append(entry)
            except queue.Empty:
                pass

            # Write to file if we have entries
            if entries_to_write:
                self._write_entries(entries_to_write)

            # Sleep before next flush
            self._shutdown_event.wait(timeout=self._flush_interval)

    def _write_entries(self, entries: List[AuditEntry]) -> None:
        """
        Write entries to file with minimal lock time.

        WINDOWS SAFE:
        - Opens file in append mode
        - Writes all entries
        - Flushes immediately
        - Closes file (releases lock)

        Args:
            entries: Entries to write
        """
        try:
            # Ensure parent directory exists
            self._log_path.parent.mkdir(parents=True, exist_ok=True)

            # Open, write, close in one operation
            with open(self._log_path, "a", encoding="utf-8") as f:
                for entry in entries:
                    # JSON-Lines format: one JSON object per line
                    f.write(entry.to_json() + "\n")
                # Explicit flush to ensure disk write
                f.flush()
            # File handle closed here - lock released

        except IOError as e:
            # Log to stderr if file write fails
            logger.error(f"AUDIT LOG WRITE FAILED: {e}")


class RotatingAuditLogger(AsyncFileAuditLogger):
    """
    Audit logger with Windows-safe rotation.

    WINDOWS CONSTRAINT: Cannot rename open files.
    SOLUTION: Close file before rotation, use timestamp-based names.

    File Naming:
        gate_audit_YYYYMMDD_HHMMSS.jsonl
    """

    def __init__(
        self, log_dir: Path, max_size_mb: int = 10, max_files: int = 30, **kwargs
    ) -> None:
        """
        Initialize rotating logger.

        Args:
            log_dir: Directory for log files
            max_size_mb: Max size per file in megabytes
            max_files: Max files to keep
        """
        self._log_dir = Path(log_dir)
        self._max_size = max_size_mb * 1024 * 1024
        self._max_files = max_files

        # Ensure directory exists
        self._log_dir.mkdir(parents=True, exist_ok=True)

        # Generate initial filename
        current_file = self._generate_filename()

        # Initialize parent
        super().__init__(log_path=current_file, **kwargs)

        # Track current file for rotation
        self._current_file = current_file
        self._rotation_lock = threading.Lock()

    def _generate_filename(self) -> Path:
        """
        Generate timestamp-based log filename.

        Returns:
            Path to new log file
        """
        timestamp = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
        return self._log_dir / f"gate_audit_{timestamp}.jsonl"

    def _write_entries(self, entries: List[AuditEntry]) -> None:
        """
        Write entries, checking for rotation.

        Args:
            entries: Entries to write
        """
        with self._rotation_lock:
            # Check if rotation needed
            self._check_rotation()

            # Write entries
            super()._write_entries(entries)

    def _check_rotation(self) -> None:
        """Check if rotation needed and perform if so."""
        if self._current_file.exists():
            try:
                if self._current_file.stat().st_size > self._max_size:
                    self._rotate()
            except OSError:
                pass  # File might be in use, skip rotation

    def _rotate(self) -> None:
        """
        Rotate to new file.

        WINDOWS SAFE: New file with new name, old file untouched.
        """
        # Generate new filename
        new_file = self._generate_filename()

        # Update logger to use new file
        self._log_path = new_file
        self._current_file = new_file

        # Cleanup old files if exceeding max
        self._cleanup_old_files()

    def _cleanup_old_files(self) -> None:
        """Remove old log files if exceeding max_files."""
        try:
            log_files = sorted(
                self._log_dir.glob("gate_audit_*.jsonl"),
                key=lambda p: p.stat().st_mtime,
            )

            # Remove oldest files until within limit
            while len(log_files) > self._max_files:
                oldest = log_files.pop(0)
                try:
                    oldest.unlink()
                except OSError:
                    pass  # File might be in use

        except OSError:
            pass  # Directory access error, skip cleanup
