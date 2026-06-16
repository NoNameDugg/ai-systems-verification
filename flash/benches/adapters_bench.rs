//! Benchmarks for Flash Exchange Adapters.
//!
//! # Performance Targets (per STANDARDS.md)
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Parse message | < 1 μs |
//! | Build subscribe | < 1 μs |
//! | Adapter creation | < 100 ns |
//! | Exchange lookup | < 50 ns |
//! | is_heartbeat check | < 100 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench adapters_bench
//! ```

use astra_flash::core::types::{Exchange, Instrument};
use astra_flash::network::adapters::{
    create_adapter, BinanceAdapter, BookInterval, DeribitAdapter, ExchangeAdapter, OandaAdapter,
    RateLimit, UpdateInterval,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// =============================================================================
// TEST DATA - Real Exchange Messages
// =============================================================================

/// Deribit order book snapshot (minimal).
const DERIBIT_SNAPSHOT: &str = r#"{
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
                ["new", 41999.5, 5.0]
            ],
            "asks": [
                ["new", 42001.0, 8.0],
                ["new", 42001.5, 3.5]
            ]
        }
    }
}"#;

/// Deribit order book delta (minimal).
const DERIBIT_DELTA: &str = r#"{
    "jsonrpc": "2.0",
    "method": "subscription",
    "params": {
        "channel": "book.BTC-PERPETUAL.raw",
        "data": {
            "type": "change",
            "timestamp": 1703836800001,
            "instrument_name": "BTC-PERPETUAL",
            "prev_change_id": 12345678,
            "change_id": 12345679,
            "bids": [["change", 42000.0, 11.0]],
            "asks": []
        }
    }
}"#;

/// Deribit heartbeat message.
const DERIBIT_HEARTBEAT: &str = r#"{
    "jsonrpc": "2.0",
    "method": "heartbeat",
    "params": {"type": "heartbeat"}
}"#;

/// Binance depth update message (minimal).
const BINANCE_DEPTH: &str = r#"{
    "e": "depthUpdate",
    "E": 1703836800000,
    "s": "BTCUSDT",
    "U": 12345678,
    "u": 12345679,
    "b": [
        ["42000.00", "10.5"],
        ["41999.50", "5.0"]
    ],
    "a": [
        ["42001.00", "8.0"],
        ["42001.50", "3.5"]
    ]
}"#;

/// Binance partial depth message.
const BINANCE_PARTIAL_DEPTH: &str = r#"{
    "lastUpdateId": 12345678,
    "bids": [
        ["42000.00", "10.5"],
        ["41999.50", "5.0"],
        ["41999.00", "3.0"],
        ["41998.50", "2.0"],
        ["41998.00", "1.5"]
    ],
    "asks": [
        ["42001.00", "8.0"],
        ["42001.50", "3.5"],
        ["42002.00", "4.0"],
        ["42002.50", "2.5"],
        ["42003.00", "1.0"]
    ]
}"#;

/// OANDA price message (minimal).
const OANDA_PRICE: &str = r#"{
    "type": "PRICE",
    "time": "2023-12-29T12:00:00.000000000Z",
    "instrument": "EUR_USD",
    "bids": [
        {"price": "1.10500", "liquidity": 1000000},
        {"price": "1.10495", "liquidity": 2000000}
    ],
    "asks": [
        {"price": "1.10505", "liquidity": 1000000},
        {"price": "1.10510", "liquidity": 2000000}
    ]
}"#;

/// OANDA heartbeat message.
const OANDA_HEARTBEAT: &str = r#"{"type":"HEARTBEAT","time":"2023-12-29T12:00:00Z"}"#;

// =============================================================================
// ADAPTER CREATION BENCHMARKS
// =============================================================================

/// Benchmark DeribitAdapter::default() creation.
fn bench_deribit_adapter_creation(c: &mut Criterion) {
    c.bench_function("DeribitAdapter::default", |b| {
        b.iter(|| black_box(DeribitAdapter::default()));
    });
}

/// Benchmark BinanceAdapter::default() creation.
fn bench_binance_adapter_creation(c: &mut Criterion) {
    c.bench_function("BinanceAdapter::default", |b| {
        b.iter(|| black_box(BinanceAdapter::default()));
    });
}

/// Benchmark OandaAdapter::new() creation.
fn bench_oanda_adapter_creation(c: &mut Criterion) {
    c.bench_function("OandaAdapter::new", |b| {
        b.iter(|| black_box(OandaAdapter::new("test_account")));
    });
}

/// Benchmark create_adapter factory function.
fn bench_create_adapter_factory(c: &mut Criterion) {
    let mut group = c.benchmark_group("create_adapter");

    for exchange in [Exchange::Deribit, Exchange::Binance, Exchange::Oanda] {
        group.bench_with_input(BenchmarkId::from_parameter(exchange), &exchange, |b, ex| {
            b.iter(|| black_box(create_adapter(*ex)));
        });
    }

    group.finish();
}

