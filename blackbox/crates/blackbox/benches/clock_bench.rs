//! SimulatedClock Benchmarks (T3.1)
//!
//! This benchmark suite measures the performance of SimulatedClock operations
//! to verify the sub-10ns overhead target for Phase 3.
//!
//! Run with: cargo bench --package blackbox --bench clock_bench
//!
//! ## Performance Targets
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | `now()` | <10ns | Atomic load |
//! | `advance()` | <20ns | Atomic fetch-add |
//! | `set()` | <10ns | Atomic store |
//! | `pause()/resume()` | <10ns | Atomic store |
//! | `is_paused()` | <10ns | Atomic load |
//! | `is_warp_enabled()` | <10ns | Atomic load |

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;

use blackbox::replay::SimulatedClock;
use blackbox_types::{Clock, Timestamp};

// ============================================================
// CORE OPERATIONS
// ============================================================

fn bench_simulated_clock_now(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_now");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000));

    group.bench_function("now()", |b| {
        b.iter(|| black_box(clock.now()));
    });

    group.bench_function("now_micros()", |b| {
        b.iter(|| black_box(clock.now_micros()));
    });

    group.finish();
}

fn bench_simulated_clock_advance(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_advance");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    group.bench_function("advance(1)", |b| {
        b.iter(|| {
            clock.advance(black_box(1));
        });
    });

    group.bench_function("advance(1000)", |b| {
        b.iter(|| {
            clock.advance(black_box(1000));
        });
    });

    group.finish();
}

fn bench_simulated_clock_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_set");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));
    let timestamp = Timestamp::from_micros(1_704_067_200_000_000);

    group.bench_function("set()", |b| {
        b.iter(|| {
            clock.set(black_box(timestamp));
        });
    });

    group.finish();
}

fn bench_simulated_clock_advance_to(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_advance_to");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    group.bench_function("advance_to() (future)", |b| {
        let mut ts = 1000i64;
        b.iter(|| {
            ts += 1;
            black_box(clock.advance_to(Timestamp::from_micros(ts)));
        });
    });

    group.bench_function("advance_to() (past - no-op)", |b| {
        clock.set(Timestamp::from_micros(1_000_000));
        b.iter(|| {
            black_box(clock.advance_to(Timestamp::from_micros(100)));
        });
    });

    group.finish();
}

// ============================================================
// PAUSE/RESUME OPERATIONS
// ============================================================

fn bench_pause_resume(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_pause_resume");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    group.bench_function("pause()", |b| {
        b.iter(|| {
            clock.pause();
        });
    });

    group.bench_function("resume()", |b| {
        b.iter(|| {
            clock.resume();
        });
    });

    group.bench_function("is_paused()", |b| {
        b.iter(|| black_box(clock.is_paused()));
    });

    group.bench_function("pause_resume_cycle", |b| {
        b.iter(|| {
            clock.pause();
            clock.resume();
        });
    });

    group.finish();
}

// ============================================================
// WARP MODE OPERATIONS
// ============================================================

fn bench_warp_mode(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulated_clock_warp");
    group.significance_level(0.01).sample_size(10000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    group.bench_function("enable_warp()", |b| {
        b.iter(|| {
            clock.enable_warp();
        });
    });

    group.bench_function("disable_warp()", |b| {
        b.iter(|| {
            clock.disable_warp();
        });
    });

    group.bench_function("is_warp_enabled()", |b| {
        b.iter(|| black_box(clock.is_warp_enabled()));
    });

    group.bench_function("warp_toggle_cycle", |b| {
        b.iter(|| {
            clock.disable_warp();
            clock.enable_warp();
        });
    });

    group.finish();
}

// ============================================================
// COMPARISON WITH SYSTEMCLOCK
// ============================================================

fn bench_clock_comparison(c: &mut Criterion) {
    use blackbox_types::SystemClock;

    let mut group = c.benchmark_group("clock_comparison");
    group.significance_level(0.01).sample_size(10000);

    let system_clock = SystemClock;
    let simulated_clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000));

    group.bench_function("SystemClock::now()", |b| {
        b.iter(|| black_box(system_clock.now()));
    });

    group.bench_function("SimulatedClock::now()", |b| {
        b.iter(|| black_box(simulated_clock.now()));
    });

    group.finish();
}

// ============================================================
// GENERIC CLOCK USAGE
// ============================================================

fn bench_generic_clock(c: &mut Criterion) {
    let mut group = c.benchmark_group("generic_clock_usage");
    group.significance_level(0.01).sample_size(10000);

    fn get_time<C: Clock>(clock: &C) -> i64 {
        clock.now_micros()
    }

    let clock = SimulatedClock::new(Timestamp::from_micros(1_704_067_200_000_000));

    group.bench_function("generic_function_call", |b| {
        b.iter(|| black_box(get_time(&clock)));
    });

    group.bench_function("direct_call", |b| {
        b.iter(|| black_box(clock.now_micros()));
    });

    group.finish();
}

// ============================================================
// ARC SHARED CLOCK
// ============================================================

fn bench_arc_clock(c: &mut Criterion) {
    let mut group = c.benchmark_group("arc_simulated_clock");
    group.significance_level(0.01).sample_size(10000);

    let clock = Arc::new(SimulatedClock::new(Timestamp::from_micros(0)));

    group.bench_function("Arc::now()", |b| {
        b.iter(|| black_box(clock.now()));
    });

    group.bench_function("Arc::advance()", |b| {
        b.iter(|| {
            clock.advance(black_box(1));
        });
    });

    group.bench_function("Arc::is_paused()", |b| {
        b.iter(|| black_box(clock.is_paused()));
    });

    group.bench_function("Arc::is_warp_enabled()", |b| {
        b.iter(|| black_box(clock.is_warp_enabled()));
    });

    group.finish();
}

// ============================================================
// REALISTIC REPLAY WORKFLOW
// ============================================================

fn bench_replay_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay_workflow");
    group.significance_level(0.01).sample_size(1000);

    let clock = SimulatedClock::new(Timestamp::from_micros(0));

    // Simulate processing 1000 events at 1us intervals
    group.bench_function("process_1000_events", |b| {
        b.iter(|| {
            clock.set(Timestamp::EPOCH);
            for i in 0..1000 {
                clock.advance_to(Timestamp::from_micros(i * 1000));
                black_box(clock.now());
            }
        });
    });

    // Simulate warp-speed replay with idle period detection
    group.bench_function("warp_speed_checks", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                black_box(clock.is_warp_enabled());
                black_box(clock.is_paused());
            }
        });
    });

    // Simulate step-through debugging
    group.bench_function("step_debug_pattern", |b| {
        b.iter(|| {
            clock.pause();
            for _ in 0..10 {
                black_box(clock.is_paused());
                clock.advance(black_box(1000));
                black_box(clock.now());
            }
            clock.resume();
        });
    });

    group.finish();
}

// ============================================================
// CRITERION GROUPS
// ============================================================

criterion_group!(
    core_benches,
    bench_simulated_clock_now,
    bench_simulated_clock_advance,
    bench_simulated_clock_set,
    bench_simulated_clock_advance_to,
);

criterion_group!(control_benches, bench_pause_resume, bench_warp_mode,);

criterion_group!(
    comparison_benches,
    bench_clock_comparison,
    bench_generic_clock,
    bench_arc_clock,
    bench_replay_workflow,
);

criterion_main!(core_benches, control_benches, comparison_benches);
