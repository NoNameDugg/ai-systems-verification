//! Zero-Copy Tuning Benchmarks (Batch 4.2)
//!
//! This benchmark measures the performance impact of String vs Cow<'static, str>
//! optimizations in the Instrument struct and related hot paths.
//!
//! Run with: cargo bench --bench zero_copy_bench
//! Compare with: cargo bench --bench zero_copy_bench -- --save-baseline before_optimization

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};

use astra_flash::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side,
};
use astra_flash::gateway::{OrderBookSnapshot, OrderBookLevel};
use astra_flash::publisher::buffer::{SerializationBuffer, size_hints};
use rust_decimal_macros::dec;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Create a test instrument.
fn create_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create a test price level.
fn create_price_level(price: f64) -> PriceLevel {
    PriceLevel::new(price, dec!(1.5), 1234567890)
}

/// Create a market event with book data.
fn create_market_event(depth: usize) -> MarketEvent {
    let instrument = create_instrument();
    let bids: Vec<PriceLevel> = (0..depth)
        .map(|i| create_price_level(50000.0 - i as f64 * 10.0))
        .collect();
    let asks: Vec<PriceLevel> = (0..depth)
        .map(|i| create_price_level(50010.0 + i as f64 * 10.0))
        .collect();

    MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument,
        timestamp: 1234567890,
        local_timestamp: 1234567891,
        sequence: Some(1),
        data: MarketData::Book { bids, asks },
    }
}

// =============================================================================
// INSTRUMENT CREATION BENCHMARKS
// =============================================================================

fn bench_instrument_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("instrument_creation");
    group.throughput(Throughput::Elements(1));

    // Benchmark creating instrument with string literals
    group.bench_function("with_literals", |b| {
        b.iter(|| {
            black_box(Instrument::new(
                "BTC",
                "USD",
                Exchange::Deribit,
                "BTC-PERPETUAL",
            ))
        })
    });

    // Benchmark creating instrument with owned strings
    group.bench_function("with_owned_strings", |b| {
        b.iter(|| {
            let base = String::from("BTC");
            let quote = String::from("USD");
            let symbol = String::from("BTC-PERPETUAL");
            black_box(Instrument::new(base, quote, Exchange::Deribit, symbol))
        })
    });

    // Benchmark creating instrument with longer symbol
    group.bench_function("with_long_symbol", |b| {
        b.iter(|| {
            black_box(Instrument::new(
                "BTC",
                "USD",
                Exchange::Deribit,
                "BTC-29DEC23-40000-C", // Longer options symbol
            ))
        })
    });

    group.finish();
}

// =============================================================================
// INSTRUMENT CLONE BENCHMARKS
// =============================================================================

fn bench_instrument_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("instrument_clone");
    group.throughput(Throughput::Elements(1));

    let instrument = create_instrument();

    group.bench_function("single_clone", |b| {
        b.iter(|| black_box(instrument.clone()))
    });

    // Clone in a loop (simulates hot path)
    group.bench_function("batch_clone_100", |b| {
        b.iter(|| {
            let mut clones = Vec::with_capacity(100);
            for _ in 0..100 {
                clones.push(instrument.clone());
            }
            black_box(clones)
        })
    });

    group.finish();
}

// =============================================================================
// MARKET EVENT CLONE BENCHMARKS
// =============================================================================

fn bench_market_event_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_event_clone");

    for depth in [10, 25, 50, 100].iter() {
        let event = create_market_event(*depth);
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::new("depth", depth),
            depth,
            |b, _| {
                b.iter(|| black_box(event.clone()))
            },
        );
    }

    group.finish();
}

// =============================================================================
// STRING ALLOCATION BENCHMARKS
// =============================================================================

fn bench_string_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_allocation");

    // Benchmark String::from with short strings
    group.bench_function("string_from_short", |b| {
        b.iter(|| black_box(String::from("BTC")))
    });

    // Benchmark String::from with medium strings
    group.bench_function("string_from_medium", |b| {
        b.iter(|| black_box(String::from("BTC-PERPETUAL")))
    });

    // Benchmark String::from with longer strings
    group.bench_function("string_from_long", |b| {
        b.iter(|| black_box(String::from("BTC-29DEC2023-40000-CALL")))
    });

    // Benchmark to_string()
    group.bench_function("to_string_exchange", |b| {
        b.iter(|| black_box(Exchange::Deribit.to_string()))
    });

    group.finish();
}

// =============================================================================
// ADAPTER SIMULATION BENCHMARKS
// =============================================================================

