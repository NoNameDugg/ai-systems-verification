# BlackBox: The Golden Handbook

*Your Complete Guide to Deterministic Recording and Replay*

---

## Welcome to BlackBox

BlackBox is your **flight recorder for algorithmic trading**. Like the black box in an aircraft, it captures every critical event during trading sessions, enabling you to replay exact market conditions to debug issues that would otherwise be impossible to reproduce.

This handbook will guide you from your first recording session to advanced debugging techniques. Whether you're investigating a mysterious bug at 3 AM or building confidence in a new strategy, BlackBox is your ally.

---

## Table of Contents

1. [Getting Started](#1-getting-started)
2. [Understanding the Architecture](#2-understanding-the-architecture)
3. [Recording Sessions](#3-recording-sessions)
4. [Replaying Sessions](#4-replaying-sessions)
5. [Verification and Debugging](#5-verification-and-debugging)
6. [CLI Reference](#6-cli-reference)
7. [Integration Guide](#7-integration-guide)
8. [Best Practices](#8-best-practices)
9. [Troubleshooting](#9-troubleshooting)
10. [Performance Tuning](#10-performance-tuning)
11. [Tips and Tricks](#11-tips-and-tricks)
12. [Lessons from the Field](#12-lessons-from-the-field)
13. [FAQ](#13-faq)
14. [Quick Reference Card](#14-quick-reference-card)

---

## 1. Getting Started

### What BlackBox Does

BlackBox captures three types of events:

| Event Type | What It Captures | Why It Matters |
|------------|------------------|----------------|
| **Ingress** | Every WebSocket frame from exchanges | Market data that triggered your decisions |
| **Internal** | Order book state changes | The state your algorithm saw |
| **Egress** | Every order you sent | Your algorithm's actions |

With these three event streams captured, you can replay any trading session and get **identical results**.

### Your First Recording

```rust
use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::JournalTap;

// Create a journal file for this session
let writer = JournalWriter::new(
    "trading_session_2026_01_07.journal",
    WriterConfig::default()
)?;

// Create the tap that will record events
let tap = JournalTap::new(writer);

// Now pass `tap` to your trading components
// Every event will be automatically recorded
```

### Your First Replay

```rust
use blackbox::journal::JournalReader;

// Open the journal file
let reader = JournalReader::open("trading_session_2026_01_07.journal")?;

// Print session info
println!("Session ID: {:016x}", reader.header().session_id);
println!("Records: {}", reader.header().record_count);

// Iterate through all records
for record in reader {
    let record = record?;
    println!("[{:?}] {:?}", record.timestamp, record.record_type);
}
```

### Quick Check with CLI

```bash
# See what's in a journal file
blackbox info trading_session.journal

# Output:
# Session: a1b2c3d4e5f67890
# Created: 2026-01-07 09:30:00.000000 UTC
# Records: 1,234,567
# Duration: 6h 30m
# Size: 256 MB
```

---

## 2. Understanding the Architecture

### The Three TAP Points

Think of TAP points as microphones placed at strategic locations in your trading system:

```
                    Your Trading System
┌──────────────────────────────────────────────────────┐
│                                                      │
│   Exchange          TAP-1              Algorithm     │
│      │               │                     │         │
│      │  WebSocket    │                     │         │
│      │◀─────────────▶│                     │         │
│      │   Frames      │  "I heard that"    │         │
│                      ▼                     │         │
│              ┌───────────────┐            │         │
│              │   OrderBook   │────────────┘         │
│              │               │                      │
│              │    TAP-2      │ "My state changed"   │
│              └───────┬───────┘                      │
│                      │                              │
│                      ▼                              │
│              ┌───────────────┐                      │
│              │ OrderManager  │                      │
│              │               │                      │
│              │    TAP-3      │ "I'm sending this"   │
│              └───────┬───────┘                      │
│                      │                              │
└──────────────────────┼──────────────────────────────┘
                       │
                       ▼
              ┌─────────────────┐
              │  Journal File   │
              │  (.journal)     │
              └─────────────────┘
```

### NullTap vs JournalTap

You have two tap implementations:

| Tap | Overhead | Use Case |
|-----|----------|----------|
| **NullTap** | ~1 nanosecond | Production (recording disabled) |
| **JournalTap** | ~110 nanoseconds | Development, debugging, analysis |

The beauty of this design: **your production code doesn't change**. You just swap which tap implementation you use:

```rust
// Development/Debug mode
let tap = JournalTap::new(writer);
run_trading_system(tap);

// Production mode (zero overhead)
let tap = NullTap;
run_trading_system(tap);
```

### The Journal File Format

Each journal file is **self-describing**:

```
┌─────────────────────────────────────┐
│       Header (128 bytes)            │  Metadata about the session
├─────────────────────────────────────┤
│       Schema (compressed XML)       │  How to read the records
├─────────────────────────────────────┤
│       Record 1                      │
│       Record 2                      │
│       Record 3                      │
│       ... millions more ...         │
└─────────────────────────────────────┘
```

**Why this matters:** You can read journal files from years ago without needing external schema files. The file contains everything needed to interpret itself.

---

## 3. Recording Sessions

### Basic Recording Setup

```rust
use blackbox::journal::{JournalWriter, WriterConfig};
use blackbox::tap::{JournalTap, Tap};
use blackbox_types::{Exchange, Timestamp};

fn setup_recording() -> JournalTap {
    // Configure the writer
    let config = WriterConfig {
        initial_size: 64 * 1024 * 1024,  // 64 MB initial file
        ring_buffer_size: 8192,           // 8K event buffer
        flush_interval_ms: 10,            // Flush every 10ms
        sync_on_flush: true,              // Ensure durability
    };

    // Create the writer
    let writer = JournalWriter::new("session.journal", config)
        .expect("Failed to create journal");

    // Wrap in recording tap
    JournalTap::new(writer)
}
```

### Recording Events

```rust
// At your WebSocket handler (TAP-1: Ingress)
fn on_websocket_message(tap: &impl Tap, exchange: Exchange, data: &[u8]) {
    let timestamp = Timestamp::now();
    tap.record_ingress(exchange, data, timestamp);

    // Then process the message...
}

// At your order book (TAP-2: Internal)
fn on_book_update(tap: &impl Tap, snapshot: &BookSnapshot) {
    let timestamp = Timestamp::now();
    let payload = serialize(snapshot);
    tap.record_internal(0x0010, &payload, timestamp);

    // Then update internal state...
}

// At your order manager (TAP-3: Egress)
fn submit_order(tap: &impl Tap, order: &OrderRequest) {
    let timestamp = Timestamp::now();
    let payload = serialize(order);
    tap.record_egress(order.exchange, &payload, timestamp);

    // Then send to exchange...
}
```

### Recording Checkpoints

Checkpoints capture the **state hash** at a point in time, enabling verification during replay:

```rust
use sha2::{Sha256, Digest};

fn checkpoint_state(tap: &impl Tap, order_book: &OrderBook) {
    // Compute state hash
    let mut hasher = Sha256::new();
    hasher.update(&order_book.best_bid().to_le_bytes());
    hasher.update(&order_book.best_ask().to_le_bytes());
    hasher.update(&order_book.mid_price().to_le_bytes());

    let hash: [u8; 32] = hasher.finalize().into();
    let timestamp = Timestamp::now();

    tap.record_checkpoint(&hash, timestamp);
}

// Call periodically (e.g., every 1000 events or every minute)
```

### Feature Flag Pattern

Use Cargo feature flags to control recording at compile time:

```toml
# In Cargo.toml
[features]
default = []
recording = ["blackbox"]
```

```rust
// In your code
#[cfg(feature = "recording")]
use blackbox::tap::JournalTap as ActiveTap;

#[cfg(not(feature = "recording"))]
use blackbox::tap::NullTap as ActiveTap;

fn create_tap() -> ActiveTap {
    #[cfg(feature = "recording")]
    {
        let writer = JournalWriter::new("session.journal", Default::default()).unwrap();
        JournalTap::new(writer)
    }

    #[cfg(not(feature = "recording"))]
    NullTap
}
```

---

## 4. Replaying Sessions

### Basic Replay

```rust
use blackbox::journal::JournalReader;
use blackbox::replay::{
    ReplayEngine, JournalDataSource, BufferedDataSource,
    DataFrame, WarpConfig, StepResult
};
use blackbox_types::SimulatedClock;

fn replay_session(path: &str) {
    // Load the journal
    let reader = JournalReader::open(path).expect("Failed to open journal");

    // Convert records to data frames
    let frames: Vec<DataFrame> = reader
        .filter_map(|r| r.ok())
        .filter_map(|r| DataFrame::try_from(r).ok())
        .collect();

    println!("Loaded {} frames", frames.len());

    // Create data source and simulated clock
    let source = BufferedDataSource::from_frames(frames);
    let clock = SimulatedClock::new(Timestamp::EPOCH);

    // Create replay engine
    let mut engine = ReplayEngine::new(source, clock, WarpConfig::default());

    // Step through events
    while let StepResult::Frame(frame) = engine.step() {
        process_frame(&frame);
    }

    println!("Replay complete: {:?}", engine.stats());
}
```

### Replay Modes

BlackBox supports multiple replay modes:

#### Step Mode (Debugging)

```rust
// Step one event at a time
loop {
    match engine.step() {
        StepResult::Frame(frame) => {
            println!("Event: {:?}", frame);
            // Inspect state here...

            // Press enter to continue (in a real debugger)
        }
        StepResult::EndOfData => break,
        StepResult::Paused => {
            // Waiting for resume
        }
        StepResult::Error(e) => panic!("Replay error: {:?}", e),
    }
}
```

#### Fast-Forward Mode

```rust
// Replay at 10x speed
let config = WarpConfig::fast_forward(10.0);
let mut engine = ReplayEngine::new(source, clock, config);
engine.run_to_completion();
```

#### Warp-Speed Mode

```rust
// Skip idle periods entirely (fastest replay)
let config = WarpConfig::warp_speed();
let mut engine = ReplayEngine::new(source, clock, config);

// 24 hours of trading in seconds
engine.run_to_completion();
```

### Simulated Clock Control

```rust
let clock = SimulatedClock::new(Timestamp::from_micros(0));

// Pause time
clock.pause();
assert!(clock.is_paused());

// Resume time
clock.resume();

// Manually advance time
clock.advance(1_000_000);  // +1 second

// Jump to specific time
clock.set(Timestamp::from_micros(1704067200_000_000));

// Enable warp mode (for skipping idle periods)
clock.enable_warp();
```

---

## 5. Verification and Debugging

### Why Verification Matters

Verification ensures that replayed sessions produce **identical results** to the original. This catches:

- Non-deterministic code (random, uninitialized data)
- Race conditions
- Time-dependent bugs
- External dependencies

### Setting Up Verification

```rust
use blackbox::verify::{
    VerifyingReplayEngine, VerifyConfig, VerificationCallback,
    ComparisonReport
};

// Implement the callback to compute your system's state hash
struct MyStateCallback {
    order_book: Arc<OrderBook>,
}

impl VerificationCallback for MyStateCallback {
    fn compute_state_hash(&self) -> [u8; 32] {
        let mut hasher = StateHash::new();
        hasher.update_bids(&self.order_book.bids());
        hasher.update_asks(&self.order_book.asks());
        hasher.finalize()
    }
}

// Set up verifying engine
let callback = Box::new(MyStateCallback { order_book: order_book.clone() });
let config = VerifyConfig {
    stop_on_mismatch: true,  // Stop immediately on divergence
    stop_on_error: true,
};

let mut verifier = VerifyingReplayEngine::new(engine, callback, config);
```

### Running Verification

```rust
// Run full verification
let report = verifier.run_with_verify();

// Check results
match report.status {
    ReportStatus::Pass => {
        println!("All {} checkpoints matched!", report.total_checkpoints);
    }
    ReportStatus::Fail => {
        println!("Verification failed!");
        println!("Matched: {}/{}", report.matched, report.total_checkpoints);

        // Print mismatches
        for entry in &report.entries {
            if let Some(mismatch) = &entry.mismatch {
                println!("At sequence {}: expected {:x?}, got {:x?}",
                    mismatch.sequence,
                    &mismatch.expected[..8],
                    &mismatch.actual[..8]
                );
            }
        }

        // Print recommendations
        for rec in &report.recommendations {
            println!("Suggestion: {}", rec);
        }
    }
    ReportStatus::Incomplete => {
        println!("Verification incomplete (stopped early)");
    }
}
```

### Interpreting Reports

```rust
// Get different report formats
println!("{}", report.to_text());     // Human-readable
println!("{}", report.to_json());     // Machine-parseable
println!("{}", report.to_summary());  // Brief overview

// Calculate match rate
let rate = report.match_rate();
println!("Match rate: {:.1}%", rate * 100.0);
```

### Debugging State Divergence

When verification fails, follow this process:

1. **Identify the divergence point**
   ```rust
   // Find first mismatch
   let first_mismatch = report.entries.iter()
       .find(|e| e.mismatch.is_some())
       .unwrap();

   println!("Divergence at sequence: {}", first_mismatch.sequence);
   println!("Timestamp: {:?}", first_mismatch.timestamp);
   ```

2. **Replay to just before divergence**
   ```rust
   // Stop just before the problematic event
   let target_seq = first_mismatch.sequence - 1;
   while engine.position() < Some(target_seq) {
       engine.step();
   }

   // Now inspect state
   println!("State before divergence: {:?}", order_book);
   ```

3. **Step through the problematic event**
   ```rust
   // Process the diverging event
   let result = engine.step();
   println!("Event: {:?}", result);
   println!("State after: {:?}", order_book);

   // Compare with expected
   ```

4. **Common causes of divergence**
   - `Utc::now()` called instead of injected clock
   - `HashMap` iteration order (use `BTreeMap`)
   - Floating point accumulation differences
   - Uninitialized memory

---

## 6. CLI Reference

### info - Display Journal Information

```bash
blackbox info <journal_file>

# Example output:
# ═══════════════════════════════════════════════════════
#  BlackBox Journal Info
# ═══════════════════════════════════════════════════════
#  Session ID:    a1b2c3d4e5f67890
#  Format:        v1.0
#  Schema:        v1.0
#
#  Created:       2026-01-07 09:30:00.000000 UTC
#  Duration:      6h 30m 15s
#  Records:       1,234,567
#
#  First Event:   2026-01-07 09:30:00.001234 UTC
#  Last Event:    2026-01-07 16:00:15.987654 UTC
#
#  File Size:     256.7 MB
#  Avg Record:    207 bytes
# ═══════════════════════════════════════════════════════
```

### verify - Verify Replay Determinism

```bash
blackbox verify <journal_file> [options]

Options:
  --stop-on-mismatch    Stop at first mismatch
  --format <fmt>        Output format (text, json, summary)

# Example:
blackbox verify session.journal --format json > report.json
```

### dump - Dump Records

```bash
blackbox dump <journal_file> [options]

Options:
  --limit <n>           Maximum records to dump
  --offset <n>          Skip first n records
  --format <fmt>        Output format (text, json)
  --type <type>         Filter by record type

# Examples:
blackbox dump session.journal --limit 100 --format json
blackbox dump session.journal --type ingress --limit 50
```

### stats - Detailed Statistics

```bash
blackbox stats <journal_file> [options]

Options:
  --detailed            Show detailed breakdown

# Example output (--detailed):
# ═══════════════════════════════════════════════════════
#  Record Type Distribution
# ═══════════════════════════════════════════════════════
#  RawFrame:     892,345  (72.3%)
#  BookSnapshot:  12,456  (1.0%)
#  BookDelta:    287,654  (23.3%)
#  OrderSubmit:   34,567  (2.8%)
#  Checkpoint:     7,545  (0.6%)
# ───────────────────────────────────────────────────────
#  Exchange Distribution
# ───────────────────────────────────────────────────────
#  Deribit:      567,890  (46.0%)
#  Binance:      432,109  (35.0%)
#  Bybit:        234,568  (19.0%)
# ═══════════════════════════════════════════════════════
```

---

## 7. Integration Guide

### Integrating with a Host Engine

BlackBox is designed to integrate seamlessly with the host trading engine:

#### Step 1: Add Dependencies

```toml
# In the host engine's Cargo.toml
[features]
blackbox = ["dep:blackbox-types", "dep:blackbox"]

[dependencies]
blackbox-types = { path = "../blackbox/crates/blackbox-types", optional = true }
blackbox = { path = "../blackbox/crates/blackbox", optional = true }
```

#### Step 2: Add TAP to Connector

```rust
// In connector.rs
#[cfg(feature = "blackbox")]
use blackbox::tap::Tap;

pub struct Connector<T: Tap> {
    // ... existing fields ...
    #[cfg(feature = "blackbox")]
    tap: T,
}

impl<T: Tap> Connector<T> {
    #[cfg(feature = "blackbox")]
    pub fn with_tap(url: &str, tap: T) -> Self {
        Self {
            // ... existing initialization ...
            tap,
        }
    }

    async fn handle_message(&mut self, msg: Message) {
        let timestamp = Timestamp::now();

        #[cfg(feature = "blackbox")]
        self.tap.record_ingress(self.exchange, msg.as_bytes(), timestamp);

        // ... existing processing ...
    }
}
```

#### Step 3: Add TAP to OrderBook

```rust
// In orderbook.rs
#[cfg(feature = "blackbox")]
use blackbox::tap::Tap;

impl<T: Tap> OrderBook<T> {
    pub fn apply_snapshot(&mut self, snapshot: BookSnapshot) {
        #[cfg(feature = "blackbox")]
        {
            let payload = bincode::serialize(&snapshot).unwrap();
            self.tap.record_internal(0x0010, &payload, Timestamp::now());
        }

        // ... existing processing ...
    }
}
```

#### Step 4: Build with Recording

```bash
# Without recording (production)
cargo build --release

# With recording enabled
cargo build --release --features blackbox
```

### Integrating with Custom Systems

If you're not using the host trading engine, follow this pattern:

```rust
use blackbox::tap::Tap;

// Make your components generic over Tap
pub struct MyTradingEngine<T: Tap> {
    tap: T,
    // ... other fields ...
}

impl<T: Tap> MyTradingEngine<T> {
    pub fn new(tap: T) -> Self {
        Self { tap }
    }

    pub fn process_market_data(&mut self, data: &[u8]) {
        // Record before processing
        self.tap.record_ingress(Exchange::Deribit, data, Timestamp::now());

        // Process data...
    }
}
```

---

## 8. Best Practices

### Recording Best Practices

1. **Record Everything, Filter Later**
   ```rust
   // Good: Record all frames, filter during analysis
   tap.record_ingress(exchange, &frame, timestamp);

   // Bad: Filtering during recording (loses data)
   if frame.is_important() {
       tap.record_ingress(exchange, &frame, timestamp);
   }
   ```

2. **Use Consistent Timestamps**
   ```rust
   // Good: Single timestamp source per event
   let timestamp = clock.now();
   tap.record_ingress(exchange, &frame, timestamp);
   process_frame(&frame, timestamp);

   // Bad: Multiple timestamp sources
   tap.record_ingress(exchange, &frame, Timestamp::now());
   process_frame(&frame, Timestamp::now()); // Different time!
   ```

3. **Checkpoint Regularly**
   ```rust
   // Every 1000 events or every minute
   if event_count % 1000 == 0 || last_checkpoint.elapsed() > Duration::from_secs(60) {
       tap.record_checkpoint(&compute_state_hash(), Timestamp::now());
   }
   ```

4. **Rotate Journal Files Daily**
   ```rust
   let filename = format!("session_{}.journal",
       chrono::Utc::now().format("%Y%m%d_%H%M%S"));
   ```

### Replay Best Practices

1. **Always Verify After Code Changes**
   ```bash
   # After any algorithm change, verify old sessions still replay correctly
   blackbox verify yesterdays_session.journal
   ```

2. **Keep Reference Sessions**
   ```
   sessions/
   ├── reference/           # Known-good sessions for regression
   │   ├── volatile_day.journal
   │   ├── quiet_day.journal
   │   └── edge_cases.journal
   └── daily/               # Regular sessions
       └── ...
   ```

3. **Test Edge Cases**
   - Empty order book
   - Maximum payload sizes
   - Timestamp boundaries
   - High-frequency bursts

### Performance Best Practices

1. **Use NullTap in Production Unless Recording**
   ```rust
   #[cfg(feature = "debug_recording")]
   type ProductionTap = JournalTap;

   #[cfg(not(feature = "debug_recording"))]
   type ProductionTap = NullTap;
   ```

2. **Size Ring Buffer Appropriately**
   ```rust
   // For high-frequency trading (>10k events/sec)
   let config = WriterConfig {
       ring_buffer_size: 16384,  // 16K buffer
       flush_interval_ms: 5,     // 5ms flush
       ..Default::default()
   };
   ```

3. **Pre-allocate Journal Files**
   ```rust
   let config = WriterConfig {
       initial_size: 1024 * 1024 * 1024,  // 1 GB
       ..Default::default()
   };
   ```

---

## 9. Troubleshooting

### Common Issues

#### Journal File Won't Open

**Symptom:** `InvalidMagic` error when opening file

**Causes:**
- File is corrupted
- File is not a BlackBox journal
- File was truncated

**Solution:**
```bash
# Check file header
xxd -l 16 session.journal
# Should start with: 4153 5452 4142 4c4b (BLKBOXJL)
```

#### Schema Hash Mismatch

**Symptom:** `SchemaHashMismatch` error

**Causes:**
- Journal was written with different schema version
- File corruption

**Solution:**
```rust
// Skip schema verification (for analysis only!)
let config = ReaderConfig::skip_schema_hash();
let reader = JournalReader::open_with_config("session.journal", config)?;
```

#### Replay Produces Different Results

**Symptom:** Verification fails, state diverges

**Causes:**
- Non-deterministic code
- Time-dependent logic
- Missing events

**Debug Process:**
1. Find first divergence point
2. Add debug logging around that event
3. Check for `Utc::now()`, `rand()`, `HashMap` iteration
4. Verify all inputs are recorded

#### Recording Slows Down System

**Symptom:** Latency increases when recording enabled

**Causes:**
- Disk I/O blocking
- Ring buffer overflow
- Large payloads

**Solutions:**
```rust
// Increase buffer size
let config = WriterConfig {
    ring_buffer_size: 32768,
    flush_interval_ms: 20,
    sync_on_flush: false,  // Don't fsync on every flush
    ..Default::default()
};
```

#### Out of Disk Space

**Symptom:** Writer enters degraded mode

**Solution:**
```rust
// Monitor writer state
if writer.is_degraded() {
    alert_operator("Journal writer degraded - check disk space");
}

// Trading continues even when recording fails (fail-open)
```

### Error Messages Reference

| Error | Meaning | Action |
|-------|---------|--------|
| `InvalidMagic` | Not a journal file | Check file path |
| `UnsupportedVersion` | Future schema version | Update BlackBox |
| `SchemaHashMismatch` | Schema changed | Use skip_schema_hash |
| `CorruptedRecord` | CRC check failed | Investigate file corruption |
| `RingBufferFull` | Events arriving too fast | Increase buffer size |
| `DiskFull` | No space left | Free disk space |

---

## 10. Performance Tuning

### Recording Performance

#### Latency Optimization

| Scenario | Recommended Config |
|----------|-------------------|
| Low-latency trading | NullTap (no recording) |
| Development | JournalTap, default config |
| High-throughput | Larger ring buffer, less frequent flush |
| Debugging | Smaller buffer, frequent flush |

```rust
// Ultra-low-latency (production, no recording)
let tap = NullTap;  // ~1.2ns overhead

// Balanced (development)
let config = WriterConfig::default();  // ~110ns overhead

// High-throughput (many events)
let config = WriterConfig {
    ring_buffer_size: 65536,
    flush_interval_ms: 50,
    sync_on_flush: false,
    ..Default::default()
};
```

#### Throughput Optimization

```rust
// For >1M events/second
let config = WriterConfig {
    initial_size: 4 * 1024 * 1024 * 1024,  // 4 GB
    ring_buffer_size: 131072,               // 128K buffer
    flush_interval_ms: 100,                 // 100ms batch
    sync_on_flush: false,
};
```

### Replay Performance

#### Warp Configuration

```rust
// Quick analysis (skip idle time)
let config = WarpConfig {
    idle_threshold_us: 10_000,    // 10ms idle = warp
    max_warp_factor: None,        // Unlimited speed
};

// Controlled fast-forward
let config = WarpConfig {
    idle_threshold_us: 100_000,   // 100ms idle = warp
    max_warp_factor: Some(100.0), // Max 100x speed
};

// Real-time replay (no warping)
let config = WarpConfig {
    idle_threshold_us: i64::MAX,  // Never warp
    max_warp_factor: Some(1.0),   // Real-time
};
```

### Memory Optimization

| Component | Memory Usage | Tuning |
|-----------|-------------|--------|
| RingBuffer | ~8KB per 1K capacity | Size for expected burst |
| Journal MMAP | Initial size + growth | Pre-allocate for session |
| Replay frames | ~200 bytes per frame | Stream instead of load all |

---

## 11. Tips and Tricks

### Useful Patterns

#### 1. Session Tagging

Add metadata to identify sessions:

```rust
// Create meaningful session IDs
let session_id = format!("{}_{}_{}",
    strategy_name,
    exchange,
    chrono::Utc::now().format("%Y%m%d%H%M%S")
);
```

#### 2. Event Correlation

Correlate ingress and egress events:

```rust
// Generate correlation ID
let correlation_id = generate_unique_id();

// Record with correlation in payload
let payload = format!("corr:{} data:{}", correlation_id, order_data);
tap.record_egress(exchange, payload.as_bytes(), timestamp);
```

#### 3. Selective Recording

Record only interesting periods:

```rust
// Start recording when volatility spikes
if market.volatility() > threshold && !tap.is_active() {
    tap.activate();
}

// Stop when things calm down
if market.volatility() < threshold / 2 && tap.is_active() {
    tap.deactivate();
}
```

#### 4. Replay Breakpoints

Set breakpoints at specific events:

```rust
fn replay_to_event(engine: &mut ReplayEngine, target_seq: u64) {
    while engine.position() < Some(target_seq as usize) {
        engine.step();
    }
    // Now paused at target event
}
```

#### 5. Differential Analysis

Compare two sessions:

```bash
# Dump both sessions
blackbox dump session_a.journal --format json > a.json
blackbox dump session_b.journal --format json > b.json

# Compare with standard tools
diff a.json b.json
```

### Hidden Features

#### 1. Header Inspection

```rust
let reader = JournalReader::open("session.journal")?;
let header = reader.header();

// Access hidden fields
println!("Internal flags: {:08x}", header.flags);
println!("Reserved bytes: {:?}", &header.reserved[..8]);
```

#### 2. Schema Extraction

```rust
let reader = JournalReader::open("session.journal")?;
let schema_xml = reader.schema();

// Save schema for external tools
std::fs::write("schema.xml", schema_xml)?;
```

#### 3. Manual Record Construction

```rust
use blackbox::journal::{Record, RecordType};

// Build custom record
let record = Record {
    sequence: 0,
    timestamp: Timestamp::now(),
    record_type: RecordType::StateChange,
    exchange: Exchange::Unknown,
    event_type: 0x9999,  // Custom event
    payload: custom_payload.to_vec(),
};

writer.write(record)?;
```

---

## 12. Lessons from the Field

### What We Learned Building BlackBox

#### 1. Zero-Overhead Abstraction is Real

Rust's zero-sized types (ZST) make NullTap truly zero-overhead:
```rust
assert_eq!(std::mem::size_of::<NullTap>(), 0);
// The tap literally doesn't exist in compiled code when disabled
```

#### 2. Lock-Free is Worth the Complexity

Our SPSC ring buffer avoids mutex contention entirely. The extra complexity paid off in predictable latency.

#### 3. Self-Describing Formats Save Future You

Embedding the schema in every journal file was worth the extra bytes. You'll thank yourself years later.

#### 4. Fail-Open is Critical for Trading

When disk I/O fails, trading must continue. BlackBox enters degraded mode but never stops your trading.

#### 5. Test the Tests

Our regression test suite tests that replay produces identical results. This catches non-determinism early.

### Common Mistakes We Made

1. **Using `HashMap` in state computation** - Iteration order isn't deterministic. Use `BTreeMap`.

2. **Calling `Utc::now()` directly** - Always inject the clock. Every. Single. Time.

3. **Forgetting to record before processing** - The event must be recorded BEFORE you act on it.

4. **Not checkpointing enough** - Without checkpoints, you can't verify replay correctness.

5. **Assuming payload size is small** - Some WebSocket frames are huge. Budget for it.

### War Stories

#### The Midnight Bug

A bug only appeared during Asian trading hours. Without BlackBox, we'd never have caught the timezone-dependent code path. Recording let us replay the exact sequence that triggered it.

#### The Missing Microsecond

Two events arrived within the same microsecond, but our timestamp granularity was milliseconds. They were processed in reverse order during replay. Lesson: microsecond precision matters.

#### The Phantom Order

An order appeared in production that our replay couldn't explain. Turns out a webhook callback wasn't being recorded. TAP-3 fixed it.

---

## 13. FAQ

### General Questions

**Q: How much disk space does recording use?**

A: Approximately 200 bytes per event plus payload size. A typical trading day (1M events) uses ~300 MB.

**Q: Can I record in production?**

A: Yes, with JournalTap (~110ns overhead). For ultra-low-latency systems, use NullTap and enable recording only when debugging.

**Q: How long can I keep journal files?**

A: Indefinitely. The self-describing format ensures readability for 10+ years.

**Q: Can I compress old journals?**

A: Yes, use standard compression (gzip, zstd). Decompress before reading with BlackBox.

### Technical Questions

**Q: Why not use `dyn Tap`?**

A: Dynamic dispatch adds ~0.5ns per call. With millions of events, that adds up. Generics give zero overhead.

**Q: Why embedded schema instead of external file?**

A: External files get lost, renamed, or modified. Embedded schemas are guaranteed to match the data.

**Q: Can I have multiple writers to one file?**

A: No. Each writer must have exclusive access. Use separate files for parallel recording.

**Q: What happens if the process crashes?**

A: Journal files are memory-mapped. The OS flushes pending writes. You might lose the last few events before crash.

### Troubleshooting Questions

**Q: Why is my replay slower than expected?**

A: Check if you're in real-time mode. Enable warp-speed for analysis:
```rust
let config = WarpConfig::warp_speed();
```

**Q: Why doesn't verification pass?**

A: Most common causes:
- `HashMap` iteration order
- Floating point accumulation
- `Utc::now()` instead of injected clock
- Missing recorded events

**Q: How do I debug a non-deterministic bug?**

A: Record multiple sessions of the same scenario. Compare them to find where they diverge. The divergence point reveals the non-determinism.

---

## 14. Quick Reference Card

### Essential Commands

```bash
# View journal info
blackbox info session.journal

# Verify replay
blackbox verify session.journal

# Dump first 100 records
blackbox dump session.journal --limit 100

# Get statistics
blackbox stats session.journal --detailed
```

### Essential Code

```rust
// Recording
let writer = JournalWriter::new("session.journal", WriterConfig::default())?;
let tap = JournalTap::new(writer);
tap.record_ingress(exchange, data, timestamp);

// Replay
let reader = JournalReader::open("session.journal")?;
for record in reader {
    process(record?);
}

// Verification
let verifier = VerifyingReplayEngine::new(engine, callback, config);
let report = verifier.run_with_verify();
```

### Performance Targets

| Operation | Target | Actual |
|-----------|--------|--------|
| NullTap | <10ns | ~1.2ns |
| JournalTap | <1μs | ~110ns |
| Clock.now() | <10ns | ~0.5ns |
| Step (replay) | <10μs | ~500ns |

### File Format Quick Reference

```
Header:     128 bytes (starts with "BLKBOXJL")
Schema:     32-byte header + Zstd XML
Records:    32-byte header + variable payload
```

### Feature Flags

```toml
[features]
blackbox = ["dep:blackbox"]  # Enable recording
```

---

## Final Words

BlackBox was built from the frustration of debugging trading bugs without being able to reproduce them. It represents hundreds of hours of design, implementation, and refinement.

Use it well. May your bugs be reproducible and your replays deterministic.

*Happy Trading!*

---

*Handbook Version: 1.0.0*
*Last Updated: 2026-01-07*
*Feedback: Open an issue on GitHub*
