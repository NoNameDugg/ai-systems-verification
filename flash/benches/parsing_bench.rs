//! Benchmarks for Flash Message Parsing.
//!
//! # Performance Targets (per STANDARDS.md)
//!
//! | Operation | Target |
//! |-----------|--------|
//! | JSON parsing | < 1 μs |
//! | Message deserialization | < 5 μs |
//! | Order book update parsing | < 2 μs |
//! | Price level extraction | < 500 ns |
//! | Timestamp parsing | < 200 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench parsing_bench
//! ```

use astra_flash::core::types::{Exchange, Instrument};
use astra_flash::network::adapters::{
    BinanceAdapter, DeribitAdapter, ExchangeAdapter, OandaAdapter,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

// =============================================================================
// TEST DATA - Varying Complexity
// =============================================================================

/// Minimal JSON message (for baseline).
const MINIMAL_JSON: &str = r#"{"type":"test"}"#;

/// Small order book (2 levels each side).
const SMALL_BOOK: &str = r#"{
    "jsonrpc": "2.0",
    "method": "subscription",
    "params": {
        "channel": "book.BTC-PERPETUAL.raw",
        "data": {
            "type": "change",
            "timestamp": 1703836800000,
            "instrument_name": "BTC-PERPETUAL",
            "change_id": 12345679,
            "bids": [["change", 42000.0, 10.5]],
            "asks": [["change", 42001.0, 8.0]]
        }
    }
}"#;

/// Medium order book (10 levels each side).
const MEDIUM_BOOK: &str = r#"{
    "jsonrpc": "2.0",
    "method": "subscription",
    "params": {
        "channel": "book.BTC-PERPETUAL.raw",
        "data": {
            "type": "snapshot",
            "timestamp": 1703836800000,
            "instrument_name": "BTC-PERPETUAL",
            "change_id": 12345678,
            "bids": [
                ["new", 42000.0, 10.5],
                ["new", 41999.5, 5.0],
                ["new", 41999.0, 3.0],
                ["new", 41998.5, 2.5],
                ["new", 41998.0, 2.0],
                ["new", 41997.5, 1.5],
                ["new", 41997.0, 1.0],
                ["new", 41996.5, 0.8],
                ["new", 41996.0, 0.5],
                ["new", 41995.5, 0.3]
            ],
            "asks": [
                ["new", 42001.0, 8.0],
                ["new", 42001.5, 4.0],
                ["new", 42002.0, 3.5],
                ["new", 42002.5, 3.0],
                ["new", 42003.0, 2.5],
                ["new", 42003.5, 2.0],
                ["new", 42004.0, 1.5],
                ["new", 42004.5, 1.0],
                ["new", 42005.0, 0.8],
                ["new", 42005.5, 0.5]
            ]
        }
    }
}"#;

/// Binance depth snapshot (20 levels).
const BINANCE_20_LEVELS: &str = r#"{
    "lastUpdateId": 12345678,
    "bids": [
        ["42000.00", "10.5"],
        ["41999.50", "5.0"],
        ["41999.00", "3.0"],
        ["41998.50", "2.5"],
        ["41998.00", "2.0"],
        ["41997.50", "1.5"],
        ["41997.00", "1.0"],
        ["41996.50", "0.8"],
        ["41996.00", "0.5"],
        ["41995.50", "0.3"],
        ["41995.00", "0.25"],
        ["41994.50", "0.2"],
        ["41994.00", "0.15"],
        ["41993.50", "0.12"],
        ["41993.00", "0.1"],
        ["41992.50", "0.08"],
        ["41992.00", "0.06"],
        ["41991.50", "0.05"],
        ["41991.00", "0.04"],
        ["41990.50", "0.03"]
    ],
    "asks": [
        ["42001.00", "8.0"],
        ["42001.50", "4.0"],
        ["42002.00", "3.5"],
        ["42002.50", "3.0"],
        ["42003.00", "2.5"],
        ["42003.50", "2.0"],
        ["42004.00", "1.5"],
        ["42004.50", "1.0"],
        ["42005.00", "0.8"],
        ["42005.50", "0.5"],
        ["42006.00", "0.25"],
        ["42006.50", "0.2"],
        ["42007.00", "0.15"],
        ["42007.50", "0.12"],
        ["42008.00", "0.1"],
        ["42008.50", "0.08"],
        ["42009.00", "0.06"],
        ["42009.50", "0.05"],
        ["42010.00", "0.04"],
        ["42010.50", "0.03"]
    ]
}"#;

