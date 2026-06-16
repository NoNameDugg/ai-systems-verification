//! DataSource Benchmarks (T3.6)
//!
//! This benchmark suite measures the performance of DataSource operations
//! to verify the performance targets for Phase 3.
//!
//! Run with: cargo bench --package blackbox --bench data_source_bench
//!
//! ## Performance Targets
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | `peek()` | <100ns | Read-only, no state change |
//! | `next()` | <1μs | May involve buffer management |
//! | `is_active()` | <10ns | Simple state check |
//! | `has_next()` | <100ns | Peek-based check |

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use blackbox::replay::{
    BufferedDataSource, DataFrame, DataSource, FrameType, LiveDataSource, NullDataSource,
};
use blackbox_types::{Exchange, Timestamp};

// ============================================================
// NULLDATASOURCE BENCHMARKS
// ============================================================

fn bench_null_data_source(c: &mut Criterion) {
    let mut group = c.benchmark_group("null_data_source");
    group.significance_level(0.01).sample_size(10000);

    let mut source = NullDataSource;

    group.bench_function("next()", |b| {
        b.iter(|| black_box(source.next()));
    });

    group.bench_function("peek()", |b| {
        b.iter(|| black_box(source.peek()));
    });

    group.bench_function("has_next()", |b| {
        b.iter(|| black_box(source.has_next()));
    });

    group.bench_function("is_active()", |b| {
        b.iter(|| black_box(source.is_active()));
    });

    group.bench_function("peek_timestamp()", |b| {
        b.iter(|| black_box(source.peek_timestamp()));
    });

    group.bench_function("frame_count()", |b| {
        b.iter(|| black_box(source.frame_count()));
    });

    group.bench_function("position()", |b| {
        b.iter(|| black_box(source.position()));
    });

    group.bench_function("reset()", |b| {
        b.iter(|| {
            source.reset();
        });
    });

    group.finish();
}

// ============================================================
// BUFFEREDDATASOURCE BENCHMARKS
// ============================================================

fn create_test_frames(count: usize) -> Vec<DataFrame> {
    (0..count)
        .map(|i| {
            DataFrame::new(
                Timestamp::from_micros(i as i64 * 1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                vec![0u8; 64], // 64-byte payload
            )
        })
        .collect()
}

fn bench_buffered_data_source_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffered_data_source");
    group.significance_level(0.01).sample_size(10000);

    let frames = create_test_frames(1000);
    let mut source = BufferedDataSource::new(frames.clone());

    group.bench_function("peek()", |b| {
        source.reset();
        b.iter(|| black_box(source.peek()));
    });

    group.bench_function("peek_timestamp()", |b| {
        source.reset();
        b.iter(|| black_box(source.peek_timestamp()));
    });

    group.bench_function("has_next()", |b| {
        source.reset();
        b.iter(|| black_box(source.has_next()));
    });

    group.bench_function("is_active()", |b| {
        b.iter(|| black_box(source.is_active()));
    });

    group.bench_function("frame_count()", |b| {
        b.iter(|| black_box(source.frame_count()));
    });

    group.bench_function("position()", |b| {
        b.iter(|| black_box(source.position()));
    });

    group.bench_function("remaining()", |b| {
        b.iter(|| black_box(source.remaining()));
    });

    group.finish();
}

