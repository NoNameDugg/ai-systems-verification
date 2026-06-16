# Benchmark Tests

Dedicated benchmark tests for performance measurement.

## Files

| File | Benchmarks |
|------|------------|
| `test_provider_benchmarks.py` | Provider performance |

## Purpose

These benchmarks use `pytest-benchmark` for accurate timing:
- Statistical analysis
- Warm-up iterations
- Multiple rounds
- Outlier detection

## Running

```bash
# Basic run
pytest tests/benchmarks/ --benchmark-enable

# Detailed output
pytest tests/benchmarks/ --benchmark-verbose

# JSON output
pytest tests/benchmarks/ --benchmark-json=results.json
```

## Benchmark Categories

- Provider fetch latency
- Cache operations
- Position parsing
- Serialization

