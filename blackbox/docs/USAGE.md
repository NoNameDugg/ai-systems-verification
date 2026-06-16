# BlackBox User Guide

**Version:** 1.0.0
**Last Updated:** 2026-01-06

---

## Table of Contents

1. [Overview](#overview)
2. [Quick Start](#quick-start)
3. [Core Concepts](#core-concepts)
4. [Recording Sessions](#recording-sessions)
5. [Replaying Sessions](#replaying-sessions)
6. [CLI Commands](#cli-commands)
7. [Verification and Debugging](#verification-and-debugging)
8. [API Reference](#api-reference)
9. [Best Practices](#best-practices)

---

## Overview

BlackBox is a flight recorder for the trading system. It provides:

- **Zero-allocation journaling**: <1μs overhead in production
- **Deterministic replay**: Bit-for-bit reproducible executions
- **State verification**: Checkpoint-based regression testing
- **10-year readability**: Self-describing journal format

### Use Cases

| Scenario | Solution |
|----------|----------|
| "Heisenbug" occurred at 3am | Replay the recorded session |
| Strategy behaving differently | Compare state hashes at checkpoints |
| Performance regression | Benchmark against historical sessions |
| Compliance audit | Extract exact order sequence |

---

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
blackbox = { workspace = true }
blackbox-types = { workspace = true }
```

### Recording a Session

```rust
use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, Tap};
use blackbox_types::{Exchange, Timestamp};

// Create a journal writer
let config = WriterConfig::default();
let writer = JournalWriter::new("session.journal", config)?;

// Create a tap for recording
let tap = JournalTap::new(writer);

// Record market data (TAP-1: Ingress)
tap.record_ingress(
    Exchange::Deribit,
    b"{\"type\":\"quote\",\"bid\":50000.0}",
    Timestamp::from_micros(1704067200_000_000),
);

// Record internal state (TAP-2: Internal)
tap.record_internal(
    0x0010, // BOOK_SNAPSHOT
    b"serialized_orderbook_state",
    Timestamp::from_micros(1704067200_001_000),
);

// Record outbound orders (TAP-3: Egress)
tap.record_egress(
    Exchange::Deribit,
    b"{\"order\":\"buy\",\"qty\":1.0}",
    Timestamp::from_micros(1704067200_002_000),
);
```

### Replaying a Session

```rust
use blackbox::journal::JournalReader;
use blackbox::replay::{BufferedDataSource, DataFrame, ReplayEngine, WarpConfig};

// Read journal file
let reader = JournalReader::open("session.journal")?;

// Convert to data frames
let frames: Vec<DataFrame> = reader
    .iter()
    .filter_map(|r| r.ok())
    .map(|record| DataFrame::from(record))
    .collect();

// Create replay engine with warp-speed (skip idle periods)
let source = BufferedDataSource::new(frames);
let mut engine = ReplayEngine::with_data_source(source, WarpConfig::warp_speed());

// Play through all events
engine.play();
while let Some(frame) = engine.step().frame {
    // Process frame exactly as in live trading
    process_market_data(&frame);
}
```

### Verify Replay

```bash
# Using the CLI
blackbox verify session.journal --format text

# With detailed output
blackbox info session.journal --schema
```

---

## Core Concepts

### TAP Points

BlackBox uses three instrumentation points (TAPs):

| TAP | Type | Location | Data Captured |
|-----|------|----------|---------------|
| TAP-1 | Ingress | WebSocket connector | Raw market data frames |
| TAP-2 | Internal | OrderBook engine | State snapshots/deltas |
| TAP-3 | Egress | Order manager | Outbound order submissions |

### Journal Format

Each journal file is self-describing:

```
┌──────────────────────────────────────┐
│ File Header (128 bytes)              │
├──────────────────────────────────────┤
│ Embedded Schema Block (variable)     │
├──────────────────────────────────────┤
│ Record 1: [Header][Payload]          │
│ Record 2: [Header][Payload]          │
│ ...                                  │
├──────────────────────────────────────┤
│ File Footer (32 bytes)               │
└──────────────────────────────────────┘
```

### Clock Control

BlackBox supports three clock modes:

| Mode | Description | Use Case |
|------|-------------|----------|
| Real-time | 1:1 playback speed | Testing timing-sensitive code |
| Fast-forward | N× speed multiplier | Quick validation runs |
| Warp-speed | Skip idle periods | Full session replay in seconds |

---

## Recording Sessions

### WriterConfig Options

```rust
use blackbox::journal::WriterConfig;

let config = WriterConfig {
    // Ring buffer size for async writes
    ring_buffer_capacity: 65536,

    // Maximum file size before rotation
    file_size: 64 * 1024 * 1024, // 64 MB

    // Compress embedded schema
    compress_schema: true,

    // Sync to disk on close
    sync_on_close: true,

    // Pre-fault pages for consistent latency
    prefault_pages: true,
};
```

### Using NullTap for Production

When recording is disabled, use `NullTap` for zero overhead:

```rust
use blackbox::tap::{NullTap, Tap};

fn process_data<T: Tap>(tap: &T, data: &[u8]) {
    // NullTap: This compiles to a no-op (0 bytes, 0 instructions)
    tap.record_ingress(Exchange::Binance, data, timestamp);

    // ... actual processing
}

// Production (disabled recording)
let tap = NullTap;
process_data(&tap, market_data);

// Development (enabled recording)
let tap = JournalTap::new(writer);
process_data(&tap, market_data);
```

### Feature Flag Pattern

```rust
#[cfg(feature = "blackbox")]
use blackbox::tap::JournalTap;

#[cfg(not(feature = "blackbox"))]
use blackbox::tap::NullTap;

fn create_tap() -> impl Tap {
    #[cfg(feature = "blackbox")]
    {
        let writer = JournalWriter::new("session.journal", config)?;
        JournalTap::new(writer)
    }

    #[cfg(not(feature = "blackbox"))]
    {
        NullTap
    }
}
```

---

## Replaying Sessions

### Step-Through Debugging

```rust
use blackbox::replay::{ReplayEngine, SimulatedClock};

let mut engine = ReplayEngine::with_data_source(source, config);
engine.play();

// Step one event at a time
loop {
    let result = engine.step();

    if let Some(frame) = result.frame {
        println!("Frame at {}: {:?}", frame.timestamp, frame.frame_type);

        // Pause for inspection
        engine.pause();

        // Inspect state...

        // Continue
        engine.play();
    } else {
        break; // No more events
    }
}
```

### Fast-Forward Mode

```rust
use blackbox::replay::WarpConfig;

// 10x speed (100ms becomes 10ms)
let config = WarpConfig::fast_forward(10);
let mut engine = ReplayEngine::with_data_source(source, config);
engine.play();
engine.run_to_completion();
```

### Warp-Speed Mode

```rust
// Skip all idle periods (>100ms gaps)
let config = WarpConfig::warp_speed();
let mut engine = ReplayEngine::with_data_source(source, config);
engine.play();

// A 24-hour session replays in ~60 seconds
engine.run_to_completion();
```

---

## CLI Commands

### blackbox info

Display journal file information:

```bash
# Basic info
blackbox info session.journal

# Output:
# Journal: session.journal
# Format Version: 1.2
# Schema Version: 1.0
# Session Start: 2026-01-04 00:00:00 UTC
# Session End: 2026-01-04 23:59:59 UTC
# Record Count: 1,234,567
# File Size: 45.2 MB

# Include embedded schema
blackbox info session.journal --schema
```

### blackbox verify

Verify replay determinism:

```bash
# Basic verification
blackbox verify session.journal

# JSON output
blackbox verify session.journal --format json --output report.json

# Stop on first mismatch
blackbox verify session.journal --stop-on-mismatch
```

### blackbox dump

Dump records for inspection:

```bash
# Dump first 100 records
blackbox dump session.journal --limit 100

# Filter by record type
blackbox dump session.journal --record-type ingress

# JSON format
blackbox dump session.journal --format json > records.json
```

### blackbox stats

Show statistics:

```bash
# Summary statistics
blackbox stats session.journal

# Detailed breakdown by record type
blackbox stats session.journal --detailed

# Output:
# Total Records: 1,234,567
# Duration: 23:59:59
#
# By Type:
#   WS_TEXT:        800,000 (64.8%)
#   WS_BINARY:      200,000 (16.2%)
#   BOOK_SNAPSHOT:   50,000 (4.0%)
#   BOOK_DELTA:     150,000 (12.2%)
#   ORDER_SUBMIT:    34,567 (2.8%)
```

---

## Verification and Debugging

### State Hashing

Use `StateHash` to track system state at checkpoints:

```rust
use blackbox::verify::{StateHash, Hashable};

// Create a hash of your state
let mut hasher = StateHash::new();
hasher.update_raw(&orderbook.best_bid.to_le_bytes());
hasher.update_raw(&orderbook.best_ask.to_le_bytes());
hasher.update_sequence(orderbook.sequence_number);

let state_hash = hasher.finalize();

// Record checkpoint
tap.record_checkpoint(sequence_number, timestamp, state_hash.as_bytes());
```

### Comparison Reports

```rust
use blackbox::verify::{ReplayComparator, ComparisonReport};

let mut comparator = ReplayComparator::new();

// Add results during replay
comparator.record_match(checkpoint, computed_hash);
// or
comparator.record_mismatch(checkpoint, expected_hash, actual_hash);

// Generate report
let report = ComparisonReport::from_comparator(&comparator);
println!("{}", report.to_text());
```

### Debugging Mismatches

When a state mismatch occurs:

1. **Identify the checkpoint**:
   ```bash
   blackbox verify session.journal --format json | jq '.mismatches[0]'
   ```

2. **Dump records around the checkpoint**:
   ```bash
   blackbox dump session.journal --limit 100 --offset 1000
   ```

3. **Compare hashes**:
   ```rust
   println!("Expected: {}", expected_hash.to_hex());
   println!("Actual:   {}", actual_hash.to_hex());
   ```

4. **Step through replay** at the problematic point to identify divergence.

---

## API Reference

### Journal Module

| Type | Description |
|------|-------------|
| `JournalWriter` | MMAP-based binary writer |
| `JournalReader` | Sequential record iterator |
| `WriterConfig` | Writer configuration |
| `ReaderConfig` | Reader configuration |
| `RecordType` | Record type discriminant |

### Tap Module

| Type | Description |
|------|-------------|
| `Tap` | Trait for instrumentation |
| `NullTap` | Zero-overhead no-op |
| `JournalTap` | Recording implementation |

### Replay Module

| Type | Description |
|------|-------------|
| `ReplayEngine` | Main replay controller |
| `SimulatedClock` | Time simulation |
| `DataSource` | Abstract data source |
| `BufferedDataSource` | In-memory data source |
| `WarpConfig` | Warp-speed configuration |

### Verify Module

| Type | Description |
|------|-------------|
| `StateHash` | Deterministic state hasher |
| `Checkpoint` | State checkpoint record |
| `ReplayComparator` | Comparison tracker |
| `ComparisonReport` | Report generator |

For complete API documentation:

```bash
cargo doc --package blackbox --open
```

---

## Best Practices

### 1. Always Use Feature Flags

```toml
[features]
default = []
blackbox = ["blackbox"]
```

This ensures zero overhead in production when recording is disabled.

### 2. Record at Entry Points Only

Record raw data at system boundaries, not after processing:

```rust
// GOOD: Record raw WebSocket frame
tap.record_ingress(exchange, raw_frame, timestamp);
let parsed = parse(raw_frame);

// BAD: Record parsed data (not reproducible)
tap.record_ingress(exchange, &parsed.to_bytes(), timestamp);
```

### 3. Use Checkpoints Strategically

Place checkpoints at:
- After processing each market data update
- Before and after order submissions
- At regular intervals (e.g., every 1000 events)

### 4. Verify Schema Compatibility

When updating the schema:
```bash
# Test old journals are still readable
blackbox verify old_session.journal
```

### 5. Monitor Journal Size

```bash
# Check disk usage during recording
blackbox stats current_session.journal
```

Rotate journals when they exceed recommended size (100MB-1GB).

---

## Troubleshooting

See [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for common issues and solutions.

## Performance

See [PERFORMANCE.md](PERFORMANCE.md) for detailed performance specifications.

## Integration

See [INTEGRATION.md](INTEGRATION.md) for the host-engine integration guide.

---

*Generated for BlackBox v1.0.0*
