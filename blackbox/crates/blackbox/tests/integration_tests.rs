//! Full Integration Tests (T5.3)
//!
//! End-to-end integration tests verifying the complete BlackBox workflow:
//! - Cross-crate type compatibility (blackbox-types ↔ blackbox)
//! - All 3 TAP points (Ingress, Internal, Egress) working together
//! - Full record → replay → verify cycle
//! - Deterministic replay verification
//!
//! These tests validate production-like scenarios to ensure the BlackBox
//! system works correctly for trading infrastructure.

use blackbox::journal::{JournalReader, JournalWriter, RecordType, WriterConfig};
use blackbox::replay::{
    BufferedDataSource, DataFrame, FrameType, ReplayEngine, SimulatedClock, WarpConfig,
};
use blackbox::tap::{JournalTap, NullTap, Tap};
use blackbox::verify::{
    Checkpoint, CheckpointBuilder, CompareResult, ComparisonReport, Hashable, ReplayComparator,
    ReportStatus, StateHash, VerificationStats,
};
use blackbox_types::{Clock, Exchange, Timestamp};
use std::path::PathBuf;
use std::sync::Arc;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Create a unique test journal path.
fn create_test_path(suffix: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("integration_test_{}_{}.journal", suffix, id))
}

/// Clean up test files.
fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

// =============================================================================
// CROSS-CRATE TYPE COMPATIBILITY TESTS
// =============================================================================

/// Test: blackbox-types Exchange enum is compatible with blackbox usage.
#[test]
fn integration_exchange_type_compatibility() {
    // All exchange variants should be usable in tap methods
    let exchanges = [
        Exchange::Deribit,
        Exchange::Binance,
        Exchange::Unknown,
        Exchange::Bybit,
        Exchange::OKX,
    ];

    let tap = NullTap;
    let timestamp = Timestamp::from_micros(1_704_067_200_000_000);

    for exchange in exchanges {
        // Should not panic
        tap.record_ingress(exchange, b"test payload", timestamp);
        tap.record_egress(exchange, b"order payload", timestamp);
    }
}

/// Test: blackbox-types Timestamp is compatible with blackbox usage.
#[test]
fn integration_timestamp_type_compatibility() {
    // Test various timestamp edge cases
    let timestamps = [
        Timestamp::from_micros(0),
        Timestamp::from_micros(1_704_067_200_000_000), // 2024-01-01
        Timestamp::from_micros(i64::MAX),
        Timestamp::from_micros(i64::MIN),
    ];

    let tap = NullTap;

    for ts in timestamps {
        // Should not panic
        tap.record_ingress(Exchange::Deribit, b"data", ts);
        tap.record_internal(0x0010, b"state", ts);

        // Timestamp should round-trip correctly
        let micros = ts.as_micros();
        let restored = Timestamp::from_micros(micros);
        assert_eq!(ts.as_micros(), restored.as_micros());
    }
}

/// Test: Clock trait compatibility between blackbox-types and blackbox.
#[test]
fn integration_clock_trait_compatibility() {
    // SimulatedClock implements blackbox_types::Clock
    let clock = SimulatedClock::new(Timestamp::from_micros(1000));

    // Clock operations should work
    let now1 = clock.now();
    assert_eq!(now1.as_micros(), 1000);

    clock.advance(500); // advance takes i64 micros
    let now2 = clock.now();
    assert_eq!(now2.as_micros(), 1500);

    // Pause/resume should work
    clock.pause();
    assert!(clock.is_paused());
    clock.resume();
    assert!(!clock.is_paused());
}

// =============================================================================
// TAP POINT INTEGRATION TESTS
// =============================================================================

/// Test: NullTap works correctly for all tap points.
#[test]
fn integration_null_tap_all_points() {
    let tap = NullTap;
    let ts = Timestamp::from_micros(1_704_067_200_000_000);

    // TAP-1: Ingress (WebSocket frame)
    tap.record_ingress(Exchange::Deribit, b"ws frame data", ts);

    // TAP-2: Internal (OrderBook state change)
    tap.record_internal(0x0010, b"book snapshot", ts);
    tap.record_internal(0x0011, b"book delta", ts);

    // TAP-3: Egress (Order submission)
    tap.record_egress(Exchange::Binance, b"order submit", ts);

    // Checkpoint
    tap.record_checkpoint(&[0xABu8; 32], ts);

    // NullTap should always be inactive
    assert!(!tap.is_active());
}

