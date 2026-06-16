# Race Condition & Chaos Tests

Tests for thread safety, concurrency, and chaos engineering.

## Files

| File | Tests | Focus |
|------|-------|-------|
| `harness.py` | - | Test infrastructure |
| `test_concurrent.py` | 9 | Concurrent serialization |
| `test_high_frequency.py` | 9 | High-frequency stress |
| `test_chaos.py` | 10 | Chaos engineering |

## Test Harness

`harness.py` provides infrastructure for race condition testing:

```python
from tests.race.harness import ConcurrentTestHarness

harness = ConcurrentTestHarness(num_threads=10)
results = harness.run_concurrent(gate.evaluate, signals)
harness.verify_serialization(results)
```

## Test Scenarios

### Concurrent Serialization (`test_concurrent.py`)
Validates that concurrent evaluations are properly serialized:
- Multiple threads evaluating simultaneously
- Verification that no interleaving occurs
- State consistency checks

### High-Frequency (`test_high_frequency.py`)
Stress tests with rapid-fire signals:
- 1000+ evaluations per second
- Memory stability
- No deadlocks

### Chaos Engineering (`test_chaos.py`)
Tests behavior under failure conditions:
- Provider failures mid-evaluation
- Timeout scenarios
- Random delays
- Resource exhaustion

## Running

```bash
# All race tests
pytest tests/race/ -v

# With thread debugging
pytest tests/race/ -v --tb=long

# Increase iterations for thoroughness
pytest tests/race/ -v --count=10
```

## Safety Guarantees

These tests verify:
1. No race conditions in gate evaluation
2. No deadlocks under any scenario
3. Proper timeout handling
4. State consistency under stress

