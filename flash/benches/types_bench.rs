//! Benchmarks for Flash core types.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | PriceLevel::new() | < 10 ns |
//! | PriceLevel::clone() | < 100 ns |
//! | Instrument::clone() | < 50 ns |
//! | Exchange::as_str() | < 1 ns |
//! | Side::opposite() | < 1 ns |
//! | Serde JSON round-trip | < 1 μs |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench types_bench
//! ```

use astra_flash::core::types::*;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal_macros::dec;

// =============================================================================
// TYPE CREATION BENCHMARKS
// =============================================================================

/// Benchmark PriceLevel creation.
fn bench_price_level_new(c: &mut Criterion) {
    c.bench_function("PriceLevel::new", |b| {
        b.iter(|| {
            PriceLevel::new(
                black_box(50000.0),
                black_box(dec!(1.5)),
                black_box(1234567890_i64),
            )
        });
    });
}

/// Benchmark Instrument creation.
fn bench_instrument_new(c: &mut Criterion) {
    c.bench_function("Instrument::new", |b| {
        b.iter(|| {
            Instrument::new(
                black_box("BTC"),
                black_box("USD"),
                black_box(Exchange::Deribit),
                black_box("BTC-PERPETUAL"),
            )
        });
    });
}

/// Benchmark MarketEvent creation with Book data.
fn bench_market_event_book(c: &mut Criterion) {
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let timestamp = 1234567890_i64;
    let level = PriceLevel::new(50000.0, dec!(1.0), timestamp);

    c.bench_function("MarketEvent::Book (1 level)", |b| {
        b.iter(|| MarketEvent {
            event_type: black_box(MarketEventType::Snapshot),
            instrument: black_box(instrument.clone()),
            timestamp: black_box(timestamp),
            local_timestamp: black_box(timestamp + 100),
            sequence: black_box(Some(1)),
            data: MarketData::Book {
                bids: vec![black_box(level.clone())],
                asks: vec![black_box(level.clone())],
            },
        });
    });
}

// =============================================================================
// CLONE BENCHMARKS
// =============================================================================

/// Benchmark PriceLevel clone.
fn bench_price_level_clone(c: &mut Criterion) {
    let level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);

    c.bench_function("PriceLevel::clone", |b| {
        b.iter(|| black_box(&level).clone());
    });
}

/// Benchmark Instrument clone.
fn bench_instrument_clone(c: &mut Criterion) {
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");

    c.bench_function("Instrument::clone", |b| {
        b.iter(|| black_box(&instrument).clone());
    });
}

/// Benchmark MarketEvent clone with varying book sizes.
fn bench_market_event_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("MarketEvent::clone");

    for size in [1, 10, 50].iter() {
        let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
        let timestamp = 1234567890_i64;

        let levels: Vec<PriceLevel> = (0..*size)
            .map(|i| PriceLevel::new(50000.0 - i as f64, dec!(1.0), timestamp))
            .collect();

        let event = MarketEvent {
            event_type: MarketEventType::Snapshot,
            instrument,
            timestamp,
            local_timestamp: timestamp + 100,
            sequence: Some(1),
            data: MarketData::Book {
                bids: levels.clone(),
                asks: levels,
            },
        };

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| black_box(&event).clone());
        });
    }

    group.finish();
}

// =============================================================================
// ENUM METHOD BENCHMARKS
// =============================================================================

/// Benchmark Exchange::as_str().
fn bench_exchange_as_str(c: &mut Criterion) {
    let exchange = Exchange::Deribit;

    c.bench_function("Exchange::as_str", |b| {
        b.iter(|| black_box(exchange).as_str());
    });
}

/// Benchmark Side::opposite().
fn bench_side_opposite(c: &mut Criterion) {
    let side = Side::Bid;

    c.bench_function("Side::opposite", |b| {
        b.iter(|| black_box(side).opposite());
    });
}

/// Benchmark Instrument::symbol().
fn bench_instrument_symbol(c: &mut Criterion) {
    let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");

    c.bench_function("Instrument::symbol", |b| {
        b.iter(|| black_box(&instrument).symbol());
    });
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark PriceLevel JSON serialization.
fn bench_price_level_serialize_json(c: &mut Criterion) {
    let level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);

    c.bench_function("PriceLevel::to_json", |b| {
        b.iter(|| serde_json::to_string(black_box(&level)));
    });
}

/// Benchmark PriceLevel JSON deserialization.
fn bench_price_level_deserialize_json(c: &mut Criterion) {
    let level = PriceLevel::new(50000.0, dec!(1.5), 1234567890);
    let json = serde_json::to_string(&level).unwrap();

    c.bench_function("PriceLevel::from_json", |b| {
        b.iter(|| serde_json::from_str::<PriceLevel>(black_box(&json)));
    });
}

/// Benchmark MarketEvent JSON round-trip with varying book sizes.
fn bench_market_event_json_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("MarketEvent JSON round-trip");

    for size in [1, 10, 50].iter() {
        let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
        let timestamp = 1234567890_i64;

        let levels: Vec<PriceLevel> = (0..*size)
            .map(|i| PriceLevel::new(50000.0 - i as f64, dec!(1.0), timestamp))
            .collect();

        let event = MarketEvent {
            event_type: MarketEventType::Snapshot,
            instrument,
            timestamp,
            local_timestamp: timestamp + 100,
            sequence: Some(1),
            data: MarketData::Book {
                bids: levels.clone(),
                asks: levels,
            },
        };

        let json = serde_json::to_string(&event).unwrap();

        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let serialized = serde_json::to_string(black_box(&event)).unwrap();
                let _: MarketEvent = serde_json::from_str(black_box(&serialized)).unwrap();
            });
        });
    }

    group.finish();
}

// =============================================================================
// MEMORY SIZE BENCHMARKS (compile-time verification)
// =============================================================================

/// Benchmark type size calculations (for documentation).
fn bench_type_sizes(c: &mut Criterion) {
    c.bench_function("std::mem::size_of::<PriceLevel>", |b| {
        b.iter(|| black_box(std::mem::size_of::<PriceLevel>()));
    });
}

// =============================================================================
// TIMESTAMP BENCHMARKS
// =============================================================================

/// Benchmark timestamp conversion to DateTime.
fn bench_timestamp_to_datetime(c: &mut Criterion) {
    let timestamp: Timestamp = 1703808000000000;

    c.bench_function("timestamp_to_datetime", |b| {
        b.iter(|| timestamp_to_datetime(black_box(timestamp)));
    });
}

/// Benchmark now_micros().
fn bench_now_micros(c: &mut Criterion) {
    c.bench_function("now_micros", |b| {
        b.iter(|| now_micros());
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    creation,
    bench_price_level_new,
    bench_instrument_new,
    bench_market_event_book,
);

criterion_group!(
    cloning,
    bench_price_level_clone,
    bench_instrument_clone,
    bench_market_event_clone,
);

criterion_group!(
    enum_methods,
    bench_exchange_as_str,
    bench_side_opposite,
    bench_instrument_symbol,
);

criterion_group!(
    serialization,
    bench_price_level_serialize_json,
    bench_price_level_deserialize_json,
    bench_market_event_json_roundtrip,
);

criterion_group!(
    misc,
    bench_type_sizes,
    bench_timestamp_to_datetime,
    bench_now_micros,
);

criterion_main!(creation, cloning, enum_methods, serialization, misc);
