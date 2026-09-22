# Provider Unit Tests

Unit tests for position provider implementations.

## Test Files

| File | Coverage |
| ------ | ---------- |
| `test_oanda_provider.py` | OANDA API integration |
| `test_simulator_provider.py` | In-memory provider |
| `test_fallback_provider.py` | Failover logic |
| `test_cache.py` | Position cache |
| `test_factory.py` | Provider factory |
| `test_health.py` | Health monitoring |
| `test_coverage_supplement.py` | Edge cases |

## Key Test Scenarios

### OANDA Provider
- Authentication handling
- Position parsing
- Error recovery
- Rate limit handling
- Cache integration

### Simulator Provider
- Add/remove positions
- Thread-safe operations
- Bulk operations
- State reset

### Fallback Provider
- Primary failure detection
- Automatic failover
- Recovery detection
- Health threshold behavior

## Running

```bash
pytest tests/unit/providers/ -v
```

