//! Benchmarks for the Batching & Backpressure module.
//!
//! Run with: `cargo bench --bench batch_bench`

use astra_flash::core::types::now_micros;
use astra_flash::publisher::batch::{
    BackpressureMonitor, BackpressureStatus, BatchAccumulator, BatchConfig, BatchConfigBuilder,
    BatchMessage, BatcherStats, DropReason, FlushTrigger, MessageBatch, OverflowAction,
};
use astra_flash::publisher::stream::SerializationFormat;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test batch message with specified data size.
fn test_batch_message(size: usize) -> BatchMessage {
    BatchMessage {
        topic: "market_data.deribit.btc_usd.book".to_string(),
        data: vec![0u8; size],
        format: SerializationFormat::Bincode,
        enqueued_at: now_micros(),
        priority: 0,
    }
}

/// Create a test batch message with specified topic.
fn test_batch_message_with_topic(topic: &str, size: usize) -> BatchMessage {
    BatchMessage {
        topic: topic.to_string(),
        data: vec![0u8; size],
        format: SerializationFormat::Bincode,
        enqueued_at: now_micros(),
        priority: 0,
    }
}

// =============================================================================
// BATCH CONFIG BENCHMARKS
// =============================================================================

/// Benchmark BatchConfig default creation.
fn bench_batch_config_default(c: &mut Criterion) {
    c.bench_function("BatchConfig::default", |b| {
        b.iter(|| {
            let config = BatchConfig::default();
            black_box(config)
        });
    });
}

/// Benchmark BatchConfig cloning.
fn bench_batch_config_clone(c: &mut Criterion) {
    let config = BatchConfig::default();

    c.bench_function("BatchConfig::clone", |b| {
        b.iter(|| {
            let cloned = config.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark BatchConfig validation.
fn bench_batch_config_validate(c: &mut Criterion) {
    let config = BatchConfig::default();

    c.bench_function("BatchConfig::validate", |b| {
        b.iter(|| {
            let result = config.validate();
            black_box(result)
        });
    });
}

/// Benchmark BatchConfigBuilder creation and build.
fn bench_batch_config_builder(c: &mut Criterion) {
    c.bench_function("BatchConfigBuilder::build", |b| {
        b.iter(|| {
            let config = BatchConfigBuilder::new()
                .max_batch_size(100)
                .max_batch_delay(Duration::from_millis(10))
                .warn_threshold(0.80)
                .critical_threshold(0.95)
                .build();
            black_box(config)
        });
    });
}

// =============================================================================
// BATCH MESSAGE BENCHMARKS
// =============================================================================

/// Benchmark BatchMessage creation.
fn bench_batch_message_new(c: &mut Criterion) {
    c.bench_function("BatchMessage::new_100", |b| {
        b.iter(|| {
            let msg = test_batch_message(100);
            black_box(msg)
        });
    });
}

/// Benchmark BatchMessage cloning.
fn bench_batch_message_clone(c: &mut Criterion) {
    let msg = test_batch_message(1024);

    c.bench_function("BatchMessage::clone_1kb", |b| {
        b.iter(|| {
            let cloned = msg.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark BatchMessage size calculation.
fn bench_batch_message_size(c: &mut Criterion) {
    let msg = test_batch_message(1024);

    c.bench_function("BatchMessage::size", |b| {
        b.iter(|| {
            let size = msg.size();
            black_box(size)
        });
    });
}

// =============================================================================
// MESSAGE BATCH BENCHMARKS
// =============================================================================

/// Benchmark MessageBatch creation.
fn bench_message_batch_new(c: &mut Criterion) {
    c.bench_function("MessageBatch::new", |b| {
        b.iter(|| {
            let batch = MessageBatch::new();
            black_box(batch)
        });
    });
}

/// Benchmark adding messages to batch.
fn bench_message_batch_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("MessageBatch::add");

    for count in [1, 10, 50, 100].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter(|| {
                let mut batch = MessageBatch::new();
                for _ in 0..count {
                    batch.add(test_batch_message(100));
                }
                black_box(batch)
            });
        });
    }

    group.finish();
}

/// Benchmark clearing a batch.
fn bench_message_batch_clear(c: &mut Criterion) {
    let mut batch = MessageBatch::new();
    for _ in 0..100 {
        batch.add(test_batch_message(100));
    }

    c.bench_function("MessageBatch::clear_100", |b| {
        b.iter(|| {
            batch.clear();
            // Re-add for next iteration
            for _ in 0..100 {
                batch.add(test_batch_message(100));
            }
            black_box(batch.len())
        });
    });
}

// =============================================================================
// BATCH ACCUMULATOR BENCHMARKS
// =============================================================================

/// Benchmark BatchAccumulator creation.
fn bench_accumulator_new(c: &mut Criterion) {
    c.bench_function("BatchAccumulator::new", |b| {
        b.iter(|| {
            let acc = BatchAccumulator::new(100);
            black_box(acc)
        });
    });
}

/// Benchmark adding messages to accumulator.
fn bench_accumulator_add(c: &mut Criterion) {
    c.bench_function("BatchAccumulator::add", |b| {
        let mut acc = BatchAccumulator::new(1000);
        b.iter(|| {
            acc.add(test_batch_message(100));
            if acc.is_ready() {
                acc.take();
            }
            black_box(acc.len())
        });
    });
}

/// Benchmark taking batch from accumulator.
fn bench_accumulator_take(c: &mut Criterion) {
    let mut group = c.benchmark_group("BatchAccumulator::take");

    for count in [10, 50, 100].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter(|| {
                let mut acc = BatchAccumulator::new(count + 1);
                for _ in 0..count {
                    acc.add(test_batch_message(100));
                }
                let batch = acc.take();
                black_box(batch)
            });
        });
    }

    group.finish();
}

/// Benchmark checking if accumulator is ready.
fn bench_accumulator_is_ready(c: &mut Criterion) {
    let mut acc = BatchAccumulator::new(100);
    for _ in 0..50 {
        acc.add(test_batch_message(100));
    }

    c.bench_function("BatchAccumulator::is_ready", |b| {
        b.iter(|| {
            let ready = acc.is_ready();
            black_box(ready)
        });
    });
}

/// Benchmark latency calculation.
fn bench_accumulator_latency(c: &mut Criterion) {
    let mut acc = BatchAccumulator::new(100);
    acc.add(test_batch_message(100));

    c.bench_function("BatchAccumulator::latency_us", |b| {
        b.iter(|| {
            let latency = acc.latency_us();
            black_box(latency)
        });
    });
}

// =============================================================================
// BACKPRESSURE MONITOR BENCHMARKS
// =============================================================================

/// Benchmark BackpressureMonitor creation.
fn bench_backpressure_monitor_new(c: &mut Criterion) {
    c.bench_function("BackpressureMonitor::new", |b| {
        b.iter(|| {
            let monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);
            black_box(monitor)
        });
    });
}

/// Benchmark backpressure check.
fn bench_backpressure_check(c: &mut Criterion) {
    let monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);

    c.bench_function("BackpressureMonitor::check", |b| {
        b.iter(|| {
            let status = monitor.check(black_box(5000));
            black_box(status)
        });
    });
}

/// Benchmark backpressure update.
fn bench_backpressure_update(c: &mut Criterion) {
    let mut monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);

    c.bench_function("BackpressureMonitor::update", |b| {
        let mut depth = 5000;
        b.iter(|| {
            depth = (depth + 100) % 10_000;
            let (status, changed) = monitor.update(depth);
            black_box((status, changed))
        });
    });
}