fn bench_buffered_next(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffered_next");
    group.significance_level(0.01).sample_size(1000);

    for frame_count in [10, 100, 1000] {
        let frames = create_test_frames(frame_count);

        group.bench_with_input(
            BenchmarkId::new("next()", frame_count),
            &frames,
            |b, frames| {
                b.iter_batched(
                    || BufferedDataSource::new(frames.clone()),
                    |mut source| {
                        let mut count = 0;
                        while source.next().is_some() {
                            count += 1;
                        }
                        black_box(count)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_buffered_reset(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffered_reset");
    group.significance_level(0.01).sample_size(10000);

    let frames = create_test_frames(1000);
    let mut source = BufferedDataSource::new(frames);

    group.bench_function("reset()", |b| {
        b.iter(|| {
            source.reset();
        });
    });

    group.finish();
}

// ============================================================
// DATAFRAME BENCHMARKS
// ============================================================

fn bench_dataframe_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("dataframe_creation");
    group.significance_level(0.01).sample_size(10000);

    group.bench_function("new(64B)", |b| {
        b.iter(|| {
            black_box(DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                vec![0u8; 64],
            ))
        });
    });

    group.bench_function("new(1KB)", |b| {
        b.iter(|| {
            black_box(DataFrame::new(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                vec![0u8; 1024],
            ))
        });
    });

    group.bench_function("empty()", |b| {
        b.iter(|| {
            black_box(DataFrame::empty(
                Timestamp::from_micros(1000),
                Exchange::Deribit,
                FrameType::Heartbeat,
            ))
        });
    });

    group.finish();
}

fn bench_dataframe_accessors(c: &mut Criterion) {
    let mut group = c.benchmark_group("dataframe_accessors");
    group.significance_level(0.01).sample_size(10000);

    let frame = DataFrame::new(
        Timestamp::from_micros(1_704_067_200_000_000),
        Exchange::Binance,
        FrameType::BookSnapshot,
        vec![0u8; 256],
    );

    group.bench_function("payload_len()", |b| {
        b.iter(|| black_box(frame.payload_len()));
    });

    group.bench_function("is_empty()", |b| {
        b.iter(|| black_box(frame.is_empty()));
    });

    group.bench_function("timestamp", |b| {
        b.iter(|| black_box(frame.timestamp));
    });

    group.bench_function("exchange", |b| {
        b.iter(|| black_box(frame.exchange));
    });

    group.bench_function("frame_type", |b| {
        b.iter(|| black_box(frame.frame_type));
    });

    group.finish();
}

fn bench_dataframe_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("dataframe_clone");
    group.significance_level(0.01).sample_size(1000);

    for size in [64, 256, 1024, 4096] {
        let frame = DataFrame::new(
            Timestamp::from_micros(1000),
            Exchange::Deribit,
            FrameType::WebSocketText,
            vec![0u8; size],
        );

        group.bench_with_input(BenchmarkId::new("clone", size), &frame, |b, frame| {
            b.iter(|| black_box(frame.clone()));
        });
    }

    group.finish();
}

// ============================================================
// FRAMETYPE BENCHMARKS
// ============================================================

fn bench_frametype(c: &mut Criterion) {
    let mut group = c.benchmark_group("frametype");
    group.significance_level(0.01).sample_size(10000);

    group.bench_function("from_record_type()", |b| {
        b.iter(|| black_box(FrameType::from_record_type(black_box(0x10))));
    });

    group.bench_function("to_record_type()", |b| {
        let ft = FrameType::BookSnapshot;
        b.iter(|| black_box(ft.to_record_type()));
    });

    group.finish();
}

// ============================================================
// GENERIC USAGE BENCHMARKS
// ============================================================

fn bench_generic_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("generic_usage");
    group.significance_level(0.01).sample_size(1000);

    fn process_source<D: DataSource>(source: &mut D) -> usize {
        let mut count = 0;
        while source.next().is_some() {
            count += 1;
        }
        count
    }

    // NullDataSource with generic function
    group.bench_function("generic_null", |b| {
        let mut source = NullDataSource;
        b.iter(|| black_box(process_source(&mut source)));
    });

    // BufferedDataSource with generic function (100 frames)
    group.bench_function("generic_buffered_100", |b| {
        let frames = create_test_frames(100);
        b.iter_batched(
            || BufferedDataSource::new(frames.clone()),
            |mut source| black_box(process_source(&mut source)),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// REALISTIC REPLAY WORKFLOW BENCHMARKS
// ============================================================

fn bench_replay_workflow(c: &mut Criterion) {
    use blackbox::replay::SimulatedClock;
    use blackbox_types::Clock;

    let mut group = c.benchmark_group("replay_workflow");
    group.significance_level(0.01).sample_size(100);

    // Simulate processing 1000 frames with clock advancement
    group.bench_function("process_1000_frames", |b| {
        let frames = create_test_frames(1000);
        b.iter_batched(
            || {
                (
                    BufferedDataSource::new(frames.clone()),
                    SimulatedClock::at_epoch(),
                )
            },
            |(mut source, clock)| {
                let mut count = 0;
                while let Some(ts) = source.peek_timestamp() {
                    clock.advance_to(ts);
                    source.next();
                    count += 1;
                }
                black_box(count)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Simulate warp detection in replay
    group.bench_function("warp_detection_1000_frames", |b| {
        // Create frames with some large gaps
        let mut frames = Vec::with_capacity(1000);
        let mut ts = 0i64;
        for i in 0..1000 {
            // Every 100th frame has a 1-second gap
            if i % 100 == 0 && i > 0 {
                ts += 1_000_000; // 1 second
            } else {
                ts += 1000; // 1ms
            }
            frames.push(DataFrame::empty(
                Timestamp::from_micros(ts),
                Exchange::Deribit,
                FrameType::WebSocketText,
            ));
        }

        b.iter_batched(
            || {
                (
                    BufferedDataSource::new(frames.clone()),
                    SimulatedClock::at_epoch(),
                )
            },
            |(mut source, clock)| {
                let warp_threshold = 10_000i64;
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
                black_box(warp_count)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// LIVEDATASOURCE BENCHMARKS
// ============================================================

fn bench_live_data_source_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("live_data_source");
    group.significance_level(0.01).sample_size(10000);

    // Pre-create frames for sending
    let frames: Vec<DataFrame> = (0..100)
        .map(|i| {
            DataFrame::new(
                Timestamp::from_micros(i * 1000),
                Exchange::Deribit,
                FrameType::WebSocketText,
                vec![0u8; 64],
            )
        })
        .collect();

    group.bench_function("is_active()", |b| {
        let (source, _sender) = LiveDataSource::with_sender();
        b.iter(|| black_box(source.is_active()));
    });

    group.bench_function("peek() empty", |b| {
        let (source, _sender) = LiveDataSource::with_sender();
        b.iter(|| black_box(source.peek()));
    });

    group.bench_function("peek() with data", |b| {
        let (source, sender) = LiveDataSource::with_sender();
        sender.send(frames[0].clone()).unwrap();
        b.iter(|| black_box(source.peek()));
    });

    group.bench_function("has_next() empty", |b| {
        let (source, _sender) = LiveDataSource::with_sender();
        b.iter(|| black_box(source.has_next()));
    });

    group.bench_function("has_next() with data", |b| {
        let (source, sender) = LiveDataSource::with_sender();
        sender.send(frames[0].clone()).unwrap();
        b.iter(|| black_box(source.has_next()));
    });

    group.bench_function("peek_timestamp() with data", |b| {
        let (source, sender) = LiveDataSource::with_sender();
        sender.send(frames[0].clone()).unwrap();
        b.iter(|| black_box(source.peek_timestamp()));
    });

    group.finish();
}

fn bench_live_next(c: &mut Criterion) {
    let mut group = c.benchmark_group("live_next");
    group.significance_level(0.01).sample_size(1000);

    for frame_count in [10, 100, 1000] {
        let frames: Vec<DataFrame> = (0..frame_count)
            .map(|i| {
                DataFrame::new(
                    Timestamp::from_micros(i as i64 * 1000),
                    Exchange::Deribit,
                    FrameType::WebSocketText,
                    vec![0u8; 64],
                )
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("next()", frame_count),
            &frames,
            |b, frames| {
                b.iter_batched(
                    || {
                        let (source, sender) = LiveDataSource::with_sender();
                        for frame in frames {
                            sender.send(frame.clone()).unwrap();
                        }
                        source
                    },
                    |mut source| {
                        let mut count = 0;
                        while source.next().is_some() {
                            count += 1;
                        }
                        black_box(count)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_live_channel_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("live_channel_throughput");
    group.significance_level(0.01).sample_size(100);

    // Measure send+receive throughput
    group.bench_function("send_receive_1000_frames", |b| {
        let frames: Vec<DataFrame> = (0..1000)
            .map(|i| {
                DataFrame::new(
                    Timestamp::from_micros(i * 1000),
                    Exchange::Binance,
                    FrameType::Trade,
                    vec![0u8; 64],
                )
            })
            .collect();

        b.iter_batched(
            || {
                let (source, sender) = LiveDataSource::with_sender();
                (source, sender, frames.clone())
            },
            |(mut source, sender, frames)| {
                // Send all frames
                for frame in frames {
                    sender.send(frame).unwrap();
                }
                // Receive all frames
                let mut count = 0;
                while source.next().is_some() {
                    count += 1;
                }
                black_box(count)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// COMPARISON BENCHMARKS
// ============================================================

fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("comparison");
    group.significance_level(0.01).sample_size(10000);

    let mut null_source = NullDataSource;
    let frames = create_test_frames(100);
    let mut buffered_source = BufferedDataSource::new(frames);

    group.bench_function("NullDataSource::is_active()", |b| {
        b.iter(|| black_box(null_source.is_active()));
    });

    group.bench_function("BufferedDataSource::is_active()", |b| {
        b.iter(|| black_box(buffered_source.is_active()));
    });

    group.bench_function("NullDataSource::next()", |b| {
        b.iter(|| black_box(null_source.next()));
    });

    group.bench_function("BufferedDataSource::peek()", |b| {
        buffered_source.reset();
        b.iter(|| black_box(buffered_source.peek()));
    });

    group.finish();
}

// ============================================================
// CRITERION GROUPS
// ============================================================

criterion_group!(null_benches, bench_null_data_source,);

criterion_group!(
    buffered_benches,
    bench_buffered_data_source_ops,
    bench_buffered_next,
    bench_buffered_reset,
);

criterion_group!(
    live_benches,
    bench_live_data_source_ops,
    bench_live_next,
    bench_live_channel_throughput,
);

criterion_group!(
    dataframe_benches,
    bench_dataframe_creation,
    bench_dataframe_accessors,
    bench_dataframe_clone,
    bench_frametype,
);

criterion_group!(
    workflow_benches,
    bench_generic_usage,
    bench_replay_workflow,
    bench_comparison,
);

criterion_main!(
    null_benches,
    buffered_benches,
    live_benches,
    dataframe_benches,
    workflow_benches
);