/// Test: JournalTap records all tap points correctly.
#[test]
fn integration_journal_tap_all_points() {
    let path = create_test_path("journal_tap_all");
    let config = WriterConfig::minimal();

    // Scope for writer/tap lifetime
    {
        let writer = JournalWriter::new(&path, config).expect("create writer");
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1_704_067_200_000_000);

        // JournalTap should be active
        assert!(tap.is_active());

        // TAP-1: Ingress
        tap.record_ingress(Exchange::Deribit, b"ingress_payload", ts);

        // TAP-2: Internal
        tap.record_internal(0x0010, b"internal_payload", ts);

        // TAP-3: Egress
        tap.record_egress(Exchange::Binance, b"egress_payload", ts);

        // Checkpoint
        tap.record_checkpoint(&[0xCDu8; 32], ts);

        // Wait for async writes to complete
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Tap dropped here, which should close the writer
    }

    // Read back and verify
    let reader = JournalReader::open(&path).expect("open reader");
    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read records");

    // Should have at least 4 records (ingress, internal, egress, checkpoint)
    assert!(
        records.len() >= 4,
        "Expected at least 4 records, got {}",
        records.len()
    );

    cleanup(&path);
}

/// Test: Tap works with Arc for shared access.
#[test]
fn integration_tap_shared_access() {
    let tap = Arc::new(NullTap);
    let ts = Timestamp::from_micros(1000);

    // Clone Arc and use from multiple "threads" (simulated)
    let tap1 = Arc::clone(&tap);
    let tap2 = Arc::clone(&tap);

    tap1.record_ingress(Exchange::Deribit, b"from tap1", ts);
    tap2.record_egress(Exchange::Binance, b"from tap2", ts);

    // Both should complete without issue
    assert!(!tap.is_active());
}

/// Test: Tap works with Box<dyn Tap>.
#[test]
fn integration_tap_dyn_trait() {
    let tap: Box<dyn Tap> = Box::new(NullTap);
    let ts = Timestamp::from_micros(1000);

    // All methods should work through trait object
    tap.record_ingress(Exchange::Deribit, b"ingress", ts);
    tap.record_internal(0x0010, b"internal", ts);
    tap.record_egress(Exchange::Binance, b"egress", ts);
    tap.record_checkpoint(&[0u8; 32], ts);
    assert!(!tap.is_active());
}

// =============================================================================
// FULL WORKFLOW TESTS
// =============================================================================

/// Test: Complete record → read → verify workflow.
#[test]
fn integration_full_record_read_verify() {
    let path = create_test_path("full_workflow");
    let config = WriterConfig::minimal();

    // Phase 1: Record events
    {
        let mut writer = JournalWriter::new(&path, config).expect("create writer");

        // Simulate a trading session
        for i in 0..100 {
            let ts = (i as i64 * 1000) + 1_704_067_200_000_000;

            // Every 10 events, insert a checkpoint
            if i % 10 == 0 {
                let checkpoint = CheckpointBuilder::new()
                    .sequence(i as u64)
                    .timestamp(ts)
                    .hash([i as u8; 32])
                    .build();
                writer
                    .write(RecordType::Checkpoint, 0, &checkpoint.to_bytes())
                    .expect("write checkpoint");
            } else if i % 3 == 0 {
                // Trade event
                writer
                    .write(RecordType::Trade, 1, format!("trade_{}", i).as_bytes())
                    .expect("write trade");
            } else {
                // Quote update
                writer
                    .write(
                        RecordType::QuoteUpdate,
                        1,
                        format!("quote_{}", i).as_bytes(),
                    )
                    .expect("write quote");
            }
        }

        writer.close().expect("close writer");
    }

    // Phase 2: Read and verify structure
    let reader = JournalReader::open(&path).expect("open reader");
    assert_eq!(reader.record_count(), 100);

    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read all");

    // Verify checkpoint count
    let checkpoint_count = records
        .iter()
        .filter(|r| r.record_type() == RecordType::Checkpoint)
        .count();
    assert_eq!(checkpoint_count, 10); // 0, 10, 20, ..., 90

    // Phase 3: Verify checkpoints are readable
    for record in records
        .iter()
        .filter(|r| r.record_type() == RecordType::Checkpoint)
    {
        let checkpoint = Checkpoint::from_bytes(&record.payload).expect("parse checkpoint");
        assert!(checkpoint.sequence() % 10 == 0);
    }

    // Phase 4: Generate verification report
    let stats = VerificationStats {
        events_processed: records.len() as u64,
        checkpoints_found: checkpoint_count as u64,
        checkpoints_matched: checkpoint_count as u64,
        checkpoints_mismatched: 0,
        errors: 0,
        first_mismatch_sequence: None,
    };

    let report = ComparisonReport::from_stats(&stats, &[]);
    assert!(report.is_pass());
    assert_eq!(report.status, ReportStatus::Pass);

    cleanup(&path);
}

