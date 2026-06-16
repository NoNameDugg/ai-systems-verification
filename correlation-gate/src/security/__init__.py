"""
Security Module for Correlation Gate
====================================

Provides atomic operations and audit logging.

Components:
- AtomicGateGuard: Thread-safe atomic evaluation
- StateVersionTracker: Monotonic state tracking
- AuditLogger: Non-blocking audit trail
"""

from src.security.atomic import (
    AtomicGateGuard,
    StateVersionTracker,
    AtomicOperationTimeout,
    AtomicPendingSignal,
)
from src.security.audit import (
    AuditLogger,
    AsyncFileAuditLogger,
    RotatingAuditLogger,
    NullAuditLogger,
    AuditEntry,
    AuditEventType,
)

__all__ = [
    # Atomic operations
    "AtomicGateGuard",
    "StateVersionTracker",
    "AtomicOperationTimeout",
    "AtomicPendingSignal",
    # Audit logging
    "AuditLogger",
    "AsyncFileAuditLogger",
    "RotatingAuditLogger",
    "NullAuditLogger",
    "AuditEntry",
    "AuditEventType",
]
