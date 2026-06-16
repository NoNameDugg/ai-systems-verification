# Performance Tests

Performance benchmarks and budget validation.

## Test Files

| File | Benchmarks | Focus |
|------|------------|-------|
| `test_benchmarks.py` | 16+ | Core gate performance |
| `test_provider_benchmarks.py` | 16+ | Provider performance |

## Performance Budgets

| Operation | Budget | Typical |
|-----------|--------|---------|
| Gate Evaluation | < 5ms | ~1ms |
| Directional Parsing | < 0.1ms | ~0.05ms |
| Basket Calculation | < 1ms | ~0.5ms |
| Cache Hit | < 1ms | ~0.1ms |
| Position Fetch (API) | < 500ms | ~50ms |

## Running Benchmarks

```bash
# Run with benchmark plugin
pytest tests/performance/ -v --benchmark-enable

# Save results
pytest tests/performance/ --benchmark-save=baseline

# Compare to baseline
pytest tests/performance/ --benchmark-compare=baseline
```

## Benchmark Configuration

```python
@pytest.mark.benchmark(
    group="gate",
    min_rounds=100,
    warmup=True
)
def test_evaluation_performance(benchmark):
    result = benchmark(gate.evaluate, signal)
    assert result.decision in VALID_DECISIONS
```

## Stress Tests

High-volume tests to validate performance under load:
- 1000 sequential evaluations
- 100 concurrent threads
- Memory usage tracking

