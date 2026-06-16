//! Benchmarks for Flash Snapshot Processing.
//!
//! # Performance Targets (per STANDARDS.md)
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Validate (50 levels) | < 10 μs |
//! | Diff (50 vs 50) | < 100 μs |
//! | Compute checksum | < 5 μs |
//! | Serialize JSON | < 20 μs |
//! | Serialize bincode | < 5 μs |
//! | Deserialize bincode | < 5 μs |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench snapshot_bench
//! cargo bench --bench snapshot_bench -- --test  # Quick verify
//! ```

use astra_flash::book::{
    BookSnapshot, OrderBook, OrderBookConfig, SnapshotDiff, SnapshotMetadata, SnapshotProcessor,
    SnapshotProcessorConfig, SnapshotValidator, ValidationConfig,
};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

/// Creates a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Creates a price level.
fn level(price: f64, quantity: Decimal) -> PriceLevel {
    PriceLevel::new(price, quantity, 1234567890)
}

/// Creates N bid levels starting at a price (descending).
fn create_bids(start_price: f64, count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| level(start_price - (i as f64 * 0.5), dec!(1)))
        .collect()
}

/// Creates N ask levels starting at a price (ascending).
fn create_asks(start_price: f64, count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| level(start_price + (i as f64 * 0.5), dec!(1)))
        .collect()
}

/// Creates a test snapshot with N levels per side.
fn create_snapshot(n: usize) -> BookSnapshot {
    BookSnapshot {
        instrument: test_instrument(),
        timestamp: 1234567890,
        bids: create_bids(50000.0, n),
        asks: create_asks(50001.0, n),
    }
}

/// Creates two snapshots that differ.
fn create_diff_pair(n: usize) -> (BookSnapshot, BookSnapshot) {
    let old = create_snapshot(n);

    // Create new with modifications
    let mut new_bids = old.bids.clone();
    let mut new_asks = old.asks.clone();

    // Modify some levels
    if !new_bids.is_empty() {
        new_bids[0].quantity = dec!(10); // Modify first bid
    }
    if new_bids.len() > 1 {
        new_bids.pop(); // Remove last bid
    }
    new_bids.push(level(49900.0, dec!(5))); // Add new bid

    let new = BookSnapshot {
        instrument: test_instrument(),
        timestamp: 1234567891,
        bids: new_bids,
        asks: new_asks,
    };

    (old, new)
}

// =============================================================================
// VALIDATION BENCHMARKS
// =============================================================================

fn bench_validate(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_validate");

    let validator = SnapshotValidator::new(ValidationConfig::default());

    for size in [10, 25, 50, 100].iter() {
        let bids = create_bids(50000.0, *size);
        let asks = create_asks(50001.0, *size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| validator.validate(black_box(&bids), black_box(&asks)));
            },
        );
    }

    group.finish();
}

fn bench_validate_components(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_validate_detail");

    let bids = create_bids(50000.0, 50);
    let asks = create_asks(50001.0, 50);

    // Default validation
    group.bench_function("default_config", |b| {
        let validator = SnapshotValidator::new(ValidationConfig::default());
        b.iter(|| validator.validate(black_box(&bids), black_box(&asks)));
    });

    // Strict validation
    group.bench_function("strict_config", |b| {
        let config = ValidationConfig {
            allow_crossed: false,
            allow_duplicates: false,
            allow_empty_side: false,
            min_levels_per_side: 1,
            max_price: 100_000.0,
            min_price: 0.01,
        };
        let validator = SnapshotValidator::new(config);
        b.iter(|| validator.validate(black_box(&bids), black_box(&asks)));
    });

    // Permissive validation
    group.bench_function("permissive_config", |b| {
        let config = ValidationConfig {
            allow_crossed: true,
            allow_duplicates: true,
            allow_empty_side: true,
            min_levels_per_side: 0,
            max_price: f64::MAX,
            min_price: 0.0,
        };
        let validator = SnapshotValidator::new(config);
        b.iter(|| validator.validate(black_box(&bids), black_box(&asks)));
    });

    group.finish();
}

// =============================================================================
// DIFF BENCHMARKS
// =============================================================================

