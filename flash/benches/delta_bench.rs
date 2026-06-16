//! Benchmarks for Delta Processing module.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Single delta processing | < 1 μs |
//! | Batch processing (10 updates) | < 5 μs |
//! | Sequence gap detection | O(1), < 50 ns |
//! | Delta validation | < 200 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! # Run all delta benchmarks
//! cargo bench --bench delta_bench
//!
//! # Run specific benchmark
//! cargo bench --bench delta_bench -- process_delta
//!
//! # Save baseline
//! cargo bench --bench delta_bench -- --save-baseline main
//!
//! # Compare to baseline
//! cargo bench --bench delta_bench -- --baseline main
//! ```

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};

use astra_flash::book::{
    DeltaBatch, DeltaProcessor, DeltaProcessorConfig, DeltaUpdate, DeltaValidationConfig,
    DeltaValidator, OrderBook, OrderBookConfig, SequenceConfig, SequenceTracker,
};
use astra_flash::core::types::{now_micros, Exchange, Instrument, PriceLevel, Side};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Creates a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Creates a test OrderBook.
fn test_book() -> OrderBook {
    OrderBook::new(test_instrument(), OrderBookConfig::default())
}

/// Creates a populated test OrderBook.
fn populated_book(levels: usize) -> OrderBook {
    let mut book = test_book();
    let ts = now_micros();

    let bids: Vec<_> = (0..levels)
        .map(|i| PriceLevel::new(50000.0 - (i as f64) * 0.5, dec!(1), ts))
        .collect();
    let asks: Vec<_> = (0..levels)
        .map(|i| PriceLevel::new(50001.0 + (i as f64) * 0.5, dec!(1), ts))
        .collect();

    book.apply_snapshot(bids, asks, ts);
    book
}

/// Creates a price level.
fn level(price: f64, qty: Decimal) -> PriceLevel {
    PriceLevel::new(price, qty, now_micros())
}

/// Creates N levels.
fn create_levels(count: usize, base_price: f64) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| level(base_price + (i as f64) * 0.1, dec!(1)))
        .collect()
}

// =============================================================================
// DELTA UPDATE BENCHMARKS
// =============================================================================

fn bench_delta_update_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaUpdate::creation");

    group.bench_function("new", |b| {
        b.iter(|| {
            let levels = vec![level(50000.0, dec!(10))];
            black_box(DeltaUpdate::new(Side::Bid, levels, now_micros()))
        })
    });

    group.bench_function("with_sequence", |b| {
        b.iter(|| {
            let levels = vec![level(50000.0, dec!(10))];
            black_box(DeltaUpdate::with_sequence(
                42,
                Side::Bid,
                levels,
                now_micros(),
            ))
        })
    });

    group.bench_function("5_levels", |b| {
        b.iter(|| {
            let levels = create_levels(5, 50000.0);
            black_box(DeltaUpdate::new(Side::Bid, levels, now_micros()))
        })
    });

    group.finish();
}

// =============================================================================
// DELTA BATCH BENCHMARKS
// =============================================================================

fn bench_delta_batch_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaBatch::creation");

    for update_count in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(update_count),
            update_count,
            |b, &count| {
                b.iter(|| {
                    let updates: Vec<_> = (0..count)
                        .map(|i| {
                            let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                            DeltaUpdate::new(side, vec![level(50000.0, dec!(1))], now_micros())
                        })
                        .collect();
                    black_box(DeltaBatch::new(updates, now_micros()))
                })
            },
        );
    }

    group.finish();
}

fn bench_batch_total_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaBatch::total_levels");

    for update_count in [5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(update_count),
            update_count,
            |b, &count| {
                let updates: Vec<_> = (0..count)
                    .map(|i| {
                        let levels = create_levels(3, 50000.0 + (i as f64) * 10.0);
                        DeltaUpdate::new(Side::Bid, levels, now_micros())
                    })
                    .collect();
                let batch = DeltaBatch::new(updates, now_micros());

                b.iter(|| black_box(batch.total_levels()))
            },
        );
    }

    group.finish();
}

// =============================================================================
// SEQUENCE TRACKER BENCHMARKS
// =============================================================================

