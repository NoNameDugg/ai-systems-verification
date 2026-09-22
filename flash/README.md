# Flash

**Ultra-Low Latency Market Data Adapter for High-Frequency Trading**

[![Status](https://img.shields.io/badge/status-research%2Fportfolio%20prototype-yellow.svg)](#status)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Overview

Flash is a high-frequency market data adapter built in Rust. It implements an OANDA (FOREX) Level 2 market-data adapter that connects over WebSocket, reconstructs order books in memory, and distributes data through Redis Streams with Python bindings for downstream consumers. The order book engine, adapter traits, and publisher are general; this build's wired, exercised data path is the OANDA forex feed (scaffolding for other venues exists but is not validated against live exchanges).

### Status

**Status:** Research / portfolio prototype

This is a personal research and portfolio project. It is not deployed in a regulated production environment; "shadow mode" / "cutover" below refer to the author's own local validation against the prior Python implementation, not a third-party-audited production rollout.

| Milestone | Status |
|-----------|--------|
| Rust Implementation Complete | DONE |
| Local Shadow Validation (vs prior Python prototype) | DONE |
| Local Cutover (replaced Python prototype) | DONE |
| Earlier Python prototype | **DEPRECATED** |

> **Note:** An earlier Python prototype of this adapter has been deprecated and replaced by this Rust implementation.

### Known gaps

- The optional `blackbox` flight-recorder integration records ingress and internal events; the egress tap point is not wired.
- The Python stream binding surfaces delta events but does not yet surface order-book snapshots.

### Performance vs Python Shim

> Single-run local comparison against the prior Python implementation on the
> author's dev machine; indicative, not independently verified.

| Metric | Python V1 | Rust V2 | Improvement |
|--------|-----------|---------|-------------|
| P50 Latency | 0.433 ms | <0.05 ms | ~8.7x |
| P95 Latency | 0.515 ms | <0.08 ms | ~6.4x |
| P99 Latency | 0.813 ms | <0.10 ms | ~8.1x |
| Memory | ~50 MB | <10 MB | ~5x |

### Internal Performance Highlights

> These are single-run Criterion micro-benchmark figures measured on the author's
> development machine (AMD Ryzen 7 5700X, Windows). They are illustrative of the
> order of magnitude, not independently verified or guaranteed numbers; re-run
> `cargo bench` on your own hardware to reproduce. Sub-nanosecond figures in
> particular reflect cache-resident hot-loop micro-benchmarks, not end-to-end
> production latency.

| Metric | Target | Measured (single run) |
|--------|--------|-----------------------|
| Best Bid/Ask Lookup | < 10 ns | ~0.48 ns |
| Order Book Update | < 1 μs | ~80 ns |
| 50-Level Snapshot | < 50 μs | ~451 ns |
| E2E Pipeline | < 50 μs | ~3.0 μs |
| Burst Throughput | > 100K/s | ~14M/s |

---

## Quick Start

### Build from source

This crate is not published to crates.io or PyPI. Build it from this source tree:

```bash
# From the flash directory (Rust 1.75+ toolchain required)
cd flash

# Build release binary
cargo build --release

# Run tests
cargo test
```

### Python bindings (build from source)

The Python extension is built locally with maturin (it is not on PyPI):

```bash
# From the flash directory
pip install maturin
maturin develop --release --features python
```

### Basic Usage

```python
from astra_flash import Exchange, Instrument, OrderBook, PriceLevel, Side

# Create order book
instrument = Instrument("BTC", "USD", Exchange.Deribit, "BTC-PERPETUAL")
book = OrderBook(instrument)

# Apply an L2 snapshot
book.apply_snapshot(
    bids=[PriceLevel(50000.0, "1.5", 1234567890)],
    asks=[PriceLevel(50100.0, "2.0", 1234567890)],
    timestamp=1234567890,
)

# Query best prices (cached; ~sub-ns in micro-benchmarks; exposed as properties)
best_bid = book.best_bid
best_ask = book.best_ask
mid_price = book.mid_price
spread_bps = book.spread_bps
```

---

## Features

- **Low-Latency Order Book:** Cached best-price lookups (sub-nanosecond in micro-benchmarks)
- **OANDA FOREX Adapter:** Wired and exercised; an extensible adapter-trait layer with unvalidated Deribit/Binance scaffolding
- **Thread-Safe:** parking_lot RwLock for concurrent access
- **Redis Streams:** High-throughput data distribution
- **Python Bindings:** Zero-copy PyO3 integration
- **Flight-Recorder Tap (optional `blackbox` feature):** ingress and internal tap points into the `blackbox/` journal; the egress tap is not wired yet
- **Well-Tested:** 1,221 tests pass in CI (`cargo test --tests --lib`, 37 ignored); 1,231 with `--features blackbox`; plus chaos tests and an operations runbook
- **Prometheus Metrics:** Built-in observability

---

## Repository Structure

```
flash/
├── benches/                          # Criterion benchmarks (26 files)
│   ├── adapters_bench.rs             # Exchange adapter benchmarks
│   ├── batch_bench.rs                # Batch accumulator benchmarks
│   ├── bindings_bench.rs             # Python bindings benchmarks
│   ├── config_bench.rs               # Configuration benchmarks
│   ├── connector_bench.rs            # WebSocket connector benchmarks
│   ├── delta_bench.rs                # Delta processing benchmarks
│   ├── error_bench.rs                # Error handling benchmarks
│   ├── heartbeat_bench.rs            # Heartbeat manager benchmarks
│   ├── metrics_bench.rs              # Metrics system benchmarks
│   ├── orderbook_bench.rs            # Order book benchmarks
│   ├── parsing_bench.rs              # JSON parsing benchmarks
│   ├── performance_validation_bench.rs # Performance validation suite
│   ├── pool_bench.rs                 # Redis pool benchmarks
│   ├── publisher_bench.rs            # Stream publisher benchmarks
│   ├── reconnect_bench.rs            # Reconnection benchmarks
│   ├── snapshot_bench.rs             # Snapshot benchmarks
│   ├── stream_bench.rs               # Stream processing benchmarks
│   ├── thread_safe_bench.rs          # Thread-safe wrapper benchmarks
│   ├── topics_bench.rs               # Topic routing benchmarks
│   └── types_bench.rs                # Core types benchmarks (+ more)
│
├── config/                           # Example YAML configurations
│   ├── flash.yaml                    # Default profile (empty credentials)
│   ├── flash.docker.yaml             # Docker profile
│   ├── flash.production.yaml         # Production profile
│   └── flash.shadow.yaml             # Shadow-mode profile
│
├── python/                           # Python package
│   └── astra_flash/
│       ├── __init__.py               # Package init + docstring
│       ├── __init__.pyi              # Type stubs
│       └── py.typed                  # PEP 561 typing marker
│
├── src/                              # Source code
│   ├── bindings/                     # Python bindings (PyO3)
│   │   ├── mod.rs                    # Bindings module
│   │   ├── orderbook.rs              # PyOrderBook wrapper
│   │   ├── stream.rs                 # FlashClient, StreamConfig
│   │   └── types.rs                  # Type conversions
│   ├── book/                         # Order book engine
│   │   ├── mod.rs                    # Book module
│   │   ├── delta.rs                  # Delta processing
│   │   ├── orderbook.rs              # Core OrderBook
│   │   ├── snapshot.rs               # Snapshot validation
│   │   └── thread_safe.rs            # ThreadSafeOrderBook
│   ├── core/                         # Core foundation
│   │   ├── mod.rs                    # Core module
│   │   ├── config.rs                 # Configuration system
│   │   ├── error.rs                  # FlashError types
│   │   ├── metrics.rs                # Prometheus metrics
│   │   └── types.rs                  # Core types
│   ├── network/                      # WebSocket engine
│   │   ├── adapters/                 # Exchange adapters
│   │   │   ├── mod.rs                # Adapters module
│   │   │   ├── binance.rs            # Binance adapter
│   │   │   ├── common.rs             # Common utilities
│   │   │   ├── deribit.rs            # Deribit adapter
│   │   │   ├── oanda.rs              # Oanda adapter
│   │   │   └── traits.rs             # Adapter traits
│   │   ├── mod.rs                    # Network module
│   │   ├── connector.rs              # WebSocket connector
│   │   ├── heartbeat.rs              # Heartbeat manager
│   │   └── reconnect.rs              # Reconnection logic
│   ├── publisher/                    # Redis publisher
│   │   ├── mod.rs                    # Publisher module
│   │   ├── batch.rs                  # BatchAccumulator
│   │   ├── pool.rs                   # RedisPool
│   │   ├── stream.rs                 # StreamPublisher
│   │   └── topics.rs                 # TopicRouter
│   ├── lib.rs                        # Library entry point
│   └── main.rs                       # Binary entry point
│
├── tests/                            # Wired test suites (1,221 tests pass in CI, 37 ignored; see Testing)
│   ├── chaos/                        # Chaos tests (61 tests)
│   │   ├── helpers/
│   │   │   └── mod.rs                # Chaos test helpers
│   │   ├── mod.rs
│   │   ├── combined_chaos_test.rs    # 6 tests
│   │   ├── message_chaos_test.rs     # 10 tests
│   │   ├── network_chaos_test.rs     # 12 tests
│   │   ├── redis_chaos_test.rs       # 12 tests
│   │   ├── resource_chaos_test.rs    # 9 tests
│   │   └── timing_chaos_test.rs      # 8 tests
│   ├── core/                         # Core foundation tests
│   │   ├── mod.rs
│   │   ├── config_test.rs            # 38 tests
│   │   ├── error_test.rs             # 36 tests
│   │   ├── metrics_test.rs           # 38 tests
│   │   └── types_test.rs             # 21 tests
│   ├── e2e/                          # E2E pipeline tests (72 tests)
│   │   ├── common/
│   │   │   ├── mod.rs
│   │   │   ├── fixtures.rs           # Test fixtures
│   │   │   ├── mock_server.rs        # Mock WebSocket server
│   │   │   └── test_consumer.rs      # Test consumer
│   │   ├── mod.rs
│   │   ├── failure_test.rs           # Failure scenario tests
│   │   ├── multi_exchange_test.rs    # Multi-exchange tests
│   │   ├── performance_test.rs       # Performance validation
│   │   └── pipeline_test.rs          # Full pipeline tests
│   ├── fixtures/                     # Test fixtures (JSON)
│   │   ├── binance/
│   │   │   ├── depth_update.json
│   │   │   ├── error.json
│   │   │   ├── partial_depth.json
│   │   │   └── trade.json
│   │   ├── deribit/
│   │   │   ├── book_delta.json
│   │   │   ├── book_snapshot.json
│   │   │   ├── error.json
│   │   │   ├── heartbeat.json
│   │   │   └── trade.json
│   │   └── oanda/
│   │       ├── error.json
│   │       ├── heartbeat.json
│   │       └── price.json
│   ├── network/                      # Network tests
│   │   ├── mod.rs
│   │   ├── adapters_test.rs          # 40 tests
│   │   ├── connector_test.rs         # 36 tests
│   │   ├── heartbeat_test.rs         # 36 tests
│   │   └── reconnect_test.rs         # 36 tests
│   ├── production/                   # Production tests (41 tests)
│   │   ├── helpers/
│   │   │   └── mod.rs                # Production test helpers
│   │   ├── config_test.rs            # 8 tests
│   │   ├── deployment_test.rs        # 4 tests
│   │   ├── health_test.rs            # 6 tests
│   │   ├── logging_test.rs           # 6 tests
│   │   ├── monitoring_test.rs        # 6 tests
│   │   ├── recovery_test.rs          # 5 tests
│   │   └── security_test.rs          # 6 tests
│   ├── publisher/                    # Publisher tests
│   │   ├── mod.rs
│   │   ├── batch_test.rs             # 32 tests
│   │   ├── pool_test.rs              # 25 tests
│   │   ├── stream_test.rs            # 30 tests
│   │   └── topics_test.rs            # 28 tests
│   ├── chaos_tests.rs                # Chaos test runner
│   ├── e2e_tests.rs                  # E2E test runner
│   └── production_tests.rs           # Production test runner
│
├── Cargo.lock                        # Dependency lock file
├── Cargo.toml                        # Rust project configuration
├── Dockerfile                        # Multi-stage container build
├── LICENSE                           # MIT license
├── pyproject.toml                    # Python package configuration
├── README.md                         # This file
└── rust-toolchain.toml               # Rust toolchain configuration
```

---

## Testing

### Run All Tests

```bash
# Run all tests. CI runs `cargo test --tests --lib`: 1,221 tests pass (37 ignored); 1,231 with `--features blackbox`
cargo test

# Run with output
cargo test -- --nocapture
```

### Test Categories

```bash
# Library tests (311 tests)
cargo test --lib

# Publisher tests (351 tests, includes shadow/cutover)
cargo test --test publisher_tests

# Core tests (145 tests)
cargo test --test core_tests

# E2E pipeline tests (72 tests)
cargo test --test e2e_tests

# Chaos tests (61 tests)
cargo test --test chaos_tests

# Production tests (41 tests)
cargo test --test production_tests
```

### Benchmarks

```bash
# Run all performance benchmarks
cargo bench --bench performance_validation_bench

# Run specific category
cargo bench --bench performance_validation_bench -- "latency"
cargo bench --bench performance_validation_bench -- "throughput"
```

---

## Documentation

Each module ships its own `README.md` describing design and usage:

| Module | Purpose |
|--------|---------|
| [src/core/](src/core/README.md) | Core types, configuration, errors, metrics |
| [src/book/](src/book/README.md) | Order book engine (snapshots, deltas, thread-safe wrapper) |
| [src/network/](src/network/README.md) | WebSocket connector, reconnection, exchange adapters |
| [src/publisher/](src/publisher/README.md) | Redis stream publisher, batching, topic routing |
| [src/bindings/](src/bindings/README.md) | PyO3 Python bindings |
| [tests/](tests/README.md) | Test-suite layout (unit, e2e, chaos, production) |
| [benches/](benches/README.md) | Criterion benchmark layout |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Exchange APIs                         │
│              (Deribit, Binance, Oanda, ...)                 │
└─────────────────────────────────────────────────────────────┘
                              │ WebSocket
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Exchange Adapters                        │
│                    (src/network/adapters/)                   │
└─────────────────────────────────────────────────────────────┘
                              │ Parsed Messages
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Order Book Engine                       │
│                        (src/book/)                           │
│         BTreeMap + Cached Best Prices + parking_lot          │
└─────────────────────────────────────────────────────────────┘
                              │ Snapshots/Deltas
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Redis Publisher                         │
│                      (src/publisher/)                        │
│              Batching + Topic Routing + Pool                 │
└─────────────────────────────────────────────────────────────┘
                              │ Redis Streams
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Python Bindings                          │
│                      (src/bindings/)                         │
│                   PyO3 Zero-Copy Access                      │
└─────────────────────────────────────────────────────────────┘
```

---

## Signal Output (AlphaSignal Format)

Flash can publish order-book-derived signals in a standardized **AlphaSignal** format intended for a downstream decision-aggregation consumer (see `src/fusion/`).

### Output Format

Flash publishes to Redis Streams using the `astra:signals:flash` topic prefix:

```
astra:signals:flash:{exchange}:{symbol}
```

Examples:
- `astra:signals:flash:deribit:BTC_USD`
- `astra:signals:flash:binance:ETH_USD`
- `astra:signals:flash:oanda:EUR_USD`

### AlphaSignal Payload

```json
{
  "signal_id": "550e8400-e29b-41d4-a716-446655440000",
  "timestamp": "2026-01-17T12:00:00Z",
  "symbol": "BTC_USD",
  "signal": "LONG",
  "strength": 0.75,
  "metadata": {
    "source_strategy": "flash",
    "confidence": 0.85,
    "raw_direction": 0.42,
    "extra": {
      "bid_depth": 25.0,
      "ask_depth": 20.0,
      "imbalance": 0.11,
      "spread": 10.0
    }
  }
}
```

### Signal Derivation

Flash derives signals from order book imbalance:

| Imbalance | Direction | Interpretation |
|-----------|-----------|----------------|
| > 0.1 | LONG | More bid pressure, bullish |
| < -0.1 | SHORT | More ask pressure, bearish |
| -0.1 to 0.1 | NEUTRAL | Balanced order flow |

### Usage in Code

```rust
use astra_flash::fusion::{AlphaSignal, SignalDirection, build_fusion_topic};
use astra_flash::book::orderbook::BookSnapshot;

// Convert order book snapshot to AlphaSignal
let signal = AlphaSignal::from_book_snapshot(&snapshot, 0.85);

// Get Fusion-compatible topic
let topic = build_fusion_topic(&snapshot.instrument);
// Returns: "astra:signals:flash:deribit:BTC_USD"

// Serialize for Redis
let json = signal.to_json()?;
```

---

## Performance

> All numbers below are single-run Criterion micro-benchmarks on the author's
> development machine (AMD Ryzen 7 5700X, Windows). Treat them as indicative,
> not as independently verified guarantees. Reproduce with `cargo bench`.

### Latency Breakdown (single run, dev machine)

| Component | Latency |
|-----------|---------|
| Best Bid/Ask Lookup | ~0.48 ns |
| Order Book Update | ~80 ns |
| Snapshot (50 levels) | ~451 ns |
| E2E Pipeline | ~3.0 μs |

### Throughput (single run, dev machine)

| Operation | Rate |
|-----------|------|
| Order Book Updates | ~150K/s (single thread) |
| Concurrent Reads | ~4.8M/s (8 threads) |
| Burst Processing | ~14M msg/s |

---

## Configuration

```toml
# config.toml
[general]
log_level = "info"
metrics_port = 9090

[redis]
url = "redis://localhost:6379"
pool_size = 10

[exchanges.deribit]
enabled = true
ws_url = "wss://www.deribit.com/ws/api/v2"
instruments = ["BTC-PERPETUAL", "ETH-PERPETUAL"]

[orderbook]
max_depth = 50
max_levels = 100
auto_prune = true

[batch]
max_size = 100
max_wait_ms = 10
```

See the example profiles under [config/](config/README.md) for the full YAML schema actually consumed at runtime.

---

## Contributing

1. Follow a test-driven methodology (write tests first)
2. Ensure all quality gates pass (clippy, tests, benchmarks)
3. Run `cargo fmt`, `cargo clippy`, `cargo test`

---

## License

MIT License - see [LICENSE](LICENSE) for details.

---

## Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/) - Systems programming language
- [tokio](https://tokio.rs/) - Async runtime
- [PyO3](https://pyo3.rs/) - Python bindings
- [criterion](https://bheisler.github.io/criterion.rs/) - Benchmarking
- [parking_lot](https://github.com/Amanieu/parking_lot) - Fast synchronization

---

**Flash** - *Where nanoseconds matter.*
