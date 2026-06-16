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
guard = AtomicGateGuard(timeout_seconds=1.0)

with guard.atomic_operation("evaluate"):
    # This code block is atomic
    # No other thread can enter until we exit
    pass
```

**Features:**
- Reentrant locking (RLock)
- Configurable timeout
- Deadlock prevention
- Operation tracking

#### `StateVersionTracker`
Monotonically increasing version number for state changes.

```python
tracker = StateVersionTracker()
version = tracker.increment()  # Returns new version
current = tracker.get_version()  # Current version
```

#### `AtomicPendingSignal`
Enhanced pending signal with version tracking.

```python
pending = AtomicPendingSignal(
    pending_id="abc123",
    signal=trade_signal,
    decision=gate_decision,
    state_version=42
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
logger = AsyncFileAuditLogger(
    log_file=Path("/var/log/audit.jsonl")
)
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
logger = RotatingAuditLogger(
    log_dir=Path("/var/log/audit"),
    max_size_mb=100,
    max_files=30
)
```

**Features:**
- Size-based rotation
- Automatic old file cleanup
- Timestamp-based filenames
- Thread-safe rotation

#### `NullAuditLogger`
No-op logger for testing/high-performance scenarios.

```python
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