/// OANDA price with multiple liquidity levels.
const OANDA_MULTI_LEVEL: &str = r#"{
    "type": "PRICE",
    "time": "2023-12-29T12:00:00.123456789Z",
    "instrument": "EUR_USD",
    "bids": [
        {"price": "1.10500", "liquidity": 1000000},
        {"price": "1.10495", "liquidity": 2000000},
        {"price": "1.10490", "liquidity": 3000000},
        {"price": "1.10485", "liquidity": 4000000},
        {"price": "1.10480", "liquidity": 5000000}
    ],
    "asks": [
        {"price": "1.10505", "liquidity": 1000000},
        {"price": "1.10510", "liquidity": 2000000},
        {"price": "1.10515", "liquidity": 3000000},
        {"price": "1.10520", "liquidity": 4000000},
        {"price": "1.10525", "liquidity": 5000000}
    ]
}"#;

// =============================================================================
// JSON PARSING BENCHMARKS
// =============================================================================

/// Benchmark raw JSON parsing with serde_json.
fn bench_raw_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("raw_json_parse");

    group.bench_function("minimal", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(MINIMAL_JSON)).unwrap();
            black_box(v)
        });
    });

    group.bench_function("small_book", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(SMALL_BOOK)).unwrap();
            black_box(v)
        });
    });

    group.bench_function("medium_book", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(MEDIUM_BOOK)).unwrap();
            black_box(v)
        });
    });

    group.bench_function("binance_20_levels", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(BINANCE_20_LEVELS)).unwrap();
            black_box(v)
        });
    });

    group.finish();
}

// =============================================================================
// ADAPTER PARSING BY MESSAGE SIZE
// =============================================================================

/// Benchmark Deribit parsing by message complexity.
fn bench_deribit_parsing_by_size(c: &mut Criterion) {
    let adapter = DeribitAdapter::default();

    let mut group = c.benchmark_group("deribit_by_size");

    group.bench_function("small (1 level)", |b| {
        b.iter(|| black_box(adapter.parse_message(SMALL_BOOK)));
    });

    group.bench_function("medium (10 levels)", |b| {
        b.iter(|| black_box(adapter.parse_message(MEDIUM_BOOK)));
    });

    group.finish();
}

/// Benchmark Binance parsing by message complexity.
fn bench_binance_parsing_by_size(c: &mut Criterion) {
    let adapter = BinanceAdapter::default();

    let mut group = c.benchmark_group("binance_by_size");

    group.bench_function("20 levels", |b| {
        b.iter(|| black_box(adapter.parse_message(BINANCE_20_LEVELS)));
    });

    group.finish();
}

/// Benchmark OANDA parsing by message complexity.
fn bench_oanda_parsing_by_size(c: &mut Criterion) {
    let adapter = OandaAdapter::new("test");

    let mut group = c.benchmark_group("oanda_by_size");

    group.bench_function("5 levels", |b| {
        b.iter(|| black_box(adapter.parse_message(OANDA_MULTI_LEVEL)));
    });

    group.finish();
}

// =============================================================================
// STRING CONTAINS BENCHMARKS (for is_heartbeat optimization)
// =============================================================================

