# BlackBox API Reference

**Version:** 1.0.0
**Crate:** `blackbox`

---

## Table of Contents

1. [Overview](#1-overview)
2. [Journal Module](#2-journal-module)
3. [Tap Module](#3-tap-module)
4. [Replay Module](#4-replay-module)
5. [Verify Module](#5-verify-module)
6. [Codec Module](#6-codec-module)
7. [CLI Module](#7-cli-module)
8. [Types (blackbox-types)](#8-types-blackbox-types)

---

## 1. Overview

### Crate Structure

```
blackbox/
├── journal/     # Binary journaling (write/read)
├── tap/         # Instrumentation (record events)
├── replay/      # Replay engine (playback control)
├── verify/      # Verification (state hashing)
├── codec/       # Encoding/decoding (versioned)
└── cli/         # Command-line interface
```

### Quick Import

```rust
use blackbox::{
    journal::{JournalWriter, JournalReader, WriterConfig, ReaderConfig},
    tap::{Tap, NullTap, JournalTap},
    replay::{ReplayEngine, ReplayMode, BufferedDataSource, WarpConfig},
    verify::{StateHash, Checkpoint, ComparisonReport},
};
use blackbox_types::{Clock, Exchange, Instrument, Side, Timestamp};
```

---

## 2. Journal Module

### JournalWriter

High-speed binary writer using memory-mapped files.

```rust
pub struct JournalWriter { /* private */ }

impl JournalWriter {
    /// Create a new journal file
    pub fn new<P: AsRef<Path>>(path: P, config: WriterConfig) -> Result<Self, WriterError>;

    /// Create with default configuration
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, WriterError>;

    /// Write a record to the journal
    pub fn write(&mut self, record: &Record) -> Result<(), WriterError>;

    /// Flush buffered writes to disk
    pub fn flush(&mut self) -> Result<(), WriterError>;

    /// Check if writer is in degraded mode (disk issues)
    pub fn is_degraded(&self) -> bool;

    /// Get current file size
    pub fn file_size(&self) -> u64;
}
```

### WriterConfig

```rust
pub struct WriterConfig {
    /// Ring buffer capacity (default: 65536)
    pub ring_buffer_capacity: usize,

    /// Initial file size in bytes (default: 64MB)
    pub file_size: u64,

    /// Compress embedded schema (default: true)
    pub compress_schema: bool,

    /// Sync to disk on close (default: true)
    pub sync_on_close: bool,

    /// Pre-fault pages for consistent latency (default: true)
    pub prefault_pages: bool,
}

impl Default for WriterConfig { /* ... */ }
```

### JournalReader

Sequential record iterator for journal files.

```rust
pub struct JournalReader { /* private */ }

impl JournalReader {
    /// Open a journal file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ReaderError>;

    /// Open with custom configuration
    pub fn open_with_config<P: AsRef<Path>>(path: P, config: ReaderConfig) -> Result<Self, ReaderError>;

    /// Get format version (major, minor)
    pub fn format_version(&self) -> (u8, u8);

    /// Get schema version (major, minor)
    pub fn schema_version(&self) -> (u8, u8);

    /// Get session start timestamp (microseconds)
    pub fn session_start(&self) -> i64;

    /// Get session end timestamp (microseconds)
    pub fn session_end(&self) -> i64;

    /// Get total record count
    pub fn record_count(&self) -> u64;

    /// Get embedded schema XML
    pub fn schema_xml(&self) -> &str;
}

impl Iterator for JournalReader {
    type Item = Result<Record, ReaderError>;
}
```

### Record

```rust
pub struct Record {
    /// Payload bytes
    pub payload: Vec<u8>,
    /* private header fields */
}

impl Record {
    /// Get record timestamp (microseconds)
    pub fn timestamp(&self) -> i64;

    /// Get record type
    pub fn record_type(&self) -> RecordType;

    /// Get exchange ID
    pub fn exchange_id(&self) -> u8;
}
```

### RecordType

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    RawFrame,      // Raw WebSocket frame
    BookSnapshot,  // Order book snapshot
    BookDelta,     // Order book delta
    Trade,         // Trade execution
    OrderSubmit,   // Order submission
    OrderAck,      // Order acknowledgment
    Checkpoint,    // State checkpoint
    Custom(u8),    // User-defined
}
```

---

## 3. Tap Module

### Tap Trait

```rust
pub trait Tap: Send + Sync {
    /// Record ingress (incoming market data)
    fn record_ingress(&self, exchange: Exchange, data: &[u8], timestamp: Timestamp);

    /// Record internal state change
    fn record_internal(&self, event_type: u16, data: &[u8], timestamp: Timestamp);

    /// Record egress (outgoing orders)
    fn record_egress(&self, exchange: Exchange, data: &[u8], timestamp: Timestamp);

    /// Record state checkpoint
    fn record_checkpoint(&self, hash: &[u8; 32], timestamp: Timestamp);

    /// Check if tap is active (recording)
    fn is_active(&self) -> bool;
}
```

### NullTap

Zero-overhead no-op implementation.

```rust
/// Zero-sized type - compiles to nothing
pub struct NullTap;

impl Tap for NullTap {
    // All methods are inline no-ops
    #[inline(always)]
    fn record_ingress(&self, _: Exchange, _: &[u8], _: Timestamp) {}
    // ...
}
```

**Performance:** ~1.2 nanoseconds (effectively zero)

### JournalTap

Recording implementation backed by JournalWriter.

```rust
pub struct JournalTap { /* private */ }

impl JournalTap {
    /// Create a new recording tap
    pub fn new(writer: JournalWriter) -> Self;

    /// Get reference to underlying writer
    pub fn writer(&self) -> &JournalWriter;

    /// Get mutable reference to writer
    pub fn writer_mut(&mut self) -> &mut JournalWriter;
}

impl Tap for JournalTap { /* ... */ }
```

**Performance:** ~110 nanoseconds per record

---

## 4. Replay Module

### ReplayEngine

Orchestrates replay with clock control and warp capability.

```rust
pub struct ReplayEngine<D: DataSource = NullDataSource> { /* private */ }

impl<D: DataSource> ReplayEngine<D> {
    /// Create with data source and warp config
    pub fn with_data_source(data_source: D, config: WarpConfig) -> Self;

    /// Create with specific start time
    pub fn with_start_time(data_source: D, config: WarpConfig, start: Timestamp) -> Self;

    /// Get reference to simulated clock
    pub fn clock(&self) -> &Arc<SimulatedClock>;

    /// Get replay statistics
    pub fn stats(&self) -> ReplayStats;

    /// Set replay mode
    pub fn set_mode(&mut self, mode: ReplayMode);

    /// Get current mode
    pub fn mode(&self) -> ReplayMode;

    /// Get current state
    pub fn state(&self) -> ReplayState;

    /// Start/resume playback
    pub fn play(&mut self);

    /// Pause playback
    pub fn pause(&mut self);

    /// Toggle play/pause
    pub fn toggle(&mut self);

    /// Step to next event
    pub fn step(&mut self) -> StepResult;

    /// Step until event processed
    pub fn step_until_event(&mut self) -> StepResult;

    /// Process multiple events
    pub fn tick(&mut self, max_events: usize) -> usize;

    /// Process all remaining events
    pub fn run_to_completion(&mut self) -> usize;

    /// Reset to beginning
    pub fn reset(&mut self);

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self) -> Option<f64>;

    /// Check if playing
    pub fn is_playing(&self) -> bool;

    /// Check if completed
    pub fn is_completed(&self) -> bool;
}
```

### ReplayMode

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayMode {
    /// Step through events manually
    Step,

    /// Real-time playback (1x speed)
    RealTime,

    /// Fast-forward at Nx speed
    FastForward { multiplier: u32 },

    /// Skip idle periods (fastest)
    WarpSpeed,
}
```

### ReplayState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayState {
    Idle,       // Not started
    Playing,    // Active playback
    Paused,     // Paused
    Completed,  // All events processed
}
```

### StepResult

```rust
pub struct StepResult {
    /// Whether an event was processed
    pub processed: bool,

    /// The frame that was processed
    pub frame: Option<DataFrame>,

    /// Schedule result from warp scheduler
    pub schedule: Option<ScheduleResult>,

    /// Whether replay is complete
    pub completed: bool,
}
```

### WarpConfig

```rust
pub struct WarpConfig {
    /// Idle threshold for warp (microseconds)
    pub idle_threshold_us: i64,

    /// Maximum warp factor (None = unlimited)
    pub max_warp_factor: Option<f64>,
}

impl WarpConfig {
    pub fn default() -> Self;
    pub fn warp_speed() -> Self;
    pub fn fast_forward(factor: f64) -> Self;
    pub fn real_time() -> Self;
}
```

### DataSource Trait

```rust
pub trait DataSource: Send {
    /// Check if source has more data
    fn has_next(&self) -> bool;

    /// Check if source is active
    fn is_active(&self) -> bool;

    /// Peek at next timestamp
    fn peek_timestamp(&self) -> Option<Timestamp>;

    /// Get next frame
    fn next(&mut self) -> Option<DataFrame>;

    /// Reset to beginning
    fn reset(&mut self);

    /// Get current position
    fn position(&self) -> Option<usize>;

    /// Get total frame count
    fn frame_count(&self) -> Option<usize>;
}
```

### BufferedDataSource

```rust
pub struct BufferedDataSource { /* private */ }

impl BufferedDataSource {
    pub fn new(frames: Vec<DataFrame>) -> Self;
    pub fn empty() -> Self;
    pub fn from_reader(reader: JournalReader) -> Self;
}

impl DataSource for BufferedDataSource { /* ... */ }
```

### SimulatedClock

```rust
pub struct SimulatedClock { /* private */ }

impl SimulatedClock {
    pub fn new(start: Timestamp) -> Self;
    pub fn at_epoch() -> Self;

    pub fn set(&self, time: Timestamp);
    pub fn advance(&self, micros: i64);

    pub fn pause(&self);
    pub fn resume(&self);
    pub fn is_paused(&self) -> bool;

    pub fn enable_warp(&self);
    pub fn disable_warp(&self);
    pub fn is_warp_enabled(&self) -> bool;
}

impl Clock for SimulatedClock {
    fn now(&self) -> Timestamp;
}
```

---

## 5. Verify Module

### StateHash

Deterministic state hasher using SHA-256.

```rust
pub struct StateHash { /* private */ }

impl StateHash {
    pub fn new() -> Self;

    /// Update with order book data
    pub fn update_orderbook(&mut self, data: &[u8]);

    /// Update with raw bytes
    pub fn update_raw(&mut self, data: &[u8]);

    /// Update with sequence number
    pub fn update_sequence(&mut self, seq: u64);

    /// Finalize and return hash
    pub fn finalize(self) -> [u8; 32];
}
```

### Hashable Trait

```rust
pub trait Hashable {
    fn hash_into(&self, hasher: &mut StateHash);
}
```

### Checkpoint

```rust
pub struct Checkpoint {
    pub sequence: u64,
    pub timestamp: i64,
    pub state_hash: [u8; 32],
}
```

### CheckpointBuilder

```rust
pub struct CheckpointBuilder { /* private */ }

impl CheckpointBuilder {
    pub fn new() -> Self;
    pub fn sequence(self, seq: u64) -> Self;
    pub fn timestamp(self, ts: i64) -> Self;
    pub fn hash(self, hash: [u8; 32]) -> Self;
    pub fn build(self) -> Checkpoint;
}
```

### VerificationCallback

```rust
pub trait VerificationCallback: Send {
    /// Compute current state hash
    fn compute_state_hash(&self) -> [u8; 32];
}
```

### VerifyingReplayEngine

```rust
pub struct VerifyingReplayEngine<D: DataSource, C: VerificationCallback> { /* private */ }

impl<D: DataSource, C: VerificationCallback> VerifyingReplayEngine<D, C> {
    pub fn new(engine: ReplayEngine<D>, callback: C, config: VerifyConfig) -> Self;

    /// Run replay with verification
    pub fn run_with_verification(&mut self) -> VerificationStats;

    /// Get verification results
    pub fn results(&self) -> &[VerificationResult];
}
```

### VerificationStats

```rust
pub struct VerificationStats {
    pub events_processed: u64,
    pub checkpoints_found: u64,
    pub checkpoints_matched: u64,
    pub checkpoints_mismatched: u64,
    pub errors: u64,
    pub first_mismatch_sequence: Option<u64>,
}
```

### ComparisonReport

```rust
pub struct ComparisonReport { /* private */ }

impl ComparisonReport {
    pub fn from_stats(stats: &VerificationStats, results: &[VerificationResult]) -> Self;

    /// Check if verification passed
    pub fn is_pass(&self) -> bool;

    /// Get match rate (0.0 to 1.0)
    pub fn match_rate(&self) -> f64;

    /// Add recommendation
    pub fn add_recommendation(&mut self, msg: &str);

    /// Format as text
    pub fn to_text(&self) -> String;

    /// Format as JSON
    pub fn to_json(&self) -> String;

    /// Format as summary
    pub fn to_summary(&self) -> String;
}
```

---

## 6. Codec Module

### CodecRegistry

```rust
pub struct CodecRegistry { /* private */ }

impl CodecRegistry {
    pub fn new() -> Self;
    pub fn register<C: Codec>(&mut self, codec: C);
    pub fn get(&self, version: (u8, u8)) -> Option<&dyn Codec>;
}
```

### Encoder/Decoder Traits

```rust
pub trait Encoder {
    fn encode(&self, data: &RecordData, buf: &mut Vec<u8>) -> Result<(), EncodeError>;
}

pub trait Decoder {
    fn decode(&self, buf: &[u8]) -> Result<RecordData, DecodeError>;
}
```

---

## 7. CLI Module

### Commands

```rust
pub enum Commands {
    /// Display journal info
    Info { journal: PathBuf, schema: bool },

    /// Verify replay
    Verify {
        journal: PathBuf,
        format: OutputFormat,
        output: Option<PathBuf>,
        stop_on_mismatch: bool,
    },

    /// Dump records
    Dump {
        journal: PathBuf,
        limit: Option<usize>,
        record_type: Option<String>,
        format: OutputFormat,
    },

    /// Show statistics
    Stats { journal: PathBuf, detailed: bool },
}
```

### CLI Functions

```rust
pub fn execute_info(journal: &PathBuf, show_schema: bool) -> CliResult<JournalInfo>;
pub fn execute_verify(journal: &PathBuf) -> CliResult<ComparisonReport>;
pub fn execute_dump(journal: &PathBuf, limit: Option<usize>, filter: Option<&str>) -> CliResult<Vec<DumpedRecord>>;
pub fn execute_stats(journal: &PathBuf) -> CliResult<JournalStats>;
```

---

## 8. Types (blackbox-types)

### Timestamp

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const EPOCH: Timestamp = Timestamp(0);

    pub fn from_micros(micros: i64) -> Self;
    pub fn now() -> Self;
    pub fn as_micros(&self) -> i64;
}
```

### Exchange

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Unknown = 0,
    Deribit = 1,
    Binance = 2,
    Bybit = 3,
    Kraken = 4,
    // ...
}
```

### Clock Trait

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}
```

### SystemClock

```rust
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
```

---

## Error Types

### WriterError

```rust
pub enum WriterError {
    Io(std::io::Error),
    BufferFull,
    DiskFull,
    Closed,
}
```

### ReaderError

```rust
pub enum ReaderError {
    Io(std::io::Error),
    InvalidMagic,
    UnsupportedVersion,
    SchemaHashMismatch,
    CorruptedRecord,
}
```

### CliError

```rust
pub enum CliError {
    FileNotFound(PathBuf),
    ReaderError(ReaderError),
    Io(std::io::Error),
    InvalidArgument(String),
}
```

---

## Performance Specifications

| Operation | Target | Measured |
|-----------|--------|----------|
| NullTap.record_* | <10ns | ~1.2ns |
| JournalTap.record_* | <1μs | ~110ns |
| JournalWriter.write | <1μs | ~500ns |
| ReplayEngine.step | <10μs | ~5μs |
| Clock.now | <10ns | ~0.5ns |

---

## Feature Flags

```toml
[features]
default = []
# No optional features currently defined
```

---

## See Also

- **User Handbook**: [`USER_HANDBOOK.md`](USER_HANDBOOK.md)
- **Troubleshooting**: [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md)
- **Full Documentation**: `cargo doc --package blackbox --open`

---

*BlackBox v1.0.0 API Reference*
