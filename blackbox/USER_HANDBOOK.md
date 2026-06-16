# BlackBox User Handbook

> **Note**: This is a reference copy. The comprehensive version is at [`docs/USER_HANDBOOK.md`](docs/USER_HANDBOOK.md).

**The Flight Recorder for Algorithmic Trading**

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Getting Started](#2-getting-started)
3. [Recording Sessions](#3-recording-sessions)
4. [Replaying Sessions](#4-replaying-sessions)
5. [Verification](#5-verification)
6. [CLI Reference](#6-cli-reference)
7. [Best Practices](#7-best-practices)
8. [Quick Reference](#8-quick-reference)

---

## 1. Introduction

BlackBox captures every critical event during trading sessions, enabling exact replay of market conditions for debugging, analysis, and verification.

### Key Capabilities

| Feature | Description |
|---------|-------------|
| **Zero-Allocation Journaling** | <1μs overhead, MMAP-based writes |
| **Deterministic Replay** | Bit-for-bit reproducible execution |
| **State Verification** | SHA-256 checkpoint comparison |
| **Self-Describing Format** | 10-year readability guarantee |

### When to Use BlackBox

| Scenario | How BlackBox Helps |
|----------|-------------------|
| "Heisenbug" at 3 AM | Replay the exact session |
| Strategy divergence | Compare state hashes |
| Performance regression | Benchmark historical sessions |
| Compliance audit | Extract order sequence |

---

## 2. Getting Started

### Installation

```toml
[dependencies]
blackbox = { path = "crates/blackbox" }
blackbox-types = { path = "crates/blackbox-types" }
```

### Build Commands

```bash
cargo build --release           # Build
cargo test --all-features       # Test
cargo bench                     # Benchmark
cargo doc --open                # Documentation
```

---

## 3. Recording Sessions

### Basic Setup

```rust
use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, Tap};
use blackbox_types::{Exchange, Timestamp};

// Create writer and tap
let writer = JournalWriter::new("session.journal", WriterConfig::default())?;
let tap = JournalTap::new(writer);

// Record events
tap.record_ingress(Exchange::Deribit, &frame, Timestamp::now());  // Market data
tap.record_internal(0x0010, &snapshot, Timestamp::now());          // State change
tap.record_egress(Exchange::Deribit, &order, Timestamp::now());    // Orders
```

### TAP Points

| TAP | Location | Records |
|-----|----------|---------|
| TAP-1 (Ingress) | WebSocket connector | Raw market data |
| TAP-2 (Internal) | OrderBook engine | State snapshots |
| TAP-3 (Egress) | Order manager | Outbound orders |

### Production Mode (Zero Overhead)

```rust
// Use NullTap when recording disabled
let tap = NullTap;  // ~1.2ns overhead (compiles to no-op)
```

---

## 4. Replaying Sessions

### Basic Replay

```rust
use blackbox::journal::JournalReader;
use blackbox::replay::{ReplayEngine, BufferedDataSource, WarpConfig};

// Load journal
let reader = JournalReader::open("session.journal")?;
let frames: Vec<_> = reader.filter_map(|r| r.ok()).collect();

// Create engine
let source = BufferedDataSource::new(frames);
let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

// Replay
engine.play();
while !engine.is_completed() {
    let result = engine.step();
    if let Some(frame) = result.frame {
        process_frame(&frame);
    }
}
```

### Replay Modes

| Mode | Use Case | Speed |
|------|----------|-------|
| **Step** | Debugging | Manual |
| **RealTime** | Timing testing | 1x |
| **FastForward** | Quick validation | Nx |
| **WarpSpeed** | Full replay | Skip idle |

```rust
engine.set_mode(ReplayMode::WarpSpeed);  // Skip idle periods
engine.run_to_completion();              // 24h in seconds
```

---

## 5. Verification

### State Hashing

```rust
use blackbox::verify::{StateHash, Checkpoint, CheckpointBuilder};

// Create state hash
let mut hasher = StateHash::new();
hasher.update_orderbook(&orderbook_bytes);
let hash = hasher.finalize();

// Create checkpoint
let checkpoint = CheckpointBuilder::new()
    .sequence(1)
    .timestamp(Timestamp::now().as_micros())
    .hash(hash)
    .build();
```

### Verification Engine

```rust
use blackbox::verify::{VerifyingReplayEngine, VerificationCallback};

struct MyCallback { /* state reference */ }

impl VerificationCallback for MyCallback {
    fn compute_state_hash(&self) -> [u8; 32] {
        // Compute and return current state hash
    }
}

let verifier = VerifyingReplayEngine::new(engine, callback, config);
let stats = verifier.run_with_verification();
```

---

## 6. CLI Reference

### Commands

```bash
# Journal information
blackbox info session.journal
blackbox info session.journal --schema

# Verification
blackbox verify session.journal
blackbox verify session.journal --format json --output report.json
blackbox verify session.journal --stop-on-mismatch

# Record inspection
blackbox dump session.journal --limit 100
blackbox dump session.journal --format json

# Statistics
blackbox stats session.journal
blackbox stats session.journal --detailed
```

### Output Formats

| Format | Use Case |
|--------|----------|
| `text` | Human reading |
| `json` | Automation/parsing |
| `summary` | Quick overview |

---

## 7. Best Practices

### Recording

1. **Record at entry points** - Capture raw data before processing
2. **Use consistent timestamps** - Single source per event
3. **Checkpoint regularly** - Every 1000 events or every minute
4. **Rotate journals** - Daily or by size (100MB-1GB)

### Replay

1. **Verify after code changes** - Regression testing
2. **Keep reference sessions** - Known-good for comparison
3. **Test edge cases** - Empty books, max payloads, bursts

### Performance

1. **Use NullTap in production** - Unless actively debugging
2. **Size buffers appropriately** - 16K-128K for high-frequency
3. **Pre-allocate files** - Avoid growth during trading

---

## 8. Quick Reference

### Essential Commands

```bash
blackbox info session.journal       # View info
blackbox verify session.journal     # Verify replay
blackbox dump session.journal -l 100 # Dump records
blackbox stats session.journal -d    # Statistics
```

### Essential Code

```rust
// Recording
let writer = JournalWriter::new("s.journal", WriterConfig::default())?;
let tap = JournalTap::new(writer);
tap.record_ingress(exchange, data, timestamp);

// Replay
let reader = JournalReader::open("s.journal")?;
let engine = ReplayEngine::with_data_source(source, WarpConfig::warp_speed());
engine.run_to_completion();
```

### Performance Targets

| Operation | Target | Actual |
|-----------|--------|--------|
| NullTap | <10ns | ~1.2ns |
| JournalTap | <1μs | ~110ns |
| Replay Step | <10μs | ~5μs |

### File Format

```
Header:    128 bytes (magic: "BLKBOXJL")
Schema:    32-byte header + Zstd XML
Records:   24-byte header + variable payload
Footer:    32 bytes
```

---

## Additional Resources

- **Comprehensive Handbook**: [`docs/USER_HANDBOOK.md`](docs/USER_HANDBOOK.md)
- **Troubleshooting**: [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md)
- **API Reference**: [`API_REFERENCE.md`](API_REFERENCE.md)
- **Performance Specs**: [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md)
- **Integration Guide**: [`docs/INTEGRATION.md`](docs/INTEGRATION.md)

---

*BlackBox v1.0.0 - The Flight Recorder for Algorithmic Trading*
