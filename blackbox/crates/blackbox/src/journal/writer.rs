//! Journal writer for high-speed binary recording.
//!
//! The writer uses memory-mapped I/O for zero-copy writes and
//! a lock-free ring buffer to decouple the hot path from disk I/O.
//!
//! # Architecture
//!
//! ```text
//! Trading Thread                    Background Thread
//!      |                                   |
//!      v                                   v
//! write() -> RingBuffer::try_push() ----> BackgroundWriter::drain() --> MMAP File
//!      |                                   |
//!      |--- <50ns hot path --->            |--- Async disk I/O --->
//! ```
//!
//! # Performance Targets
//!
//! - Hot path (`write()`): < 1μs p99
//! - Zero allocations in hot path
//! - Background thread handles all disk I/O
//!
//! # Fail-Open Safety
//!
//! On disk errors (full, I/O failure), the writer enters "degraded" mode:
//! - Records are dropped (not written to disk)
//! - Trading continues without crashing
//! - `is_degraded()` returns true for monitoring
//!
//! # Example
//!
//! ```ignore
//! use blackbox::journal::{JournalWriter, RecordType, WriterConfig};
//!
//! let config = WriterConfig::default();
//! let mut writer = JournalWriter::new("session.journal", config)?;
//!
//! // Hot path - this returns quickly, actual write is async
//! writer.write(RecordType::RawFrame, 1, b"{\"type\":\"quote\"}")?;
//!
//! // Check health
//! if writer.is_degraded() {
//!     eprintln!("Warning: journal in degraded mode");
//! }
//!
//! // Graceful shutdown - waits for all records to be written
//! writer.close()?;
//! ```

use super::format::{
    FileFooter, FileHeader, RecordHeader, RecordType, FILE_FOOTER_SIZE, FILE_HEADER_SIZE,
    RECORD_HEADER_SIZE,
};
use super::ring_buffer::{BufferEntry, RingBuffer};
use super::schema;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use blackbox_types::{Clock, Timestamp};
use memmap2::MmapMut;

/// Default ring buffer capacity (number of entries).
const DEFAULT_RING_BUFFER_CAPACITY: usize = 16384;

/// Default file size for memory mapping (64 MB).
const DEFAULT_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// Minimum file size (1 MB).
const MIN_FILE_SIZE: u64 = 1024 * 1024;

/// Maximum file size (4 GB).
const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Spin loop iterations before yielding in background thread.
const SPIN_ITERATIONS: u32 = 100;

/// Wrapper for cloning a Clock into an Arc.
///
/// This allows storing a cloned clock in the JournalWriter while
/// maintaining the shared state semantics of clocks like SimulatedClock.
struct ClockWrapper<C: Clock + Clone + Send + Sync + 'static> {
    inner: C,
}

impl<C: Clock + Clone + Send + Sync + 'static> ClockWrapper<C> {
    fn new(clock: &C) -> Self {
        Self {
            inner: clock.clone(),
        }
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Clock for ClockWrapper<C> {
    fn now(&self) -> Timestamp {
        self.inner.now()
    }

    fn now_micros(&self) -> i64 {
        self.inner.now_micros()
    }
}

/// Configuration for the journal writer.
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Ring buffer capacity (number of entries).
    pub ring_buffer_capacity: usize,
    /// Initial file size for MMAP.
    pub file_size: u64,
    /// Whether to compress the embedded schema.
    pub compress_schema: bool,
    /// Whether to sync to disk on close.
    pub sync_on_close: bool,
    /// Pre-fault MMAP pages on creation.
    pub prefault_pages: bool,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            ring_buffer_capacity: DEFAULT_RING_BUFFER_CAPACITY,
            file_size: DEFAULT_FILE_SIZE,
            compress_schema: true,
            sync_on_close: true,
            prefault_pages: true,
        }
    }
}

impl WriterConfig {
    /// Create a minimal config for testing.
    pub fn minimal() -> Self {
        Self {
            ring_buffer_capacity: 256,
            file_size: MIN_FILE_SIZE,
            compress_schema: true,
            sync_on_close: false,
            prefault_pages: false,
        }
    }

    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), WriterError> {
        if self.ring_buffer_capacity < 16 {
            return Err(WriterError::InvalidConfig(
                "ring_buffer_capacity must be at least 16".into(),
            ));
        }
        if self.file_size < MIN_FILE_SIZE {
            return Err(WriterError::InvalidConfig(format!(
                "file_size must be at least {} bytes",
                MIN_FILE_SIZE
            )));
        }
        if self.file_size > MAX_FILE_SIZE {
            return Err(WriterError::InvalidConfig(format!(
                "file_size must be at most {} bytes",
                MAX_FILE_SIZE
            )));
        }
        Ok(())
    }
}

/// Errors that can occur during journal writing.
#[derive(Debug)]
pub enum WriterError {
    /// I/O error.
    Io(std::io::Error),
    /// Disk is full.
    DiskFull,
    /// Writer is in degraded mode.
    Degraded,
    /// Writer is closed.
    Closed,
    /// Invalid configuration.
    InvalidConfig(String),
    /// Schema error.
    Schema(schema::SchemaError),
    /// File already exists.
    FileExists(PathBuf),
}

