//! DataSource trait for abstract data injection.
//!
//! This module provides the core abstraction for market data sources,
//! enabling hot-swap between live WebSocket feeds and journal replay.
//!
//! ## Design Principles
//!
//! | Principle | Implementation |
//! |-----------|----------------|
//! | Zero allocation | References over owned data where possible |
//! | Thread-safe | Requires `Send + Sync` bounds |
//! | Generic | Use with `<D: DataSource>`, not `dyn` |
//! | Inlineable | `#[inline]` on hot-path methods |
//!
//! ## Performance Contract
//!
//! | Method | Target Latency | Notes |
//! |--------|----------------|-------|
//! | `peek()` | <100ns | Read-only, no state change |
//! | `next()` | <1μs | May involve buffer management |
//! | `is_active()` | <10ns | Simple state check |
//! | `has_next()` | <100ns | Peek-based check |
//!
//! ## Usage Pattern
//!
//! Use generics for zero-overhead data source selection:
//!
//! ```rust
//! use blackbox::replay::{DataSource, NullDataSource};
//!
//! fn process_data<D: DataSource>(source: &mut D) {
//!     while let Some(frame) = source.next() {
//!         // Process frame...
//!         println!("Frame at {}", frame.timestamp.as_micros());
//!     }
//! }
//!
//! let mut source = NullDataSource;
//! process_data(&mut source); // No frames, returns immediately
//! ```
//!
//! ## Implementations
//!
//! - `NullDataSource`: Zero-sized no-op (for testing/disabled mode)
//! - `JournalDataSource`: Reads from journal files (Phase 3)
//! - `LiveDataSource`: WebSocket feed wrapper (Phase 3)

use blackbox_types::{Exchange, Timestamp};
use crossbeam::channel::{self, Receiver, Sender, TryRecvError};
use parking_lot::Mutex;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// A frame of market data from a data source.
///
/// This struct represents a single unit of data that flows through
/// the trading system, whether from live WebSocket or journal replay.
///
/// # Memory Layout
///
/// The struct is designed for efficient copying and comparison:
/// - Fixed-size header fields (timestamp, exchange, frame_type)
/// - Variable payload as `Vec<u8>` for owned data
///
/// # Example
///
/// ```
/// use blackbox::replay::{DataFrame, FrameType};
/// use blackbox_types::{Exchange, Timestamp};
///
/// let frame = DataFrame {
///     timestamp: Timestamp::from_micros(1_704_067_200_000_000),
///     exchange: Exchange::Deribit,
///     frame_type: FrameType::WebSocketText,
///     payload: b"test data".to_vec(),
/// };
///
/// assert_eq!(frame.exchange, Exchange::Deribit);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    /// Timestamp when this frame was received/recorded.
    pub timestamp: Timestamp,
    /// The exchange this data came from.
    pub exchange: Exchange,
    /// Type of frame (WebSocket text/binary, book update, etc.)
    pub frame_type: FrameType,
    /// The raw payload data.
    pub payload: Vec<u8>,
}

impl DataFrame {
    /// Create a new DataFrame.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - When the frame was received
    /// * `exchange` - Source exchange
    /// * `frame_type` - Type of data
    /// * `payload` - Raw data bytes
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{DataFrame, FrameType};
    /// use blackbox_types::{Exchange, Timestamp};
    ///
    /// let frame = DataFrame::new(
    ///     Timestamp::from_micros(1000),
    ///     Exchange::Binance,
    ///     FrameType::WebSocketBinary,
    ///     vec![1, 2, 3, 4],
    /// );
    /// ```
    pub fn new(
        timestamp: Timestamp,
        exchange: Exchange,
        frame_type: FrameType,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            timestamp,
            exchange,
            frame_type,
            payload,
        }
    }

    /// Create a DataFrame with empty payload.
    #[inline]
    pub fn empty(timestamp: Timestamp, exchange: Exchange, frame_type: FrameType) -> Self {
        Self {
            timestamp,
            exchange,
            frame_type,
            payload: Vec::new(),
        }
    }

    /// Get the size of the payload in bytes.
    #[inline]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Check if the payload is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

/// Type of data frame.
///
/// Categorizes the type of market data for appropriate handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    /// WebSocket text frame (JSON).
    WebSocketText = 0x01,
    /// WebSocket binary frame.
    WebSocketBinary = 0x02,
    /// Order book snapshot.
    BookSnapshot = 0x10,
    /// Order book delta/update.
    BookDelta = 0x11,
    /// Trade execution.
    Trade = 0x12,
    /// Checkpoint record for state verification.
    Checkpoint = 0x03,
    /// Exchange heartbeat.
    Heartbeat = 0xF0,
    /// Unknown/other frame type.
    Unknown = 0xFF,
}

impl FrameType {
    /// Convert from record type byte.
    pub fn from_record_type(record_type: u8) -> Self {
        match record_type {
            0x01 => FrameType::WebSocketText,
            0x02 => FrameType::WebSocketBinary,
            0x03 => FrameType::Checkpoint,
            0x10 => FrameType::BookSnapshot,
            0x11 => FrameType::BookDelta,
            0x12 => FrameType::Trade,
            0xF0 => FrameType::Heartbeat,
            _ => FrameType::Unknown,
        }
    }

    /// Convert to record type byte.
    pub fn to_record_type(self) -> u8 {
        self as u8
    }
}

/// Data source trait for market data injection.
///
/// This trait abstracts the source of market data, enabling seamless
/// switching between live WebSocket feeds and journal replay.
///
/// # Thread Safety
///
/// Requires `Send + Sync` for multi-threaded use. The replay engine
/// may read from the data source on a different thread than the one
/// controlling playback.
///
/// # Design
///
/// - Returns `Option<DataFrame>` for pull-based iteration
/// - Provides `peek()` for lookahead without consuming
/// - Supports reset for replay from beginning
///
/// # Example
///
/// ```rust
/// use blackbox::replay::{DataSource, NullDataSource};
///
/// fn consume_all<D: DataSource>(source: &mut D) -> usize {
///     let mut count = 0;
///     while source.next().is_some() {
///         count += 1;
///     }
///     count
/// }
///
/// let mut source = NullDataSource;
/// assert_eq!(consume_all(&mut source), 0);
/// ```
pub trait DataSource: Send + Sync {
    /// Get the next data frame, consuming it.
    ///
    /// Returns `None` when the source is exhausted or inactive.
    ///
    /// # Performance
    ///
    /// Target: <1μs for journal-based sources.
    fn next(&mut self) -> Option<DataFrame>;

    /// Peek at the next frame without consuming it.
    ///
    /// Returns `None` if no more frames are available.
    /// Consecutive calls without `next()` should return the same frame.
    ///
    /// # Performance
    ///
    /// Target: <100ns (read-only operation).
    fn peek(&self) -> Option<&DataFrame>;

    /// Check if more frames are available.
    ///
    /// Equivalent to `self.peek().is_some()` but may be more efficient.
    #[inline]
    fn has_next(&self) -> bool {
        self.peek().is_some()
    }

    /// Get the timestamp of the next available frame.
    ///
    /// Useful for scheduling in the replay engine.
    #[inline]
    fn peek_timestamp(&self) -> Option<Timestamp> {
        self.peek().map(|f| f.timestamp)
    }

    /// Check if the data source is active.
    ///
    /// An inactive source returns `None` for all operations.
    fn is_active(&self) -> bool;

    /// Reset the data source to the beginning.
    ///
    /// For journal sources, this resets the read position.
    /// For live sources, this may be a no-op or reconnect.
    fn reset(&mut self);

    /// Get the total number of frames (if known).
    ///
    /// Returns `None` for live sources or when count is unknown.
    fn frame_count(&self) -> Option<usize> {
        None
    }

    /// Get the current position in the source (if applicable).
    ///
    /// Returns `None` for live sources.
    fn position(&self) -> Option<usize> {
        None
    }
}

