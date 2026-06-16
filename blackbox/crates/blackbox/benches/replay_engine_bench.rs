//! ReplayEngine Benchmarks (T3.9)
//!
//! This benchmark suite measures the performance of the ReplayEngine and
//! SkipIdleScheduler to verify the warp-speed replay targets.
//!
//! Run with: cargo bench --package blackbox --bench replay_engine_bench
//!
//! ## Performance Targets
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | `step()` | <10μs | Single event dispatch |
//! | `tick(100)` | <1ms | Batch processing |
//! | `schedule()` | <100ns | Warp decision |
//! | 24h session | <60s | Warp through idle |

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use blackbox::replay::{
    BufferedDataSource, DataFrame, FrameType, ReplayEngine, ReplayMode, SimulatedClock,
    SkipIdleScheduler, WarpConfig,
};
use blackbox_types::{Exchange, Timestamp};

// ============================================================
// SKIPIDLE SCHEDULER BENCHMARKS
// ============================================================

fn bench_scheduler_schedule(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduler");
    group.significance_level(0.01).sample_size(10000);

    let scheduler = SkipIdleScheduler::new(WarpConfig::default());
    let clock = SimulatedClock::at_epoch();

    // Schedule with small gap (no warp)
    group.bench_function("schedule_small_gap", |b| {
        clock.set(Timestamp::from_micros(0));
        b.iter(|| {
            black_box(scheduler.schedule(&clock, Timestamp::from_micros(1000)));
            clock.set(Timestamp::from_micros(0));
        });
    });

    // Schedule with large gap (warp)
    group.bench_function("schedule_large_gap", |b| {
        clock.set(Timestamp::from_micros(0));
        scheduler.reset();
        b.iter(|| {
            black_box(scheduler.schedule(&clock, Timestamp::from_micros(1_000_000)));
            clock.set(Timestamp::from_micros(0));
            scheduler.reset();
        });
    });

    // would_warp() check
    group.bench_function("would_warp", |b| {
        b.iter(|| black_box(scheduler.would_warp(100_000, true)));
    });

    // stats() retrieval
    group.bench_function("stats", |b| {
        b.iter(|| black_box(scheduler.stats()));
    });

    // reset()
    group.bench_function("reset", |b| {
        b.iter(|| scheduler.reset());
    });

    group.finish();
}

fn bench_warp_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("warp_config");
    group.significance_level(0.01).sample_size(10000);

    group.bench_function("default", |b| {
        b.iter(|| black_box(WarpConfig::default()));
    });

    group.bench_function("with_threshold", |b| {
        b.iter(|| black_box(WarpConfig::with_threshold(50_000)));
    });

    group.bench_function("instant", |b| {
        b.iter(|| black_box(WarpConfig::instant()));
    });

    group.finish();
}

// ============================================================
// REPLAY ENGINE BENCHMARKS
// ============================================================

fn create_test_frames(count: usize, gap_us: i64) -> Vec<DataFrame> {
    (0..count)
        .map(|i| {
            DataFrame::new(
                Timestamp::from_micros((i as i64 + 1) * gap_us),
                Exchange::Deribit,
                FrameType::Trade,
                vec![0u8; 64],
            )
        })
        .collect()
}