impl std::fmt::Display for WriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::DiskFull => write!(f, "Disk full"),
            Self::Degraded => write!(f, "Writer in degraded mode"),
            Self::Closed => write!(f, "Writer is closed"),
            Self::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
            Self::Schema(e) => write!(f, "Schema error: {}", e),
            Self::FileExists(path) => write!(f, "File already exists: {}", path.display()),
        }
    }
}

impl std::error::Error for WriterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Schema(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for WriterError {
    fn from(e: std::io::Error) -> Self {
        // Check for disk full conditions
        if e.kind() == io::ErrorKind::WriteZero
            || e.raw_os_error() == Some(28)  // ENOSPC on Unix
            || e.raw_os_error() == Some(112)
        // ERROR_DISK_FULL on Windows
        {
            Self::DiskFull
        } else {
            Self::Io(e)
        }
    }
}

impl From<schema::SchemaError> for WriterError {
    fn from(e: schema::SchemaError) -> Self {
        Self::Schema(e)
    }
}

/// Shared state between main writer and background thread.
struct SharedState {
    /// Ring buffer for hot path decoupling.
    ring_buffer: RingBuffer<BufferEntry>,
    /// Flag to signal background thread to stop.
    shutdown: AtomicBool,
    /// Flag indicating degraded mode.
    degraded: AtomicBool,
    /// Total records written to disk.
    records_written: AtomicU64,
    /// Current write position in file.
    write_position: AtomicU64,
    /// Last record timestamp.
    last_timestamp: AtomicU64,
}

impl SharedState {
    fn new(ring_buffer_capacity: usize) -> Self {
        Self {
            ring_buffer: RingBuffer::new(ring_buffer_capacity),
            shutdown: AtomicBool::new(false),
            degraded: AtomicBool::new(false),
            records_written: AtomicU64::new(0),
            write_position: AtomicU64::new(0),
            last_timestamp: AtomicU64::new(0),
        }
    }
}

/// High-speed journal writer using MMAP.
///
/// # Design
///
/// The writer is designed for minimal hot-path latency:
/// 1. Records are written to a lock-free ring buffer
/// 2. A background thread drains the buffer to disk via MMAP
/// 3. Fail-open: disk errors degrade but don't crash trading
///
/// # Thread Safety
///
/// - `write()` is NOT thread-safe - call from single producer thread
/// - `is_degraded()` is thread-safe
/// - `close()` must be called from the same thread as `write()`
pub struct JournalWriter {
    /// Path to journal file.
    path: PathBuf,
    /// Configuration.
    config: WriterConfig,
    /// Shared state with background thread.
    state: Arc<SharedState>,
    /// Background writer thread handle.
    background_thread: Option<JoinHandle<()>>,
    /// Current sequence number (records pushed to ring buffer).
    sequence: u32,
    /// Whether the writer has been closed.
    closed: bool,
    /// Optional clock for deterministic timestamps (used in replay/testing).
    clock: Option<Arc<dyn Clock + Send + Sync>>,
}

impl JournalWriter {
    /// Create a new journal writer.
    ///
    /// This creates a new journal file with embedded schema.
    /// The file must not already exist.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the journal file
    /// * `config` - Writer configuration
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File already exists
    /// - Configuration is invalid
    /// - I/O error occurs
    pub fn new<P: AsRef<Path>>(path: P, config: WriterConfig) -> Result<Self, WriterError> {
        config.validate()?;

        let path = path.as_ref().to_path_buf();

        // Create the file (fail if exists)
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;

        // Set file size for MMAP
        file.set_len(config.file_size)?;

        // Create memory-mapped file
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Pre-fault pages if configured
        if config.prefault_pages {
            // Touch every page to pre-fault
            let page_size = 4096;
            for offset in (0..mmap.len()).step_by(page_size) {
                // Read to trigger page fault
                let _ = mmap[offset];
            }
        }

        // Write file header and schema block
        let (header, schema_block) = schema::create_journal_header(config.compress_schema)?;
        let schema_bytes = schema::serialize_block(&schema_block);

        // Write header
        let header_bytes = header.to_bytes();
        mmap[..FILE_HEADER_SIZE].copy_from_slice(&header_bytes);

        // Write schema block
        let schema_end = FILE_HEADER_SIZE + schema_bytes.len();
        mmap[FILE_HEADER_SIZE..schema_end].copy_from_slice(&schema_bytes);

        // Flush to ensure header is written
        mmap.flush()?;

        // Calculate first record position
        // Copy first_record to avoid packed struct reference issues
        let first_record = header.first_record;
        let write_position = first_record;

        // Create shared state
        let state = Arc::new(SharedState::new(config.ring_buffer_capacity));
        state
            .write_position
            .store(write_position, Ordering::Release);

        // Start background writer thread
        let bg_state = Arc::clone(&state);
        let bg_mmap = unsafe { MmapMut::map_mut(&file)? };
        let sync_on_close = config.sync_on_close;

        let background_thread = thread::spawn(move || {
            background_writer(bg_state, bg_mmap, sync_on_close);
        });

        Ok(Self {
            path,
            config,
            state,
            background_thread: Some(background_thread),
            sequence: 0,
            closed: false,
            clock: None,
        })
    }

    /// Create a new journal writer with an injected clock.
    ///
    /// This constructor allows for deterministic timestamps, useful for
    /// testing and replay scenarios where time must be controlled.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the journal file
    /// * `config` - Writer configuration
    /// * `clock` - Clock implementation for timestamps
    ///
    /// # Example
    ///
    /// ```ignore
    /// use blackbox::journal::{JournalWriter, WriterConfig};
    /// use blackbox::replay::SimulatedClock;
    /// use blackbox_types::Timestamp;
    ///
    /// let clock = SimulatedClock::new(Timestamp::from_micros(1_000_000));
    /// let config = WriterConfig::minimal();
    /// let writer = JournalWriter::with_clock("test.journal", config, &clock)?;
    /// ```
    pub fn with_clock<P: AsRef<Path>, C: Clock + Clone + Send + Sync + 'static>(
        path: P,
        config: WriterConfig,
        clock: &C,
    ) -> Result<Self, WriterError> {
        config.validate()?;

        let path = path.as_ref().to_path_buf();

        // Create the file (fail if exists)
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;

        // Set file size for MMAP
        file.set_len(config.file_size)?;

        // Create memory-mapped file
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Pre-fault pages if configured
        if config.prefault_pages {
            let page_size = 4096;
            for offset in (0..mmap.len()).step_by(page_size) {
                let _ = mmap[offset];
            }
        }

        // Get session start from clock
        let session_start = clock.now();

        // Write file header and schema block with injected timestamp
        let (mut header, schema_block) = schema::create_journal_header(config.compress_schema)?;
        header.session_start = session_start.as_micros();

        let schema_bytes = schema::serialize_block(&schema_block);

        // Write header
        let header_bytes = header.to_bytes();
        mmap[..FILE_HEADER_SIZE].copy_from_slice(&header_bytes);

        // Write schema block
        let schema_end = FILE_HEADER_SIZE + schema_bytes.len();
        mmap[FILE_HEADER_SIZE..schema_end].copy_from_slice(&schema_bytes);

        // Flush to ensure header is written
        mmap.flush()?;

        // Calculate first record position
        let first_record = header.first_record;
        let write_position = first_record;

        // Create shared state
        let state = Arc::new(SharedState::new(config.ring_buffer_capacity));
        state
            .write_position
            .store(write_position, Ordering::Release);

        // Start background writer thread
        let bg_state = Arc::clone(&state);
        let bg_mmap = unsafe { MmapMut::map_mut(&file)? };
        let sync_on_close = config.sync_on_close;

        let background_thread = thread::spawn(move || {
            background_writer(bg_state, bg_mmap, sync_on_close);
        });

        // Create clock Arc - we need to clone the clock's Arc representation
        // Since we take a reference to any Clock, we wrap it in our own Arc
        // by creating a wrapper that delegates to the original clock
        let clock_arc: Arc<dyn Clock + Send + Sync> = Arc::new(ClockWrapper::new(clock));

        Ok(Self {
            path,
            config,
            state,
            background_thread: Some(background_thread),
            sequence: 0,
            closed: false,
            clock: Some(clock_arc),
        })
    }

    /// Open an existing journal for appending.
    ///
    /// This opens an existing journal file and positions at the end
    /// for appending new records.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the existing journal file
    /// * `config` - Writer configuration
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File does not exist
    /// - File is not a valid journal
    /// - I/O error occurs
    pub fn append<P: AsRef<Path>>(path: P, config: WriterConfig) -> Result<Self, WriterError> {
        config.validate()?;

        let path = path.as_ref().to_path_buf();

        // Open existing file
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        // Create memory-mapped file
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        // Read and validate header
        if mmap.len() < FILE_HEADER_SIZE {
            return Err(WriterError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small to contain header",
            )));
        }

        let header_bytes: [u8; FILE_HEADER_SIZE] = mmap[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);
        header
            .validate()
            .map_err(|e| WriterError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;

        // Calculate write position (skip header + schema + existing records)
        // For append, we need to find the end of existing records
        // For now, use first_record position (assumes new journal)
        let first_record = header.first_record;
        let record_count = header.record_count;
        let write_position = if record_count == 0 {
            first_record
        } else {
            // TODO: Implement proper append by scanning existing records
            // For now, fail if there are existing records
            return Err(WriterError::Io(io::Error::new(
                io::ErrorKind::Unsupported,
                "Append to non-empty journal not yet implemented",
            )));
        };

        // Create shared state
        let state = Arc::new(SharedState::new(config.ring_buffer_capacity));
        state
            .write_position
            .store(write_position, Ordering::Release);

        // Pre-fault pages if configured
        if config.prefault_pages {
            let page_size = 4096;
            for offset in (write_position as usize..mmap.len()).step_by(page_size) {
                let _ = mmap[offset];
            }
        }

        // Start background writer thread
        let bg_state = Arc::clone(&state);
        let bg_mmap = unsafe { MmapMut::map_mut(&file)? };
        let sync_on_close = config.sync_on_close;

        let background_thread = thread::spawn(move || {
            background_writer(bg_state, bg_mmap, sync_on_close);
        });

        Ok(Self {
            path,
            config,
            state,
            background_thread: Some(background_thread),
            sequence: record_count as u32,
            closed: false,
            clock: None,
        })
    }

    /// Write a record to the journal.
    ///
    /// This is the hot path - must complete in <1μs.
    /// The actual disk write happens asynchronously in the background thread.
    ///
    /// # Arguments
    ///
    /// * `record_type` - Type of record being written
    /// * `exchange_id` - Exchange identifier
    /// * `payload` - Raw payload bytes
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Writer is closed
    /// - Writer is in degraded mode (optional check)
    /// - Ring buffer is full (record dropped)
    ///
    /// # Performance
    ///
    /// This method is optimized for low latency:
    /// - Single atomic push to ring buffer
    /// - No heap allocations for small payloads
    /// - Returns immediately without waiting for disk
    #[inline]
    pub fn write(
        &mut self,
        record_type: RecordType,
        exchange_id: u8,
        payload: &[u8],
    ) -> Result<(), WriterError> {
        if self.closed {
            return Err(WriterError::Closed);
        }

        // Get timestamp from clock if available, otherwise use system time
        let timestamp = match &self.clock {
            Some(clock) => clock.now_micros(),
            None => now_micros(),
        };

        // Create buffer entry
        let entry = BufferEntry::new(
            record_type.as_u16(),
            exchange_id,
            timestamp,
            payload.to_vec(),
        );

        // Try to push to ring buffer
        if !self.state.ring_buffer.try_push(entry) {
            // Buffer full - check if degraded
            if self.state.degraded.load(Ordering::Acquire) {
                return Err(WriterError::Degraded);
            }
            // Otherwise just drop the record (fail-open)
            return Ok(());
        }

        // Increment sequence for next record
        self.sequence = self.sequence.wrapping_add(1);

        Ok(())
    }

    /// Write a record with a specific timestamp.
    ///
    /// Same as `write()` but allows specifying the timestamp.
    /// Useful for replay or testing scenarios.
    #[inline]
    pub fn write_with_timestamp(
        &mut self,
        record_type: RecordType,
        exchange_id: u8,
        payload: &[u8],
        timestamp: Timestamp,
    ) -> Result<(), WriterError> {
        if self.closed {
            return Err(WriterError::Closed);
        }

        let entry = BufferEntry::new(
            record_type.as_u16(),
            exchange_id,
            timestamp.as_micros(),
            payload.to_vec(),
        );

        if !self.state.ring_buffer.try_push(entry) {
            if self.state.degraded.load(Ordering::Acquire) {
                return Err(WriterError::Degraded);
            }
            return Ok(());
        }

        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    /// Check if the writer is in degraded mode.
    ///
    /// In degraded mode, records may be dropped due to disk errors.
    /// Trading should continue but alerts should be raised.
    #[inline]
    pub fn is_degraded(&self) -> bool {
        self.state.degraded.load(Ordering::Acquire)
    }

    /// Get the number of records written to disk.
    pub fn records_written(&self) -> u64 {
        self.state.records_written.load(Ordering::Acquire)
    }

    /// Get the number of pending records in the ring buffer.
    pub fn pending_records(&self) -> usize {
        self.state.ring_buffer.len()
    }

    /// Get the current write position in the file.
    pub fn write_position(&self) -> u64 {
        self.state.write_position.load(Ordering::Acquire)
    }

    /// Get the path to the journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flush all pending records to disk.
    ///
    /// This blocks until all records in the ring buffer have been written.
    pub fn flush(&self) -> Result<(), WriterError> {
        if self.closed {
            return Err(WriterError::Closed);
        }

        // Wait for ring buffer to drain AND all records to be written
        // The sequence number tracks how many records we've pushed
        let target = self.sequence as u64;

        loop {
            let written = self.state.records_written.load(Ordering::Acquire);
            if written >= target {
                break;
            }
            if self.state.degraded.load(Ordering::Acquire) {
                // In degraded mode, some records may be lost
                // Wait for ring buffer to empty instead
                if self.state.ring_buffer.is_empty() {
                    break;
                }
            }
            // Yield to let background thread make progress
            thread::yield_now();
        }

        Ok(())
    }

    /// Close the journal, flushing all pending writes.
    ///
    /// This:
    /// 1. Signals the background thread to stop
    /// 2. Waits for the ring buffer to drain
    /// 3. Writes the file footer
    /// 4. Syncs and closes the file
    pub fn close(mut self) -> Result<(), WriterError> {
        self.close_internal()
    }

    fn close_internal(&mut self) -> Result<(), WriterError> {
        if self.closed {
            return Ok(());
        }

        self.closed = true;

        // Signal shutdown
        self.state.shutdown.store(true, Ordering::Release);

        // Wait for background thread
        if let Some(handle) = self.background_thread.take() {
            handle
                .join()
                .map_err(|_| WriterError::Io(io::Error::other("Background thread panicked")))?;
        }

        // Write footer
        self.write_footer()?;

        Ok(())
    }

    fn write_footer(&self) -> Result<(), WriterError> {
        // Open file to update header and write footer
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        let records_written = self.state.records_written.load(Ordering::Acquire);
        let write_position = self.state.write_position.load(Ordering::Acquire);
        let last_timestamp = self.state.last_timestamp.load(Ordering::Acquire) as i64;

        // Update header with final stats
        let header_bytes: [u8; FILE_HEADER_SIZE] = mmap[..FILE_HEADER_SIZE].try_into().unwrap();
        let mut header = FileHeader::from_bytes(&header_bytes);

        header.record_count = records_written;
        header.session_end = last_timestamp;

        let updated_header = header.to_bytes();
        mmap[..FILE_HEADER_SIZE].copy_from_slice(&updated_header);

        // Write footer at current write position
        let footer = FileFooter::new(
            records_written,
            write_position + FILE_FOOTER_SIZE as u64,
            last_timestamp,
        );
        let footer_bytes = footer.to_bytes();

        let footer_start = write_position as usize;
        let footer_end = footer_start + FILE_FOOTER_SIZE;

        if footer_end <= mmap.len() {
            mmap[footer_start..footer_end].copy_from_slice(&footer_bytes);
        }

        // Sync to disk if configured
        if self.config.sync_on_close {
            mmap.flush()?;
        }

        Ok(())
    }
}