// =============================================================================
// EXCHANGE LOOKUP BENCHMARKS
// =============================================================================

/// Benchmark ExchangeAdapter::exchange() lookup.
fn bench_exchange_lookup(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("exchange_lookup");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.exchange()));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.exchange()));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.exchange()));
    });

    group.finish();
}

/// Benchmark websocket_url() lookup.
fn bench_websocket_url_lookup(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("websocket_url");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.websocket_url()));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.websocket_url()));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.websocket_url()));
    });

    group.finish();
}

/// Benchmark rate_limit() lookup.
fn bench_rate_limit_lookup(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("rate_limit");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.rate_limit()));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.rate_limit()));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.rate_limit()));
    });

    group.finish();
}

// =============================================================================
// MESSAGE PARSING BENCHMARKS
// =============================================================================

/// Benchmark Deribit message parsing.
fn bench_deribit_parse_message(c: &mut Criterion) {
    let adapter = DeribitAdapter::default();

    let mut group = c.benchmark_group("deribit_parse");

    group.bench_function("snapshot", |b| {
        b.iter(|| black_box(adapter.parse_message(DERIBIT_SNAPSHOT)));
    });

    group.bench_function("delta", |b| {
        b.iter(|| black_box(adapter.parse_message(DERIBIT_DELTA)));
    });

    group.bench_function("heartbeat", |b| {
        b.iter(|| black_box(adapter.parse_message(DERIBIT_HEARTBEAT)));
    });

    group.finish();
}

/// Benchmark Binance message parsing.
fn bench_binance_parse_message(c: &mut Criterion) {
    let adapter = BinanceAdapter::default();

    let mut group = c.benchmark_group("binance_parse");

    group.bench_function("depth_update", |b| {
        b.iter(|| black_box(adapter.parse_message(BINANCE_DEPTH)));
    });

    group.bench_function("partial_depth", |b| {
        b.iter(|| black_box(adapter.parse_message(BINANCE_PARTIAL_DEPTH)));
    });

    group.finish();
}

/// Benchmark OANDA message parsing.
fn bench_oanda_parse_message(c: &mut Criterion) {
    let adapter = OandaAdapter::new("test_account");

    let mut group = c.benchmark_group("oanda_parse");

    group.bench_function("price", |b| {
        b.iter(|| black_box(adapter.parse_message(OANDA_PRICE)));
    });

    group.bench_function("heartbeat", |b| {
        b.iter(|| black_box(adapter.parse_message(OANDA_HEARTBEAT)));
    });

    group.finish();
}

/// Benchmark all adapters with same message size.
fn bench_parse_message_comparison(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("parse_message_comparison");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.parse_message(DERIBIT_SNAPSHOT)));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.parse_message(BINANCE_PARTIAL_DEPTH)));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.parse_message(OANDA_PRICE)));
    });

    group.finish();
}

// =============================================================================
// HEARTBEAT DETECTION BENCHMARKS
// =============================================================================

/// Benchmark is_heartbeat() detection.
fn bench_is_heartbeat(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("is_heartbeat");

    // True positive (is a heartbeat)
    group.bench_function("Deribit_true", |b| {
        b.iter(|| black_box(deribit.is_heartbeat(DERIBIT_HEARTBEAT)));
    });

    // True negative (not a heartbeat)
    group.bench_function("Deribit_false", |b| {
        b.iter(|| black_box(deribit.is_heartbeat(DERIBIT_SNAPSHOT)));
    });

    group.bench_function("OANDA_true", |b| {
        b.iter(|| black_box(oanda.is_heartbeat(OANDA_HEARTBEAT)));
    });

    group.bench_function("OANDA_false", |b| {
        b.iter(|| black_box(oanda.is_heartbeat(OANDA_PRICE)));
    });

    group.finish();
}

// =============================================================================
// SUBSCRIPTION BUILDING BENCHMARKS
// =============================================================================

/// Benchmark build_subscribe() message generation.
fn bench_build_subscribe(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let instruments = vec![
        Instrument::new("BTC", "USDT", Exchange::Binance, "BTCUSDT"),
        Instrument::new("ETH", "USDT", Exchange::Binance, "ETHUSDT"),
        Instrument::new("BTC", "PERPETUAL", Exchange::Deribit, "BTC-PERPETUAL"),
    ];

    let mut group = c.benchmark_group("build_subscribe");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.build_subscribe(&instruments)));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.build_subscribe(&instruments)));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.build_subscribe(&instruments)));
    });

    group.finish();
}

