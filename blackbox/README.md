# BlackBox

**The Flight Recorder for Algorithmic Trading Systems**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org)

BlackBox is a **deterministic recording and replay system** for high-frequency trading applications. Like an aircraft's black box, it captures every critical event during trading sessions, enabling exact replay of market conditions for debugging, analysis, and verification.

---

## Features

- **Zero-Overhead Recording** - NullTap adds ~1.2ns overhead; JournalTap adds ~110ns
- **Deterministic Replay** - Bit-for-bit reproducible execution
- **Clock Control** - Pause, step-through, fast-forward, and warp-speed modes
- **State Verification** - SHA-256 checkpoint comparison during replay
- **Self-Describing Format** - Embedded SBE schema ensures 10+ year readability
- **Feature-Complete (portfolio project)** - Multi-platform support, 979 tests

## Performance

| Metric | Target | Actual |
|--------|--------|--------|
| NullTap overhead | <10ns | ~1.2ns |
| JournalTap overhead | <1μs | ~110ns |
| Recording throughput | >50k/s | >10M/s |
| Replay (24h session) | <1min | ~15s |

---

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
blackbox = { path = "crates/blackbox" }
blackbox-types = { path = "crates/blackbox-types" }
```

### Recording

```rust
use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, Tap};
use blackbox_types::{Exchange, Timestamp};

// Create a recording tap
let writer = JournalWriter::new("session.journal", WriterConfig::default())?;
let tap = JournalTap::new(writer);

// Record events
tap.record_ingress(Exchange::Deribit, &websocket_frame, Timestamp::now());
tap.record_internal(0x0010, &orderbook_snapshot, Timestamp::now());
tap.record_egress(Exchange::Deribit, &order_payload, Timestamp::now());
```

### Replay

```rust
use blackbox::journal::JournalReader;
use blackbox::replay::{ReplayEngine, JournalDataSource, WarpConfig};
use blackbox_types::SimulatedClock;

// Load journal
let reader = JournalReader::open("session.journal")?;
let frames: Vec<_> = reader.filter_map(|r| r.ok()).collect();

// Replay with warp-speed
let source = JournalDataSource::from_frames(frames);
let clock = SimulatedClock::new(Timestamp::EPOCH);
let mut engine = ReplayEngine::new(source, clock, WarpConfig::warp_speed());

engine.run_to_completion();
```

### CLI

```bash
# View journal info
blackbox info session.journal

# Verify replay determinism
blackbox verify session.journal

# Dump records
blackbox dump session.journal --limit 100 --format json

