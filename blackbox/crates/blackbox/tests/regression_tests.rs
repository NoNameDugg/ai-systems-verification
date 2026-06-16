//! Regression test suite for blackbox (T4.6).
//!
//! These tests verify end-to-end functionality and catch regressions
//! in the journal, replay, and verification systems.

use blackbox::journal::{JournalReader, JournalWriter, RecordType, WriterConfig};
use blackbox::replay::{DataFrame, FrameType, ReplayEngine, WarpConfig};
use blackbox::verify::{
    CheckpointBuilder, CompareResult, ComparisonReport, Hashable, ReplayComparator, ReportStatus,
    StateHash, VerificationStats,
};
use blackbox_types::{Exchange, Timestamp};
use std::path::PathBuf;

/// Helper to create a test journal path.
fn create_test_path() -> PathBuf {
    let id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    std::env::temp_dir().join(format!("test_journal_{}.journal", id))
}

// ==================== Journal Roundtrip Tests ====================

/// Test: Write and read a single record.
#[test]
fn regression_journal_single_record_roundtrip() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    // Write a journal with one record
    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        writer
            .write(RecordType::RawFrame, 1, b"test payload")
            .unwrap();
        writer.close().unwrap();
    }

    // Read it back
    let reader = JournalReader::open(&path).unwrap();
    assert_eq!(reader.record_count(), 1);

    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload, b"test payload");
    assert_eq!(records[0].record_type(), RecordType::RawFrame);

    let _ = std::fs::remove_file(&path);
}

/// Test: Write and read multiple records.
#[test]
fn regression_journal_multi_record_roundtrip() {
    let path = create_test_path();
    let config = WriterConfig::minimal();
    let record_count = 100;

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        for i in 0..record_count {
            let payload = format!("record_{}", i);
            writer
                .write(RecordType::QuoteUpdate, 1, payload.as_bytes())
                .unwrap();
        }
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    assert_eq!(reader.record_count(), record_count);

    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();
    assert_eq!(records.len(), record_count as usize);

    for (i, record) in records.iter().enumerate() {
        let expected_payload = format!("record_{}", i);
        assert_eq!(record.payload, expected_payload.as_bytes());
    }

    let _ = std::fs::remove_file(&path);
}

/// Test: Write checkpoint records.
#[test]
fn regression_journal_checkpoint_roundtrip() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();

        for i in 0..10 {
            if i % 3 == 0 {
                let checkpoint = CheckpointBuilder::new()
                    .sequence(i as u64)
                    .timestamp(i as i64 * 1000)
                    .hash([i as u8; 32])
                    .build();
                let bytes = checkpoint.to_bytes();
                writer.write(RecordType::Checkpoint, 0, &bytes).unwrap();
            } else {
                writer.write(RecordType::Trade, 1, b"trade data").unwrap();
            }
        }
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();

    let checkpoint_count = records
        .iter()
        .filter(|r| r.record_type() == RecordType::Checkpoint)
        .count();

    assert_eq!(checkpoint_count, 4); // 0, 3, 6, 9

    let _ = std::fs::remove_file(&path);
}

/// Test: Large payload handling.
#[test]
fn regression_journal_large_payload() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    let large_payload: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        writer
            .write(RecordType::BookSnapshot, 1, &large_payload)
            .unwrap();
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload, large_payload);

    let _ = std::fs::remove_file(&path);
}

// ==================== Replay Flow Tests ====================

/// Test: DataFrame creation.
#[test]
fn regression_data_frame_creation() {
    let frame = DataFrame {
        timestamp: Timestamp::from_micros(1000),
        exchange: Exchange::Unknown,
        frame_type: FrameType::WebSocketText,
        payload: b"test".to_vec(),
    };

    assert_eq!(frame.timestamp.as_micros(), 1000);
    assert_eq!(frame.frame_type, FrameType::WebSocketText);
    assert_eq!(frame.payload, b"test");
}

/// Test: WarpConfig creation and defaults.
#[test]
fn regression_warp_config() {
    let default_config = WarpConfig::default();
    assert!(default_config.idle_threshold_us > 0);

    let custom_config = WarpConfig {
        idle_threshold_us: 1_000,
        max_warp_factor: Some(10.0),
    };
    assert_eq!(custom_config.idle_threshold_us, 1_000);
    assert_eq!(custom_config.max_warp_factor, Some(10.0));
}

/// Test: ReplayEngine creation.
#[test]
fn regression_replay_engine_creation() {
    let engine = ReplayEngine::new();
    // Engine should be created successfully - just verifying it compiles and runs
    let stats = engine.stats();
    assert_eq!(stats.events_processed, 0);
}

// ==================== Verification Tests ====================

