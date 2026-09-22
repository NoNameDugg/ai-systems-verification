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
from tests.fixtures.directional_fixtures import KNOWN_CURRENCIES, SUPPORTED_PAIRS

def test_pair_is_supported():
    assert "EUR_USD" in SUPPORTED_PAIRS
    assert "EUR" in KNOWN_CURRENCIES
```

### Mock Providers

```python
from tests.fixtures.mock_providers import (
    FailingPositionProvider,
    MockPositionProvider,
    SlowPositionProvider,
)

def test_with_mock():
    provider = MockPositionProvider(positions=[])
    assert provider.fetch_positions() == []
    assert provider.is_available()

def test_failure_paths():
    failing = FailingPositionProvider()                        # always raises
    slow = SlowPositionProvider(delay_seconds=0.01, positions=[])  # artificial delay
```

## Available Fixtures

### `directional_fixtures.py`
- `KNOWN_CURRENCIES`: the currency codes used by the directional-mapping tests
- `SUPPORTED_PAIRS`: the instrument pairs used by the directional-mapping tests
- `PERFORMANCE_ITERATIONS`, `PERFORMANCE_TARGET_MS`, `BATCH_PERFORMANCE_SIZE`,
  `BATCH_PERFORMANCE_TARGET_MS`: budgets for the parse-performance tests

### `mock_providers.py`
- `MockPositionProvider(positions)`: returns pre-configured positions
- `FailingPositionProvider(exception=None)`: always raises
- `SlowPositionProvider(delay_seconds, positions)`: introduces an artificial delay
- `TimeoutPositionProvider(timeout_seconds)`: sleeps longer than the timeout
- `ExceptionPositionProvider(exception)`: raises a specific exception type
- `StatefulPositionProvider()`: state can be changed during a test
- `MockPriceProvider(prices=None)`, `MockPriceSnapshot(instrument, mid_price, age_ms=0)`: price fixtures
