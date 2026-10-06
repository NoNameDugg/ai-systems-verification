//! External review (2026-10) reproductions — blackbox
//!
//! Each test reproduces one finding from an independent code review of
//! v1.1.0. They were committed `#[ignore]`d-and-failing before any fix so the
//! defect is in the tree as a test, not as prose. When the fix lands the
//! `#[ignore]` is removed and the test must pass; a later regression fails it.
//!
//! Run them on purpose with `cargo test --test external_review_v12 -- --ignored`.
//!
//! Findings covered:
//!   #10 the tap writes a 32-byte checkpoint; the verifier requires 48.
//!   #11 `verify` reports every checkpoint as matched without parsing one.
//!   #12 after a crash (no footer) the zero-filled tail reads back as records.

use blackbox::cli::execute_verify;
use blackbox::journal::{
    JournalReader, JournalWriter, RecordType, WriterConfig, FILE_FOOTER_SIZE, FILE_HEADER_SIZE,
};
use blackbox::tap::{JournalTap, Tap};
use blackbox::verify::{Checkpoint, CheckpointBuilder};
use blackbox_types::Timestamp;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

fn test_path(suffix: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("review_v12_{}_{}.journal", suffix, id))
}

fn checkpoint_bytes(seq: u64, hash_byte: u8) -> [u8; 48] {
    CheckpointBuilder::new()
        .sequence(seq)
        .timestamp(1_704_067_200_000_000 + seq as i64)
        .hash([hash_byte; 32])
        .build()
        .to_bytes()
}

// =============================================================================
// #10 — a checkpoint the tap writes must be one the verifier can parse
// =============================================================================

#[test]
#[ignore = "review #10: JournalTap writes 32-byte checkpoints; Checkpoint::from_bytes needs 48"]
fn tap_checkpoint_is_parseable_by_verifier() {
    let path = test_path("tap_checkpoint");
    {
        let writer = JournalWriter::new(&path, WriterConfig::minimal()).unwrap();
        let tap = JournalTap::new(writer);
        tap.record_checkpoint(&[0xAB; 32], Timestamp::from_micros(1_000));
        tap.record_checkpoint(&[0xCD; 32], Timestamp::from_micros(2_000));
        drop(tap); // close -> footer
    }

    let records: Vec<_> = JournalReader::open(&path)
        .unwrap()
        .map(|r| r.unwrap())
        .filter(|r| r.record_type() == RecordType::Checkpoint)
        .collect();
    assert_eq!(records.len(), 2);

    let first = Checkpoint::from_bytes(&records[0].payload)
        .expect("tap-written checkpoint must parse as a verifier Checkpoint");
    let second = Checkpoint::from_bytes(&records[1].payload).unwrap();

    assert_eq!(*first.state_hash(), [0xAB; 32]);
    assert_eq!(first.timestamp(), 1_000);
    assert_eq!(*second.state_hash(), [0xCD; 32]);
    assert_eq!(second.timestamp(), 2_000);
    assert!(second.is_after(&first), "checkpoint sequence must advance");

    let _ = std::fs::remove_file(&path);
}

// =============================================================================
// #11 — `verify` must not claim matches it never checked, and must FAIL on
//        a checkpoint it cannot parse or that goes backwards
// =============================================================================