# Show statistics
blackbox stats session.journal --detailed
```

---

## Repository Structure

```
blackbox/
├── Cargo.toml                 # Workspace manifest
├── Cargo.lock                 # Dependency lock file
├── README.md                  # This file
├── DESIGN_DOC.md              # Original design document
├── API_REFERENCE.md           # Public API reference
├── USER_HANDBOOK.md           # User guide and best practices
├── TROUBLESHOOTING.md         # Common issues & solutions
│
├── crates/
│   ├── blackbox-types/        # Shared type definitions
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs         # Crate entry point
│   │   │   ├── clock.rs       # Clock trait, SystemClock
│   │   │   ├── exchange.rs    # Exchange enum
│   │   │   ├── instrument.rs  # Instrument struct (Copy-able)
│   │   │   ├── side.rs        # Buy/Sell enum
│   │   │   └── timestamp.rs   # Microsecond timestamp
│   │   └── benches/
│   │       └── clock_bench.rs # Clock benchmarks
│   │
│   └── blackbox/              # Main flight recorder crate
│       ├── Cargo.toml
│       ├── build.rs           # Schema embedding
│       ├── src/
│       │   ├── lib.rs         # Crate entry point
│       │   ├── cli.rs         # CLI interface (clap)
│       │   │
│       │   ├── codec/         # Binary encoding/decoding
│       │   │   ├── mod.rs
│       │   │   ├── data.rs    # Version-independent data
│       │   │   ├── error.rs   # Error types
│       │   │   ├── traits.rs  # Encoder/Decoder traits
│       │   │   ├── registry.rs# Version dispatch
│       │   │   └── v1_0/      # v1.0 codec implementation
│       │   │       ├── mod.rs
│       │   │       ├── encoder.rs
│       │   │       └── decoder.rs
│       │   │
│       │   ├── journal/       # Binary journaling
│       │   │   ├── mod.rs
│       │   │   ├── format.rs  # File/record headers
│       │   │   ├── writer.rs  # MMAP writer
│       │   │   ├── reader.rs  # Iterator reader
│       │   │   ├── ring_buffer.rs # Lock-free SPSC
│       │   │   └── schema.rs  # Schema block handling
│       │   │
│       │   ├── tap/           # Instrumentation
│       │   │   ├── mod.rs
│       │   │   ├── traits.rs  # Tap trait definition
│       │   │   ├── null_tap.rs# Zero-overhead no-op
│       │   │   └── journal_tap.rs # Recording tap
│       │   │
│       │   ├── replay/        # Replay engine
│       │   │   ├── mod.rs
│       │   │   ├── engine.rs  # ReplayEngine
│       │   │   ├── data_source.rs # DataSource trait
│       │   │   ├── scheduler.rs   # SkipIdleScheduler
│       │   │   └── simulated_clock.rs # Controllable clock
│       │   │
│       │   └── verify/        # State verification
│       │       ├── mod.rs
│       │       ├── state_hash.rs    # SHA-256 accumulator
│       │       ├── checkpoint.rs    # State checkpoints
│       │       ├── comparator.rs    # State comparison
│       │       ├── verifying_engine.rs # Replay + verify
│       │       └── report.rs        # Comparison reports
│       │
│       ├── src/bin/
│       │   └── main.rs        # CLI binary entry point
│       │
│       ├── benches/           # Performance benchmarks
│       │   ├── journal_bench.rs
│       │   ├── tap_latency_bench.rs
│       │   ├── clock_bench.rs
│       │   ├── data_source_bench.rs
│       │   ├── replay_engine_bench.rs
│       │   └── production_overhead_bench.rs
│       │
│       └── tests/             # Integration tests
│           ├── integration_tests.rs
│           ├── regression_tests.rs
│           └── zero_alloc_test.rs
│
├── schemas/
│   └── blackbox-v1.0.xml      # SBE schema definition
│
└── docs/                      # User documentation
    ├── USAGE.md               # How to use BlackBox
    ├── INTEGRATION.md         # Integration guide
    ├── TROUBLESHOOTING.md     # Common issues & solutions
    ├── PERFORMANCE.md         # Performance specifications
    ├── PROJECT_SUMMARY.md     # Comprehensive technical documentation
    ├── USER_HANDBOOK.md       # User guide and best practices
    └── TIME_AUDIT.md          # Time source audit
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [docs/PROJECT_SUMMARY.md](docs/PROJECT_SUMMARY.md) | Comprehensive technical documentation |
| [USER_HANDBOOK.md](USER_HANDBOOK.md) | User guide, best practices, tips |
| [API_REFERENCE.md](API_REFERENCE.md) | Public API reference |
| [docs/USAGE.md](docs/USAGE.md) | Quick start and API reference |
| [docs/INTEGRATION.md](docs/INTEGRATION.md) | Integration guide |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Common issues and solutions |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Performance specifications |
| [DESIGN_DOC.md](DESIGN_DOC.md) | Original architecture design |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       Trading System                         │
│                                                              │
│  ┌─────────┐     ┌─────────┐     ┌─────────┐               │
│  │WebSocket│────▶│  TAP-1  │────▶│OrderBook│               │
│  │  Feed   │     │(Ingress)│     │         │               │
│  └─────────┘     └────┬────┘     └────┬────┘               │
│                       │               │                     │
│                       │          ┌────┴────┐               │
│                       │          │  TAP-2  │               │
│                       │          │(Internal)│               │
│                       │          └────┬────┘               │
│                       │               │                     │
│                       │          ┌────┴────┐               │
│                       │          │  Order  │               │
│                       │          │ Manager │               │
│                       │          └────┬────┘               │
│                       │               │                     │
│                       │          ┌────┴────┐               │
│                       │          │  TAP-3  │               │
│                       │          │(Egress) │               │
│                       │          └────┬────┘               │
│                       │               │                     │
└───────────────────────┼───────────────┼─────────────────────┘
                        │               │
                        ▼               ▼
              ┌─────────────────────────────────┐
              │      MMAP Journal Writer        │
              │       (Background Thread)       │
              └───────────────┬─────────────────┘
                              │
                              ▼
                   ┌─────────────────────┐
                   │   .journal file     │
                   │   (Binary, MMAP)    │
                   └─────────────────────┘
