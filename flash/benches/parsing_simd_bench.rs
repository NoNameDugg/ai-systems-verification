//! SIMD vs serde_json Parsing Performance Benchmarks.
//!
//! # Benchmarks
//!
//! 1. OandaPrice parsing (small payload ~200 bytes)
//! 2. OandaPrice parsing (large payload ~1KB with many levels)
//! 3. OandaHeartbeat parsing
//! 4. Message type detection
//!
//! # Performance Targets
//!
//! | Operation | Target | Notes |
//! |-----------|--------|-------|
//! | OandaPrice small (SIMD) | < 500 ns | SIMD-accelerated |
//! | OandaPrice small (serde) | < 1.5 μs | Baseline |
//! | Expected speedup | 2-4x | SIMD advantage |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench parsing_simd_bench
//! ```

use astra_flash::parsing::{
    parse_oanda_heartbeat_serde, parse_oanda_heartbeat_simd, parse_oanda_price_serde,
    parse_oanda_price_simd, SimdParser,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// =============================================================================
// TEST FIXTURES
// =============================================================================

/// Small OANDA price payload (~200 bytes).
fn small_oanda_price_json() -> Vec<u8> {
    br#"{
        "type": "PRICE",
        "time": "2023-12-29T00:00:00.000000Z",
        "instrument": "EUR_USD",
        "bids": [
            {"price": "1.08505", "liquidity": 1000000}
        ],
        "asks": [
            {"price": "1.08520", "liquidity": 750000}
        ]
    }"#
    .to_vec()
}

/// Large OANDA price payload (~1KB with multiple levels).
fn large_oanda_price_json() -> Vec<u8> {
    br#"{
        "type": "PRICE",
        "time": "2023-12-29T00:00:00.000000Z",
        "instrument": "EUR_USD",
        "bids": [
            {"price": "1.08505", "liquidity": 1000000},
            {"price": "1.08504", "liquidity": 950000},
            {"price": "1.08503", "liquidity": 900000},
            {"price": "1.08502", "liquidity": 850000},
            {"price": "1.08501", "liquidity": 800000},
            {"price": "1.08500", "liquidity": 750000},
            {"price": "1.08499", "liquidity": 700000},
            {"price": "1.08498", "liquidity": 650000},
            {"price": "1.08497", "liquidity": 600000},
            {"price": "1.08496", "liquidity": 550000}
        ],
        "asks": [
            {"price": "1.08520", "liquidity": 1000000},
            {"price": "1.08521", "liquidity": 950000},
            {"price": "1.08522", "liquidity": 900000},
            {"price": "1.08523", "liquidity": 850000},
            {"price": "1.08524", "liquidity": 800000},
            {"price": "1.08525", "liquidity": 750000},
            {"price": "1.08526", "liquidity": 700000},
            {"price": "1.08527", "liquidity": 650000},
            {"price": "1.08528", "liquidity": 600000},
            {"price": "1.08529", "liquidity": 550000}
        ]
    }"#
    .to_vec()
}

/// OANDA heartbeat payload.
fn heartbeat_json() -> Vec<u8> {
    br#"{"type": "HEARTBEAT", "time": "2023-12-29T00:00:00.000000Z"}"#.to_vec()
}

// =============================================================================
// OANDA PRICE PARSING BENCHMARKS
// =============================================================================

fn bench_oanda_price_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("oanda_price_small");
    let base_data = small_oanda_price_json();
    group.throughput(Throughput::Bytes(base_data.len() as u64));

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut data = base_data.clone();
            let result = parse_oanda_price_simd(black_box(&mut data));
            black_box(result)
        });
    });

    group.bench_function("serde_json", |b| {
        let data = base_data.clone();
        b.iter(|| {
            let result = parse_oanda_price_serde(black_box(&data));
            black_box(result)
        });
    });

    group.finish();
}