/// A no-op data source that produces no frames.
///
/// This is the data source equivalent of `NullTap`. Use it when
/// data injection is disabled or for testing.
///
/// # Zero-Sized Type
///
/// `NullDataSource` is a zero-sized type (ZST), meaning it has no
/// memory footprint and all methods are optimized away by the compiler.
///
/// # Performance Contract
///
/// | Method | Latency | Allocation |
/// |--------|---------|------------|
/// | `next()` | <1ns | 0 bytes |
/// | `peek()` | <1ns | 0 bytes |
/// | `is_active()` | <1ns | 0 bytes |
///
/// # Example
///
/// ```
/// use blackbox::replay::{DataSource, NullDataSource};
///
/// let mut source = NullDataSource;
///
/// // Always returns None
/// assert!(source.next().is_none());
/// assert!(source.peek().is_none());
/// assert!(!source.is_active());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NullDataSource;

impl DataSource for NullDataSource {
    #[inline(always)]
    fn next(&mut self) -> Option<DataFrame> {
        None
    }

    #[inline(always)]
    fn peek(&self) -> Option<&DataFrame> {
        None
    }

    #[inline(always)]
    fn has_next(&self) -> bool {
        false
    }

    #[inline(always)]
    fn peek_timestamp(&self) -> Option<Timestamp> {
        None
    }

    #[inline(always)]
    fn is_active(&self) -> bool {
        false
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No-op
    }

    #[inline(always)]
    fn frame_count(&self) -> Option<usize> {
        Some(0)
    }

    #[inline(always)]
    fn position(&self) -> Option<usize> {
        Some(0)
    }
}

// Compile-time verification that NullDataSource is zero-sized
const _: () = assert!(std::mem::size_of::<NullDataSource>() == 0);

/// A buffered data source for testing.
///
/// This implementation holds a vector of frames and returns them sequentially.
/// Useful for testing and simulation.
///
/// # Example
///
/// ```
/// use blackbox::replay::{DataSource, BufferedDataSource, DataFrame, FrameType};
/// use blackbox_types::{Exchange, Timestamp};
///
/// let frames = vec![
///     DataFrame::new(Timestamp::from_micros(1000), Exchange::Deribit, FrameType::WebSocketText, b"frame1".to_vec()),
///     DataFrame::new(Timestamp::from_micros(2000), Exchange::Deribit, FrameType::WebSocketText, b"frame2".to_vec()),
/// ];
///
/// let mut source = BufferedDataSource::new(frames);
/// assert_eq!(source.frame_count(), Some(2));
///
/// let frame1 = source.next().unwrap();
/// assert_eq!(frame1.timestamp.as_micros(), 1000);
///
/// let frame2 = source.next().unwrap();
/// assert_eq!(frame2.timestamp.as_micros(), 2000);
///
/// assert!(source.next().is_none());
/// ```
#[derive(Debug, Clone)]
pub struct BufferedDataSource {
    frames: Vec<DataFrame>,
    position: usize,
    active: bool,
}

impl BufferedDataSource {
    /// Create a new buffered data source with the given frames.
    pub fn new(frames: Vec<DataFrame>) -> Self {
        Self {
            frames,
            position: 0,
            active: true,
        }
    }

    /// Create an empty buffered data source.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Create an inactive buffered data source.
    pub fn inactive() -> Self {
        Self {
            frames: Vec::new(),
            position: 0,
            active: false,
        }
    }

    /// Set whether this source is active.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Get the remaining frame count.
    pub fn remaining(&self) -> usize {
        self.frames.len().saturating_sub(self.position)
    }
}

impl DataSource for BufferedDataSource {
    fn next(&mut self) -> Option<DataFrame> {
        if !self.active {
            return None;
        }
        if self.position < self.frames.len() {
            let frame = self.frames[self.position].clone();
            self.position += 1;
            Some(frame)
        } else {
            None
        }
    }

    fn peek(&self) -> Option<&DataFrame> {
        if !self.active {
            return None;
        }
        self.frames.get(self.position)
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn reset(&mut self) {
        self.position = 0;
    }

    fn frame_count(&self) -> Option<usize> {
        Some(self.frames.len())
    }

    fn position(&self) -> Option<usize> {
        Some(self.position)
    }
}

/// A live data source that receives frames from a channel.
///
/// This implementation wraps a `crossbeam::channel::Receiver<DataFrame>` to
/// provide live market data from WebSocket connections or other real-time sources.
///
/// # Thread Safety
///
/// `LiveDataSource` is `Send + Sync` through careful use of interior mutability.
/// The `peek()` method uses a synchronization mutex to ensure thread-safe access
/// to the peeked frame buffer.
///
/// # Performance Contract
///
/// | Method | Target Latency | Notes |
/// |--------|----------------|-------|
/// | `next()` | <1μs | Channel receive + optional buffer read |
/// | `peek()` | <100ns | Cached read or channel receive |
/// | `is_active()` | <10ns | Atomic read |
/// | `has_next()` | <100ns | Peek-based check |
///
/// # Example
///
/// ```
/// use blackbox::replay::{DataSource, LiveDataSource, DataFrame, FrameType};
/// use blackbox_types::{Exchange, Timestamp};
/// use crossbeam::channel;
///
/// // Create a channel for live data
/// let (sender, receiver) = channel::unbounded();
///
/// // Create the live data source
/// let mut source = LiveDataSource::new(receiver);
/// assert!(source.is_active());
///
/// // Send a frame
/// sender.send(DataFrame::new(
///     Timestamp::from_micros(1000),
///     Exchange::Deribit,
///     FrameType::WebSocketText,
///     b"test".to_vec(),
/// )).unwrap();
///
/// // Receive it
/// let frame = source.next().unwrap();
/// assert_eq!(frame.timestamp.as_micros(), 1000);
/// ```
///
/// # Design Notes
///
/// - Uses `UnsafeCell` for the peeked buffer to allow returning `&DataFrame`
/// - Synchronization via `parking_lot::Mutex` ensures thread-safe peek
/// - `reset()` clears the buffer but cannot rewind live data
/// - `frame_count()` and `position()` return `None` (unknown for live sources)
pub struct LiveDataSource {
    /// The receiver for incoming data frames.
    receiver: Receiver<DataFrame>,
    /// Synchronization mutex for peek buffer access.
    sync: Mutex<()>,
    /// Peeked frame storage.
    ///
    /// # Safety
    ///
    /// Access to this field is synchronized via the `sync` mutex in `peek()`.
    /// In `next(&mut self)`, exclusive access is guaranteed by `&mut self`.
    peeked: UnsafeCell<Option<DataFrame>>,
    /// Whether this source is active.
    active: AtomicBool,
}

// Safety: Access to `peeked` is synchronized via the `sync` mutex in peek(),
// and by exclusive access in next(&mut self). The Receiver is Send+Sync,
// and AtomicBool is naturally thread-safe.
unsafe impl Sync for LiveDataSource {}

impl LiveDataSource {
    /// Create a new live data source from a receiver.
    ///
    /// The source is immediately active and ready to receive frames.
    ///
    /// # Arguments
    ///
    /// * `receiver` - The channel receiver for incoming frames
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{DataSource, LiveDataSource};
    ///
    /// // Use with_sender() for a convenient way to create both ends
    /// let (source, _sender) = LiveDataSource::with_sender();
    /// assert!(source.is_active());
    /// ```
    pub fn new(receiver: Receiver<DataFrame>) -> Self {
        Self {
            receiver,
            sync: Mutex::new(()),
            peeked: UnsafeCell::new(None),
            active: AtomicBool::new(true),
        }
    }

    /// Create an inactive live data source.
    ///
    /// Useful for testing or placeholder scenarios where live data
    /// is not yet available.
    pub fn inactive(receiver: Receiver<DataFrame>) -> Self {
        Self {
            receiver,
            sync: Mutex::new(()),
            peeked: UnsafeCell::new(None),
            active: AtomicBool::new(false),
        }
    }

