# Test Suites

Comprehensive test coverage with 1,033+ tests across all modules.

## Test Categories

| Directory | Tests | Purpose |
|-----------|-------|---------|
| `bindings/` | 162 | Python binding tests |
| `book/` | 119 | Order book engine tests |
| `chaos/` | 61 | Chaos/fault injection tests |
| `core/` | 133 | Configuration, error, metrics tests |
| `e2e/` | 72 | End-to-end pipeline tests |
| `gateway/` | 40 | Gateway output tests |
| `network/` | 148 | WebSocket and adapter tests |
| `production/` | 41 | Production readiness tests |
| `publisher/` | 351 | Redis publishing tests (includes shadow/cutover) |
| `fixtures/` | - | JSON test fixtures |

## Running Tests

```bash
cargo test              # All tests
cargo test --lib        # Library tests (311)
cargo test --test publisher_tests  # Publisher tests (351)
cargo test cutover      # Cutover tests (26)
```