/// Benchmark utilization calculation.
fn bench_backpressure_utilization(c: &mut Criterion) {
    let monitor = BackpressureMonitor::new(0.80, 0.95, 10_000);

    c.bench_function("BackpressureMonitor::utilization", |b| {
        b.iter(|| {
            let util = monitor.utilization(black_box(7500));
            black_box(util)
        });
    });
}

// =============================================================================
// BATCHER STATS BENCHMARKS
// =============================================================================

/// Benchmark BatcherStats default creation.
fn bench_batcher_stats_default(c: &mut Criterion) {
    c.bench_function("BatcherStats::default", |b| {
        b.iter(|| {
            let stats = BatcherStats::default();
            black_box(stats)
        });
    });
}

/// Benchmark BatcherStats cloning.
fn bench_batcher_stats_clone(c: &mut Criterion) {
    let mut stats = BatcherStats::default();
    stats.messages_enqueued = 1_000_000;
    stats.messages_published = 999_000;
    stats.peak_queue_depth = 5000;

    c.bench_function("BatcherStats::clone", |b| {
        b.iter(|| {
            let cloned = stats.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark updating peak queue depth.
fn bench_batcher_stats_update_peak(c: &mut Criterion) {
    let mut stats = BatcherStats::default();

    c.bench_function("BatcherStats::update_peak", |b| {
        let mut depth = 0;
        b.iter(|| {
            depth = (depth + 1) % 1000;
            stats.update_peak(depth);
            black_box(stats.peak_queue_depth)
        });
    });
}

/// Benchmark updating average batch size.
fn bench_batcher_stats_update_avg(c: &mut Criterion) {
    let mut stats = BatcherStats::default();

    c.bench_function("BatcherStats::update_avg_batch_size", |b| {
        let mut size = 50;
        b.iter(|| {
            size = ((size + 1) % 100) + 1;
            stats.update_avg_batch_size(size);
            black_box(stats.avg_batch_size)
        });
    });
}

/// Benchmark stats reset.
fn bench_batcher_stats_reset(c: &mut Criterion) {
    let mut stats = BatcherStats::default();
    stats.messages_enqueued = 1_000_000;
    stats.messages_published = 999_000;

    c.bench_function("BatcherStats::reset", |b| {
        b.iter(|| {
            stats.messages_enqueued = 1_000_000;
            stats.messages_published = 999_000;
            stats.reset();
            black_box(stats.messages_enqueued)
        });
    });
}

// =============================================================================
// ENUM BENCHMARKS
// =============================================================================

/// Benchmark FlushTrigger comparison.
fn bench_flush_trigger_eq(c: &mut Criterion) {
    let trigger1 = FlushTrigger::BatchFull;
    let trigger2 = FlushTrigger::Timeout;

    c.bench_function("FlushTrigger::eq", |b| {
        b.iter(|| {
            let eq = trigger1 == black_box(trigger2);
            black_box(eq)
        });
    });
}

/// Benchmark OverflowAction comparison.
fn bench_overflow_action_eq(c: &mut Criterion) {
    let action1 = OverflowAction::Block;
    let action2 = OverflowAction::DropNewest;

    c.bench_function("OverflowAction::eq", |b| {
        b.iter(|| {
            let eq = action1 == black_box(action2);
            black_box(eq)
        });
    });
}

/// Benchmark BackpressureStatus default.
fn bench_backpressure_status_default(c: &mut Criterion) {
    c.bench_function("BackpressureStatus::default", |b| {
        b.iter(|| {
            let status = BackpressureStatus::default();
            black_box(status)
        });
    });
}

/// Benchmark DropReason display.
fn bench_drop_reason_display(c: &mut Criterion) {
    let reason = DropReason::QueueFull;

    c.bench_function("DropReason::to_string", |b| {
        b.iter(|| {
            let s = reason.to_string();
            black_box(s)
        });
    });
}

// =============================================================================
// TYPE SIZE BENCHMARKS
// =============================================================================

/// Benchmark memory sizes of types.
fn bench_type_sizes(c: &mut Criterion) {
    use std::mem::size_of;

    c.bench_function("type_sizes", |b| {
        b.iter(|| {
            let config_size = size_of::<BatchConfig>();
            let message_size = size_of::<BatchMessage>();
            let batch_size = size_of::<MessageBatch>();
            let stats_size = size_of::<BatcherStats>();
            let monitor_size = size_of::<BackpressureMonitor>();
            let accumulator_size = size_of::<BatchAccumulator>();

            black_box((
                config_size,
                message_size,
                batch_size,
                stats_size,
                monitor_size,
                accumulator_size,
            ))
        });
    });

    // Print sizes for reference
    println!("\nType sizes:");
    println!(
        "  BatchConfig: {} bytes",
        std::mem::size_of::<BatchConfig>()
    );
    println!(
        "  BatchMessage: {} bytes",
        std::mem::size_of::<BatchMessage>()
    );
    println!(
        "  MessageBatch: {} bytes",
        std::mem::size_of::<MessageBatch>()
    );
    println!(
        "  BatcherStats: {} bytes",
        std::mem::size_of::<BatcherStats>()
    );
    println!(
        "  BackpressureMonitor: {} bytes",
        std::mem::size_of::<BackpressureMonitor>()
    );
    println!(
        "  BatchAccumulator: {} bytes",
        std::mem::size_of::<BatchAccumulator>()
    );
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    config_benchmarks,
    bench_batch_config_default,
    bench_batch_config_clone,
    bench_batch_config_validate,
    bench_batch_config_builder,
);

criterion_group!(
    message_benchmarks,
    bench_batch_message_new,
    bench_batch_message_clone,
    bench_batch_message_size,
);

criterion_group!(
    batch_benchmarks,
    bench_message_batch_new,
    bench_message_batch_add,
    bench_message_batch_clear,
);

criterion_group!(
    accumulator_benchmarks,
    bench_accumulator_new,
    bench_accumulator_add,
    bench_accumulator_take,
    bench_accumulator_is_ready,
    bench_accumulator_latency,
);

criterion_group!(
    backpressure_benchmarks,
    bench_backpressure_monitor_new,
    bench_backpressure_check,
    bench_backpressure_update,
    bench_backpressure_utilization,
);

criterion_group!(
    stats_benchmarks,
    bench_batcher_stats_default,
    bench_batcher_stats_clone,
    bench_batcher_stats_update_peak,
    bench_batcher_stats_update_avg,
    bench_batcher_stats_reset,
);

criterion_group!(
    enum_benchmarks,
    bench_flush_trigger_eq,
    bench_overflow_action_eq,
    bench_backpressure_status_default,
    bench_drop_reason_display,
);

criterion_group!(misc_benchmarks, bench_type_sizes,);

criterion_main!(
    config_benchmarks,
    message_benchmarks,
    batch_benchmarks,
    accumulator_benchmarks,
    backpressure_benchmarks,
    stats_benchmarks,
    enum_benchmarks,
    misc_benchmarks,
);