    /// Create a live data source with associated sender.
    ///
    /// This is a convenience method for testing that creates both
    /// the sender and receiver.
    ///
    /// # Returns
    ///
    /// A tuple of `(LiveDataSource, Sender<DataFrame>)`.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::replay::{DataSource, LiveDataSource, DataFrame, FrameType};
    /// use blackbox_types::{Exchange, Timestamp};
    ///
    /// let (mut source, sender) = LiveDataSource::with_sender();
    ///
    /// sender.send(DataFrame::empty(
    ///     Timestamp::from_micros(1000),
    ///     Exchange::Binance,
    ///     FrameType::Trade,
    /// )).unwrap();
    ///
    /// assert!(source.has_next());
    /// ```
    pub fn with_sender() -> (Self, Sender<DataFrame>) {
        let (sender, receiver) = channel::unbounded();
        (Self::new(receiver), sender)
    }

    /// Set the active state of this source.
    ///
    /// When inactive, all operations return `None`.
    pub fn set_active(&mut self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    /// Check if the channel is disconnected.
    ///
    /// A disconnected channel means no more frames will arrive.
    pub fn is_disconnected(&self) -> bool {
        self.receiver.is_empty() && {
            // Try a non-blocking receive to check for disconnection
            matches!(self.receiver.try_recv(), Err(TryRecvError::Disconnected))
        }
    }

    /// Get the sender for this data source (for testing).
    ///
    /// Creates a new sender connected to the same channel.
    /// Note: This only works if the original sender is still alive.
    pub fn sender(&self) -> Option<Sender<DataFrame>> {
        // We can't create a sender from a receiver, so this method
        // returns None. Users should use with_sender() instead.
        None
    }
}

impl DataSource for LiveDataSource {
    fn next(&mut self) -> Option<DataFrame> {
        if !self.active.load(Ordering::Acquire) {
            return None;
        }

        // Safety: &mut self guarantees exclusive access
        let peeked = unsafe { &mut *self.peeked.get() };

        // First check if we have a peeked frame
        if let Some(frame) = peeked.take() {
            return Some(frame);
        }

        // Otherwise try to receive from channel
        self.receiver.try_recv().ok()
    }

    fn peek(&self) -> Option<&DataFrame> {
        if !self.active.load(Ordering::Acquire) {
            return None;
        }

        // Lock for synchronization
        let _lock = self.sync.lock();

        // Safety: We hold the sync lock, so exclusive access is guaranteed
        let peeked = unsafe { &mut *self.peeked.get() };

        // Populate if empty
        if peeked.is_none() {
            if let Ok(frame) = self.receiver.try_recv() {
                *peeked = Some(frame);
            }
        }

        // Return reference - safe because frame lives in self
        peeked.as_ref()
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn reset(&mut self) {
        // Clear the peeked buffer
        // Safety: &mut self guarantees exclusive access
        let peeked = unsafe { &mut *self.peeked.get() };
        *peeked = None;

        // Note: We cannot rewind a live channel, so we just clear the buffer.
        // Frames already sent but not received are still available.
    }

    fn frame_count(&self) -> Option<usize> {
        // Unknown for live sources
        None
    }

    fn position(&self) -> Option<usize> {
        // Unknown for live sources
        None
    }
}

impl std::fmt::Debug for LiveDataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveDataSource")
            .field("active", &self.active.load(Ordering::Relaxed))
            .field("has_peeked", &self.peek().is_some())
            .finish()
    }
}

// ============================================================
// JournalDataSource Implementation (T3.8)
// ============================================================

/// A data source that replays frames from a journal or pre-loaded vector.
///
/// `JournalDataSource` provides a `DataSource` implementation for replay
/// from journal files or in-memory frame collections. It supports:
/// - Sequential iteration through frames
/// - Peek-ahead without consuming
/// - Reset to replay from the beginning
/// - Position tracking
///
/// # Performance
///
/// | Operation | Target Latency |
/// |-----------|----------------|
/// | `peek()` | <100ns |
/// | `next()` | <500ns |
/// | `reset()` | O(1) |
///
/// # Example
///
/// ```rust
/// use blackbox::replay::{JournalDataSource, DataSource, DataFrame, FrameType};
/// use blackbox_types::{Exchange, Timestamp};
///
/// // Create from frames
/// let frames = vec![
///     DataFrame::new(
///         Timestamp::from_micros(1000),
///         Exchange::Deribit,
///         FrameType::Trade,
///         b"data".to_vec(),
///     ),
/// ];
///
/// let mut source = JournalDataSource::from_frames(frames);
///
/// assert!(source.has_next());
/// let frame = source.next().unwrap();
/// assert_eq!(frame.timestamp.as_micros(), 1000);
/// ```
#[derive(Debug)]
pub struct JournalDataSource {
    /// The frames to replay.
    frames: Vec<DataFrame>,
    /// Current position in the frame vector.
    position: usize,
    /// Whether the source is active (has frames).
    active: bool,
}

impl JournalDataSource {
    /// Create a new JournalDataSource from a vector of frames.
    ///
    /// # Arguments
    ///
    /// * `frames` - The frames to replay
    ///
    /// # Example
    ///
    /// ```rust
    /// use blackbox::replay::{JournalDataSource, DataFrame, FrameType};
    /// use blackbox_types::{Exchange, Timestamp};
    ///
    /// let frames = vec![
    ///     DataFrame::empty(Timestamp::from_micros(1000), Exchange::Deribit, FrameType::Trade),
    /// ];
    /// let source = JournalDataSource::from_frames(frames);
    /// ```
    pub fn from_frames(frames: Vec<DataFrame>) -> Self {
        let active = !frames.is_empty();
        Self {
            frames,
            position: 0,
            active,
        }
    }

    /// Create an empty JournalDataSource with no frames.
    ///
    /// This is useful for testing or as a placeholder when no data is available.
    pub fn empty() -> Self {
        Self {
            frames: Vec::new(),
            position: 0,
            active: false,
        }
    }

    /// Create from a single frame.
    pub fn single(frame: DataFrame) -> Self {
        Self::from_frames(vec![frame])
    }

    /// Get the total number of frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Check if the source is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

impl DataSource for JournalDataSource {
    fn next(&mut self) -> Option<DataFrame> {
        if self.position >= self.frames.len() {
            return None;
        }

        let frame = self.frames[self.position].clone();
        self.position += 1;
        Some(frame)
    }