#[test]
#[ignore = "review #11: execute_verify sets checkpoints_matched = checkpoints_found and reports PASS"]
fn verify_does_not_claim_matches_without_a_hasher() {
    let path = test_path("verify_unverified");
    {
        let mut writer = JournalWriter::new(&path, WriterConfig::minimal()).unwrap();
        for seq in 0..3u64 {
            writer
                .write(RecordType::Trade, 1, format!("t{}", seq).as_bytes())
                .unwrap();
            writer
                .write(RecordType::Checkpoint, 0, &checkpoint_bytes(seq, seq as u8))
                .unwrap();
        }
        writer.close().unwrap();
    }

    let report = execute_verify(&path).unwrap();

    assert_eq!(report.stats.checkpoints_found, 3);
    assert_eq!(
        report.stats.checkpoints_matched, 0,
        "no state hasher was supplied, so nothing can have been matched"
    );
    assert!(
        !report.is_pass(),
        "a journal whose state hashes were never compared must not be reported PASS"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
#[ignore = "review #11: execute_verify never parses a checkpoint payload"]
fn verify_fails_on_a_corrupt_checkpoint() {
    // Case A: a malformed (32-byte) checkpoint — exactly what the tap writes today.
    let path_a = test_path("verify_malformed");
    {
        let mut writer = JournalWriter::new(&path_a, WriterConfig::minimal()).unwrap();
        writer.write(RecordType::Trade, 1, b"t0").unwrap();
        writer
            .write(RecordType::Checkpoint, 0, &[0xAB; 32])
            .unwrap();
        writer.close().unwrap();
    }
    let report_a = execute_verify(&path_a).unwrap();
    assert_eq!(report_a.stats.checkpoints_found, 1);
    assert_eq!(
        report_a.stats.errors, 1,
        "a 32-byte checkpoint is unparseable"
    );
    assert!(!report_a.is_pass());

    // Case B: well-formed checkpoints whose sequence goes backwards (0, 1, then 0).
    let path_b = test_path("verify_regress");
    {
        let mut writer = JournalWriter::new(&path_b, WriterConfig::minimal()).unwrap();
        writer
            .write(RecordType::Checkpoint, 0, &checkpoint_bytes(0, 0))
            .unwrap();
        writer
            .write(RecordType::Checkpoint, 0, &checkpoint_bytes(1, 1))
            .unwrap();
        writer
            .write(RecordType::Checkpoint, 0, &checkpoint_bytes(0, 2))
            .unwrap();
        writer.close().unwrap();
    }
    let report_b = execute_verify(&path_b).unwrap();
    assert_eq!(report_b.stats.checkpoints_found, 3);
    assert!(
        report_b.stats.checkpoints_mismatched >= 1,
        "a checkpoint sequence that goes backwards is a mismatch"
    );
    assert_eq!(report_b.stats.first_mismatch_sequence, Some(0));
    assert!(!report_b.is_pass());

    let _ = std::fs::remove_file(&path_a);
    let _ = std::fs::remove_file(&path_b);
}

// =============================================================================
// #12 — a crashed journal (valid prefix, no footer, zero-filled tail) must
//        read back as exactly its real records
// =============================================================================

#[test]
#[ignore = "review #12: the reader parses the zero-filled tail as an endless run of empty records"]
fn reader_stops_at_zero_filled_tail_when_footer_is_missing() {
    let path = test_path("crash_tail");
    let footer_offset: u64;
    {
        let mut writer = JournalWriter::new(&path, WriterConfig::minimal()).unwrap();
        for i in 0..5 {
            writer
                .write(RecordType::RawFrame, 1, format!("frame-{}", i).as_bytes())
                .unwrap();
        }
        writer.flush().unwrap();
        footer_offset = writer.write_position();
        writer.close().unwrap();
    }

    // Simulate the crash deterministically: erase the footer and the header's
    // record_count (both are written only on a clean close), leaving the
    // pre-allocated zero tail exactly as a killed process would.
    {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut probe = [0u8; 8];
        file.seek(SeekFrom::Start(footer_offset)).unwrap();
        file.read_exact(&mut probe).unwrap();
        assert_eq!(&probe, b"BLKBOXND", "sanity: footer sits at write_position");

        file.seek(SeekFrom::Start(footer_offset)).unwrap();
        file.write_all(&[0u8; FILE_FOOTER_SIZE]).unwrap();
        // record_count lives at header offset 0x38 (see journal/format.rs)
        assert!(0x38 + 8 <= FILE_HEADER_SIZE);
        file.seek(SeekFrom::Start(0x38)).unwrap();
        file.write_all(&[0u8; 8]).unwrap();
        file.sync_all().unwrap();
    }

    let reader = JournalReader::open(&path).unwrap();
    assert!(
        reader.read_footer().is_none(),
        "sanity: footer really is gone"
    );

    let mut real = 0usize;
    let mut phantom = 0usize;
    for result in reader {
        match result {
            Ok(record) if record.record_type() == RecordType::RawFrame => real += 1,
            Ok(_) => phantom += 1,
            Err(_) => break, // an explicit end-of-data error is acceptable
        }
    }

    assert_eq!(real, 5, "all five real records must be read");
    assert_eq!(
        phantom, 0,
        "zero-filled tail must not be surfaced as records (got {} phantom records)",
        phantom
    );

    let _ = std::fs::remove_file(&path);
}