fn bench_engine_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_step");
    group.significance_level(0.01).sample_size(1000);

    for frame_count in [10, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("step", frame_count),
            &frame_count,
            |b, &count| {
                let frames = create_test_frames(count, 1000);
                b.iter_batched(
                    || {
                        let source = BufferedDataSource::new(frames.clone());
                        let mut engine =
                            ReplayEngine::with_data_source(source, WarpConfig::default());
                        engine.play();
                        engine
                    },
                    |mut engine| {
                        for _ in 0..count {
                            black_box(engine.step());
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_engine_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_tick");
    group.significance_level(0.01).sample_size(100);

    for batch_size in [10, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("tick", batch_size),
            &batch_size,
            |b, &batch| {
                let frames = create_test_frames(10000, 1000);
                b.iter_batched(
                    || {
                        let source = BufferedDataSource::new(frames.clone());
                        let mut engine =
                            ReplayEngine::with_data_source(source, WarpConfig::default());
                        engine.play();
                        engine
                    },
                    |mut engine| {
                        let mut total = 0;
                        while !engine.is_completed() {
                            total += engine.tick(batch);
                        }
                        black_box(total)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_engine_run_to_completion(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_run_to_completion");
    group.significance_level(0.01).sample_size(100);

    for frame_count in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("run", frame_count),
            &frame_count,
            |b, &count| {
                let frames = create_test_frames(count, 1000);
                b.iter_batched(
                    || {
                        let source = BufferedDataSource::new(frames.clone());
                        let mut engine =
                            ReplayEngine::with_data_source(source, WarpConfig::default());
                        engine.play();
                        engine
                    },
                    |mut engine| black_box(engine.run_to_completion()),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_engine_warp_mode(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_warp");
    group.significance_level(0.01).sample_size(100);

    // Create frames with idle periods (1 second gaps every 100 frames)
    let mut frames = Vec::with_capacity(1000);
    let mut current_time = 0i64;
    for i in 0..1000 {
        if i % 100 == 0 && i > 0 {
            current_time += 1_000_000; // 1 second gap (warp)
        } else {
            current_time += 1000; // 1ms gap (no warp)
        }
        frames.push(DataFrame::new(
            Timestamp::from_micros(current_time),
            Exchange::Deribit,
            FrameType::Trade,
            vec![0u8; 32],
        ));
    }

    group.bench_function("with_warp_mode", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.set_mode(ReplayMode::WarpSpeed);
                engine.play();
                engine
            },
            |mut engine| black_box(engine.run_to_completion()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("without_warp_mode", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::real_time());
                engine.set_mode(ReplayMode::Step);
                engine.play();
                engine
            },
            |mut engine| black_box(engine.run_to_completion()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// 24-HOUR SESSION SIMULATION BENCHMARK
// ============================================================

fn bench_24h_session(c: &mut Criterion) {
    let mut group = c.benchmark_group("24h_session");
    group.significance_level(0.01).sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(30));

    // Create a realistic 24-hour trading session:
    // - 8 hours of active trading with events every 1ms
    // - 16 hours of idle time (nights, weekends) - 2 hours of idle per active hour
    let mut frames = Vec::new();
    let mut current_time = 0i64;

    for hour in 0..8 {
        // Active period: 1000 events, 1ms apart
        for _ in 0..1000 {
            current_time += 1000; // 1ms
            frames.push(DataFrame::new(
                Timestamp::from_micros(current_time),
                Exchange::Deribit,
                FrameType::Trade,
                vec![0u8; 64],
            ));
        }

        // Idle period: 2 hours (except after last hour)
        if hour < 7 {
            current_time += 7_200_000_000; // 2 hours in microseconds
        }
    }

    let total_frames = frames.len();
    let total_simulated_time_us = current_time;
    let total_simulated_hours = total_simulated_time_us as f64 / 3_600_000_000.0;

    println!(
        "\n24h Session: {} frames, {:.1} hours simulated time",
        total_frames, total_simulated_hours
    );

    group.bench_function("warp_speed", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.set_mode(ReplayMode::WarpSpeed);
                engine.play();
                engine
            },
            |mut engine| {
                let processed = engine.run_to_completion();
                let stats = engine.stats();
                black_box((processed, stats.warp_count, stats.total_warped_us))
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// FAST-FORWARD MODE BENCHMARKS
// ============================================================

fn bench_fast_forward_scheduler(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_forward_scheduler");
    group.significance_level(0.01).sample_size(1000);

    // Benchmark bounded warp with different multipliers
    for multiplier in [1, 10, 100] {
        group.bench_with_input(
            BenchmarkId::new("bounded_warp", multiplier),
            &multiplier,
            |b, &mult| {
                let scheduler = SkipIdleScheduler::new(WarpConfig::fast_forward(mult));
                let clock = SimulatedClock::at_epoch();

                b.iter(|| {
                    clock.set(Timestamp::from_micros(0));
                    scheduler.reset();
                    // Schedule with 1 second gap (requires multiple bounded warps)
                    black_box(scheduler.schedule(&clock, Timestamp::from_micros(1_000_000)));
                });
            },
        );
    }

    // Compare instant vs bounded
    let instant_scheduler = SkipIdleScheduler::new(WarpConfig::instant());
    let bounded_scheduler = SkipIdleScheduler::new(WarpConfig::fast_forward(10));
    let clock = SimulatedClock::at_epoch();

    group.bench_function("instant_warp", |b| {
        b.iter(|| {
            clock.set(Timestamp::from_micros(0));
            instant_scheduler.reset();
            black_box(instant_scheduler.schedule(&clock, Timestamp::from_micros(1_000_000)));
        });
    });

    group.bench_function("bounded_10x", |b| {
        b.iter(|| {
            clock.set(Timestamp::from_micros(0));
            bounded_scheduler.reset();
            black_box(bounded_scheduler.schedule(&clock, Timestamp::from_micros(1_000_000)));
        });
    });

    group.finish();
}

fn bench_fast_forward_engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_forward_engine");
    group.significance_level(0.01).sample_size(100);

    // Create frames with large gaps (100ms each)
    let frames = create_test_frames(100, 100_000);

    // Compare different modes
    group.bench_function("warp_speed", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.set_mode(ReplayMode::WarpSpeed);
                engine.play();
                engine
            },
            |mut engine| black_box(engine.run_to_completion()),
            criterion::BatchSize::SmallInput,
        );
    });

    for multiplier in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("fast_forward", multiplier),
            &multiplier,
            |b, &mult| {
                b.iter_batched(
                    || {
                        let source = BufferedDataSource::new(frames.clone());
                        let mut engine =
                            ReplayEngine::with_data_source(source, WarpConfig::default());
                        engine.set_mode(ReplayMode::FastForward { multiplier: mult });
                        engine.play();
                        engine
                    },
                    |mut engine| black_box(engine.run_to_completion()),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.bench_function("step_mode", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.set_mode(ReplayMode::Step);
                engine.play();
                engine
            },
            |mut engine| black_box(engine.run_to_completion()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_step_until_event(c: &mut Criterion) {
    let mut group = c.benchmark_group("step_until_event");
    group.significance_level(0.01).sample_size(100);

    // Create frames with large gaps to test step_until_event
    let frames = vec![
        DataFrame::new(
            Timestamp::from_micros(1_000_000), // 1s
            Exchange::Deribit,
            FrameType::Trade,
            vec![0u8; 64],
        ),
        DataFrame::new(
            Timestamp::from_micros(2_000_000), // 2s
            Exchange::Deribit,
            FrameType::Trade,
            vec![0u8; 64],
        ),
    ];

    for multiplier in [10, 100] {
        group.bench_with_input(
            BenchmarkId::new("step_until", multiplier),
            &multiplier,
            |b, &mult| {
                b.iter_batched(
                    || {
                        let source = BufferedDataSource::new(frames.clone());
                        let mut engine =
                            ReplayEngine::with_data_source(source, WarpConfig::default());
                        engine.set_mode(ReplayMode::FastForward { multiplier: mult });
                        engine.play();
                        engine
                    },
                    |mut engine| {
                        black_box(engine.step_until_event());
                        black_box(engine.step_until_event());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ============================================================
// ENGINE OPERATIONS BENCHMARKS
// ============================================================

fn bench_engine_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_ops");
    group.significance_level(0.01).sample_size(10000);

    let frames = create_test_frames(100, 1000);

    group.bench_function("new", |b| {
        b.iter(|| black_box(ReplayEngine::new()));
    });

    group.bench_function("with_data_source", |b| {
        b.iter_batched(
            || BufferedDataSource::new(frames.clone()),
            |source| {
                black_box(ReplayEngine::with_data_source(
                    source,
                    WarpConfig::default(),
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });

    let source = BufferedDataSource::new(frames.clone());
    let engine = ReplayEngine::with_data_source(source, WarpConfig::default());

    group.bench_function("clock", |b| {
        b.iter(|| black_box(engine.clock()));
    });

    group.bench_function("state", |b| {
        b.iter(|| black_box(engine.state()));
    });

    group.bench_function("mode", |b| {
        b.iter(|| black_box(engine.mode()));
    });

    group.bench_function("stats", |b| {
        b.iter(|| black_box(engine.stats()));
    });

    group.bench_function("progress", |b| {
        b.iter(|| black_box(engine.progress()));
    });

    group.bench_function("position", |b| {
        b.iter(|| black_box(engine.position()));
    });

    group.finish();
}

fn bench_engine_state_transitions(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_state");
    group.significance_level(0.01).sample_size(10000);

    let frames = create_test_frames(100, 1000);

    group.bench_function("play", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                ReplayEngine::with_data_source(source, WarpConfig::default())
            },
            |mut engine| {
                engine.play();
                black_box(engine.state())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("pause", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.play();
                engine
            },
            |mut engine| {
                engine.pause();
                black_box(engine.state())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("toggle", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                ReplayEngine::with_data_source(source, WarpConfig::default())
            },
            |mut engine| {
                engine.toggle();
                black_box(engine.state())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("reset", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                let mut engine = ReplayEngine::with_data_source(source, WarpConfig::default());
                engine.play();
                engine.tick(50);
                engine
            },
            |mut engine| {
                engine.reset();
                black_box(engine.state())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("set_mode", |b| {
        b.iter_batched(
            || {
                let source = BufferedDataSource::new(frames.clone());
                ReplayEngine::with_data_source(source, WarpConfig::default())
            },
            |mut engine| {
                engine.set_mode(ReplayMode::WarpSpeed);
                black_box(engine.mode())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================
// CRITERION GROUPS
// ============================================================

criterion_group!(
    scheduler_benches,
    bench_scheduler_schedule,
    bench_warp_config,
);

criterion_group!(
    engine_benches,
    bench_engine_step,
    bench_engine_tick,
    bench_engine_run_to_completion,
    bench_engine_warp_mode,
);

criterion_group!(
    ops_benches,
    bench_engine_operations,
    bench_engine_state_transitions,
);

criterion_group!(session_benches, bench_24h_session,);

criterion_group!(
    fast_forward_benches,
    bench_fast_forward_scheduler,
    bench_fast_forward_engine,
    bench_step_until_event,
);

criterion_main!(
    scheduler_benches,
    engine_benches,
    ops_benches,
    session_benches,
    fast_forward_benches
);