fn bench_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_diff");

    let processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());

    for size in [10, 25, 50, 100].iter() {
        let (old, new) = create_diff_pair(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| processor.diff(black_box(&old), black_box(&new)));
            },
        );
    }

    group.finish();
}

fn bench_diff_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_diff_scenarios");

    let processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());

    // Identical snapshots (no changes)
    group.bench_function("identical_50", |b| {
        let snapshot = create_snapshot(50);
        let old = snapshot.clone();
        let new = snapshot;
        b.iter(|| processor.diff(black_box(&old), black_box(&new)));
    });

    // All levels changed
    group.bench_function("all_changed_50", |b| {
        let old = create_snapshot(50);
        let new = BookSnapshot {
            instrument: test_instrument(),
            timestamp: 1234567891,
            bids: create_bids(49000.0, 50), // Different prices
            asks: create_asks(49001.0, 50),
        };
        b.iter(|| processor.diff(black_box(&old), black_box(&new)));
    });

    // Half added, half removed
    group.bench_function("half_added_half_removed", |b| {
        let old = BookSnapshot {
            instrument: test_instrument(),
            timestamp: 1234567890,
            bids: create_bids(50000.0, 25),
            asks: create_asks(50001.0, 25),
        };
        let new = BookSnapshot {
            instrument: test_instrument(),
            timestamp: 1234567891,
            bids: create_bids(49000.0, 25), // All different
            asks: create_asks(49001.0, 25),
        };
        b.iter(|| processor.diff(black_box(&old), black_box(&new)));
    });

    group.finish();
}

// =============================================================================
// METADATA BENCHMARKS
// =============================================================================

fn bench_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_metadata");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotMetadata::from_snapshot(black_box(&snapshot), Some(12345)));
            },
        );
    }

    group.finish();
}

fn bench_checksum(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_checksum");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotMetadata::compute_checksum(black_box(&snapshot)));
            },
        );
    }

    group.finish();
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

fn bench_serialize_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_serialize_json");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotProcessor::serialize_json(black_box(&snapshot)));
            },
        );
    }

    group.finish();
}

fn bench_deserialize_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_deserialize_json");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);
        let json = SnapshotProcessor::serialize_json(&snapshot).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotProcessor::deserialize_json(black_box(&json)));
            },
        );
    }

    group.finish();
}

fn bench_serialize_bincode(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_serialize_bincode");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotProcessor::serialize_bincode(black_box(&snapshot)));
            },
        );
    }

    group.finish();
}

fn bench_deserialize_bincode(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_deserialize_bincode");

    for size in [10, 25, 50, 100].iter() {
        let snapshot = create_snapshot(*size);
        let bytes = SnapshotProcessor::serialize_bincode(&snapshot).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                b.iter(|| SnapshotProcessor::deserialize_bincode(black_box(&bytes)));
            },
        );
    }

    group.finish();
}

fn bench_format_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_format_comparison");

    let snapshot = create_snapshot(50);

    // JSON serialize
    group.bench_function("json_serialize", |b| {
        b.iter(|| SnapshotProcessor::serialize_json(black_box(&snapshot)));
    });

    // Bincode serialize
    group.bench_function("bincode_serialize", |b| {
        b.iter(|| SnapshotProcessor::serialize_bincode(black_box(&snapshot)));
    });

    let json_bytes = SnapshotProcessor::serialize_json(&snapshot).unwrap();
    let bincode_bytes = SnapshotProcessor::serialize_bincode(&snapshot).unwrap();

    // JSON deserialize
    group.bench_function("json_deserialize", |b| {
        b.iter(|| SnapshotProcessor::deserialize_json(black_box(&json_bytes)));
    });

    // Bincode deserialize
    group.bench_function("bincode_deserialize", |b| {
        b.iter(|| SnapshotProcessor::deserialize_bincode(black_box(&bincode_bytes)));
    });

    group.finish();
}

// =============================================================================
// INTEGRATION BENCHMARKS
// =============================================================================