/// Test: State hash determinism across multiple runs.
#[test]
fn integration_state_hash_determinism() {
    // Same data should produce identical hashes
    let data1 = b"order_book_state_snapshot_data";
    let data2 = b"order_book_state_snapshot_data";

    let hash1 = StateHash::hash_once(data1);
    let hash2 = StateHash::hash_once(data2);

    assert_eq!(hash1, hash2);

    // Different data should produce different hashes
    let data3 = b"different_state_data";
    let hash3 = StateHash::hash_once(data3);
    assert_ne!(hash1, hash3);

    // Hash should be non-zero
    assert_ne!(hash1, [0u8; 32]);
}

/// Test: Checkpoint sequence ordering.
#[test]
fn integration_checkpoint_ordering() {
    let checkpoint1 = CheckpointBuilder::new()
        .sequence(10)
        .timestamp(1000)
        .hash([1u8; 32])
        .build();

    let checkpoint2 = CheckpointBuilder::new()
        .sequence(20)
        .timestamp(2000)
        .hash([2u8; 32])
        .build();

    let checkpoint3 = CheckpointBuilder::new()
        .sequence(15)
        .timestamp(1500)
        .hash([3u8; 32])
        .build();

    // Sequence ordering
    assert!(checkpoint2.is_after(&checkpoint1));
    assert!(checkpoint3.is_after(&checkpoint1));
    assert!(checkpoint2.is_after(&checkpoint3));
}

/// Test: Comparator match and mismatch detection.
#[test]
fn integration_comparator_workflow() {
    let mut comparator = ReplayComparator::new();

    // Matching checkpoints
    let result1 = comparator.compare(1, 1000, &[0xAAu8; 32], &[0xAAu8; 32]);
    assert!(matches!(result1, CompareResult::Match));

    let result2 = comparator.compare(2, 2000, &[0xBBu8; 32], &[0xBBu8; 32]);
    assert!(matches!(result2, CompareResult::Match));

    // Mismatching checkpoint
    let result3 = comparator.compare(3, 3000, &[0xCCu8; 32], &[0xDDu8; 32]);
    assert!(matches!(result3, CompareResult::Mismatch { .. }));

    // Check mismatch tracking
    let mismatches = comparator.mismatches();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].sequence, 3);
}

// =============================================================================
// REPLAY ENGINE INTEGRATION TESTS
// =============================================================================

/// Test: ReplayEngine with DataSource integration.
#[test]
fn integration_replay_engine_data_source() {
    // Create a data source with test frames
    let frames = vec![
        DataFrame {
            timestamp: Timestamp::from_micros(1000),
            exchange: Exchange::Deribit,
            frame_type: FrameType::WebSocketText,
            payload: b"frame1".to_vec(),
        },
        DataFrame {
            timestamp: Timestamp::from_micros(2000),
            exchange: Exchange::Binance,
            frame_type: FrameType::Trade,
            payload: b"frame2".to_vec(),
        },
        DataFrame {
            timestamp: Timestamp::from_micros(3000),
            exchange: Exchange::Unknown,
            frame_type: FrameType::BookSnapshot,
            payload: b"frame3".to_vec(),
        },
    ];

    let source = BufferedDataSource::new(frames);

    // Create replay engine with data source and default warp config
    let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());

    // Must call play() before stepping
    engine.play();

    // Process all frames - step() returns StepResult, check frame field
    let mut processed = 0;
    loop {
        let result = engine.step();
        if result.frame.is_some() {
            processed += 1;
        } else {
            break;
        }
    }

    assert_eq!(processed, 3);
}

/// Test: ReplayEngine with WarpConfig (skip-idle).
#[test]
fn integration_replay_engine_warp_mode() {
    let frames = vec![
        DataFrame {
            timestamp: Timestamp::from_micros(1_000_000), // 1s
            exchange: Exchange::Deribit,
            frame_type: FrameType::Trade,
            payload: b"trade1".to_vec(),
        },
        DataFrame {
            timestamp: Timestamp::from_micros(5_000_000), // 5s (4s gap)
            exchange: Exchange::Deribit,
            frame_type: FrameType::Trade,
            payload: b"trade2".to_vec(),
        },
    ];

    let source = BufferedDataSource::new(frames);
    let warp_config = WarpConfig {
        idle_threshold_us: 1_000_000, // 1s threshold
        max_warp_factor: Some(100.0),
    };

    let mut engine = ReplayEngine::with_data_source(source, warp_config);

    // Must call play() before run_to_completion
    engine.play();

    // Run to completion - returns count of processed frames
    let processed = engine.run_to_completion();

    // Should process both frames
    assert_eq!(processed, 2);
}

