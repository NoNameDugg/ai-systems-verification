# Unit Tests

Isolated unit tests for individual components.

## Structure

```
unit/
├── __init__.py
├── test_logic_core.py         # Core logic tests (directional, basket, gate)
├── test_coverage_supplement.py # Additional coverage tests
├── providers/                  # Provider unit tests
│   ├── test_oanda_provider.py
│   ├── test_simulator_provider.py
│   ├── test_fallback_provider.py
│   ├── test_cache.py
│   ├── test_factory.py
│   └── test_health.py
└── security/                   # Security unit tests
    ├── test_atomic.py
    └── test_audit.py
```

## Test Files

### `test_logic_core.py`
Tests for core gate logic:
- Directional mapping (35+ tests)
- Basket calculation (30+ tests)
- Gate evaluation (90+ tests)

### `test_coverage_supplement.py`
Additional tests for edge cases and error paths.

### `providers/`
Provider-specific unit tests:
- OANDA API mocking
- Simulator operations
- Fallback logic
- Cache behavior
- Health monitoring

### `security/`
Security component tests:
- Atomic operation guards
- Audit logging
- Race condition scenarios

## Running

```bash
# All unit tests
pytest tests/unit/ -v

# Specific module
pytest tests/unit/test_logic_core.py -v

# With coverage
pytest tests/unit/ --cov=src --cov-report=term-missing
```

## Fixtures

Each test file uses pytest fixtures for setup.
Common fixtures include:
- `gate_config`: Standard gate configuration
- `sample_positions`: Test position data
- `sample_signal`: Test trade signal