fn bench_adapter_instrument_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("adapter_instrument_creation");
    group.throughput(Throughput::Elements(1));

    // Simulate Deribit adapter parse_instrument
    group.bench_function("deribit_style", |b| {
        let symbol = "BTC-PERPETUAL";
        b.iter(|| {
            let base = symbol.split('-').next().unwrap_or(symbol);
            black_box(Instrument::new(base, "USD", Exchange::Deribit, symbol))
        })
    });

    // Simulate Binance adapter parse_instrument
    group.bench_function("binance_style", |b| {
        let symbol = "BTCUSDT";
        b.iter(|| {
            // Binance: BTCUSDT -> BTC/USDT
            let base = &symbol[..3]; // Simplified
            let quote = &symbol[3..];
            black_box(Instrument::new(base, quote, Exchange::Binance, symbol))
        })
    });

    // Simulate OANDA adapter parse_instrument
    group.bench_function("oanda_style", |b| {
        let symbol = "EUR_USD";
        b.iter(|| {
            let parts: Vec<&str> = symbol.split('_').collect();
            let (base, quote) = if parts.len() == 2 {
                (parts[0], parts[1])
            } else {
                ("UNK", "UNK")
            };
            black_box(Instrument::new(base, quote, Exchange::Oanda, symbol))
        })
    });

    group.finish();
}

// =============================================================================
// TRADE EVENT BENCHMARKS
// =============================================================================

fn bench_trade_event_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("trade_event_creation");
    group.throughput(Throughput::Elements(1));

    // Trade event with trade_id (String allocation)
    group.bench_function("with_trade_id", |b| {
        let instrument = create_instrument();
        b.iter(|| {
            black_box(MarketEvent {
                event_type: MarketEventType::Trade,
                instrument: instrument.clone(),
                timestamp: 1234567890,
                local_timestamp: 1234567891,
                sequence: None,
                data: MarketData::Trade {
                    price: 50000.0,
                    quantity: dec!(0.1),
                    side: Side::Bid,
                    trade_id: Some("12345678".to_string()),
                },
            })
        })
    });

    // Trade event without trade_id (no extra allocation)
    group.bench_function("without_trade_id", |b| {
        let instrument = create_instrument();
        b.iter(|| {
            black_box(MarketEvent {
                event_type: MarketEventType::Trade,
                instrument: instrument.clone(),
                timestamp: 1234567890,
                local_timestamp: 1234567891,
                sequence: None,
                data: MarketData::Trade {
                    price: 50000.0,
                    quantity: dec!(0.1),
                    side: Side::Bid,
                    trade_id: None,
                },
            })
        })
    });

    group.finish();
}

// =============================================================================
// BATCH PROCESSING BENCHMARKS
// =============================================================================

