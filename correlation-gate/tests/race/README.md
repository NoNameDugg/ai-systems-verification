# Race Condition & Chaos Tests

Tests for thread safety, concurrency, and chaos engineering.

## Files

| File | Focus |
| ------ | ------- |
| `harness.py` | Test infrastructure |
| `test_concurrent.py` | Concurrent serialization |
| `test_high_frequency.py` | High-frequency stress |
| `test_chaos.py` | Chaos engineering |

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
pytest tests/race/ -v --count=10   # requires pytest-repeat (not in requirements.txt)
```

## Safety Guarantees

These tests verify:
1. No deadlocks under any scenario
2. Proper timeout handling
3. State consistency under stress
4. Serialisation of same-basket approvals under concurrency — **only**
   `test_review_v12_race.py::test_concurrent_same_basket_never_exceeds_limit` and
   `::test_lock_timeout_waiter_is_hard_blocked_with_reason` can detect a missing lock; an outside review
   (2026-10) showed the other 28 tests here pass with the gate's lock replaced by a no-op, because they
   assert only completion counts, valid decision strings, or weak inequalities. Run the suite with the
   lock removed before trusting a new race test.