/// Benchmark string contains checks (used in fast-path detection).
fn bench_string_contains(c: &mut Criterion) {
    let heartbeat_deribit =
        r#"{"jsonrpc":"2.0","method":"heartbeat","params":{"type":"heartbeat"}}"#;
    let heartbeat_oanda = r#"{"type":"HEARTBEAT","time":"2023-12-29T12:00:00Z"}"#;
    let data_message = MEDIUM_BOOK;

    let mut group = c.benchmark_group("string_contains");

    // Check for heartbeat pattern
    group.bench_function("heartbeat_pattern_found", |b| {
        b.iter(|| black_box(heartbeat_deribit.contains("\"method\":\"heartbeat\"")));
    });

    group.bench_function("heartbeat_pattern_not_found", |b| {
        b.iter(|| black_box(data_message.contains("\"method\":\"heartbeat\"")));
    });

    group.bench_function("oanda_heartbeat_found", |b| {
        b.iter(|| black_box(heartbeat_oanda.contains("\"type\":\"HEARTBEAT\"")));
    });

    group.finish();
}

// =============================================================================
// TIMESTAMP PARSING BENCHMARKS
// =============================================================================

/// Benchmark timestamp parsing approaches.
fn bench_timestamp_parsing(c: &mut Criterion) {
    let unix_ms = 1703836800000i64;
    let iso_timestamp = "2023-12-29T12:00:00.123456789Z";

    let mut group = c.benchmark_group("timestamp_parse");

    // Unix milliseconds to microseconds
    group.bench_function("unix_ms_to_us", |b| {
        b.iter(|| black_box(unix_ms * 1000));
    });

    // ISO 8601 parsing (simplified)
    group.bench_function("iso8601_parse", |b| {
        b.iter(|| {
            // Simplified parsing - just extract components
            let result = iso_timestamp
                .split('T')
                .next()
                .map(|date| date.replace('-', ""));
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// PRICE PARSING BENCHMARKS
// =============================================================================

/// Benchmark price string parsing.
fn bench_price_parsing(c: &mut Criterion) {
    let price_str = "42000.50";
    let price_f64 = 42000.50f64;

    let mut group = c.benchmark_group("price_parse");

    // String to f64
    group.bench_function("str_to_f64", |b| {
        b.iter(|| black_box(price_str.parse::<f64>().unwrap()));
    });

    // f64 already parsed
    group.bench_function("f64_passthrough", |b| {
        b.iter(|| black_box(price_f64));
    });

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark parsing throughput for production workloads.
fn bench_production_throughput(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("production_throughput");
    group.throughput(Throughput::Elements(1_000));

    // Simulate 1000 messages
    group.bench_function("deribit_1K_deltas", |b| {
        b.iter(|| {
            for _ in 0..1_000 {
                black_box(deribit.parse_message(SMALL_BOOK));
            }
        });
    });

    group.bench_function("binance_1K_updates", |b| {
        b.iter(|| {
            for _ in 0..1_000 {
                black_box(binance.parse_message(BINANCE_20_LEVELS));
            }
        });
    });

    group.bench_function("oanda_1K_prices", |b| {
        b.iter(|| {
            for _ in 0..1_000 {
                black_box(oanda.parse_message(OANDA_MULTI_LEVEL));
            }
        });
    });

    group.finish();
}

// =============================================================================
// MEMORY ALLOCATION BENCHMARKS
// =============================================================================

/// Benchmark to track allocations in parsing.
fn bench_allocation_patterns(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();

    let mut group = c.benchmark_group("allocation_patterns");

    // Parse and extract data (forces allocations)
    group.bench_function("full_parse_extract", |b| {
        b.iter(|| {
            let events = deribit.parse_message(MEDIUM_BOOK).unwrap();
            for event in &events {
                black_box(&event.instrument);
                black_box(&event.timestamp);
            }
            black_box(events)
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(json_parsing, bench_raw_json_parsing,);

criterion_group!(
    adapter_parsing,
    bench_deribit_parsing_by_size,
    bench_binance_parsing_by_size,
    bench_oanda_parsing_by_size,
);

criterion_group!(string_operations, bench_string_contains,);

criterion_group!(value_parsing, bench_timestamp_parsing, bench_price_parsing,);

criterion_group!(throughput, bench_production_throughput,);

criterion_group!(allocations, bench_allocation_patterns,);

criterion_main!(
    json_parsing,
    adapter_parsing,
    string_operations,
    value_parsing,
    throughput,
    allocations,
);
