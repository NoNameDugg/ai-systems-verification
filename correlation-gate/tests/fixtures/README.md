# Test Fixtures

Shared test fixtures and mock objects.

## Files

| File | Purpose |
|------|---------|
| `directional_fixtures.py` | Test data for directional mapping |
| `mock_providers.py` | Mock position providers |

## Usage

### Directional Fixtures

```python
from tests.fixtures.directional_fixtures import (
    MAJOR_PAIRS,
    CROSS_PAIRS,
    SAMPLE_POSITIONS,
    create_test_signal
)

def test_with_fixture():
    signal = create_test_signal("EUR_USD", "LONG")
    # Use signal in test
```

### Mock Providers

```python
from tests.fixtures.mock_providers import (
    MockPositionProvider,
    FailingProvider,
    SlowProvider
)

def test_with_mock():
    provider = MockPositionProvider(positions=[...])
    gate = CorrelationGate(config, provider)
```

## Available Fixtures

### Position Data
- `SAMPLE_POSITIONS`: Standard test positions
- `EMPTY_POSITIONS`: Empty position list
- `MAX_EXPOSURE_POSITIONS`: At-limit positions

### Providers
- `MockPositionProvider`: Configurable mock
- `FailingProvider`: Always fails
- `SlowProvider`: Configurable delay
- `RandomFailureProvider`: Random failures

### Signals
- `create_test_signal()`: Factory function
- `SAMPLE_SIGNALS`: Pre-built signals