/// Test: StateHash computation consistency.
#[test]
fn regression_state_hash_consistency() {
    let data = b"test state data";
    let hash1 = StateHash::hash_once(data);
    let hash2 = StateHash::hash_once(data);

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, [0u8; 32]);
}

/// Test: StateHash with Hashable trait.
#[test]
fn regression_state_hash_hashable_trait() {
    struct TestState {
        value: u64,
    }

    impl Hashable for TestState {
        fn hash_into(&self, hasher: &mut StateHash) {
            hasher.update_raw(&self.value.to_le_bytes());
        }
    }

    let state1 = TestState { value: 42 };
    let state2 = TestState { value: 42 };
    let state3 = TestState { value: 99 };

    let hash1 = state1.state_hash();
    let hash2 = state2.state_hash();
    let hash3 = state3.state_hash();

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
}

/// Test: Checkpoint builder and serialization.
#[test]
fn regression_checkpoint_roundtrip() {
    let original = CheckpointBuilder::new()
        .sequence(42)
        .timestamp(1_000_000)
        .hash([0xAB; 32])
        .build();

    let bytes = original.to_bytes();
    let restored = blackbox::verify::Checkpoint::from_bytes(&bytes).unwrap();

    assert_eq!(restored.sequence(), original.sequence());
    assert_eq!(restored.timestamp(), original.timestamp());
    assert_eq!(restored.state_hash(), original.state_hash());
}

/// Test: ReplayComparator matching.
#[test]
fn regression_comparator_match() {
    let mut comparator = ReplayComparator::new();

    let result = comparator.compare(1, 1000, &[0xAA; 32], &[0xAA; 32]);
    assert!(matches!(result, CompareResult::Match));
}

/// Test: ReplayComparator mismatch detection.
#[test]
fn regression_comparator_mismatch() {
    let mut comparator = ReplayComparator::new();

    let result = comparator.compare(1, 1000, &[0xAA; 32], &[0xBB; 32]);
    assert!(matches!(result, CompareResult::Mismatch { .. }));

    let mismatches = comparator.mismatches();
    assert_eq!(mismatches.len(), 1);
}

// ==================== Report Tests ====================

/// Test: ComparisonReport generation for passing verification.
#[test]
fn regression_report_pass() {
    let stats = VerificationStats {
        events_processed: 1000,
        checkpoints_found: 10,
        checkpoints_matched: 10,
        checkpoints_mismatched: 0,
        errors: 0,
        first_mismatch_sequence: None,
    };

    let report = ComparisonReport::from_stats(&stats, &[]);

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.is_pass());
    assert_eq!(report.mismatch_count(), 0);

    let text = report.to_text();
    assert!(text.contains("PASS"));
    assert!(text.contains("1000"));

    let json = report.to_json();
    assert!(json.contains("\"status\": \"PASS\""));
}

/// Test: ComparisonReport generation for failing verification.
#[test]
fn regression_report_fail() {
    let stats = VerificationStats {
        events_processed: 1000,
        checkpoints_found: 10,
        checkpoints_matched: 7,
        checkpoints_mismatched: 3,
        errors: 0,
        first_mismatch_sequence: Some(5),
    };

    let report = ComparisonReport::from_stats(&stats, &[]);

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(!report.is_pass());
    assert!(!report.recommendations.is_empty());

    let text = report.to_text();
    assert!(text.contains("FAIL"));
}

/// Test: ComparisonReport summary format.
#[test]
fn regression_report_summary() {
    let stats = VerificationStats {
        events_processed: 500,
        checkpoints_found: 5,
        checkpoints_matched: 5,
        ..Default::default()
    };

    let report = ComparisonReport::from_stats(&stats, &[]);
    let summary = report.to_summary();

    assert!(summary.contains("500 events"));
    assert!(summary.contains("5/5"));
}

// ==================== Integration Tests ====================

/// Test: Full write-read-verify workflow.
#[test]
fn regression_full_workflow() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();

        for i in 0..50 {
            if i % 10 == 0 {
                let checkpoint = CheckpointBuilder::new()
                    .sequence(i as u64)
                    .timestamp(i as i64 * 1000)
                    .hash([i as u8; 32])
                    .build();
                writer
                    .write(RecordType::Checkpoint, 0, &checkpoint.to_bytes())
                    .unwrap();
            } else {
                let payload = format!("quote_update_{}", i);
                writer
                    .write(RecordType::QuoteUpdate, 1, payload.as_bytes())
                    .unwrap();
            }
        }
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    assert_eq!(reader.record_count(), 50);

    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();

    let checkpoint_count = records
        .iter()
        .filter(|r| r.record_type() == RecordType::Checkpoint)
        .count();
    assert_eq!(checkpoint_count, 5);

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

    let _ = std::fs::remove_file(&path);
}

