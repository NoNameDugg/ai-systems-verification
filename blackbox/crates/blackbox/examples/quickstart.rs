//! The README Quick Start as a compiled, runnable example.
//!
//! `cargo run --example quickstart` writes a three-record journal to a
//! temporary directory, reads it back, replays two frames at warp speed,
//! and prints both counts. CI runs `cargo build --all-targets` on every
//! push, so this file (and therefore the README snippet) cannot rot.
//! The only difference from the README is that the journal goes to a
//! temp path instead of `session.journal` in the working directory.

use blackbox::journal::{JournalReader, JournalWriter, WriterConfig};
use blackbox::replay::{BufferedDataSource, DataFrame, FrameType, ReplayEngine, WarpConfig};
use blackbox::tap::{JournalTap, Tap};
use blackbox_types::{Clock, Exchange, SystemClock, Timestamp};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("session.journal");

    // ----- Recording -----
    let clock = SystemClock;
    let writer = JournalWriter::new(&path, WriterConfig::default())?;
    let tap = JournalTap::new(writer);

    // Record events (ingress / internal / egress tap points)
    tap.record_ingress(Exchange::Deribit, b"websocket frame", clock.now());
    tap.record_internal(0x0010, b"orderbook snapshot", clock.now());
    tap.record_egress(Exchange::Deribit, b"order payload", clock.now());
    drop(tap); // flush + close

    // ----- Reading back and replaying -----
    // Read the journal back
    let reader = JournalReader::open(&path)?;
    let records: Vec<_> = reader.filter_map(|r| r.ok()).collect();

    // Replay a frame sequence at warp speed (idle gaps skipped)
    let frames = vec![
        DataFrame::new(
            Timestamp::from_micros(1_000),
            Exchange::Deribit,
            FrameType::Trade,
            vec![],
        ),
        DataFrame::new(
            Timestamp::from_micros(2_000),
            Exchange::Deribit,
            FrameType::Trade,
            vec![],
        ),
    ];
    let mut engine =
        ReplayEngine::with_data_source(BufferedDataSource::new(frames), WarpConfig::instant());
    engine.play();
    let replayed = engine.run_to_completion();

    println!("records read back: {}", records.len());
    println!("frames replayed: {}", replayed);
    Ok(())
}