```

---

## Building

### Requirements

- Rust 1.75 or later
- Cargo

### Build Commands

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run all tests
cargo test --all-features

# Run benchmarks
cargo bench

# Build documentation
cargo doc --open
```

### Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Standard functionality | Yes |

---

## Testing

```bash
# Run all tests
cargo test --all-features

# Run specific test category
cargo test --test integration_tests
cargo test --test regression_tests
cargo test --test zero_alloc_test

# Run with output
cargo test -- --nocapture

# Run benchmarks
cargo bench
```

### Test Coverage

| Category | Tests |
|----------|-------|
| Unit tests (blackbox) | 826 |
| Integration tests | 23 |
| Regression tests | 22 |
| Zero-allocation tests | 15 |
| Unit tests (blackbox-types) | 25 |
| Doc-tests | 68 |
| **Total** | **979** |

---

## Quality Gates

All commits must pass these gates:

| Gate | Command | Description |
|------|---------|-------------|
| 1. Build | `cargo check --all-features` | Compilation check |
| 2. Tests | `cargo test --all-features` | All tests pass |
| 3. Lint | `cargo clippy -- -D warnings` | No linter warnings |
| 4. Format | `cargo fmt --check` | Consistent formatting |
| 5. Docs | `cargo doc --no-deps` | Documentation builds |
| 6. Bench | `cargo bench --no-run` | Benchmarks compile |

---

## Integration with a Trading Engine

BlackBox integrates with a host trading engine via a feature flag, so recording
can be compiled in or out with zero overhead when disabled:

```toml
# In the host engine's Cargo.toml
[features]
blackbox = ["dep:blackbox-types", "dep:blackbox"]
```

```bash
# Build without recording
cargo build --release

# Build with recording enabled
cargo build --release --features blackbox
```

See [docs/INTEGRATION.md](docs/INTEGRATION.md) for detailed integration instructions.

---

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Ensure all quality gates pass
4. Commit your changes (`git commit -m 'Add amazing feature'`)
5. Push to the branch (`git push origin feature/amazing-feature`)
6. Open a Pull Request

### Development Guidelines

- Follow Rust API guidelines
- Add tests for new functionality
- Update documentation as needed
- Run `cargo fmt` before committing
- Ensure `cargo clippy` passes with no warnings

---

## License

This project is licensed under the MIT License.

---

## Acknowledgments

- Built with Rust's zero-cost abstractions
- Memory-mapped I/O via `memmap2`
- Lock-free structures via `crossbeam`
- Performance benchmarking via `criterion`

---

## Project Status

**Version:** 1.0.0
**Status:** Feature-complete (portfolio project)

All development phases completed:
- Phase 0: Foundation (shared types)
- Phase 1: The Journal (binary I/O)
- Phase 2: The Tap (instrumentation)
- Phase 3: The Player (replay engine)
- Phase 4: Integration (verification)
- Phase 5: Trading-engine integration & handover

---

*Built with precision for algorithmic trading.*
