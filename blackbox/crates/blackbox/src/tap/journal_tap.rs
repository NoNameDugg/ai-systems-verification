//! Journal tap - Records events to journal file.
//!
//! The `JournalTap` provides a production-ready implementation of the [`Tap`] trait
//! that records events to a BlackBox journal file. It uses a mutex-based
//! synchronization strategy for thread-safe access from multiple instrumentation points.
//!
//! ## Design
//!
//! The JournalTap wraps a [`JournalWriter`] and translates tap method calls into
//! journal record writes with appropriate record types:
//!
//! | Tap Method | Record Type | Use Case |
//! |------------|-------------|----------|
//! | `record_ingress` | `RawFrame` | WebSocket frames from exchanges |
//! | `record_internal` | `StateChange` | Internal state changes (orderbook updates) |
//! | `record_egress` | `OrderSubmit` | Outbound order submissions |
//! | `record_checkpoint` | `Checkpoint` | State hash for replay verification |
//!
//! ## Thread Safety
//!
//! `JournalTap` is `Send + Sync` and can be safely shared across threads using `Arc`.
//! The internal mutex ensures serialized access to the journal writer.
//!
//! **Note:** This mutex-based design is a Phase 2 implementation. Future versions
//! may use a lock-free design for lower latency.
//!
//! ## Usage Pattern
//!
//! ```rust,no_run
//! use blackbox::tap::{JournalTap, Tap};
//! use blackbox::journal::{JournalWriter, WriterConfig};
//! use blackbox_types::{Exchange, Timestamp};
//! use std::sync::Arc;
//!
//! // Create a journal writer
//! let config = WriterConfig::minimal();
//! let writer = JournalWriter::new("session.journal", config).unwrap();
//!
//! // Wrap in JournalTap for thread-safe recording
//! let tap = Arc::new(JournalTap::new(writer));
//!
//! // Record events from any thread
//! let ts = Timestamp::from_micros(1704067200_000_000);
//! tap.record_ingress(Exchange::Deribit, b"ws frame", ts);
//! ```
//!
//! ## Feature Flag Pattern
//!
//! Typically used with feature flags to switch between recording and no-op:
//!
//! ```rust,ignore
//! use blackbox::tap::{JournalTap, NullTap, Tap};
//!
//! #[cfg(feature = "blackbox")]
//! type ActiveTap = JournalTap;
//!
//! #[cfg(not(feature = "blackbox"))]
//! type ActiveTap = NullTap;
//! ```

use super::traits::Tap;
use crate::journal::{JournalWriter, RecordType};
use blackbox_types::{Exchange, Timestamp};
use std::sync::Mutex;

/// Tap implementation that records events to a journal file.
///
/// `JournalTap` provides thread-safe event recording to a BlackBox journal.
/// It wraps a [`JournalWriter`] with mutex synchronization for shared access.
///
/// # Performance Contract
///
/// | Property | Value |
/// |----------|-------|
/// | Size | `size_of::<Mutex<Option<JournalWriter>>>() + 1` bytes |
/// | `record_ingress` | Mutex lock + disk write (depends on I/O) |
/// | `record_egress` | Mutex lock + disk write (depends on I/O) |
/// | `record_internal` | Mutex lock + disk write (depends on I/O) |
/// | `record_checkpoint` | Mutex lock + disk write (depends on I/O) |
/// | `is_active` | ~1ns (reads bool field) |
///
/// # Thread Safety
///
/// `JournalTap` is `Send + Sync` and can be safely shared across threads.
/// The mutex ensures serialized access to the underlying journal writer.
///
/// # Example
///
/// ```rust,no_run
/// use blackbox::tap::{JournalTap, Tap};
/// use blackbox::journal::{JournalWriter, WriterConfig};
/// use blackbox_types::{Exchange, Timestamp};
///
/// let config = WriterConfig::minimal();
/// let writer = JournalWriter::new("session.journal", config).unwrap();
/// let tap = JournalTap::new(writer);
///
/// let ts = Timestamp::from_micros(1704067200_000_000);
/// tap.record_ingress(Exchange::Deribit, b"market data", ts);
///
/// assert!(tap.is_active());
/// ```
pub struct JournalTap {
    // TODO: Replace with lock-free design in Phase 2
    writer: Mutex<Option<JournalWriter>>,
    active: bool,
}