fn bench_apply_with_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_apply_with_validation");

    for size in [10, 25, 50, 100].iter() {
        let bids = create_bids(50000.0, *size);
        let asks = create_asks(50001.0, *size);
        let processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
                b.iter(|| {
                    processor.apply_with_validation(
                        &mut book,
                        black_box(bids.clone()),
                        black_box(asks.clone()),
                        black_box(1234567890),
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_apply_without_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_apply_without_validation");

    let processor = SnapshotProcessor::new(SnapshotProcessorConfig {
        validate_before_apply: false,
        ..SnapshotProcessorConfig::default()
    });

    for size in [10, 25, 50, 100].iter() {
        let bids = create_bids(50000.0, *size);
        let asks = create_asks(50001.0, *size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", size)),
            size,
            |b, _| {
                let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
                b.iter(|| {
                    processor.apply_with_validation(
                        &mut book,
                        black_box(bids.clone()),
                        black_box(asks.clone()),
                        black_box(1234567890),
                    )
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// SEQUENCE BENCHMARKS
// =============================================================================

fn bench_sequence_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_sequence");

    group.bench_function("record_sequence", |b| {
        let mut processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());
        let mut seq = 0u64;
        b.iter(|| {
            seq += 1;
            processor.record_sequence(black_box(seq));
        });
    });

    group.bench_function("check_gap_no_gap", |b| {
        let mut processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());
        processor.record_sequence(1000);
        b.iter(|| processor.check_sequence_gap(black_box(1001)));
    });

    group.bench_function("check_gap_with_gap", |b| {
        let mut processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());
        processor.record_sequence(1000);
        b.iter(|| processor.check_sequence_gap(black_box(1005)));
    });

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_throughput");
    group.throughput(Throughput::Elements(1000));

    // 1K validation operations
    group.bench_function("1K_validations", |b| {
        let validator = SnapshotValidator::new(ValidationConfig::default());
        let bids = create_bids(50000.0, 50);
        let asks = create_asks(50001.0, 50);

        b.iter(|| {
            for _ in 0..1000 {
                let _ = validator.validate(&bids, &asks);
            }
        });
    });

    // 1K diff operations
    group.bench_function("1K_diffs", |b| {
        let processor = SnapshotProcessor::new(SnapshotProcessorConfig::default());
        let (old, new) = create_diff_pair(50);

        b.iter(|| {
            for _ in 0..1000 {
                let _ = processor.diff(&old, &new);
            }
        });
    });

    // 1K bincode serializations
    group.bench_function("1K_bincode_serialize", |b| {
        let snapshot = create_snapshot(50);

        b.iter(|| {
            for _ in 0..1000 {
                let _ = SnapshotProcessor::serialize_bincode(&snapshot);
            }
        });
    });

    group.finish();
}

// =============================================================================
// MEMORY BENCHMARKS
// =============================================================================

fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_memory");

    // Clone cost for diff
    group.bench_function("diff_clone_cost", |b| {
        let diff = SnapshotDiff {
            added_bids: create_bids(100.0, 10),
            removed_bids: vec![99.0, 98.0, 97.0],
            modified_bids: vec![],
            added_asks: vec![],
            removed_asks: vec![],
            modified_asks: vec![],
            best_bid_changed: true,
            best_ask_changed: false,
            spread_changed: true,
        };

        b.iter(|| black_box(diff.clone()));
    });

    // Clone cost for metadata
    group.bench_function("metadata_clone_cost", |b| {
        let snapshot = create_snapshot(50);
        let metadata = SnapshotMetadata::from_snapshot(&snapshot, Some(12345));

        b.iter(|| black_box(metadata.clone()));
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(validation, bench_validate, bench_validate_components,);

criterion_group!(diff, bench_diff, bench_diff_scenarios,);

criterion_group!(metadata, bench_metadata, bench_checksum,);

criterion_group!(
    serialization,
    bench_serialize_json,
    bench_deserialize_json,
    bench_serialize_bincode,
    bench_deserialize_bincode,
    bench_format_comparison,
);

criterion_group!(
    integration,
    bench_apply_with_validation,
    bench_apply_without_validation,
);

criterion_group!(sequence, bench_sequence_tracking,);

criterion_group!(throughput, bench_throughput,);

criterion_group!(memory, bench_memory_patterns,);

criterion_main!(
    validation,
    diff,
    metadata,
    serialization,
    integration,
    sequence,
    throughput,
    memory,
);