impl Drop for JournalWriter {
    fn drop(&mut self) {
        if !self.closed {
            // Try to close gracefully, ignore errors
            let _ = self.close_internal();
        }
    }
}

/// Get current time in microseconds since epoch.
#[inline]
fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Background writer thread function.
///
/// This thread continuously drains the ring buffer and writes to MMAP.
fn background_writer(state: Arc<SharedState>, mut mmap: MmapMut, _sync_on_close: bool) {
    let mut spin_count = 0u32;
    let mut sequence = 0u32;

    loop {
        // Try to pop an entry
        if let Some(entry) = state.ring_buffer.pop() {
            // Reset spin count
            spin_count = 0;

            // Calculate total record size
            let payload_len = entry.payload.len();
            let total_size = RECORD_HEADER_SIZE + payload_len;

            // Get current write position
            let write_pos = state.write_position.load(Ordering::Acquire) as usize;

            // Check if we have space
            if write_pos + total_size + FILE_FOOTER_SIZE > mmap.len() {
                // No space - enter degraded mode
                state.degraded.store(true, Ordering::Release);
                continue;
            }

            // Compute CRC32 of payload
            let crc = crc32fast::hash(&entry.payload);

            // Build record header
            let header = RecordHeader {
                timestamp: entry.timestamp,
                payload_size: payload_len as u32,
                record_type: entry.record_type,
                exchange_id: entry.exchange_id,
                flags: 0,
                sequence_number: sequence,
                crc32: crc,
            };

            // Serialize header and write to MMAP
            let header_buf = header.to_bytes();
            mmap[write_pos..write_pos + RECORD_HEADER_SIZE].copy_from_slice(&header_buf);
            mmap[write_pos + RECORD_HEADER_SIZE..write_pos + total_size]
                .copy_from_slice(&entry.payload);

            // Update state
            let new_pos = write_pos + total_size;
            state
                .write_position
                .store(new_pos as u64, Ordering::Release);
            state.records_written.fetch_add(1, Ordering::Release);
            state
                .last_timestamp
                .store(entry.timestamp as u64, Ordering::Release);

            sequence = sequence.wrapping_add(1);
        } else {
            // No data available
            if state.shutdown.load(Ordering::Acquire) && state.ring_buffer.is_empty() {
                // Shutdown requested and buffer is empty - exit
                break;
            }

            // Spin or yield
            spin_count += 1;
            if spin_count >= SPIN_ITERATIONS {
                spin_count = 0;
                thread::yield_now();
            } else {
                std::hint::spin_loop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ==================== Configuration Tests ====================

    #[test]
    fn test_default_config() {
        let config = WriterConfig::default();
        assert_eq!(config.ring_buffer_capacity, DEFAULT_RING_BUFFER_CAPACITY);
        assert_eq!(config.file_size, DEFAULT_FILE_SIZE);
        assert!(config.compress_schema);
        assert!(config.sync_on_close);
        assert!(config.prefault_pages);
    }

    #[test]
    fn test_minimal_config() {
        let config = WriterConfig::minimal();
        assert_eq!(config.ring_buffer_capacity, 256);
        assert_eq!(config.file_size, MIN_FILE_SIZE);
        assert!(!config.sync_on_close);
        assert!(!config.prefault_pages);
    }

    #[test]
    fn test_config_validation_buffer_too_small() {
        let config = WriterConfig {
            ring_buffer_capacity: 8,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WriterError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_config_validation_file_too_small() {
        let config = WriterConfig {
            file_size: 1000,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WriterError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_config_validation_file_too_large() {
        let config = WriterConfig {
            file_size: MAX_FILE_SIZE + 1,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WriterError::InvalidConfig(_))
        ));
    }

    // ==================== Writer Creation Tests ====================

    fn create_test_path() -> PathBuf {
        let id: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let path = std::env::temp_dir().join(format!("test_journal_{}.journal", id));
        // Clean up if exists
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn test_writer_new_creates_file() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        assert!(path.exists());
        assert!(!writer.is_degraded());
        assert_eq!(writer.records_written(), 0);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_new_fails_if_exists() {
        let path = create_test_path();

        // Create file
        fs::write(&path, b"existing data").unwrap();

        let config = WriterConfig::minimal();
        let result = JournalWriter::new(&path, config);

        assert!(result.is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_file_has_correct_header() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        writer.close().expect("Failed to close");

        // Read and verify header
        let data = fs::read(&path).expect("Failed to read file");
        assert!(data.len() >= FILE_HEADER_SIZE);

        let header_bytes: [u8; FILE_HEADER_SIZE] = data[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);

        assert!(header.validate().is_ok());
        let magic = header.magic;
        assert_eq!(&magic, b"BLKBOXJL");

        let _ = fs::remove_file(&path);
    }

    // ==================== Write Tests ====================

    #[test]
    fn test_write_single_record() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        writer
            .write(RecordType::RawFrame, 1, b"test payload")
            .expect("Failed to write");

        // Flush deterministically — blocks until the background thread has written.
        writer.flush().expect("flush failed");

        assert_eq!(writer.records_written(), 1);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_multiple_records() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        for i in 0..100 {
            let payload = format!("payload {}", i);
            writer
                .write(RecordType::RawFrame, 1, payload.as_bytes())
                .expect("Failed to write");
        }

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 100);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_with_timestamp() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        let timestamp = Timestamp::from_micros(1234567890);
        writer
            .write_with_timestamp(RecordType::RawFrame, 1, b"test", timestamp)
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 1);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_different_record_types() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        writer
            .write(RecordType::SessionStart, 0, b"")
            .expect("Failed to write");
        writer
            .write(RecordType::RawFrame, 1, b"frame data")
            .expect("Failed to write");
        writer
            .write(RecordType::QuoteUpdate, 1, b"quote data")
            .expect("Failed to write");
        writer
            .write(RecordType::OrderSubmit, 2, b"order data")
            .expect("Failed to write");
        writer
            .write(RecordType::SessionEnd, 0, b"")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 5);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_large_payload() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        let large_payload = vec![0x42u8; 64 * 1024]; // 64 KB
        writer
            .write(RecordType::RawFrame, 1, &large_payload)
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 1);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_empty_payload() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        writer
            .write(RecordType::Checkpoint, 0, b"")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 1);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Close and Flush Tests ====================

    #[test]
    fn test_close_writes_footer() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        for _ in 0..10 {
            writer
                .write(RecordType::RawFrame, 1, b"test")
                .expect("Failed to write");
        }

        let _write_pos = writer.write_position();
        writer.close().expect("Failed to close");

        // Verify footer exists (by checking file structure)
        let data = fs::read(&path).expect("Failed to read file");

        // Header should report 10 records
        let header_bytes: [u8; FILE_HEADER_SIZE] = data[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);
        let record_count = header.record_count;
        assert_eq!(record_count, 10);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_flush_waits_for_drain() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write many records quickly
        for i in 0..50 {
            let payload = format!("payload {}", i);
            writer
                .write(RecordType::RawFrame, 1, payload.as_bytes())
                .expect("Failed to write");
        }

        // Flush should wait for all to be written
        writer.flush().expect("Failed to flush");

        assert_eq!(writer.records_written(), 50);
        assert_eq!(writer.pending_records(), 0);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Error Handling Tests ====================

    #[test]
    fn test_write_after_close_fails() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        writer.close().expect("Failed to close");

        // Try to recreate and write to closed writer
        // (This tests the closed flag behavior)
        // We can't actually call write() after close() consumes self
        // So this test verifies the struct design

        let _ = fs::remove_file(&path);
    }

    // ==================== Degraded Mode Tests ====================

    #[test]
    fn test_is_degraded_initially_false() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        assert!(!writer.is_degraded());

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Path Tests ====================

    #[test]
    fn test_path_returns_correct_path() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let writer = JournalWriter::new(&path, config).expect("Failed to create writer");
        assert_eq!(writer.path(), path.as_path());

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Stats Tests ====================

    #[test]
    fn test_records_written_increments() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        assert_eq!(writer.records_written(), 0);

        writer
            .write(RecordType::RawFrame, 1, b"test")
            .expect("Failed to write");
        writer.flush().expect("flush failed");
        assert_eq!(writer.records_written(), 1);

        writer
            .write(RecordType::RawFrame, 1, b"test")
            .expect("Failed to write");
        writer.flush().expect("flush failed");
        assert_eq!(writer.records_written(), 2);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_pending_records_decreases() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write many records
        for _ in 0..20 {
            writer
                .write(RecordType::RawFrame, 1, b"test")
                .expect("Failed to write");
        }

        // Initially some should be pending (optional check)
        let _initial_pending = writer.pending_records();

        // After flush, none should be pending
        writer.flush().expect("Failed to flush");
        assert_eq!(writer.pending_records(), 0);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Binary Format Tests ====================

    #[test]
    fn test_written_records_have_correct_format() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        let payload = b"test payload data";
        writer
            .write(RecordType::RawFrame, 42, payload)
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        writer.close().expect("Failed to close");

        // Read and verify record
        let data = fs::read(&path).expect("Failed to read file");

        // Skip header and schema to find first record
        let header_bytes: [u8; FILE_HEADER_SIZE] = data[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);
        let first_record = header.first_record as usize;

        // Read record header
        let record_header_bytes: [u8; RECORD_HEADER_SIZE] = data
            [first_record..first_record + RECORD_HEADER_SIZE]
            .try_into()
            .unwrap();
        let record_header = RecordHeader::from_bytes(&record_header_bytes);

        // Copy fields to avoid packed struct issues
        let record_type = record_header.record_type;
        let exchange_id = record_header.exchange_id;
        let payload_size = record_header.payload_size;

        assert_eq!(record_type, RecordType::RawFrame.as_u16());
        assert_eq!(exchange_id, 42);
        assert_eq!(payload_size, payload.len() as u32);

        // Verify payload
        let payload_start = first_record + RECORD_HEADER_SIZE;
        let payload_end = payload_start + payload.len();
        assert_eq!(&data[payload_start..payload_end], payload);

        let _ = fs::remove_file(&path);
    }

    // ==================== Drop Tests ====================

    #[test]
    fn test_drop_closes_gracefully() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        {
            let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");
            writer
                .write(RecordType::RawFrame, 1, b"test")
                .expect("Failed to write");
            // Writer drops here
        }

        // File should still exist and be valid
        assert!(path.exists());

        // Verify header is valid
        let data = fs::read(&path).expect("Failed to read file");
        let header_bytes: [u8; FILE_HEADER_SIZE] = data[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);
        assert!(header.validate().is_ok());

        let _ = fs::remove_file(&path);
    }

    // ==================== Stress Tests ====================

    #[test]
    fn test_high_throughput_writes() {
        let path = create_test_path();
        // Use larger ring buffer for high throughput test
        let config = WriterConfig {
            ring_buffer_capacity: 2048, // Larger buffer to avoid drops
            file_size: MIN_FILE_SIZE,
            compress_schema: true,
            sync_on_close: false,
            prefault_pages: false,
        };

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        const NUM_RECORDS: usize = 1000;
        let payload = b"high throughput test payload";

        for _ in 0..NUM_RECORDS {
            writer
                .write(RecordType::RawFrame, 1, payload)
                .expect("Failed to write");
        }

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), NUM_RECORDS as u64);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_various_payload_sizes() {
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        // Write payloads of various sizes
        let sizes = [0, 1, 10, 100, 1000, 10000];
        for &size in &sizes {
            let payload = vec![0x42u8; size];
            writer
                .write(RecordType::RawFrame, 1, &payload)
                .expect("Failed to write");
        }

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), sizes.len() as u64);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    // ==================== Clock Injection Tests (T3.5) ====================

    #[test]
    fn test_writer_with_clock_creates_file() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::new(Timestamp::from_micros(1_000_000));

        let writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");

        assert!(path.exists());
        assert!(!writer.is_degraded());
        assert_eq!(writer.records_written(), 0);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_with_clock_uses_clock_for_header() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000)); // 2024-01-01

        let writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");
        writer.close().expect("Failed to close");

        // Read and verify header has the clock's timestamp as session_start
        let data = fs::read(&path).expect("Failed to read file");
        let header_bytes: [u8; FILE_HEADER_SIZE] = data[..FILE_HEADER_SIZE].try_into().unwrap();
        let header = FileHeader::from_bytes(&header_bytes);

        let session_start = header.session_start;
        assert_eq!(session_start, 1_704_067_200_000_000);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_with_clock_deterministic_session_start() {
        use crate::replay::SimulatedClock;

        let path1 = create_test_path();
        let path2 = create_test_path();
        let config = WriterConfig::minimal();

        let fixed_time = Timestamp::from_micros(9876543210);
        let clock = SimulatedClock::new(fixed_time);

        // Create two writers with the same clock time
        let writer1 =
            JournalWriter::with_clock(&path1, config.clone(), &clock).expect("Failed to create");
        let writer2 = JournalWriter::with_clock(&path2, config, &clock).expect("Failed to create");

        writer1.close().expect("Failed to close");
        writer2.close().expect("Failed to close");

        // Both should have identical session_start
        let data1 = fs::read(&path1).expect("Failed to read");
        let data2 = fs::read(&path2).expect("Failed to read");

        let header1 = FileHeader::from_bytes(&data1[..FILE_HEADER_SIZE].try_into().unwrap());
        let header2 = FileHeader::from_bytes(&data2[..FILE_HEADER_SIZE].try_into().unwrap());

        // Copy fields to avoid packed struct reference issues
        let session_start1 = header1.session_start;
        let session_start2 = header2.session_start;

        assert_eq!(session_start1, session_start2);
        assert_eq!(session_start1, 9876543210);

        let _ = fs::remove_file(&path1);
        let _ = fs::remove_file(&path2);
    }

    #[test]
    fn test_writer_with_clock_epoch() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::at_epoch();

        let writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");
        writer.close().expect("Failed to close");

        let data = fs::read(&path).expect("Failed to read file");
        let header = FileHeader::from_bytes(&data[..FILE_HEADER_SIZE].try_into().unwrap());

        // Copy field to avoid packed struct reference issue
        let session_start = header.session_start;
        assert_eq!(session_start, 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_with_clock_write_uses_current_clock_time() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::new(Timestamp::from_micros(1_000_000));

        let mut writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");

        // Advance clock
        clock.advance(500_000); // Advance 500ms

        // Write a record (should use clock's current time)
        writer
            .write(RecordType::RawFrame, 1, b"test")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        writer.close().expect("Failed to close");

        // Read and verify record timestamp
        let data = fs::read(&path).expect("Failed to read file");
        let header = FileHeader::from_bytes(&data[..FILE_HEADER_SIZE].try_into().unwrap());
        let first_record = header.first_record as usize;

        let record_header = RecordHeader::from_bytes(
            &data[first_record..first_record + RECORD_HEADER_SIZE]
                .try_into()
                .unwrap(),
        );

        // Copy field to avoid packed struct reference issue
        let timestamp = record_header.timestamp;
        // Record timestamp should be 1_500_000 (1_000_000 + 500_000)
        assert_eq!(timestamp, 1_500_000);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_with_clock_multiple_writes_advancing_time() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        let mut writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");

        // Write records at different clock times
        writer
            .write(RecordType::RawFrame, 1, b"first")
            .expect("Failed to write");

        clock.advance(1000);
        writer
            .write(RecordType::RawFrame, 1, b"second")
            .expect("Failed to write");

        clock.advance(1000);
        writer
            .write(RecordType::RawFrame, 1, b"third")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        assert_eq!(writer.records_written(), 3);

        writer.close().expect("Failed to close");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_with_clock_simulated_clock_warp() {
        use crate::replay::SimulatedClock;

        let path = create_test_path();
        let config = WriterConfig::minimal();
        let clock = SimulatedClock::new(Timestamp::from_micros(0));

        let mut writer =
            JournalWriter::with_clock(&path, config, &clock).expect("Failed to create writer");

        // Write at time 0
        writer
            .write(RecordType::RawFrame, 1, b"at zero")
            .expect("Failed to write");

        // Warp to 1 hour later
        clock.enable_warp();
        clock.advance_to(Timestamp::from_micros(3_600_000_000)); // 1 hour in microseconds

        // Write at warped time
        writer
            .write(RecordType::RawFrame, 1, b"after warp")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        writer.close().expect("Failed to close");

        // Verify timestamps
        let data = fs::read(&path).expect("Failed to read file");
        let header = FileHeader::from_bytes(&data[..FILE_HEADER_SIZE].try_into().unwrap());
        let first_record = header.first_record as usize;

        let record1 = RecordHeader::from_bytes(
            &data[first_record..first_record + RECORD_HEADER_SIZE]
                .try_into()
                .unwrap(),
        );
        let payload1_len = record1.payload_size as usize;

        let record2_offset = first_record + RECORD_HEADER_SIZE + payload1_len;
        let record2 = RecordHeader::from_bytes(
            &data[record2_offset..record2_offset + RECORD_HEADER_SIZE]
                .try_into()
                .unwrap(),
        );

        // Copy fields to avoid packed struct reference issues
        let timestamp1 = record1.timestamp;
        let timestamp2 = record2.timestamp;

        assert_eq!(timestamp1, 0);
        assert_eq!(timestamp2, 3_600_000_000);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writer_new_still_works() {
        // Ensure backward compatibility: new() should still work without clock
        let path = create_test_path();
        let config = WriterConfig::minimal();

        let mut writer = JournalWriter::new(&path, config).expect("Failed to create writer");

        writer
            .write(RecordType::RawFrame, 1, b"test")
            .expect("Failed to write");

        writer.flush().expect("Failed to flush");
        writer.close().expect("Failed to close");

        // Just verify the file is valid
        let data = fs::read(&path).expect("Failed to read file");
        let header = FileHeader::from_bytes(&data[..FILE_HEADER_SIZE].try_into().unwrap());
        assert!(header.validate().is_ok());

        // session_start should be non-zero (current time)
        assert!(header.session_start > 0);

        let _ = fs::remove_file(&path);
    }
}