impl std::fmt::Debug for JournalTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalTap")
            .field("active", &self.active)
            .field("writer", &"<JournalWriter>")
            .finish()
    }
}

impl JournalTap {
    /// Create a new journal tap with the given writer.
    pub fn new(writer: JournalWriter) -> Self {
        Self {
            writer: Mutex::new(Some(writer)),
            active: true,
        }
    }

    /// Create an inactive journal tap (for disabled recording).
    pub fn inactive() -> Self {
        Self {
            writer: Mutex::new(None),
            active: false,
        }
    }
}

impl Tap for JournalTap {
    fn record_ingress(&self, exchange: Exchange, payload: &[u8], _timestamp: Timestamp) {
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                let _ = writer.write(RecordType::RawFrame, exchange.as_u8(), payload);
            }
        }
    }

    fn record_internal(&self, event_type: u16, payload: &[u8], _timestamp: Timestamp) {
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                let _ = writer.write(RecordType::StateChange, event_type as u8, payload);
            }
        }
    }

    fn record_egress(&self, exchange: Exchange, payload: &[u8], _timestamp: Timestamp) {
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                let _ = writer.write(RecordType::OrderSubmit, exchange.as_u8(), payload);
            }
        }
    }

    fn record_checkpoint(&self, state_hash: &[u8; 32], _timestamp: Timestamp) {
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                let _ = writer.write(RecordType::Checkpoint, 0, state_hash);
            }
        }
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{JournalReader, ReaderConfig, WriterConfig};
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;

    // ==================== Send + Sync Verification ====================

    #[test]
    fn test_journal_tap_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<JournalTap>();
    }

    #[test]
    fn test_journal_tap_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<JournalTap>();
    }

    #[test]
    fn test_journal_tap_implements_tap() {
        fn assert_tap<T: Tap>() {}
        assert_tap::<JournalTap>();
    }

    // ==================== Inactive Tap Tests ====================

    #[test]
    fn test_inactive_tap_is_inactive() {
        let tap = JournalTap::inactive();
        assert!(!tap.is_active());
    }

    #[test]
    fn test_inactive_tap_record_ingress_no_panic() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);

        // Should not panic even though no writer exists
        tap.record_ingress(Exchange::Deribit, b"test data", ts);
    }

    #[test]
    fn test_inactive_tap_record_egress_no_panic() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Binance, b"order data", ts);
    }

    #[test]
    fn test_inactive_tap_record_internal_no_panic() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0010, b"state change", ts);
    }

    #[test]
    fn test_inactive_tap_record_checkpoint_no_panic() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);
        let hash = [0u8; 32];

        tap.record_checkpoint(&hash, ts);
    }

    // ==================== Active Tap Tests ====================

    #[test]
    fn test_active_tap_is_active() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        assert!(tap.is_active());
    }

    #[test]
    fn test_active_tap_writes_ingress() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_ingress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        // Write some ingress records
        tap.record_ingress(Exchange::Deribit, b"frame1", ts);
        tap.record_ingress(Exchange::Binance, b"frame2", ts);
        tap.record_ingress(Exchange::Bybit, b"frame3", ts);

        // Drop tap to close the file
        drop(tap);

        // Verify records were written
        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_active_tap_writes_egress() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_egress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Deribit, b"order1", ts);
        tap.record_egress(Exchange::Binance, b"order2", ts);

        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_active_tap_writes_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_checkpoint.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash = [0xAB; 32];

        tap.record_checkpoint(&hash, ts);

        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 1);
        // Verify the checkpoint payload contains the hash
        if let Ok(ref record) = records[0] {
            assert_eq!(record.payload.len(), 32);
        }
    }

    // ==================== Thread Safety Tests ====================

    #[test]
    fn test_journal_tap_concurrent_writes() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_concurrent.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = Arc::new(JournalTap::new(writer));

        let mut handles = vec![];

        // Spawn multiple threads writing concurrently
        for i in 0..4 {
            let tap_clone = Arc::clone(&tap);
            handles.push(thread::spawn(move || {
                let ts = Timestamp::from_micros(1000);
                for j in 0..10 {
                    let payload = format!("thread{}:record{}", i, j);
                    tap_clone.record_ingress(Exchange::Deribit, payload.as_bytes(), ts);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Drop the Arc to release the tap
        drop(tap);

        // Verify all records were written (4 threads * 10 records = 40)
        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 40);
    }

    #[test]
    fn test_journal_tap_mixed_concurrent_operations() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_mixed.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = Arc::new(JournalTap::new(writer));
        let ts = Timestamp::from_micros(1000);

        let mut handles = vec![];

        // Thread 1: ingress
        let tap1 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..10 {
                tap1.record_ingress(Exchange::Deribit, b"ingress", ts);
            }
        }));

        // Thread 2: egress
        let tap2 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..10 {
                tap2.record_egress(Exchange::Binance, b"egress", ts);
            }
        }));

        // Thread 3: internal
        let tap3 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            for _ in 0..10 {
                tap3.record_internal(0x0010, b"internal", ts);
            }
        }));

        // Thread 4: checkpoint
        let tap4 = Arc::clone(&tap);
        handles.push(thread::spawn(move || {
            let hash = [0u8; 32];
            for _ in 0..10 {
                tap4.record_checkpoint(&hash, ts);
            }
        }));

        for handle in handles {
            handle.join().unwrap();
        }

        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        // 4 threads * 10 records = 40 total
        assert_eq!(records.len(), 40);
    }

    // ==================== Record Type Verification Tests ====================

    #[test]
    fn test_ingress_uses_raw_frame_record_type() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_record_type_ingress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Deribit, b"frame data", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type(), RecordType::RawFrame);
    }

    #[test]
    fn test_egress_uses_order_submit_record_type() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_record_type_egress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Binance, b"order data", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type(), RecordType::OrderSubmit);
    }

    #[test]
    fn test_internal_uses_state_change_record_type() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_record_type_internal.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0010, b"state data", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type(), RecordType::StateChange);
    }

    #[test]
    fn test_checkpoint_uses_checkpoint_record_type() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_record_type_checkpoint.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash = [0xAB; 32];

        tap.record_checkpoint(&hash, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_type(), RecordType::Checkpoint);
    }

    // ==================== Payload Content Verification Tests ====================

    #[test]
    fn test_ingress_preserves_payload_content() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_payload_ingress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let payload = b"Hello, WebSocket Frame!";

        tap.record_ingress(Exchange::Deribit, payload, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), payload);
    }

    #[test]
    fn test_egress_preserves_payload_content() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_payload_egress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let payload = b"Order: BUY BTC-PERP 1.0 @ 50000";

        tap.record_egress(Exchange::Binance, payload, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), payload);
    }

    #[test]
    fn test_internal_preserves_payload_content() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_payload_internal.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let payload = b"BookSnapshot: 10 bids, 10 asks";

        tap.record_internal(0x0010, payload, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), payload);
    }

    #[test]
    fn test_checkpoint_preserves_hash() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_payload_checkpoint.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C,
            0x1D, 0x1E, 0x1F, 0x20,
        ];

        tap.record_checkpoint(&hash, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), &hash);
    }

    // ==================== Exchange ID Verification Tests ====================

    #[test]
    fn test_ingress_all_exchanges() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_exchange_ingress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Unknown, b"unknown", ts);
        tap.record_ingress(Exchange::Deribit, b"deribit", ts);
        tap.record_ingress(Exchange::Binance, b"binance", ts);
        tap.record_ingress(Exchange::Bybit, b"bybit", ts);
        tap.record_ingress(Exchange::OKX, b"okx", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 5);
        assert_eq!(records[0].exchange_id(), Exchange::Unknown.as_u8());
        assert_eq!(records[1].exchange_id(), Exchange::Deribit.as_u8());
        assert_eq!(records[2].exchange_id(), Exchange::Binance.as_u8());
        assert_eq!(records[3].exchange_id(), Exchange::Bybit.as_u8());
        assert_eq!(records[4].exchange_id(), Exchange::OKX.as_u8());
    }

    #[test]
    fn test_egress_all_exchanges() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_exchange_egress.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_egress(Exchange::Unknown, b"order", ts);
        tap.record_egress(Exchange::Deribit, b"order", ts);
        tap.record_egress(Exchange::Binance, b"order", ts);
        tap.record_egress(Exchange::Bybit, b"order", ts);
        tap.record_egress(Exchange::OKX, b"order", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 5);
        assert_eq!(records[0].exchange_id(), Exchange::Unknown.as_u8());
        assert_eq!(records[1].exchange_id(), Exchange::Deribit.as_u8());
        assert_eq!(records[2].exchange_id(), Exchange::Binance.as_u8());
        assert_eq!(records[3].exchange_id(), Exchange::Bybit.as_u8());
        assert_eq!(records[4].exchange_id(), Exchange::OKX.as_u8());
    }

    // ==================== Payload Edge Case Tests ====================

    #[test]
    fn test_empty_payload() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_empty_payload.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_ingress(Exchange::Deribit, b"", ts);
        tap.record_egress(Exchange::Binance, b"", ts);
        tap.record_internal(0x0010, b"", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 3);
        for record in &records {
            assert!(record.payload.is_empty());
        }
    }

    #[test]
    fn test_large_payload() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_large_payload.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let large_payload = vec![0xAB; 64 * 1024]; // 64KB

        tap.record_ingress(Exchange::Deribit, &large_payload, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.len(), 64 * 1024);
    }

    // ==================== Timestamp Edge Case Tests ====================

    #[test]
    fn test_epoch_timestamp() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_ts_epoch.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::EPOCH);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_min_timestamp() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_ts_min.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::MIN);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_max_timestamp() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_ts_max.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::MAX);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    // ==================== Event Type Boundary Tests ====================

    #[test]
    fn test_internal_event_type_zero() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_event_zero.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0x0000, b"event", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_internal_event_type_max() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_event_max.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        tap.record_internal(0xFFFF, b"event", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    // ==================== Checkpoint Hash Edge Case Tests ====================

    #[test]
    fn test_checkpoint_all_zeros_hash() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hash_zeros.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash = [0u8; 32];

        tap.record_checkpoint(&hash, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), &hash);
    }

    #[test]
    fn test_checkpoint_all_ones_hash() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hash_ones.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash = [0xFF; 32];

        tap.record_checkpoint(&hash, ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.as_slice(), &hash);
    }

    // ==================== Record Sequence Tests ====================

    #[test]
    fn test_multiple_record_types_interleaved() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_interleaved.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);
        let hash = [0xAB; 32];

        // Interleave all record types
        tap.record_ingress(Exchange::Deribit, b"ingress1", ts);
        tap.record_internal(0x0010, b"internal1", ts);
        tap.record_egress(Exchange::Binance, b"egress1", ts);
        tap.record_checkpoint(&hash, ts);
        tap.record_ingress(Exchange::Deribit, b"ingress2", ts);
        tap.record_internal(0x0011, b"internal2", ts);
        tap.record_egress(Exchange::Binance, b"egress2", ts);
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 7);
        assert_eq!(records[0].record_type(), RecordType::RawFrame);
        assert_eq!(records[1].record_type(), RecordType::StateChange);
        assert_eq!(records[2].record_type(), RecordType::OrderSubmit);
        assert_eq!(records[3].record_type(), RecordType::Checkpoint);
        assert_eq!(records[4].record_type(), RecordType::RawFrame);
        assert_eq!(records[5].record_type(), RecordType::StateChange);
        assert_eq!(records[6].record_type(), RecordType::OrderSubmit);
    }

    #[test]
    fn test_high_volume_sequential_writes() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_high_volume.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1000);

        // Write 100 records (minimal config has limited buffer)
        for i in 0..100 {
            let payload = format!("record_{}", i);
            tap.record_ingress(Exchange::Deribit, payload.as_bytes(), ts);
        }
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

        assert_eq!(records.len(), 100);
    }

    // ==================== Generic Usage Tests ====================

    #[test]
    fn test_journal_tap_in_box_dyn_tap() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_box_dyn.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap: Box<dyn Tap> = Box::new(JournalTap::new(writer));

        assert!(tap.is_active());
        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::from_micros(0));
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_journal_tap_in_arc_dyn_tap() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_arc_dyn.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap: Arc<dyn Tap> = Arc::new(JournalTap::new(writer));

        assert!(tap.is_active());
        tap.record_ingress(Exchange::Deribit, b"data", Timestamp::from_micros(0));
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_journal_tap_generic_function() {
        fn process_event<T: Tap>(tap: &T, data: &[u8]) -> bool {
            let ts = Timestamp::from_micros(1000);
            if tap.is_active() {
                tap.record_ingress(Exchange::Bybit, data, ts);
                true
            } else {
                false
            }
        }

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_generic_fn.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        // Active tap should return true
        assert!(process_event(&tap, b"test"));
        drop(tap);

        let reader =
            JournalReader::open_with_config(&path, ReaderConfig::skip_schema_hash()).unwrap();
        let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_inactive_tap_in_generic_function() {
        fn process_event<T: Tap>(tap: &T, data: &[u8]) -> bool {
            let ts = Timestamp::from_micros(1000);
            if tap.is_active() {
                tap.record_ingress(Exchange::Bybit, data, ts);
                true
            } else {
                false
            }
        }

        let tap = JournalTap::inactive();
        // Inactive tap should return false
        assert!(!process_event(&tap, b"test"));
    }

    // ==================== Debug Formatting Tests ====================

    #[test]
    fn test_journal_tap_debug_format() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_debug.journal");

        let config = WriterConfig::minimal();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        // JournalTap should be formattable with Debug
        let debug_str = format!("{:?}", tap);
        assert!(debug_str.contains("JournalTap"));
    }

    // ==================== Inactive Tap All Methods Tests ====================

    #[test]
    fn test_inactive_tap_all_exchanges() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);

        // All these should not panic
        tap.record_ingress(Exchange::Unknown, b"data", ts);
        tap.record_ingress(Exchange::Deribit, b"data", ts);
        tap.record_ingress(Exchange::Binance, b"data", ts);
        tap.record_ingress(Exchange::Bybit, b"data", ts);
        tap.record_ingress(Exchange::OKX, b"data", ts);
    }

    #[test]
    fn test_inactive_tap_large_payload() {
        let tap = JournalTap::inactive();
        let ts = Timestamp::from_micros(1000);
        let large_payload = vec![0u8; 1024 * 1024]; // 1MB

        // Should not panic
        tap.record_ingress(Exchange::Deribit, &large_payload, ts);
    }

    #[test]
    fn test_inactive_tap_is_always_inactive() {
        let tap = JournalTap::inactive();
        // Verify is_active() is consistent
        for _ in 0..100 {
            assert!(!tap.is_active());
        }
    }
}