    fn peek(&self) -> Option<&DataFrame> {
        self.frames.get(self.position)
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn reset(&mut self) {
        self.position = 0;
    }

    fn frame_count(&self) -> Option<usize> {
        Some(self.frames.len())
    }

    fn position(&self) -> Option<usize> {
        Some(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;

    // ============================================================
    // DATAFRAME TESTS
    // ============================================================

    #[test]
    fn test_dataframe_new() {
        let frame = DataFrame::new(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
            vec![1, 2, 3],
        );
        assert_eq!(frame.timestamp.as_micros(), 1000);
        assert_eq!(frame.exchange, Exchange::Deribit);
        assert_eq!(frame.frame_type, FrameType::WebSocketText);
        assert_eq!(frame.payload, vec![1, 2, 3]);
    }

    #[test]
    fn test_dataframe_empty() {
        let frame = DataFrame::empty(
            Timestamp::from_micros(2000),
            Exchange::Binance,
            FrameType::Heartbeat,
        );
        assert_eq!(frame.timestamp.as_micros(), 2000);
        assert!(frame.payload.is_empty());
        assert!(frame.is_empty());
    }

    #[test]
    fn test_dataframe_payload_len() {
        let frame = DataFrame::new(
            Timestamp::EPOCH,
            Exchange::Bybit,
            FrameType::WebSocketBinary,
            vec![0; 100],
        );
        assert_eq!(frame.payload_len(), 100);
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_dataframe_clone() {
        let frame = DataFrame::new(
            Timestamp::from_micros(5000),
            Exchange::OKX,
            FrameType::BookSnapshot,
            b"snapshot".to_vec(),
        );
        let cloned = frame.clone();
        assert_eq!(frame, cloned);
    }

    #[test]
    fn test_dataframe_debug() {
        let frame = DataFrame::new(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::Trade,
            vec![],
        );
        let debug = format!("{:?}", frame);
        assert!(debug.contains("DataFrame"));
        assert!(debug.contains("Deribit"));
    }

    #[test]
    fn test_dataframe_all_exchanges() {
        for exchange in [
            Exchange::Unknown,
            Exchange::Deribit,
            Exchange::Binance,
            Exchange::Bybit,
            Exchange::OKX,
        ] {
            let frame = DataFrame::empty(Timestamp::EPOCH, exchange, FrameType::Unknown);
            assert_eq!(frame.exchange, exchange);
        }
    }

    // ============================================================
    // FRAMETYPE TESTS
    // ============================================================

    #[test]
    fn test_frametype_values() {
        assert_eq!(FrameType::WebSocketText as u8, 0x01);
        assert_eq!(FrameType::WebSocketBinary as u8, 0x02);
        assert_eq!(FrameType::Checkpoint as u8, 0x03);
        assert_eq!(FrameType::BookSnapshot as u8, 0x10);
        assert_eq!(FrameType::BookDelta as u8, 0x11);
        assert_eq!(FrameType::Trade as u8, 0x12);
        assert_eq!(FrameType::Heartbeat as u8, 0xF0);
        assert_eq!(FrameType::Unknown as u8, 0xFF);
    }

    #[test]
    fn test_frametype_from_record_type() {
        assert_eq!(FrameType::from_record_type(0x01), FrameType::WebSocketText);
        assert_eq!(
            FrameType::from_record_type(0x02),
            FrameType::WebSocketBinary
        );
        assert_eq!(FrameType::from_record_type(0x03), FrameType::Checkpoint);
        assert_eq!(FrameType::from_record_type(0x10), FrameType::BookSnapshot);
        assert_eq!(FrameType::from_record_type(0x11), FrameType::BookDelta);
        assert_eq!(FrameType::from_record_type(0x12), FrameType::Trade);
        assert_eq!(FrameType::from_record_type(0xF0), FrameType::Heartbeat);
        assert_eq!(FrameType::from_record_type(0xFF), FrameType::Unknown);
    }

    #[test]
    fn test_frametype_from_unknown_value() {
        assert_eq!(FrameType::from_record_type(0x00), FrameType::Unknown);
        assert_eq!(FrameType::from_record_type(0x99), FrameType::Unknown);
        assert_eq!(FrameType::from_record_type(0xFE), FrameType::Unknown);
    }

    #[test]
    fn test_frametype_roundtrip() {
        for ft in [
            FrameType::WebSocketText,
            FrameType::WebSocketBinary,
            FrameType::Checkpoint,
            FrameType::BookSnapshot,
            FrameType::BookDelta,
            FrameType::Trade,
            FrameType::Heartbeat,
            FrameType::Unknown,
        ] {
            let byte = ft.to_record_type();
            let restored = FrameType::from_record_type(byte);
            assert_eq!(ft, restored);
        }
    }

    #[test]
    fn test_frametype_clone_copy() {
        let ft = FrameType::BookSnapshot;
        let cloned = ft;
        assert_eq!(ft, cloned);
    }

    #[test]
    fn test_frametype_debug() {
        let ft = FrameType::Trade;
        let debug = format!("{:?}", ft);
        assert_eq!(debug, "Trade");
    }

    // ============================================================
    // NULLDATASOURCE TESTS
    // ============================================================

    #[test]
    fn test_null_data_source_is_zero_sized() {
        assert_eq!(std::mem::size_of::<NullDataSource>(), 0);
    }

    #[test]
    fn test_null_data_source_next_returns_none() {
        let mut source = NullDataSource;
        assert!(source.next().is_none());
    }

    #[test]
    fn test_null_data_source_peek_returns_none() {
        let source = NullDataSource;
        assert!(source.peek().is_none());
    }

    #[test]
    fn test_null_data_source_has_next_false() {
        let source = NullDataSource;
        assert!(!source.has_next());
    }

    #[test]
    fn test_null_data_source_peek_timestamp_none() {
        let source = NullDataSource;
        assert!(source.peek_timestamp().is_none());
    }

    #[test]
    fn test_null_data_source_not_active() {
        let source = NullDataSource;
        assert!(!source.is_active());
    }

    #[test]
    fn test_null_data_source_reset_noop() {
        let mut source = NullDataSource;
        source.reset(); // Should not panic
        assert!(!source.is_active());
    }

    #[test]
    fn test_null_data_source_frame_count_zero() {
        let source = NullDataSource;
        assert_eq!(source.frame_count(), Some(0));
    }

    #[test]
    fn test_null_data_source_position_zero() {
        let source = NullDataSource;
        assert_eq!(source.position(), Some(0));
    }

    #[test]
    fn test_null_data_source_default() {
        // NullDataSource is a unit struct with Default trait, verify it works
        fn accepts_default<T: Default>(_: T) {}
        accepts_default(NullDataSource);
        assert!(!NullDataSource.is_active());
    }

    #[test]
    fn test_null_data_source_clone() {
        let source = NullDataSource;
        let cloned = source;
        assert_eq!(source, cloned);
    }

    #[test]
    fn test_null_data_source_debug() {
        let source = NullDataSource;
        let debug = format!("{:?}", source);
        assert_eq!(debug, "NullDataSource");
    }

    #[test]
    fn test_null_data_source_multiple_next_calls() {
        let mut source = NullDataSource;
        for _ in 0..100 {
            assert!(source.next().is_none());
        }
    }

    // ============================================================
    // BUFFEREDDATASOURCE TESTS
    // ============================================================

    #[test]
    fn test_buffered_source_new() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
        )];
        let source = BufferedDataSource::new(frames);
        assert!(source.is_active());
        assert_eq!(source.frame_count(), Some(1));
    }

    #[test]
    fn test_buffered_source_empty() {
        let source = BufferedDataSource::empty();
        assert!(source.is_active());
        assert_eq!(source.frame_count(), Some(0));
        assert!(!source.has_next());
    }

    #[test]
    fn test_buffered_source_inactive() {
        let source = BufferedDataSource::inactive();
        assert!(!source.is_active());
    }

    #[test]
    fn test_buffered_source_next() {
        let frames = vec![
            DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"frame1".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(2000),
                Exchange::Binance,
                FrameType::WebSocketText,
                b"frame2".to_vec(),
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        let f1 = source.next().unwrap();
        assert_eq!(f1.timestamp.as_micros(), 1000);
        assert_eq!(f1.payload, b"frame1");

        let f2 = source.next().unwrap();
        assert_eq!(f2.timestamp.as_micros(), 2000);

        assert!(source.next().is_none());
    }

    #[test]
    fn test_buffered_source_peek() {
        let frames = vec![DataFrame::new(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::BookSnapshot,
            vec![1, 2, 3],
        )];
        let source = BufferedDataSource::new(frames);

        let peeked = source.peek().unwrap();
        assert_eq!(peeked.timestamp.as_micros(), 1000);

        // Peek again - same result
        let peeked2 = source.peek().unwrap();
        assert_eq!(peeked.timestamp, peeked2.timestamp);
    }

    #[test]
    fn test_buffered_source_peek_timestamp() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(5000),
            Exchange::OKX,
            FrameType::Trade,
        )];
        let source = BufferedDataSource::new(frames);

        assert_eq!(source.peek_timestamp(), Some(Timestamp::from_micros(5000)));
    }

    #[test]
    fn test_buffered_source_has_next() {
        let frames = vec![DataFrame::empty(
            Timestamp::EPOCH,
            Exchange::Bybit,
            FrameType::Heartbeat,
        )];
        let mut source = BufferedDataSource::new(frames);

        assert!(source.has_next());
        source.next();
        assert!(!source.has_next());
    }

    #[test]
    fn test_buffered_source_reset() {
        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        // Consume all
        source.next();
        source.next();
        assert!(!source.has_next());

        // Reset
        source.reset();
        assert!(source.has_next());
        assert_eq!(source.position(), Some(0));
    }

    #[test]
    fn test_buffered_source_position() {
        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        assert_eq!(source.position(), Some(0));
        source.next();
        assert_eq!(source.position(), Some(1));
        source.next();
        assert_eq!(source.position(), Some(2));
        source.next();
        assert_eq!(source.position(), Some(3));
    }