/// Benchmark build_unsubscribe() message generation.
fn bench_build_unsubscribe(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let instruments = vec![Instrument::new(
        "BTC",
        "PERPETUAL",
        Exchange::Deribit,
        "BTC-PERPETUAL",
    )];

    let mut group = c.benchmark_group("build_unsubscribe");

    group.bench_function("Deribit", |b| {
        b.iter(|| black_box(deribit.build_unsubscribe(&instruments)));
    });

    group.bench_function("Binance", |b| {
        b.iter(|| black_box(binance.build_unsubscribe(&instruments)));
    });

    group.bench_function("OANDA", |b| {
        b.iter(|| black_box(oanda.build_unsubscribe(&instruments)));
    });

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark message parsing throughput (messages per second).
fn bench_parse_throughput(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let binance = BinanceAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("parse_throughput");
    group.throughput(Throughput::Elements(10_000));

    group.bench_function("Deribit_10K", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(deribit.parse_message(DERIBIT_DELTA));
            }
        });
    });

    group.bench_function("Binance_10K", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(binance.parse_message(BINANCE_DEPTH));
            }
        });
    });

    group.bench_function("OANDA_10K", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                black_box(oanda.parse_message(OANDA_PRICE));
            }
        });
    });

    group.finish();
}

/// Benchmark heartbeat check throughput.
fn bench_heartbeat_throughput(c: &mut Criterion) {
    let deribit = DeribitAdapter::default();
    let oanda = OandaAdapter::new("test");

    let mut group = c.benchmark_group("heartbeat_throughput");
    group.throughput(Throughput::Elements(100_000));

    group.bench_function("Deribit_100K", |b| {
        b.iter(|| {
            for _ in 0..100_000 {
                black_box(deribit.is_heartbeat(DERIBIT_HEARTBEAT));
            }
        });
    });

    group.bench_function("OANDA_100K", |b| {
        b.iter(|| {
            for _ in 0..100_000 {
                black_box(oanda.is_heartbeat(OANDA_HEARTBEAT));
            }
        });
    });

    group.finish();
}

// =============================================================================
// RATE LIMIT CONSTANT BENCHMARKS
// =============================================================================

/// Benchmark RateLimit constant access.
fn bench_rate_limit_constants(c: &mut Criterion) {
    c.bench_function("RateLimit::DERIBIT", |b| {
        b.iter(|| black_box(RateLimit::DERIBIT));
    });

    c.bench_function("RateLimit::BINANCE", |b| {
        b.iter(|| black_box(RateLimit::BINANCE));
    });

    c.bench_function("RateLimit::OANDA", |b| {
        b.iter(|| black_box(RateLimit::OANDA));
    });
}

// =============================================================================
// ADAPTER CONFIGURATION BENCHMARKS
// =============================================================================

/// Benchmark DeribitAdapter with different book intervals.
fn bench_deribit_book_intervals(c: &mut Criterion) {
    let mut group = c.benchmark_group("deribit_book_interval");

    group.bench_function("Raw", |b| {
        b.iter(|| black_box(DeribitAdapter::new(BookInterval::Raw)));
    });

    group.bench_function("Ms100", |b| {
        b.iter(|| black_box(DeribitAdapter::new(BookInterval::Ms100)));
    });

    group.bench_function("None", |b| {
        b.iter(|| black_box(DeribitAdapter::new(BookInterval::None)));
    });

    group.finish();
}

/// Benchmark BinanceAdapter with different update intervals.
fn bench_binance_update_intervals(c: &mut Criterion) {
    let mut group = c.benchmark_group("binance_update_interval");

    group.bench_function("Ms1000", |b| {
        b.iter(|| black_box(BinanceAdapter::new(UpdateInterval::Ms1000)));
    });

    group.bench_function("Ms100", |b| {
        b.iter(|| black_box(BinanceAdapter::new(UpdateInterval::Ms100)));
    });

    group.finish();
}

/// Benchmark OandaAdapter with token.
fn bench_oanda_with_token(c: &mut Criterion) {
    c.bench_function("OandaAdapter::with_token", |b| {
        b.iter(|| black_box(OandaAdapter::with_token("account_id", "access_token")));
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    adapter_creation,
    bench_deribit_adapter_creation,
    bench_binance_adapter_creation,
    bench_oanda_adapter_creation,
    bench_create_adapter_factory,
);

criterion_group!(
    adapter_lookup,
    bench_exchange_lookup,
    bench_websocket_url_lookup,
    bench_rate_limit_lookup,
);

criterion_group!(
    message_parsing,
    bench_deribit_parse_message,
    bench_binance_parse_message,
    bench_oanda_parse_message,
    bench_parse_message_comparison,
);

criterion_group!(heartbeat_detection, bench_is_heartbeat,);

criterion_group!(
    subscription_building,
    bench_build_subscribe,
    bench_build_unsubscribe,
);

criterion_group!(
    throughput,
    bench_parse_throughput,
    bench_heartbeat_throughput,
);

criterion_group!(rate_limits, bench_rate_limit_constants,);

criterion_group!(
    adapter_config,
    bench_deribit_book_intervals,
    bench_binance_update_intervals,
    bench_oanda_with_token,
);

criterion_main!(
    adapter_creation,
    adapter_lookup,
    message_parsing,
    heartbeat_detection,
    subscription_building,
    throughput,
    rate_limits,
    adapter_config,
);
