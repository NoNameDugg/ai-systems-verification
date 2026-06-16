//! Serialization Performance Benchmarks for Batch 2.1 Data Structures.
//!
//! # Benchmarks
//!
//! 1. OandaPrice deserialization
//! 2. OandaLevel deserialization
//! 3. OrderBookSnapshot serialization
//! 4. OrderBookLevel serialization
//! 5. AlphaSignal serialization
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | OandaPrice deserialize | < 1 μs |
//! | OrderBookSnapshot serialize | < 5 μs |
//! | AlphaSignal serialize | < 2 μs |

use astra_flash::book::BookSnapshot;
use astra_flash::core::types::{Exchange, Instrument, PriceLevel};
use astra_flash::fusion::{AlphaSignal, SignalDirection};
use astra_flash::gateway::{OrderBookLevel, OrderBookSnapshot};
use astra_flash::network::adapters::{OandaLevel, OandaPrice};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use rust_decimal_macros::dec;

// =============================================================================
// TEST FIXTURES
// =============================================================================

fn test_oanda_price_json() -> String {
    r#"{
        "type": "PRICE",
        "time": "2023-12-29T00:00:00.000000Z",
        "instrument": "EUR_USD",
        "bids": [
            {"price": "1.08505", "liquidity": 1000000},
            {"price": "1.08500", "liquidity": 500000}
        ],
        "asks": [
            {"price": "1.08520", "liquidity": 750000},
            {"price": "1.08525", "liquidity": 250000}
        ]
    }"#
    .to_string()
}

fn test_oanda_level_json() -> String {
    r#"{"price": "1.08505", "liquidity": 1000000}"#.to_string()
}

fn test_order_book_snapshot() -> OrderBookSnapshot {
    OrderBookSnapshot::new(
        "EUR_USD",
        "oanda",
        1234567890,
        (0..20)
            .map(|i| OrderBookLevel::new(1.0850 - (i as f64 * 0.0001), 1000000.0))
            .collect(),
        (0..20)
            .map(|i| OrderBookLevel::new(1.0852 + (i as f64 * 0.0001), 750000.0))
            .collect(),
    )
}

fn test_alpha_signal() -> AlphaSignal {
    AlphaSignal::new("EUR_USD", SignalDirection::Long, 0.75, 0.85)
}

fn test_book_snapshot() -> BookSnapshot {
    let ts = 1234567890_i64;
    BookSnapshot {
        instrument: Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD"),
        timestamp: ts,
        bids: (0..20)
            .map(|i| PriceLevel::new(1.0850 - (i as f64 * 0.0001), dec!(1000000), ts))
            .collect(),
        asks: (0..20)
            .map(|i| PriceLevel::new(1.0852 + (i as f64 * 0.0001), dec!(750000), ts))
            .collect(),
    }
}

// =============================================================================
// OANDA DESERIALIZATION BENCHMARKS
// =============================================================================

fn bench_oanda_price_deserialize(c: &mut Criterion) {
    let json = test_oanda_price_json();
    let bytes = json.as_bytes().len();

    let mut group = c.benchmark_group("oanda_price_deserialize");
    group.throughput(Throughput::Bytes(bytes as u64));

    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let price: OandaPrice = serde_json::from_str(black_box(&json)).unwrap();
            black_box(price)
        });
    });

    group.finish();
}

fn bench_oanda_level_deserialize(c: &mut Criterion) {
    let json = test_oanda_level_json();

    c.bench_function("oanda_level_deserialize", |b| {
        b.iter(|| {
            let level: OandaLevel = serde_json::from_str(black_box(&json)).unwrap();
            black_box(level)
        });
    });
}

// =============================================================================
// GATEWAY SERIALIZATION BENCHMARKS
// =============================================================================

fn bench_order_book_snapshot_serialize(c: &mut Criterion) {
    let snapshot = test_order_book_snapshot();

    let mut group = c.benchmark_group("order_book_snapshot_serialize");

    group.bench_function("to_json", |b| {
        b.iter(|| {
            let json = black_box(&snapshot).to_json().unwrap();
            black_box(json)
        });
    });

    group.bench_function("to_json_bytes", |b| {
        b.iter(|| {
            let bytes = black_box(&snapshot).to_json_bytes().unwrap();
            black_box(bytes)
        });
    });

    group.finish();
}

fn bench_order_book_level_serialize(c: &mut Criterion) {
    let level = OrderBookLevel::new(1.0850, 1000000.0);

    c.bench_function("order_book_level_serialize", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&level)).unwrap();
            black_box(json)
        });
    });
}

// =============================================================================
// ALPHA SIGNAL SERIALIZATION BENCHMARKS
// =============================================================================

fn bench_alpha_signal_serialize(c: &mut Criterion) {
    let signal = test_alpha_signal();

    let mut group = c.benchmark_group("alpha_signal_serialize");

    group.bench_function("to_json", |b| {
        b.iter(|| {
            let json = black_box(&signal).to_json().unwrap();
            black_box(json)
        });
    });

    group.bench_function("to_json_bytes", |b| {
        b.iter(|| {
            let bytes = black_box(&signal).to_json_bytes().unwrap();
            black_box(bytes)
        });
    });

    group.finish();
}

// =============================================================================
// CONVERSION BENCHMARKS
// =============================================================================

fn bench_order_book_snapshot_from_book(c: &mut Criterion) {
    let book_snapshot = test_book_snapshot();

    c.bench_function("order_book_snapshot_from_book", |b| {
        b.iter(|| {
            let snapshot = OrderBookSnapshot::from_book_snapshot(black_box(&book_snapshot));
            black_box(snapshot)
        });
    });
}

fn bench_alpha_signal_from_book(c: &mut Criterion) {
    let book_snapshot = test_book_snapshot();

    c.bench_function("alpha_signal_from_book", |b| {
        b.iter(|| {
            let signal = AlphaSignal::from_book_snapshot(black_box(&book_snapshot), 0.85);
            black_box(signal)
        });
    });
}

// =============================================================================
// CRITERION SETUP
// =============================================================================

criterion_group!(
    benches,
    bench_oanda_price_deserialize,
    bench_oanda_level_deserialize,
    bench_order_book_snapshot_serialize,
    bench_order_book_level_serialize,
    bench_alpha_signal_serialize,
    bench_order_book_snapshot_from_book,
    bench_alpha_signal_from_book,
);

criterion_main!(benches);
