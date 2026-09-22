# Security Unit Tests

Unit tests for security-critical components.

## Test Files

| File | Coverage |
| ------ | ---------- |
| `test_atomic.py` | Atomic operations |
| `test_audit.py` | Audit logging |

## Key Test Scenarios

### Atomic Operations (`test_atomic.py`)
- Lock acquisition/release
- Timeout behavior
- Reentrant locking
- State version tracking
- Pending signal management
- Concurrent access patterns

### Audit Logging (`test_audit.py`)
- Log entry formatting
- JSON-Lines output
- Non-blocking writes
- Log rotation
- Cleanup operations
- Shutdown behavior
- Windows compatibility
- Error handling

## Running

```bash
pytest tests/unit/security/ -v
```

## Thread Safety Tests

These tests verify thread-safe behavior:
```python
def test_concurrent_lock_acquisition():
    """Multiple threads competing for lock."""
    # Spawns 10 threads, verifies serialization
```