fn bench_batch_instrument_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_instrument_creation");

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));

        group.bench_with_input(
            BenchmarkId::new("count", count),
            count,
            |b, &count| {
                let bases = ["BTC", "ETH", "SOL", "XRP", "ADA"];
                let quotes = ["USD", "USDT", "EUR"];

                b.iter(|| {
                    let instruments: Vec<Instrument> = (0..count)
                        .map(|i| {
                            let base = bases[i % bases.len()];
                            let quote = quotes[i % quotes.len()];
                            let raw_symbol = format!("{}-PERPETUAL", base);
                            Instrument::new(base, quote, Exchange::Deribit, raw_symbol)
                        })
                        .collect();
                    black_box(instruments)
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// SYMBOL FORMATTING BENCHMARKS
// =============================================================================

fn bench_symbol_formatting(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_formatting");

    let instrument = create_instrument();

    // Benchmark Instrument::symbol() which allocates
    group.bench_function("instrument_symbol", |b| {
        b.iter(|| black_box(instrument.symbol()))
    });

    // Benchmark Display trait
    group.bench_function("instrument_display", |b| {
        b.iter(|| black_box(format!("{}", instrument)))
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

// =============================================================================
// SERIALIZATION BUFFER BENCHMARKS
// =============================================================================

fn create_gateway_snapshot(depth: usize) -> OrderBookSnapshot {
    let bids: Vec<OrderBookLevel> = (0..depth)
        .map(|i| OrderBookLevel::new(50000.0 - i as f64 * 10.0, 1.5))
        .collect();
    let asks: Vec<OrderBookLevel> = (0..depth)
        .map(|i| OrderBookLevel::new(50010.0 + i as f64 * 10.0, 2.0))
        .collect();

    OrderBookSnapshot::new(
        "BTC_USD",
        "deribit",
        1234567890,
        bids,
        asks,
    )
}

fn bench_serialization_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_buffer");

    for depth in [10, 25, 50].iter() {
        let snapshot = create_gateway_snapshot(*depth);
        group.throughput(Throughput::Elements(1));

        // Benchmark: JSON without buffer reuse (allocates every time)
        group.bench_with_input(
            BenchmarkId::new("json_no_reuse", depth),
            depth,
            |b, _| {
                b.iter(|| {
                    let json = serde_json::to_vec(&snapshot).expect("serialize");
                    black_box(json)
                })
            },
        );

        // Benchmark: JSON with buffer reuse
        group.bench_with_input(
            BenchmarkId::new("json_with_buffer", depth),
            depth,
            |b, _| {
                let mut buffer = SerializationBuffer::with_capacity(size_hints::JSON_BOOK_50_LEVELS);
                b.iter(|| {
                    let bytes = buffer.write_json(&snapshot).expect("serialize");
                    black_box(bytes.len())
                })
            },
        );

        // Benchmark: Bincode without buffer reuse
        group.bench_with_input(
            BenchmarkId::new("bincode_no_reuse", depth),
            depth,
            |b, _| {
                b.iter(|| {
                    let bytes = bincode::serialize(&snapshot).expect("serialize");
                    black_box(bytes)
                })
            },
        );

        // Benchmark: Bincode with buffer reuse
        group.bench_with_input(
            BenchmarkId::new("bincode_with_buffer", depth),
            depth,
            |b, _| {
                let mut buffer = SerializationBuffer::with_capacity(size_hints::BINCODE_BOOK_50_LEVELS);
                b.iter(|| {
                    let bytes = buffer.write_bincode(&snapshot).expect("serialize");
                    black_box(bytes.len())
                })
            },
        );
    }

    group.finish();
}

fn bench_cow_vs_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("cow_vs_string");
    group.throughput(Throughput::Elements(1));

    // Benchmark: OrderBookSnapshot with static strings (Cow::Borrowed)
    group.bench_function("orderbook_static_strings", |b| {
        let bids = vec![OrderBookLevel::new(50000.0, 1.5)];
        let asks = vec![OrderBookLevel::new(50010.0, 2.0)];
        b.iter(|| {
            black_box(OrderBookSnapshot::new(
                "BTC_USD",
                "deribit",
                1234567890,
                bids.clone(),
                asks.clone(),
            ))
        })
    });

    // Benchmark: OrderBookSnapshot with owned strings
    group.bench_function("orderbook_owned_strings", |b| {
        let bids = vec![OrderBookLevel::new(50000.0, 1.5)];
        let asks = vec![OrderBookLevel::new(50010.0, 2.0)];
        b.iter(|| {
            black_box(OrderBookSnapshot::new_owned(
                "BTC_USD".to_string(),
                "deribit".to_string(),
                1234567890,
                bids.clone(),
                asks.clone(),
            ))
        })
    });

    group.finish();
}

fn bench_high_throughput_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_throughput");
    group.throughput(Throughput::Elements(1000));

    let snapshot = create_gateway_snapshot(50);

    // Benchmark: 1000 serializations without buffer reuse
    group.bench_function("1000_json_no_reuse", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let json = serde_json::to_vec(&snapshot).expect("serialize");
                black_box(json.len());
            }
        })
    });

    // Benchmark: 1000 serializations with buffer reuse
    group.bench_function("1000_json_with_buffer", |b| {
        let mut buffer = SerializationBuffer::with_capacity(size_hints::JSON_BOOK_50_LEVELS);
        b.iter(|| {
            for _ in 0..1000 {
                let bytes = buffer.write_json(&snapshot).expect("serialize");
                black_box(bytes.len());
            }
        })
    });

    group.finish();
}

criterion_group!(
    string_benches,
    bench_instrument_creation,
    bench_instrument_clone,
    bench_string_allocation,
);

criterion_group!(
    event_benches,
    bench_market_event_clone,
    bench_trade_event_creation,
);

criterion_group!(
    adapter_benches,
    bench_adapter_instrument_creation,
    bench_batch_instrument_creation,
    bench_symbol_formatting,
);

criterion_group!(
    buffer_benches,
    bench_serialization_buffer,
    bench_cow_vs_string,
    bench_high_throughput_serialization,
);

criterion_main!(string_benches, event_benches, adapter_benches, buffer_benches);
