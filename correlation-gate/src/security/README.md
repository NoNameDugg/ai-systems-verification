# Security Components

This directory contains security-critical components for the ASTRA Correlation Gate.

## Module Structure

```
security/
├── __init__.py    # Package exports
├── atomic.py      # Atomic operations & race protection
└── audit.py       # Audit logging system
```

## Components

### `atomic.py` - Atomic Operations

Provides race-condition protection for gate operations.

**Classes:**

#### `AtomicGateGuard`
RLock-based guard for atomic operations.

```python
from src.security import AtomicGateGuard

guard = AtomicGateGuard(lock_timeout_seconds=1.0, operation_timeout_seconds=5.0)

# The callable runs under the guard's re-entrant lock: no other thread can
# enter until it returns. A lock wait longer than lock_timeout_seconds
# raises AtomicOperationTimeout (fail-closed).
result = guard.execute_atomic(lambda: "evaluated", operation_id="evaluate")
```

**Features:**
- Reentrant locking (RLock)
- Configurable timeout
- Deadlock prevention
- Operation tracking

#### `StateVersionTracker`
Monotonically increasing version number for state changes.

```python
from src.security import StateVersionTracker

tracker = StateVersionTracker()
version = tracker.increment()  # Returns new version
current = tracker.get()        # Current version
```

#### `AtomicPendingSignal`
Enhanced pending signal with version tracking.

```python
from datetime import datetime, timedelta, timezone

from src.security import AtomicPendingSignal

now = datetime.now(timezone.utc)
pending = AtomicPendingSignal(
    pending_id="abc123",
    signal=trade_signal,          # a TradeSignal
    decision="ALLOW",             # or "SOFT_WARNING"
    state_version=42,
    registered_at=now,
    expires_at=now + timedelta(seconds=30),
    operation_id="op-001",
)
```

### `audit.py` - Audit Logging

Windows-safe audit logging system for compliance and debugging.

**Classes:**

#### `AuditLogger` (Abstract)
Base interface for all audit loggers.

#### `AsyncFileAuditLogger`
Non-blocking file logger with background writer.

```python
from pathlib import Path

from src.security import AsyncFileAuditLogger

logger = AsyncFileAuditLogger(log_path=Path("audit.jsonl"), buffer_size=1000, flush_interval_ms=100)
logger.log_decision(signal, decision)
logger.shutdown()  # Flush and close
```

**Features:**
- Non-blocking writes (queue-based)
- JSON-Lines format
- Windows-safe (no file locking during idle)
- Graceful shutdown

#### `RotatingAuditLogger`
Auto-rotating logger with size limits.

```python
from pathlib import Path

from src.security import RotatingAuditLogger

logger = RotatingAuditLogger(log_dir=Path("audit_logs"), max_size_mb=100, max_files=30)
```

**Features:**
- Size-based rotation
- Automatic old file cleanup
- Timestamp-based filenames
- Thread-safe rotation

#### `NullAuditLogger`
No-op logger for testing/high-performance scenarios.

```python
from src.security import NullAuditLogger

logger = NullAuditLogger()  # Does nothing
```

## Security Guarantees

### Race Protection
- All gate evaluations are serialized
- No interleaving of state reads/writes
- Pending signals tracked atomically

### Audit Trail
- Every gate decision is logged
- Timestamps in ISO 8601 format
- Structured JSON for analysis
- No sensitive data in logs

### Fail-Safe Design
- Gate fails closed on errors
- Timeouts prevent deadlocks
- Graceful degradation

## Windows Compatibility

Special considerations for Windows:
- No file locking during idle (prevents "file in use" errors)
- Deferred file handle release
- Safe log rotation

