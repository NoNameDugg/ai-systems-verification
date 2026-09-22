# Test Suite

Comprehensive test suite for the ASTRA Correlation Gate.

## Directory Structure

```
tests/
├── __init__.py
├── unit/                 # Unit tests (isolated component testing)
│   ├── test_logic_core.py
│   ├── test_coverage_supplement.py
│   ├── providers/        # Provider unit tests
│   └── security/         # Security unit tests
├── integration/          # Integration tests (component interaction)
│   ├── test_integration.py
│   ├── test_provider_integration.py
│   └── test_security_integration.py
├── performance/          # Performance & benchmark tests
│   ├── test_benchmarks.py
│   └── test_provider_benchmarks.py
├── race/                 # Race condition & chaos tests
│   ├── harness.py
│   ├── test_concurrent.py
│   ├── test_high_frequency.py
│   └── test_chaos.py
├── benchmarks/           # Additional benchmark tests
│   └── test_provider_benchmarks.py
└── fixtures/             # Shared test fixtures
    ├── directional_fixtures.py
    └── mock_providers.py
```

## Test Categories

### Unit Tests (`unit/`)
Isolated tests for individual components.
- 99% line coverage measured with `pytest --cov=src` (abstract-method bodies excluded); generate the HTML report with `--cov-report=html`
- Fast execution (< 30 seconds total)

### Integration Tests (`integration/`)
Tests for component interaction.
- Gate + Provider integration
- Security stack integration
- End-to-end scenarios

### Performance Tests (`performance/`)
Validates performance budgets.
- Gate evaluation < 5ms
- Cache hit < 1ms
- 100+ concurrent operations

### Race Condition Tests (`race/`)
Validates thread safety.
- Concurrent evaluation serialization
- High-frequency stress tests
- Chaos engineering scenarios

## Running Tests

### All Tests
```bash
pytest tests/ -v
```

### With Coverage
```bash
pytest tests/ --cov=src --cov-report=html
```

### Specific Categories
```bash
# Unit tests only
pytest tests/unit/ -v

# Integration tests
pytest tests/integration/ -v

# Performance benchmarks
pytest tests/performance/ -v --benchmark-enable   # requires pytest-benchmark (not in requirements.txt)

# Race condition tests
pytest tests/race/ -v
```

### Parallel Execution
```bash
pytest tests/ -n auto  # requires pytest-xdist (not in requirements.txt)
```

## Test Fixtures

Common fixtures are located in `fixtures/`:
- `directional_fixtures.py`: Test data for directional mapping
- `mock_providers.py`: Mock position providers

### Using Fixtures

```python
import pytest
from tests.fixtures.mock_providers import MockPositionProvider

@pytest.fixture
def mock_provider():
    return MockPositionProvider([...])
```

## Writing Tests

### Naming Convention
```python
def test_<method>_<scenario>_<expected_outcome>():
    """Test that <method> <does something> when <scenario>."""
    pass
```

### Test Structure (AAA Pattern)
```python
def test_gate_blocks_excessive_exposure():
    """Test that gate blocks when exposure exceeds threshold."""
    # ARRANGE
    gate = create_test_gate()

    # ACT
    decision = gate.evaluate(signal)

    # ASSERT
    assert decision.decision == "HARD_BLOCK"
```

## Coverage Requirements

- **Target:** 100% coverage
- **Current:** 99% measured with `pytest --cov=src` (abstract-method bodies excluded); generate the HTML report with `--cov-report=html`

## Quality Gates

All tests must pass before merge:
1. Unit tests pass
2. Integration tests pass
3. Performance budgets met
4. Race condition tests pass
5. Coverage ≥ 95% (enforced in CI via `--cov-fail-under=95`)