fn bench_sequence_tracker(c: &mut Criterion) {
    let mut group = c.benchmark_group("SequenceTracker");

    group.bench_function("record_sequential", |b| {
        b.iter_batched(
            || {
                let mut tracker = SequenceTracker::new(SequenceConfig::default());
                tracker.record(1);
                (tracker, 2u64)
            },
            |(mut tracker, seq)| black_box(tracker.record(seq)),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("record_with_gap", |b| {
        b.iter_batched(
            || {
                let mut tracker = SequenceTracker::new(SequenceConfig::default());
                tracker.record(1);
                (tracker, 100u64)
            },
            |(mut tracker, seq)| black_box(tracker.record(seq)),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("state_check", |b| {
        let mut tracker = SequenceTracker::new(SequenceConfig::default());
        tracker.record(1);
        b.iter(|| black_box(tracker.state()))
    });

    group.bench_function("needs_recovery_check", |b| {
        let mut tracker = SequenceTracker::new(SequenceConfig::default());
        tracker.record(1);
        tracker.record(100); // Create gap
        b.iter(|| black_box(tracker.needs_recovery()))
    });

    group.bench_function("reset", |b| {
        b.iter_batched(
            || {
                let mut tracker = SequenceTracker::new(SequenceConfig::default());
                tracker.record(1);
                tracker.record(100);
                tracker
            },
            |mut tracker| {
                tracker.reset();
                black_box(tracker)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

// =============================================================================
// DELTA VALIDATOR BENCHMARKS
// =============================================================================

fn bench_delta_validator(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaValidator");

    let validator = DeltaValidator::new(DeltaValidationConfig::default());

    group.bench_function("validate_single_level", |b| {
        let update = DeltaUpdate::new(Side::Bid, vec![level(50000.0, dec!(10))], now_micros());
        b.iter(|| black_box(validator.validate(&update)))
    });

    for level_count in [5, 10, 20, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new("validate_levels", level_count),
            level_count,
            |b, &count| {
                let levels = create_levels(count, 50000.0);
                let update = DeltaUpdate::new(Side::Bid, levels, now_micros());
                b.iter(|| black_box(validator.validate(&update)))
            },
        );
    }

    group.bench_function("validate_batch_10", |b| {
        let updates: Vec<_> = (0..10)
            .map(|i| {
                DeltaUpdate::new(
                    if i % 2 == 0 { Side::Bid } else { Side::Ask },
                    vec![level(50000.0 + (i as f64), dec!(1))],
                    now_micros(),
                )
            })
            .collect();
        let batch = DeltaBatch::new(updates, now_micros());

        b.iter(|| black_box(validator.validate_batch(&batch)))
    });

    group.finish();
}

// =============================================================================
// DELTA PROCESSOR BENCHMARKS
// =============================================================================

fn bench_process_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaProcessor::process_delta");
    group.throughput(Throughput::Elements(1));

    // Single level update
    group.bench_function("single_level", |b| {
        b.iter_batched(
            || {
                let mut processor = DeltaProcessor::with_defaults();
                let mut book = populated_book(50);
                processor.reset_with_sequence(0);
                let update = DeltaUpdate::with_sequence(
                    1,
                    Side::Bid,
                    vec![level(50000.0, dec!(15))],
                    now_micros(),
                );
                (processor, book, update)
            },
            |(mut processor, mut book, update)| {
                black_box(processor.process_delta(&mut book, update))
            },
            BatchSize::SmallInput,
        )
    });

    // Update at best bid
    group.bench_function("best_bid_update", |b| {
        b.iter_batched(
            || {
                let mut processor = DeltaProcessor::with_defaults();
                let mut book = populated_book(50);
                processor.reset_with_sequence(0);
                // Update the best bid price
                let update = DeltaUpdate::with_sequence(
                    1,
                    Side::Bid,
                    vec![level(50000.0, dec!(100))],
                    now_micros(),
                );
                (processor, book, update)
            },
            |(mut processor, mut book, update)| {
                black_box(processor.process_delta(&mut book, update))
            },
            BatchSize::SmallInput,
        )
    });

    // Multiple levels in one update
    for level_count in [5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("levels", level_count),
            level_count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut processor = DeltaProcessor::with_defaults();
                        let mut book = populated_book(50);
                        processor.reset_with_sequence(0);
                        let levels = create_levels(count, 49900.0);
                        let update = DeltaUpdate::with_sequence(1, Side::Bid, levels, now_micros());
                        (processor, book, update)
                    },
                    |(mut processor, mut book, update)| {
                        black_box(processor.process_delta(&mut book, update))
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    // Without validation
    group.bench_function("no_validation", |b| {
        let config = DeltaProcessorConfig {
            validate_before_apply: false,
            ..Default::default()
        };

        b.iter_batched(
            || {
                let mut processor = DeltaProcessor::new(config.clone());
                let mut book = populated_book(50);
                processor.reset_with_sequence(0);
                let update = DeltaUpdate::with_sequence(
                    1,
                    Side::Bid,
                    vec![level(50000.0, dec!(15))],
                    now_micros(),
                );
                (processor, book, update)
            },
            |(mut processor, mut book, update)| {
                black_box(processor.process_delta(&mut book, update))
            },
            BatchSize::SmallInput,
        )
    });

    // Without sequence tracking
    group.bench_function("no_sequence_tracking", |b| {
        let config = DeltaProcessorConfig {
            track_sequences: false,
            ..Default::default()
        };

        b.iter_batched(
            || {
                let mut processor = DeltaProcessor::new(config.clone());
                let mut book = populated_book(50);
                let update =
                    DeltaUpdate::new(Side::Bid, vec![level(50000.0, dec!(15))], now_micros());
                (processor, book, update)
            },
            |(mut processor, mut book, update)| {
                black_box(processor.process_delta(&mut book, update))
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn bench_process_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaProcessor::process_batch");

    for update_count in [2, 5, 10, 20].iter() {
        group.throughput(Throughput::Elements(*update_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(update_count),
            update_count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mut processor = DeltaProcessor::with_defaults();
                        let mut book = populated_book(50);
                        processor.reset_with_sequence(0);

                        let updates: Vec<_> = (0..count)
                            .map(|i| {
                                let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                                let price = if side == Side::Bid {
                                    49999.0 - (i as f64)
                                } else {
                                    50002.0 + (i as f64)
                                };
                                DeltaUpdate::new(side, vec![level(price, dec!(1))], now_micros())
                            })
                            .collect();
                        let batch = DeltaBatch::with_sequence(1, updates, now_micros());

                        (processor, book, batch)
                    },
                    |(mut processor, mut book, batch)| {
                        black_box(processor.process_batch(&mut book, batch))
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

// =============================================================================
// BOOK SIZE SCALING BENCHMARKS
// =============================================================================

fn bench_process_delta_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaProcessor::scaling");

    for book_size in [10, 50, 100, 200, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("book_levels", book_size),
            book_size,
            |b, &size| {
                b.iter_batched(
                    || {
                        let mut processor = DeltaProcessor::with_defaults();
                        let mut book = populated_book(size);
                        processor.reset_with_sequence(0);
                        let update = DeltaUpdate::with_sequence(
                            1,
                            Side::Bid,
                            vec![level(50000.0, dec!(15))],
                            now_micros(),
                        );
                        (processor, book, update)
                    },
                    |(mut processor, mut book, update)| {
                        black_box(processor.process_delta(&mut book, update))
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

// =============================================================================
// SEQUENTIAL DELTA BENCHMARKS
// =============================================================================

fn bench_sequential_deltas(c: &mut Criterion) {
    let mut group = c.benchmark_group("DeltaProcessor::sequential");

    for delta_count in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*delta_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(delta_count),
            delta_count,
            |b, &count| {
                let updates: Vec<_> = (0..count)
                    .map(|i| {
                        DeltaUpdate::with_sequence(
                            (i + 1) as u64,
                            if i % 2 == 0 { Side::Bid } else { Side::Ask },
                            vec![level(50000.0 + (i as f64) * 0.1, dec!(1))],
                            now_micros(),
                        )
                    })
                    .collect();

                b.iter_batched(
                    || {
                        let mut processor = DeltaProcessor::with_defaults();
                        let mut book = populated_book(50);
                        processor.reset_with_sequence(0);
                        (processor, book, updates.clone())
                    },
                    |(mut processor, mut book, updates)| {
                        for update in updates {
                            black_box(processor.process_delta(&mut book, update).ok());
                        }
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

// =============================================================================
// CRITERION CONFIGURATION
// =============================================================================

criterion_group!(
    benches,
    bench_delta_update_creation,
    bench_delta_batch_creation,
    bench_batch_total_levels,
    bench_sequence_tracker,
    bench_delta_validator,
    bench_process_delta,
    bench_process_batch,
    bench_process_delta_scaling,
    bench_sequential_deltas,
);

criterion_main!(benches);