    #[test]
    fn test_buffered_source_remaining() {
        let frames = vec![
            DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        assert_eq!(source.remaining(), 2);
        source.next();
        assert_eq!(source.remaining(), 1);
        source.next();
        assert_eq!(source.remaining(), 0);
    }

    #[test]
    fn test_buffered_source_set_active() {
        let mut source = BufferedDataSource::empty();
        assert!(source.is_active());

        source.set_active(false);
        assert!(!source.is_active());

        source.set_active(true);
        assert!(source.is_active());
    }

    #[test]
    fn test_buffered_source_inactive_returns_none() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
        )];
        let mut source = BufferedDataSource::new(frames);
        source.set_active(false);

        assert!(source.next().is_none());
        assert!(source.peek().is_none());
        assert!(!source.has_next());
    }

    #[test]
    fn test_buffered_source_clone() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
        )];
        let source = BufferedDataSource::new(frames);
        let cloned = source.clone();

        assert_eq!(source.frame_count(), cloned.frame_count());
        assert_eq!(source.position(), cloned.position());
    }

    // ============================================================
    // TRAIT IMPLEMENTATION TESTS
    // ============================================================

    #[test]
    fn test_datasource_send_null() {
        fn assert_send<T: Send>() {}
        assert_send::<NullDataSource>();
    }

    #[test]
    fn test_datasource_sync_null() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<NullDataSource>();
    }

    #[test]
    fn test_datasource_send_buffered() {
        fn assert_send<T: Send>() {}
        assert_send::<BufferedDataSource>();
    }

    #[test]
    fn test_datasource_sync_buffered() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<BufferedDataSource>();
    }

    #[test]
    fn test_datasource_trait_requires_send_sync() {
        fn assert_send_sync<T: DataSource>() {}
        assert_send_sync::<NullDataSource>();
        assert_send_sync::<BufferedDataSource>();
    }

    // ============================================================
    // GENERIC USAGE TESTS
    // ============================================================

    #[test]
    fn test_generic_function_with_null() {
        fn count_frames<D: DataSource>(source: &mut D) -> usize {
            let mut count = 0;
            while source.next().is_some() {
                count += 1;
            }
            count
        }

        let mut source = NullDataSource;
        assert_eq!(count_frames(&mut source), 0);
    }

    #[test]
    fn test_generic_function_with_buffered() {
        fn count_frames<D: DataSource>(source: &mut D) -> usize {
            let mut count = 0;
            while source.next().is_some() {
                count += 1;
            }
            count
        }

        let frames = vec![
            DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        assert_eq!(count_frames(&mut source), 3);
    }

    #[test]
    fn test_generic_function_with_peek() {
        fn peek_all_timestamps<D: DataSource>(source: &mut D) -> Vec<i64> {
            let mut timestamps = Vec::new();
            while let Some(ts) = source.peek_timestamp() {
                timestamps.push(ts.as_micros());
                source.next();
            }
            timestamps
        }

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(300),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let timestamps = peek_all_timestamps(&mut source);
        assert_eq!(timestamps, vec![100, 200, 300]);
    }

    // ============================================================
    // THREAD SAFETY TESTS
    // ============================================================

    #[test]
    fn test_null_source_thread_sharing() {
        let source = Arc::new(NullDataSource);
        let mut handles = vec![];

        for _ in 0..4 {
            let s = Arc::clone(&source);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    assert!(!s.is_active());
                    assert!(s.peek().is_none());
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_buffered_source_send_across_thread() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
        )];
        let mut source = BufferedDataSource::new(frames);

        // Move to another thread
        let handle = thread::spawn(move || {
            let frame = source.next();
            assert!(frame.is_some());
            frame.unwrap().timestamp.as_micros()
        });

        let result = handle.join().unwrap();
        assert_eq!(result, 1000);
    }

    // ============================================================
    // EDGE CASE TESTS
    // ============================================================

    #[test]
    fn test_dataframe_large_payload() {
        let large_payload = vec![0u8; 1024 * 1024]; // 1MB
        let frame = DataFrame::new(
            Timestamp::EPOCH,
            Exchange::Deribit,
            FrameType::BookSnapshot,
            large_payload.clone(),
        );
        assert_eq!(frame.payload_len(), 1024 * 1024);
    }

    #[test]
    fn test_dataframe_timestamp_min() {
        let frame = DataFrame::empty(Timestamp::MIN, Exchange::Unknown, FrameType::Unknown);
        assert_eq!(frame.timestamp, Timestamp::MIN);
    }

    #[test]
    fn test_dataframe_timestamp_max() {
        let frame = DataFrame::empty(Timestamp::MAX, Exchange::Unknown, FrameType::Unknown);
        assert_eq!(frame.timestamp, Timestamp::MAX);
    }

    #[test]
    fn test_buffered_source_empty_reset() {
        let mut source = BufferedDataSource::empty();
        source.reset(); // Should not panic
        assert_eq!(source.position(), Some(0));
    }

    #[test]
    fn test_buffered_source_multiple_resets() {
        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
        )];
        let mut source = BufferedDataSource::new(frames);

        for _ in 0..10 {
            source.next();
            source.reset();
            assert_eq!(source.position(), Some(0));
            assert!(source.has_next());
        }
    }

    // ============================================================
    // COUNTING TESTS (Mock for metrics)
    // ============================================================

    /// A data source that counts method calls.
    struct CountingDataSource {
        next_count: AtomicU64,
        peek_count: AtomicU64,
    }

    impl CountingDataSource {
        fn new() -> Self {
            Self {
                next_count: AtomicU64::new(0),
                peek_count: AtomicU64::new(0),
            }
        }

        fn next_count(&self) -> u64 {
            self.next_count.load(Ordering::Relaxed)
        }

        fn peek_count(&self) -> u64 {
            self.peek_count.load(Ordering::Relaxed)
        }
    }

    impl DataSource for CountingDataSource {
        fn next(&mut self) -> Option<DataFrame> {
            self.next_count.fetch_add(1, Ordering::Relaxed);
            None
        }

        fn peek(&self) -> Option<&DataFrame> {
            self.peek_count.fetch_add(1, Ordering::Relaxed);
            None
        }

        fn is_active(&self) -> bool {
            true
        }

        fn reset(&mut self) {}
    }

    #[test]
    fn test_counting_source_next() {
        let mut source = CountingDataSource::new();
        for _ in 0..10 {
            source.next();
        }
        assert_eq!(source.next_count(), 10);
    }

    #[test]
    fn test_counting_source_peek() {
        let source = CountingDataSource::new();
        for _ in 0..5 {
            source.peek();
        }
        assert_eq!(source.peek_count(), 5);
    }

    #[test]
    fn test_counting_source_has_next_calls_peek() {
        let source = CountingDataSource::new();
        for _ in 0..3 {
            source.has_next();
        }
        assert_eq!(source.peek_count(), 3);
    }

    #[test]
    fn test_counting_source_peek_timestamp_calls_peek() {
        let source = CountingDataSource::new();
        source.peek_timestamp();
        assert_eq!(source.peek_count(), 1);
    }

    // ============================================================
    // INTEGRATION TESTS: DataSource + SimulatedClock workflow
    // ============================================================

    #[test]
    fn test_datasource_with_simulated_clock() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        let frames = vec![
            DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"frame1".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"frame2".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(5000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"frame3".to_vec(),
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let clock = SimulatedClock::new(Timestamp::EPOCH);

        // Simulate replay: advance clock to each frame's timestamp
        while let Some(ts) = source.peek_timestamp() {
            clock.advance_to(ts);
            let frame = source.next().unwrap();
            assert_eq!(clock.now(), frame.timestamp);
        }

        assert_eq!(clock.now().as_micros(), 5000);
    }

    #[test]
    fn test_datasource_replay_with_warp_detection() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        // Frames with large gap (idle period)
        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            // Gap of 1 second (warp-worthy)
            DataFrame::empty(
                Timestamp::from_micros(1_002_000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let clock = SimulatedClock::new(Timestamp::EPOCH);

        let warp_threshold = 10_000; // 10ms
        let mut warp_count = 0;

        while let Some(next_ts) = source.peek_timestamp() {
            let current = clock.now();
            let gap = next_ts.as_micros() - current.as_micros();

            if gap > warp_threshold && clock.is_warp_enabled() {
                warp_count += 1;
            }

            clock.advance_to(next_ts);
            source.next();
        }

        // Should have detected the 1-second gap as warp-worthy
        assert_eq!(warp_count, 1);
    }

    #[test]
    fn test_datasource_step_through_pattern() {
        use super::super::SimulatedClock;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(300),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let clock = SimulatedClock::new(Timestamp::EPOCH);

        // Simulate step-through debugging
        clock.pause();

        let mut step_count = 0;
        while source.has_next() {
            // Simulate user pressing "step"
            clock.resume();

            if let Some(ts) = source.peek_timestamp() {
                clock.advance_to(ts);
            }
            source.next();
            step_count += 1;

            clock.pause();
        }

        assert_eq!(step_count, 3);
        assert!(clock.is_paused());
    }

    #[test]
    fn test_datasource_generic_replay_function() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        fn replay_session<D: DataSource, C: Clock>(source: &mut D, _clock: &C) -> (usize, i64) {
            let mut frame_count = 0;
            let mut last_ts = 0i64;

            while let Some(frame) = source.next() {
                frame_count += 1;
                last_ts = frame.timestamp.as_micros();
            }

            (frame_count, last_ts)
        }

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Binance,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::OKX,
                FrameType::BookSnapshot,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let clock = SimulatedClock::at_epoch();

        let (count, last_ts) = replay_session(&mut source, &clock);
        assert_eq!(count, 3);
        assert_eq!(last_ts, 3000);
    }

    #[test]
    fn test_datasource_reset_replay() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);
        let clock = SimulatedClock::new(Timestamp::EPOCH);

        // First replay
        while let Some(ts) = source.peek_timestamp() {
            clock.advance_to(ts);
            source.next();
        }
        assert_eq!(clock.now().as_micros(), 200);
        assert!(!source.has_next());

        // Reset both
        source.reset();
        clock.set(Timestamp::EPOCH);

        // Replay again
        assert!(source.has_next());
        assert_eq!(clock.now(), Timestamp::EPOCH);

        while let Some(ts) = source.peek_timestamp() {
            clock.advance_to(ts);
            source.next();
        }
        assert_eq!(clock.now().as_micros(), 200);
    }

    #[test]
    fn test_null_datasource_with_clock() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        let mut source = NullDataSource;
        let clock = SimulatedClock::new(Timestamp::from_micros(1000));

        // NullDataSource should not affect clock
        let initial_time = clock.now();

        while source.next().is_some() {
            // This loop body never executes
            clock.advance(1);
        }

        assert_eq!(clock.now(), initial_time);
    }

    #[test]
    fn test_datasource_mixed_exchanges() {
        let frames = vec![
            DataFrame::new(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"deribit".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(150),
                Exchange::Binance,
                FrameType::Trade,
                b"binance".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(200),
                Exchange::OKX,
                FrameType::BookDelta,
                b"okx".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(250),
                Exchange::Bybit,
                FrameType::BookSnapshot,
                b"bybit".to_vec(),
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        let mut exchanges = Vec::new();
        while let Some(frame) = source.next() {
            exchanges.push(frame.exchange);
        }

        assert_eq!(
            exchanges,
            vec![
                Exchange::Deribit,
                Exchange::Binance,
                Exchange::OKX,
                Exchange::Bybit
            ]
        );
    }

    #[test]
    fn test_datasource_frame_type_filtering() {
        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::Heartbeat,
            ),
            DataFrame::empty(
                Timestamp::from_micros(300),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(400),
                Exchange::Deribit,
                FrameType::Heartbeat,
            ),
            DataFrame::empty(
                Timestamp::from_micros(500),
                Exchange::Deribit,
                FrameType::BookSnapshot,
            ),
        ];
        let mut source = BufferedDataSource::new(frames);

        // Filter out heartbeats
        let mut non_heartbeat_count = 0;
        while let Some(frame) = source.next() {
            if frame.frame_type != FrameType::Heartbeat {
                non_heartbeat_count += 1;
            }
        }

        assert_eq!(non_heartbeat_count, 3);
    }

    // ============================================================
    // LIVEDATASOURCE TESTS
    // ============================================================

    #[test]
    fn test_live_source_new() {
        let (sender, receiver) = channel::unbounded();
        let source = LiveDataSource::new(receiver);
        assert!(source.is_active());
        assert!(source.peek().is_none());
        drop(sender); // Keep sender alive until here
    }

    #[test]
    fn test_live_source_with_sender() {
        let (source, _sender) = LiveDataSource::with_sender();
        assert!(source.is_active());
    }

    #[test]
    fn test_live_source_inactive() {
        let (sender, receiver) = channel::unbounded();
        let source = LiveDataSource::inactive(receiver);
        assert!(!source.is_active());
        drop(sender);
    }

    #[test]
    fn test_live_source_next_empty() {
        let (mut source, _sender) = LiveDataSource::with_sender();
        assert!(source.next().is_none());
    }

    #[test]
    fn test_live_source_next_receives_frame() {
        let (mut source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"test".to_vec(),
            ))
            .unwrap();

        let frame = source.next().unwrap();
        assert_eq!(frame.timestamp.as_micros(), 1000);
        assert_eq!(frame.exchange, Exchange::Deribit);
        assert_eq!(frame.payload, b"test");
    }

    #[test]
    fn test_live_source_next_multiple_frames() {
        let (mut source, sender) = LiveDataSource::with_sender();

        for i in 0..5 {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(i * 1000),
                    Exchange::Binance,
                    FrameType::Trade,
                ))
                .unwrap();
        }

        for i in 0..5 {
            let frame = source.next().unwrap();
            assert_eq!(frame.timestamp.as_micros(), i * 1000);
        }

        assert!(source.next().is_none());
    }

    #[test]
    fn test_live_source_peek_empty() {
        let (source, _sender) = LiveDataSource::with_sender();
        assert!(source.peek().is_none());
    }

    #[test]
    fn test_live_source_peek_returns_frame() {
        let (source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::new(
                Timestamp::from_micros(2000),
                Exchange::OKX,
                FrameType::BookSnapshot,
                b"snapshot".to_vec(),
            ))
            .unwrap();

        let peeked = source.peek().unwrap();
        assert_eq!(peeked.timestamp.as_micros(), 2000);
        assert_eq!(peeked.exchange, Exchange::OKX);
    }

    #[test]
    fn test_live_source_peek_idempotent() {
        let (source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::Bybit,
                FrameType::Heartbeat,
            ))
            .unwrap();

        // Multiple peeks should return the same frame
        let peek1 = source.peek().unwrap().timestamp;
        let peek2 = source.peek().unwrap().timestamp;
        let peek3 = source.peek().unwrap().timestamp;

        assert_eq!(peek1, peek2);
        assert_eq!(peek2, peek3);
    }

    #[test]
    fn test_live_source_peek_then_next() {
        let (mut source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::new(
                Timestamp::from_micros(4000),
                Exchange::Deribit,
                FrameType::WebSocketBinary,
                vec![1, 2, 3, 4],
            ))
            .unwrap();

        // Peek should buffer the frame
        let peeked_ts = source.peek().unwrap().timestamp;

        // Next should return the same frame
        let frame = source.next().unwrap();
        assert_eq!(frame.timestamp, peeked_ts);

        // Now peek should try to get the next frame
        assert!(source.peek().is_none());
    }

    #[test]
    fn test_live_source_has_next() {
        let (source, sender) = LiveDataSource::with_sender();

        assert!(!source.has_next());

        sender
            .send(DataFrame::empty(
                Timestamp::EPOCH,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        assert!(source.has_next());
    }

    #[test]
    fn test_live_source_peek_timestamp() {
        let (source, sender) = LiveDataSource::with_sender();

        assert!(source.peek_timestamp().is_none());

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(5000),
                Exchange::Binance,
                FrameType::Trade,
            ))
            .unwrap();

        assert_eq!(source.peek_timestamp(), Some(Timestamp::from_micros(5000)));
    }

    #[test]
    fn test_live_source_is_active() {
        let (source, _sender) = LiveDataSource::with_sender();
        assert!(source.is_active());
    }

    #[test]
    fn test_live_source_set_active() {
        let (mut source, _sender) = LiveDataSource::with_sender();

        assert!(source.is_active());

        source.set_active(false);
        assert!(!source.is_active());

        source.set_active(true);
        assert!(source.is_active());
    }

    #[test]
    fn test_live_source_inactive_returns_none() {
        let (mut source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        source.set_active(false);

        assert!(source.next().is_none());
        assert!(source.peek().is_none());
        assert!(!source.has_next());
        assert!(source.peek_timestamp().is_none());
    }

    #[test]
    fn test_live_source_reset() {
        let (mut source, sender) = LiveDataSource::with_sender();

        // Send and peek a frame (buffers it)
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        let _ = source.peek(); // Buffer the frame

        // Reset should clear the buffer
        source.reset();

        // The buffered frame is lost, next frame in channel is available
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Binance,
                FrameType::Trade,
            ))
            .unwrap();

        let frame = source.next().unwrap();
        assert_eq!(frame.timestamp.as_micros(), 2000);
    }

    #[test]
    fn test_live_source_frame_count_none() {
        let (source, _sender) = LiveDataSource::with_sender();
        assert_eq!(source.frame_count(), None);
    }

    #[test]
    fn test_live_source_position_none() {
        let (source, _sender) = LiveDataSource::with_sender();
        assert_eq!(source.position(), None);
    }

    #[test]
    fn test_live_source_debug() {
        let (source, _sender) = LiveDataSource::with_sender();
        let debug = format!("{:?}", source);
        assert!(debug.contains("LiveDataSource"));
        assert!(debug.contains("active"));
    }

    #[test]
    fn test_live_source_disconnected_sender() {
        let (mut source, sender) = LiveDataSource::with_sender();

        // Send one frame
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        // Drop sender to disconnect
        drop(sender);

        // Should still be able to receive the buffered frame
        let frame = source.next();
        assert!(frame.is_some());

        // Now channel is empty and disconnected
        assert!(source.next().is_none());
    }

    #[test]
    fn test_live_source_all_exchanges() {
        let (mut source, sender) = LiveDataSource::with_sender();

        for exchange in [
            Exchange::Unknown,
            Exchange::Deribit,
            Exchange::Binance,
            Exchange::Bybit,
            Exchange::OKX,
        ] {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(1000),
                    exchange,
                    FrameType::WebSocketText,
                ))
                .unwrap();

            let frame = source.next().unwrap();
            assert_eq!(frame.exchange, exchange);
        }
    }

    #[test]
    fn test_live_source_all_frame_types() {
        let (mut source, sender) = LiveDataSource::with_sender();

        for frame_type in [
            FrameType::WebSocketText,
            FrameType::WebSocketBinary,
            FrameType::BookSnapshot,
            FrameType::BookDelta,
            FrameType::Trade,
            FrameType::Heartbeat,
            FrameType::Unknown,
        ] {
            sender
                .send(DataFrame::empty(
                    Timestamp::EPOCH,
                    Exchange::Deribit,
                    frame_type,
                ))
                .unwrap();

            let frame = source.next().unwrap();
            assert_eq!(frame.frame_type, frame_type);
        }
    }

    #[test]
    fn test_live_source_large_payload() {
        let (mut source, sender) = LiveDataSource::with_sender();

        let large_payload = vec![0xABu8; 1024 * 64]; // 64KB
        sender
            .send(DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::BookSnapshot,
                large_payload.clone(),
            ))
            .unwrap();

        let frame = source.next().unwrap();
        assert_eq!(frame.payload.len(), 64 * 1024);
        assert_eq!(frame.payload, large_payload);
    }

    #[test]
    fn test_live_source_timestamp_extremes() {
        let (mut source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::empty(
                Timestamp::MIN,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        sender
            .send(DataFrame::empty(
                Timestamp::MAX,
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        let frame1 = source.next().unwrap();
        assert_eq!(frame1.timestamp, Timestamp::MIN);

        let frame2 = source.next().unwrap();
        assert_eq!(frame2.timestamp, Timestamp::MAX);
    }

    // ============================================================
    // LIVEDATASOURCE THREAD SAFETY TESTS
    // ============================================================

    #[test]
    fn test_live_source_send() {
        fn assert_send<T: Send>() {}
        assert_send::<LiveDataSource>();
    }

    #[test]
    fn test_live_source_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<LiveDataSource>();
    }

    #[test]
    fn test_live_source_implements_datasource_trait() {
        fn assert_datasource<T: DataSource>() {}
        assert_datasource::<LiveDataSource>();
    }

    #[test]
    fn test_live_source_concurrent_peek() {
        let (source, sender) = LiveDataSource::with_sender();
        let source = Arc::new(source);

        // Send a frame
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        // Multiple threads can peek
        let mut handles = vec![];
        for _ in 0..4 {
            let s = Arc::clone(&source);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let _ = s.peek();
                    let _ = s.is_active();
                    let _ = s.has_next();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_live_source_send_from_thread() {
        let (mut source, sender) = LiveDataSource::with_sender();

        // Sender in another thread
        let handle = thread::spawn(move || {
            for i in 0..10 {
                sender
                    .send(DataFrame::empty(
                        Timestamp::from_micros(i * 100),
                        Exchange::Binance,
                        FrameType::Trade,
                    ))
                    .unwrap();
            }
        });

        handle.join().unwrap();

        // Receive all frames
        let mut count = 0;
        while source.next().is_some() {
            count += 1;
        }
        assert_eq!(count, 10);
    }

    #[test]
    fn test_live_source_move_across_thread() {
        let (source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        let handle = thread::spawn(move || {
            let mut s = source;
            let frame = s.next();
            frame.map(|f| f.timestamp.as_micros())
        });

        let result = handle.join().unwrap();
        assert_eq!(result, Some(1000));
    }

    // ============================================================
    // LIVEDATASOURCE GENERIC USAGE TESTS
    // ============================================================

    #[test]
    fn test_live_source_generic_function() {
        fn count_frames<D: DataSource>(source: &mut D) -> usize {
            let mut count = 0;
            while source.next().is_some() {
                count += 1;
            }
            count
        }

        let (mut source, sender) = LiveDataSource::with_sender();

        for i in 0..5 {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(i * 100),
                    Exchange::Deribit,
                    FrameType::WebSocketText,
                ))
                .unwrap();
        }

        assert_eq!(count_frames(&mut source), 5);
    }

    #[test]
    fn test_live_source_box_dyn() {
        let (source, sender) = LiveDataSource::with_sender();
        let mut boxed: Box<dyn DataSource> = Box::new(source);

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        assert!(boxed.has_next());
        assert!(boxed.next().is_some());
    }

    #[test]
    fn test_live_source_arc_dyn() {
        let (source, sender) = LiveDataSource::with_sender();
        let arc: Arc<dyn DataSource> = Arc::new(source);

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        // Can peek from Arc
        assert!(arc.has_next());
        assert!(arc.peek().is_some());
    }

    // ============================================================
    // LIVEDATASOURCE INTEGRATION WITH SIMULATEDCLOCK TESTS
    // ============================================================

    #[test]
    fn test_live_source_with_simulated_clock() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        let (mut source, sender) = LiveDataSource::with_sender();
        let clock = SimulatedClock::at_epoch();

        // Send frames
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        // Replay pattern
        while let Some(ts) = source.peek_timestamp() {
            clock.advance_to(ts);
            source.next();
        }

        assert_eq!(clock.now().as_micros(), 2000);
    }

    #[test]
    fn test_live_source_replay_workflow() {
        use super::super::SimulatedClock;
        use blackbox_types::Clock;

        let (mut source, sender) = LiveDataSource::with_sender();
        let clock = SimulatedClock::new(Timestamp::EPOCH);

        // Simulate live data arriving
        let timestamps = vec![100, 200, 500, 1000, 1500];
        for ts in &timestamps {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(*ts),
                    Exchange::Binance,
                    FrameType::Trade,
                ))
                .unwrap();
        }

        // Process with clock advancement
        let mut received_ts = Vec::new();
        while let Some(ts) = source.peek_timestamp() {
            received_ts.push(ts.as_micros());
            clock.advance_to(ts);
            source.next();
        }

        assert_eq!(received_ts, timestamps);
        assert_eq!(clock.now().as_micros(), 1500);
    }

    #[test]
    fn test_live_source_generic_replay_function() {
        use super::super::SimulatedClock;

        fn replay<D: DataSource>(source: &mut D, clock: &SimulatedClock) -> usize {
            let mut count = 0;
            while let Some(ts) = source.peek_timestamp() {
                clock.advance_to(ts);
                source.next();
                count += 1;
            }
            count
        }

        let (mut source, sender) = LiveDataSource::with_sender();
        let clock = SimulatedClock::at_epoch();

        for i in 0..3 {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(i * 1000),
                    Exchange::OKX,
                    FrameType::BookSnapshot,
                ))
                .unwrap();
        }

        let count = replay(&mut source, &clock);
        assert_eq!(count, 3);
    }

    // ============================================================
    // LIVEDATASOURCE EDGE CASE TESTS
    // ============================================================

    #[test]
    fn test_live_source_rapid_send_receive() {
        let (mut source, sender) = LiveDataSource::with_sender();

        // Rapid fire send and receive
        for i in 0..1000 {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(i),
                    Exchange::Deribit,
                    FrameType::WebSocketText,
                ))
                .unwrap();

            let frame = source.next().unwrap();
            assert_eq!(frame.timestamp.as_micros(), i);
        }
    }

    #[test]
    fn test_live_source_interleaved_peek_next() {
        let (mut source, sender) = LiveDataSource::with_sender();

        for i in 0..5 {
            sender
                .send(DataFrame::empty(
                    Timestamp::from_micros(i * 100),
                    Exchange::Deribit,
                    FrameType::WebSocketText,
                ))
                .unwrap();
        }

        // Interleave peek and next
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 0);
        assert_eq!(source.next().unwrap().timestamp.as_micros(), 0);

        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 100);
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 100);
        assert_eq!(source.next().unwrap().timestamp.as_micros(), 100);

        assert_eq!(source.next().unwrap().timestamp.as_micros(), 200);
        assert_eq!(source.next().unwrap().timestamp.as_micros(), 300);
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 400);
        assert_eq!(source.next().unwrap().timestamp.as_micros(), 400);

        assert!(source.peek().is_none());
    }

    #[test]
    fn test_live_source_empty_payload() {
        let (mut source, sender) = LiveDataSource::with_sender();

        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::Heartbeat,
            ))
            .unwrap();

        let frame = source.next().unwrap();
        assert!(frame.is_empty());
        assert_eq!(frame.payload_len(), 0);
    }

    #[test]
    fn test_live_source_bounded_channel() {
        let (sender, receiver) = channel::bounded(2);
        let mut source = LiveDataSource::new(receiver);

        // Fill the bounded channel
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();
        sender
            .send(DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .unwrap();

        // Channel should be full now
        assert!(sender
            .try_send(DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .is_err());

        // Receive one to make space
        assert!(source.next().is_some());

        // Now we can send again
        assert!(sender
            .try_send(DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ))
            .is_ok());
    }

    // ============================================================
    // JOURNALDATASOURCE TESTS (T3.8)
    // ============================================================

    #[test]
    fn test_journal_datasource_empty() {
        use super::JournalDataSource;

        // Test with empty source
        let source = JournalDataSource::empty();
        assert!(!source.is_active());
        assert!(source.peek().is_none());
        assert_eq!(source.frame_count(), Some(0));
    }

    #[test]
    fn test_journal_datasource_from_frames() {
        use super::JournalDataSource;

        let frames = vec![
            DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                b"frame1".to_vec(),
            ),
            DataFrame::new(
                Timestamp::from_micros(2000),
                Exchange::Binance,
                FrameType::Trade,
                b"frame2".to_vec(),
            ),
        ];

        let mut source = JournalDataSource::from_frames(frames);

        assert!(source.is_active());
        assert_eq!(source.frame_count(), Some(2));

        // Peek should show first frame
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 1000);

        // Next should consume first frame
        let frame1 = source.next().unwrap();
        assert_eq!(frame1.timestamp.as_micros(), 1000);
        assert_eq!(frame1.payload, b"frame1");

        // Now peek should show second frame
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 2000);

        let frame2 = source.next().unwrap();
        assert_eq!(frame2.timestamp.as_micros(), 2000);

        // Exhausted
        assert!(source.peek().is_none());
        assert!(source.next().is_none());
    }

    #[test]
    fn test_journal_datasource_reset() {
        use super::JournalDataSource;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(300),
                Exchange::Deribit,
                FrameType::Trade,
            ),
        ];

        let mut source = JournalDataSource::from_frames(frames);

        // Consume all frames
        assert!(source.next().is_some());
        assert!(source.next().is_some());
        assert!(source.next().is_some());
        assert!(source.next().is_none());

        // Reset
        source.reset();

        // Should be back at the beginning
        assert_eq!(source.peek().unwrap().timestamp.as_micros(), 100);
        assert_eq!(source.position(), Some(0));
    }

    #[test]
    fn test_journal_datasource_position_tracking() {
        use super::JournalDataSource;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(300),
                Exchange::Deribit,
                FrameType::Trade,
            ),
        ];

        let mut source = JournalDataSource::from_frames(frames);

        assert_eq!(source.position(), Some(0));
        source.next();
        assert_eq!(source.position(), Some(1));
        source.next();
        assert_eq!(source.position(), Some(2));
        source.next();
        assert_eq!(source.position(), Some(3)); // Past the end
    }

    #[test]
    fn test_journal_datasource_peek_timestamp() {
        use super::JournalDataSource;

        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(5000),
            Exchange::Deribit,
            FrameType::Trade,
        )];

        let source = JournalDataSource::from_frames(frames);

        assert_eq!(source.peek_timestamp(), Some(Timestamp::from_micros(5000)));
    }

    #[test]
    fn test_journal_datasource_has_next() {
        use super::JournalDataSource;

        let frames = vec![DataFrame::empty(
            Timestamp::from_micros(100),
            Exchange::Deribit,
            FrameType::Trade,
        )];

        let mut source = JournalDataSource::from_frames(frames);

        assert!(source.has_next());
        source.next();
        assert!(!source.has_next());
    }

    #[test]
    fn test_journal_datasource_single_frame() {
        use super::JournalDataSource;

        let frames = vec![DataFrame::new(
            Timestamp::from_micros(42),
            Exchange::OKX,
            FrameType::BookSnapshot,
            b"single".to_vec(),
        )];

        let mut source = JournalDataSource::from_frames(frames);

        assert_eq!(source.frame_count(), Some(1));
        let frame = source.next().unwrap();
        assert_eq!(frame.timestamp.as_micros(), 42);
        assert_eq!(frame.exchange, Exchange::OKX);
        assert!(source.next().is_none());
    }

    #[test]
    fn test_journal_datasource_with_clock() {
        use super::super::SimulatedClock;
        use super::JournalDataSource;
        use blackbox_types::Clock;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(2000),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(3000),
                Exchange::Deribit,
                FrameType::Trade,
            ),
        ];

        let mut source = JournalDataSource::from_frames(frames);
        let clock = SimulatedClock::at_epoch();

        while let Some(ts) = source.peek_timestamp() {
            clock.advance_to(ts);
            source.next();
        }

        assert_eq!(clock.now().as_micros(), 3000);
    }

    #[test]
    fn test_journal_datasource_replay_loop() {
        use super::JournalDataSource;

        let frames = vec![
            DataFrame::empty(
                Timestamp::from_micros(100),
                Exchange::Deribit,
                FrameType::Trade,
            ),
            DataFrame::empty(
                Timestamp::from_micros(200),
                Exchange::Deribit,
                FrameType::Trade,
            ),
        ];

        let mut source = JournalDataSource::from_frames(frames);

        // First replay
        let mut count = 0;
        while source.next().is_some() {
            count += 1;
        }
        assert_eq!(count, 2);

        // Reset and replay again
        source.reset();
        count = 0;
        while source.next().is_some() {
            count += 1;
        }
        assert_eq!(count, 2);
    }
}