fn bench_oanda_price_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("oanda_price_large");
    let base_data = large_oanda_price_json();
    group.throughput(Throughput::Bytes(base_data.len() as u64));

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut data = base_data.clone();
            let result = parse_oanda_price_simd(black_box(&mut data));
            black_box(result)
        });
    });

    group.bench_function("serde_json", |b| {
        let data = base_data.clone();
        b.iter(|| {
            let result = parse_oanda_price_serde(black_box(&data));
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// OANDA HEARTBEAT PARSING BENCHMARKS
// =============================================================================

fn bench_oanda_heartbeat(c: &mut Criterion) {
    let mut group = c.benchmark_group("oanda_heartbeat");
    let base_data = heartbeat_json();
    group.throughput(Throughput::Bytes(base_data.len() as u64));

    group.bench_function("simd_json", |b| {
        b.iter(|| {
            let mut data = base_data.clone();
            let result = parse_oanda_heartbeat_simd(black_box(&mut data));
            black_box(result)
        });
    });

    group.bench_function("serde_json", |b| {
        let data = base_data.clone();
        b.iter(|| {
            let result = parse_oanda_heartbeat_serde(black_box(&data));
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// MESSAGE TYPE DETECTION BENCHMARKS
// =============================================================================

fn bench_message_type_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_type_detection");
    let parser = SimdParser::new();

    let price_data = small_oanda_price_json();
    let heartbeat_data = heartbeat_json();

    group.bench_function("detect_price", |b| {
        b.iter(|| {
            let result = parser.detect_message_type(black_box(&price_data));
            black_box(result)
        });
    });

    group.bench_function("detect_heartbeat", |b| {
        b.iter(|| {
            let result = parser.detect_message_type(black_box(&heartbeat_data));
            black_box(result)
        });
    });

    group.bench_function("is_heartbeat_check", |b| {
        b.iter(|| {
            let result = parser.is_heartbeat(black_box(&heartbeat_data));
            black_box(result)
        });
    });

    group.bench_function("is_price_check", |b| {
        b.iter(|| {
            let result = parser.is_price(black_box(&price_data));
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// PAYLOAD SIZE SCALING BENCHMARKS
// =============================================================================

fn bench_payload_size_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_payload_scaling");

    // Create payloads of different sizes
    let sizes = [1, 5, 10, 20];

    for &num_levels in &sizes {
        let payload = create_price_payload(num_levels);

        group.throughput(Throughput::Bytes(payload.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("simd", num_levels),
            &payload,
            |b, data| {
                b.iter(|| {
                    let mut d = data.clone();
                    let result = parse_oanda_price_simd(black_box(&mut d));
                    black_box(result)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("serde", num_levels),
            &payload,
            |b, data| {
                b.iter(|| {
                    let result = parse_oanda_price_serde(black_box(data));
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Create a price payload with N levels on each side.
fn create_price_payload(num_levels: usize) -> Vec<u8> {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for i in 0..num_levels {
        let bid_price = 1.08505 - (i as f64 * 0.00001);
        let ask_price = 1.08520 + (i as f64 * 0.00001);
        let liquidity = 1000000 - (i * 50000);

        bids.push(format!(
            r#"{{"price": "{bid_price:.5}", "liquidity": {liquidity}}}"#
        ));
        asks.push(format!(
            r#"{{"price": "{ask_price:.5}", "liquidity": {liquidity}}}"#
        ));
    }

    let json = format!(
        r#"{{
            "type": "PRICE",
            "time": "2023-12-29T00:00:00.000000Z",
            "instrument": "EUR_USD",
            "bids": [{}],
            "asks": [{}]
        }}"#,
        bids.join(","),
        asks.join(",")
    );

    json.into_bytes()
}

// =============================================================================
// CRITERION SETUP
// =============================================================================

criterion_group!(
    benches,
    bench_oanda_price_small,
    bench_oanda_price_large,
    bench_oanda_heartbeat,
    bench_message_type_detection,
    bench_payload_size_scaling,
);

criterion_main!(benches);
