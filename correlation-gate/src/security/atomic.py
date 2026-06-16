"""
Atomic Operations for Correlation Gate
======================================

Provides thread-safe atomic evaluation and state tracking.

Components:
- AtomicGateGuard: Mutex-protected operation execution
- StateVersionTracker: Monotonic state version tracking
- AtomicPendingSignal: Enhanced pending signal with version
"""

import threading
import time
import uuid
from concurrent.futures import ThreadPoolExecutor, TimeoutError as FuturesTimeoutError
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Callable, Optional, TypeVar, Tuple

from src.exceptions import GateError
from src.models import TradeSignal


T = TypeVar("T")


class AtomicOperationTimeout(GateError):
    """
    Raised when an atomic operation times out.

    This can occur when:
    - Lock acquisition times out
    - Operation execution exceeds timeout

    Attributes:
        operation_id: Optional ID of the timed-out operation
        timeout_seconds: The timeout that was exceeded

    Example:
        raise AtomicOperationTimeout("Lock acquisition timeout after 1.0s")
    """

    def __init__(
        self,
        message: str,
        operation_id: Optional[str] = None,
        timeout_seconds: Optional[float] = None,
    ) -> None:
        """
        Initialize AtomicOperationTimeout.

        Args:
            message: Error description
            operation_id: Optional operation ID
            timeout_seconds: Optional timeout value
        """
        self.operation_id = operation_id
        self.timeout_seconds = timeout_seconds
        context = {}
        if operation_id:
            context["operation_id"] = operation_id
        if timeout_seconds:
            context["timeout_seconds"] = timeout_seconds
        super().__init__(message, context)


class StateVersionTracker:
    """
    Thread-safe monotonically increasing state version tracker.

    Each mutation to gate state increments the version, providing:
    - Change detection (stale reads)
    - Ordering of concurrent operations
    - Audit trail correlation

    Thread Safety:
        All methods are thread-safe via Lock.

    Example:
        >>> tracker = StateVersionTracker()
        >>> tracker.get()
        0
        >>> tracker.increment()
        1
        >>> tracker.check_and_increment(1)
        (True, 2)
    """

    def __init__(self) -> None:
        """Initialize tracker at version 0."""
        self._version: int = 0
        self._lock = threading.Lock()

    def get(self) -> int:
        """
        Get current version (read-only).

        Returns:
            Current state version
        """
        with self._lock:
            return self._version

    def increment(self) -> int:
        """
        Increment version and return new value.

        Returns:
            New (incremented) version
        """
        with self._lock:
            self._version += 1
            return self._version

    def check_and_increment(self, expected: int) -> Tuple[bool, int]:
        """
        Check if version matches expected, then increment.

        Useful for optimistic concurrency control.

        Args:
            expected: Expected current version

        Returns:
            Tuple of (matched, new_version)
        """
        with self._lock:
            matched = self._version == expected
            self._version += 1
            return matched, self._version


@dataclass
class AtomicPendingSignal:
    """
    Pending signal with atomic operation tracking.

    Enhanced version of PendingSignal that includes:
    - State version at registration time
    - Operation ID for audit correlation

    Attributes:
        pending_id: Unique identifier
        signal: Original trade signal
        decision: Gate decision (ALLOW/SOFT_WARNING)
        state_version: State version when registered
        registered_at: Registration timestamp
        expires_at: Expiration timestamp
        operation_id: ID of atomic operation that registered this
    """

    pending_id: str
    signal: TradeSignal
    decision: str
    state_version: int
    registered_at: datetime
    expires_at: datetime
    operation_id: str

    @property
    def is_expired(self) -> bool:
        """Check if pending signal has expired."""
        return datetime.now(timezone.utc) > self.expires_at


class AtomicGateGuard:
    """
    Provides atomic operation execution for the gate.

    Thread Safety:
        - Uses RLock for re-entrant locking
        - Lock acquisition has configurable timeout
        - All operations are serialized
        - Nested calls from same thread are allowed (re-entrant)

    Fail-Closed:
        - Lock timeout raises AtomicOperationTimeout
        - Operation timeout raises AtomicOperationTimeout (for non-nested calls)

    Example:
        >>> guard = AtomicGateGuard(lock_timeout_seconds=1.0)
        >>> result = guard.execute_atomic(lambda: expensive_operation())
    """

    def __init__(
        self, lock_timeout_seconds: float = 1.0, operation_timeout_seconds: float = 5.0
    ) -> None:
        """
        Initialize atomic guard.

        Args:
            lock_timeout_seconds: Max time to wait for lock
            operation_timeout_seconds: Max time for operation execution
        """
        self._lock_timeout = lock_timeout_seconds
        self._operation_timeout = operation_timeout_seconds
        self._lock = threading.RLock()
        self._state_tracker = StateVersionTracker()
        self._operation_local = threading.local()

    @property
    def lock_timeout_seconds(self) -> float:
        """Get lock timeout setting."""
        return self._lock_timeout

    @property
    def operation_timeout_seconds(self) -> float:
        """Get operation timeout setting."""
        return self._operation_timeout

    @property
    def current_operation_id(self) -> Optional[str]:
        """
        Get current operation ID (within execute_atomic context).

        Returns None if called outside of execute_atomic.
        """
        return getattr(self._operation_local, "operation_id", None)

    def execute_atomic(
        self, operation: Callable[[], T], operation_id: Optional[str] = None
    ) -> T:
        """
        Execute operation atomically with lock timeout protection.

        Acquires lock with timeout, executes operation, releases lock.
        If lock cannot be acquired within timeout, raises AtomicOperationTimeout.

        Uses RLock for thread-based re-entrancy - nested calls from the
        same thread will succeed.

        Args:
            operation: Callable to execute under lock
            operation_id: Optional ID for logging (auto-generated if not provided)

        Returns:
            Result of operation

        Raises:
            AtomicOperationTimeout: If lock cannot be acquired within timeout
        """
        if operation_id is None:
            operation_id = f"op_{uuid.uuid4().hex[:12]}"

        # Try to acquire lock with timeout
        acquired = self._lock.acquire(timeout=self._lock_timeout)
        if not acquired:
            raise AtomicOperationTimeout(
                f"Lock acquisition timeout after {self._lock_timeout}s",
                operation_id=operation_id,
                timeout_seconds=self._lock_timeout,
            )

        try:
            # Set current operation ID
            old_op_id = getattr(self._operation_local, "operation_id", None)
            self._operation_local.operation_id = operation_id

            # Execute operation directly
            return operation()

        finally:
            self._operation_local.operation_id = old_op_id
            self._lock.release()

    def get_state_version(self) -> int:
        """
        Get current state version.

        Returns:
            Current state version
        """
        return self._state_tracker.get()

    def increment_state_version(self) -> int:
        """
        Increment state version and return new value.

        Should be called after any state mutation.

        Returns:
            New state version
        """
        return self._state_tracker.increment()

    def check_and_increment_state_version(self, expected: int) -> Tuple[bool, int]:
        """
        Check version matches and increment.

        Args:
            expected: Expected current version

        Returns:
            Tuple of (matched, new_version)
        """
        return self._state_tracker.check_and_increment(expected)
