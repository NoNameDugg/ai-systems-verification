# Provider Unit Tests

Unit tests for position provider implementations.

## Test Files

| File | Tests | Coverage |
|------|-------|----------|
| `test_oanda_provider.py` | 29 | OANDA API integration |
| `test_simulator_provider.py` | 22 | In-memory provider |
| `test_fallback_provider.py` | 15 | Failover logic |
| `test_cache.py` | 12 | Position cache |
| `test_factory.py` | 10 | Provider factory |
| `test_health.py` | 14 | Health monitoring |
| `test_coverage_supplement.py` | 20+ | Edge cases |

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