/// Test: SimulatedClock step-through mode.
#[test]
fn integration_simulated_clock_step_through() {
    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    // Start paused
    clock.pause();
    assert!(clock.is_paused());

    // Advance while paused (advance takes i64 micros)
    clock.advance(1000);
    assert_eq!(clock.now().as_micros(), 1000);

    // Resume
    clock.resume();
    assert!(!clock.is_paused());

    // Advance while running
    clock.advance(500);
    assert_eq!(clock.now().as_micros(), 1500);
}

// =============================================================================
// MULTI-THREADED INTEGRATION TESTS
// =============================================================================

/// Test: Concurrent tap access from multiple threads.
#[test]
fn integration_concurrent_tap_access() {
    let tap = Arc::new(NullTap);
    let mut handles = vec![];

    for thread_id in 0..4 {
        let tap_clone = Arc::clone(&tap);
        let handle = std::thread::spawn(move || {
            let ts = Timestamp::from_micros(thread_id as i64 * 1000);

            for i in 0..1000 {
                let exchange = match i % 3 {
                    0 => Exchange::Deribit,
                    1 => Exchange::Binance,
                    _ => Exchange::Unknown,
                };

                tap_clone.record_ingress(exchange, b"concurrent payload", ts);
                tap_clone.record_internal(0x0010, b"state", ts);
                tap_clone.record_egress(exchange, b"order", ts);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("thread should not panic");
    }

    // NullTap should still be inactive
    assert!(!tap.is_active());
}

/// Test: Concurrent journal writing from multiple threads.
#[test]
fn integration_concurrent_journal_writing() {
    let path = create_test_path("concurrent_journal");
    let config = WriterConfig {
        ring_buffer_capacity: 65536,
        file_size: 64 * 1024 * 1024,
        compress_schema: false,
        sync_on_close: false,
        prefault_pages: false,
    };

    // Scope for tap lifetime
    {
        let writer = JournalWriter::new(&path, config).expect("create writer");
        let tap = Arc::new(JournalTap::new(writer));

        let mut handles = vec![];

        for thread_id in 0u8..4 {
            let tap_clone = Arc::clone(&tap);
            let handle = std::thread::spawn(move || {
                for i in 0..100 {
                    let ts = Timestamp::from_micros((thread_id as i64 * 1000) + (i as i64));
                    tap_clone.record_ingress(Exchange::Deribit, &[thread_id, i as u8], ts);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread complete");
        }

        // Wait for async writes
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Tap and writer dropped here
    }

    // Verify records were written
    let reader = JournalReader::open(&path).expect("open");
    let count = reader.count();
    assert!(count > 0, "Expected some records, got {}", count);

    cleanup(&path);
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

/// Test: Empty payload handling.
#[test]
fn integration_empty_payload() {
    let path = create_test_path("empty_payload");
    let config = WriterConfig::minimal();

    {
        let mut writer = JournalWriter::new(&path, config).expect("create");
        writer
            .write(RecordType::RawFrame, 1, b"")
            .expect("write empty");
        writer.close().expect("close");
    }

    let reader = JournalReader::open(&path).expect("open");
    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload.len(), 0);

    cleanup(&path);
}

/// Test: Maximum payload size handling.
#[test]
fn integration_large_payload() {
    let path = create_test_path("large_payload");
    let config = WriterConfig::minimal();

    // 1MB payload
    let large_payload: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

    {
        let mut writer = JournalWriter::new(&path, config).expect("create");
        writer
            .write(RecordType::BookSnapshot, 1, &large_payload)
            .expect("write large");
        writer.close().expect("close");
    }

    let reader = JournalReader::open(&path).expect("open");
    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload, large_payload);

    cleanup(&path);
}

/// Test: Timestamp boundary values.
#[test]
fn integration_timestamp_boundaries() {
    let path = create_test_path("timestamp_boundaries");
    let config = WriterConfig::minimal();

    let timestamps = [
        0i64,
        1_704_067_200_000_000, // 2024-01-01
        i64::MAX / 2,
    ];

    {
        let mut writer = JournalWriter::new(&path, config).expect("create");
        for ts in timestamps {
            let checkpoint = CheckpointBuilder::new()
                .sequence(1)
                .timestamp(ts)
                .hash([0u8; 32])
                .build();
            writer
                .write(RecordType::Checkpoint, 0, &checkpoint.to_bytes())
                .expect("write");
        }
        writer.close().expect("close");
    }

    let reader = JournalReader::open(&path).expect("open");
    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read");

    assert_eq!(records.len(), timestamps.len());

    // Verify timestamps are preserved
    for (i, record) in records.iter().enumerate() {
        let checkpoint = Checkpoint::from_bytes(&record.payload).expect("parse");
        assert_eq!(checkpoint.timestamp(), timestamps[i]);
    }

    cleanup(&path);
}

/// Test: All exchange types in a single session.
#[test]
fn integration_all_exchanges() {
    let path = create_test_path("all_exchanges");
    let config = WriterConfig::minimal();

    let exchanges = [
        (Exchange::Deribit, 1u8),
        (Exchange::Binance, 2u8),
        (Exchange::Unknown, 0u8),
        (Exchange::Bybit, 4u8),
        (Exchange::OKX, 5u8),
    ];

    {
        let writer = JournalWriter::new(&path, config).expect("create");
        let tap = JournalTap::new(writer);
        let ts = Timestamp::from_micros(1_704_067_200_000_000);

        for (exchange, _) in exchanges {
            tap.record_ingress(exchange, b"test", ts);
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        // Tap dropped here, which closes the writer
    }

    let reader = JournalReader::open(&path).expect("open");
    let records: Vec<_> = reader.collect::<Result<_, _>>().expect("read");

    assert_eq!(records.len(), exchanges.len());

    cleanup(&path);
}

// =============================================================================
// HASHABLE TRAIT INTEGRATION TESTS
// =============================================================================

/// Test: Hashable trait with custom types.
#[test]
fn integration_hashable_custom_types() {
    #[derive(Debug)]
    struct OrderBookState {
        bids: Vec<(f64, f64)>, // (price, qty)
        asks: Vec<(f64, f64)>,
        sequence: u64,
    }

    impl Hashable for OrderBookState {
        fn hash_into(&self, hasher: &mut StateHash) {
            hasher.update_sequence(self.sequence);
            for (price, qty) in &self.bids {
                hasher.update_raw(&price.to_le_bytes());
                hasher.update_raw(&qty.to_le_bytes());
            }
            for (price, qty) in &self.asks {
                hasher.update_raw(&price.to_le_bytes());
                hasher.update_raw(&qty.to_le_bytes());
            }
        }
    }

    let state1 = OrderBookState {
        bids: vec![(50000.0, 1.0), (49999.0, 2.0)],
        asks: vec![(50001.0, 1.5), (50002.0, 3.0)],
        sequence: 12345,
    };

    let state2 = OrderBookState {
        bids: vec![(50000.0, 1.0), (49999.0, 2.0)],
        asks: vec![(50001.0, 1.5), (50002.0, 3.0)],
        sequence: 12345,
    };

    let state3 = OrderBookState {
        bids: vec![(50000.0, 1.0)],
        asks: vec![(50001.0, 1.5)],
        sequence: 12345,
    };

    let hash1 = state1.state_hash();
    let hash2 = state2.state_hash();
    let hash3 = state3.state_hash();

    // Same state = same hash
    assert_eq!(hash1, hash2);

    // Different state = different hash
    assert_ne!(hash1, hash3);
}

// =============================================================================
// REPORT FORMAT INTEGRATION TESTS
// =============================================================================

/// Test: Report generation in all formats.
#[test]
fn integration_report_all_formats() {
    let stats = VerificationStats {
        events_processed: 10000,
        checkpoints_found: 100,
        checkpoints_matched: 98,
        checkpoints_mismatched: 2,
        errors: 0,
        first_mismatch_sequence: Some(42),
    };

    let report = ComparisonReport::from_stats(&stats, &[]);

    // Text format
    let text = report.to_text();
    assert!(text.contains("FAIL"));
    assert!(text.contains("10000"));
    assert!(text.contains("98"));

    // JSON format
    let json = report.to_json();
    assert!(json.contains("\"status\": \"FAIL\""));
    assert!(json.contains("\"events_processed\": 10000"));

    // Summary format
    let summary = report.to_summary();
    assert!(summary.contains("10000 events"));
    assert!(summary.contains("98/100"));
}

/// Test: Report with recommendations.
#[test]
fn integration_report_recommendations() {
    let stats = VerificationStats {
        events_processed: 1000,
        checkpoints_found: 10,
        checkpoints_matched: 5,
        checkpoints_mismatched: 5,
        errors: 0,
        first_mismatch_sequence: Some(3),
    };

    let report = ComparisonReport::from_stats(&stats, &[]);

    // Should have recommendations for failures
    assert!(!report.recommendations.is_empty());

    // Status should be Fail
    assert_eq!(report.status, ReportStatus::Fail);
}