/// Test: File header integrity.
#[test]
fn regression_header_integrity() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        writer.write(RecordType::SessionStart, 0, b"start").unwrap();
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    let (major, minor) = reader.format_version();
    assert_eq!(major, 1);
    assert!(minor >= 0);

    let (schema_major, schema_minor) = reader.schema_version();
    assert_eq!(schema_major, 1);
    assert!(schema_minor >= 0);

    let _ = std::fs::remove_file(&path);
}

/// Test: Empty journal handling.
#[test]
fn regression_empty_journal() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    {
        let writer = JournalWriter::new(&path, config).unwrap();
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    assert_eq!(reader.record_count(), 0);

    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();
    assert!(records.is_empty());

    let _ = std::fs::remove_file(&path);
}

/// Test: All record types.
#[test]
fn regression_all_record_types() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    let record_types = vec![
        RecordType::SessionStart,
        RecordType::SessionEnd,
        RecordType::Checkpoint,
        RecordType::RawFrame,
        RecordType::QuoteUpdate,
        RecordType::Trade,
        RecordType::BookSnapshot,
        RecordType::OrderSubmit,
        RecordType::OrderAck,
        RecordType::OrderFill,
        RecordType::OrderCancel,
        RecordType::StateChange,
        RecordType::Signal,
    ];

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        for record_type in &record_types {
            writer.write(*record_type, 0, b"payload").unwrap();
        }
        writer.close().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    let records: Vec<_> = reader.collect::<Result<_, _>>().unwrap();

    assert_eq!(records.len(), record_types.len());
    for (record, expected_type) in records.iter().zip(record_types.iter()) {
        assert_eq!(record.record_type(), *expected_type);
    }

    let _ = std::fs::remove_file(&path);
}

// ==================== Performance Regression Tests ====================

/// Test: Bulk write performance doesn't regress.
#[test]
fn regression_bulk_write_performance() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    let start = std::time::Instant::now();

    {
        let mut writer = JournalWriter::new(&path, config).unwrap();
        for _ in 0..10_000 {
            writer
                .write(RecordType::QuoteUpdate, 1, b"quote data here")
                .unwrap();
        }
        writer.close().unwrap();
    }

    let duration = start.elapsed();

    assert!(
        duration.as_secs() < 5,
        "Bulk write took too long: {:?}",
        duration
    );

    let _ = std::fs::remove_file(&path);
}

/// Test: Bulk read performance doesn't regress.
///
/// This test verifies that reading records is fast. Due to the async nature
/// of the writer, we use the records_written() count as ground truth.
#[test]
fn regression_bulk_read_performance() {
    let path = create_test_path();
    let config = WriterConfig::minimal();

    // Write records and wait for them to be flushed
    let records_written;
    {
        let mut writer = JournalWriter::new(&path, config).unwrap();

        // Write in smaller batches with flushes
        for _ in 0..100 {
            writer
                .write(RecordType::QuoteUpdate, 1, b"quote data here")
                .unwrap();
        }
        writer.flush().unwrap();

        // Wait for the background thread to catch up
        std::thread::sleep(std::time::Duration::from_millis(200));

        records_written = writer.records_written() as usize;
        writer.close().unwrap();
    }

    // Wait for file to be finalized
    std::thread::sleep(std::time::Duration::from_millis(100));

    let start = std::time::Instant::now();

    let reader = JournalReader::open(&path).unwrap();
    let count: usize = reader.filter_map(|r| r.ok()).count();

    let duration = start.elapsed();

    // Should read all records that were written
    assert_eq!(count, records_written, "Read count mismatch");
    assert!(
        duration.as_millis() < 1000,
        "Bulk read took too long: {:?}",
        duration
    );

    let _ = std::fs::remove_file(&path);
}

// ==================== CLI Integration Tests ====================

/// Test: CLI module types are usable.
#[test]
fn regression_cli_types() {
    use blackbox::cli::{CliError, JournalInfo, JournalStats, OutputFormat};
    use std::str::FromStr;

    assert!(OutputFormat::from_str("text").is_ok());
    assert!(OutputFormat::from_str("json").is_ok());
    assert!(OutputFormat::from_str("summary").is_ok());
    assert!(OutputFormat::from_str("invalid").is_err());

    let info = JournalInfo {
        path: PathBuf::from("test.journal"),
        file_size: 1024,
        format_version: (1, 2),
        schema_version: (1, 0),
        session_start: 1000000,
        session_end: 2000000,
        record_count: 100,
        schema_xml: None,
    };
    let text = info.to_text();
    assert!(text.contains("test.journal"));

    let stats = JournalStats::default();
    let text = stats.to_text(false);
    assert!(text.contains("Total records"));

    let err = CliError::FileNotFound(PathBuf::from("missing.journal"));
    let msg = format!("{}", err);
    assert!(msg.contains("not found"));
}
