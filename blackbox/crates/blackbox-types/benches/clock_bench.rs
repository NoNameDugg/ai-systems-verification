//! SystemClock microbenchmark.
//!
//! Run with: cargo bench --package blackbox-types
//!
//! Target: SystemClock::now() < 100ns

use blackbox_types::{Clock, SystemClock};
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_system_clock_now(c: &mut Criterion) {
    let clock = SystemClock;

    c.bench_function("SystemClock::now()", |b| {
        b.iter(|| std::hint::black_box(clock.now()));
    });
}

fn bench_system_clock_now_micros(c: &mut Criterion) {
    let clock = SystemClock;

    c.bench_function("SystemClock::now_micros()", |b| {
        b.iter(|| std::hint::black_box(clock.now_micros()));
    });
}

fn bench_timestamp_operations(c: &mut Criterion) {
    use blackbox_types::Timestamp;

    let ts = Timestamp::from_micros(1_704_067_200_000_000);

    c.bench_function("Timestamp::as_micros()", |b| {
        b.iter(|| std::hint::black_box(ts.as_micros()));
    });

    c.bench_function("Timestamp::add_micros()", |b| {
        b.iter(|| std::hint::black_box(ts.add_micros(1000)));
    });
}

criterion_group!(
    benches,
    bench_system_clock_now,
    bench_system_clock_now_micros,
    bench_timestamp_operations
);
criterion_main!(benches);
