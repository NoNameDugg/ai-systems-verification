# Test Suites

Comprehensive test coverage with 1,221 tests (CI) across the wired suites.

## Test Categories

Per-binary counts are not maintained by hand here: read the `test result:` line of each test
binary in the CI `rust (flash)` job log (`cargo test --tests --lib`), which is the source of the
1,221 figure. The suites live in `chaos/`, `core/`, `e2e/`, `gateway/`, `network/`, `production/`,
`publisher/` (each wired through a same-named `*_tests.rs` entry) plus `fixtures/` (JSON test data).

## Running Tests

```bash
cargo test              # All tests
cargo test --lib        # Library tests (311)
cargo test --test publisher_tests  # Publisher tests (351)
cargo test cutover      # Cutover tests (26)
```
