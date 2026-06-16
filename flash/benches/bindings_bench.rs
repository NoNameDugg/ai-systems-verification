//! Benchmarks for Python binding type conversions (Phase 5.1, 5.2, & 5.3).
//!
//! These benchmarks measure the performance of Rust ↔ Python type conversions,
//! OrderBook operations, and Stream interface.
//!
//! # Performance Budgets
//!
//! ## Type Conversions (Phase 5.1)
//!
//! | Operation | Budget |
//! |-----------|--------|
//! | Enum conversion | < 10 ns |
//! | PriceLevel conversion | < 100 ns |
//! | Instrument conversion | < 200 ns |
//! | MarketEvent conversion | < 1 μs |
//! | BookSnapshot (50 levels) | < 50 μs |
//!
//! ## OrderBook Operations (Phase 5.2)
//!
//! | Operation | Budget |
//! |-----------|--------|
//! | PyOrderBook::new | < 1 μs |
//! | apply_snapshot (50 levels) | < 100 μs |
//! | apply_delta (single level) | < 10 μs |
//! | best_bid/best_ask | < 1 μs |
//! | mid_price/spread | < 1 μs |
//! | top_bids(10) | < 10 μs |
//! | snapshot(50) | < 100 μs |
//!
//! ## Stream Operations (Phase 5.3)
//!
//! | Operation | Budget |
//! |-----------|--------|
//! | PyStreamConfig::new | < 100 ns |
//! | PyFlashClient::new | < 1 μs |
//! | PyStreamIterator::stop | < 10 ns |
//! | Config getters | < 10 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! # Without Python (tests internal From trait performance)
//! cargo bench --bench bindings_bench
//!
//! # With Python feature
//! cargo bench --bench bindings_bench --features python
//! ```

#![cfg(feature = "python")]

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal_macros::dec;

use astra_flash::bindings::orderbook::{PyOrderBook, PyOrderBookConfig};
use astra_flash::bindings::stream::{PyFlashClient, PyStreamConfig};
use astra_flash::bindings::types::{
    PyBookSnapshot, PyExchange, PyInstrument, PyMarketData, PyMarketEvent, PyMarketEventType,
    PyPriceLevel, PySide,
};
use astra_flash::book::BookSnapshot;
use astra_flash::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side,
};

// =============================================================================
// TEST HELPERS
// =============================================================================

fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

fn test_price_level() -> PriceLevel {
    PriceLevel::new(50000.0, dec!(1.5), 1703808000000000)
}

fn test_price_levels(count: usize) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| PriceLevel::new(50000.0 + i as f64, dec!(1.5), 1703808000000000))
        .collect()
}

fn test_book_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument: test_instrument(),
        timestamp: 1703808000000000,
        local_timestamp: 1703808000001000,
        sequence: Some(12345),
        data: MarketData::Book {
            bids: test_price_levels(50),
            asks: test_price_levels(50),
        },
    }
}

fn test_trade_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: test_instrument(),
        timestamp: 1703808000000000,
        local_timestamp: 1703808000001000,
        sequence: Some(12346),
        data: MarketData::Trade {
            price: 50050.0,
            quantity: dec!(0.5),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    }
}

fn test_book_snapshot(depth: usize) -> BookSnapshot {
    BookSnapshot {
        instrument: test_instrument(),
        timestamp: 1703808000000000,
        bids: test_price_levels(depth),
        asks: test_price_levels(depth),
    }
}

// =============================================================================
// ENUM CONVERSION BENCHMARKS
// =============================================================================

fn bench_enum_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("enum_conversions");

    // Exchange: Rust → Python
    group.bench_function("exchange_to_py", |b| {
        let exchange = Exchange::Deribit;
        b.iter(|| {
            let py: PyExchange = black_box(exchange).into();
            black_box(py)
        });
    });

    // Exchange: Python → Rust
    group.bench_function("exchange_from_py", |b| {
        let py = PyExchange::Deribit;
        b.iter(|| {
            let rust: Exchange = black_box(py).into();
            black_box(rust)
        });
    });

    // Exchange round-trip
    group.bench_function("exchange_roundtrip", |b| {
        let exchange = Exchange::Binance;
        b.iter(|| {
            let py: PyExchange = black_box(exchange).into();
            let back: Exchange = py.into();
            black_box(back)
        });
    });

    // Side: Rust → Python
    group.bench_function("side_to_py", |b| {
        let side = Side::Bid;
        b.iter(|| {
            let py: PySide = black_box(side).into();
            black_box(py)
        });
    });

    // Side: Python → Rust
    group.bench_function("side_from_py", |b| {
        let py = PySide::Bid;
        b.iter(|| {
            let rust: Side = black_box(py).into();
            black_box(rust)
        });
    });

    // Side.opposite()
    group.bench_function("side_opposite", |b| {
        let side = PySide::Bid;
        b.iter(|| {
            let opposite = black_box(side).opposite();
            black_box(opposite)
        });
    });

    // MarketEventType: Rust → Python
    group.bench_function("event_type_to_py", |b| {
        let event_type = MarketEventType::Snapshot;
        b.iter(|| {
            let py: PyMarketEventType = black_box(event_type).into();
            black_box(py)
        });
    });

    // MarketEventType: Python → Rust
    group.bench_function("event_type_from_py", |b| {
        let py = PyMarketEventType::Snapshot;
        b.iter(|| {
            let rust: MarketEventType = black_box(py).into();
            black_box(rust)
        });
    });

    group.finish();
}

// =============================================================================
// STRUCT CONVERSION BENCHMARKS
// =============================================================================

fn bench_struct_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("struct_conversions");

    // Instrument: Rust → Python
    group.bench_function("instrument_to_py", |b| {
        let inst = test_instrument();
        b.iter(|| {
            let py: PyInstrument = black_box(inst.clone()).into();
            black_box(py)
        });
    });

    // Instrument: Python → Rust
    group.bench_function("instrument_from_py", |b| {
        let py: PyInstrument = test_instrument().into();
        b.iter(|| {
            let rust: Instrument = black_box(py.clone()).into();
            black_box(rust)
        });
    });

    // PriceLevel: Rust → Python
    group.bench_function("price_level_to_py", |b| {
        let level = test_price_level();
        b.iter(|| {
            let py: PyPriceLevel = black_box(level.clone()).into();
            black_box(py)
        });
    });

    // PriceLevel: Python → Rust
    group.bench_function("price_level_from_py", |b| {
        let py: PyPriceLevel = test_price_level().into();
        b.iter(|| {
            let rust: PriceLevel = black_box(py.clone()).into();
            black_box(rust)
        });
    });

    // PriceLevel with high precision decimal
    group.bench_function("price_level_high_precision", |b| {
        let level = PriceLevel::new(50000.0, dec!(1.23456789012345678901234567), 123);
        b.iter(|| {
            let py: PyPriceLevel = black_box(level.clone()).into();
            black_box(py)
        });
    });

    group.finish();
}

// =============================================================================
// MARKET DATA CONVERSION BENCHMARKS
// =============================================================================

fn bench_market_data_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_data_conversions");

    // MarketData::Book with varying depths
    for depth in [10, 25, 50, 100] {
        group.throughput(Throughput::Elements(depth as u64 * 2)); // bids + asks

        group.bench_with_input(
            BenchmarkId::new("book_to_py", depth),
            &depth,
            |b, &depth| {
                let data = MarketData::Book {
                    bids: test_price_levels(depth),
                    asks: test_price_levels(depth),
                };
                b.iter(|| {
                    let py: PyMarketData = black_box(data.clone()).into();
                    black_box(py)
                });
            },
        );
    }

    // MarketData::Trade
    group.bench_function("trade_to_py", |b| {
        let data = MarketData::Trade {
            price: 50000.0,
            quantity: dec!(1.5),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        };
        b.iter(|| {
            let py: PyMarketData = black_box(data.clone()).into();
            black_box(py)
        });
    });

    // MarketData::Heartbeat
    group.bench_function("heartbeat_to_py", |b| {
        let data = MarketData::Heartbeat {
            exchange_time: 1234567890,
        };
        b.iter(|| {
            let py: PyMarketData = black_box(data.clone()).into();
            black_box(py)
        });
    });

    group.finish();
}

// =============================================================================
// MARKET EVENT CONVERSION BENCHMARKS
// =============================================================================

fn bench_market_event_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_event_conversions");

    // Book event with 50 levels per side
    group.bench_function("book_event_to_py", |b| {
        let event = test_book_event();
        b.iter(|| {
            let py: PyMarketEvent = black_box(event.clone()).into();
            black_box(py)
        });
    });

    // Trade event
    group.bench_function("trade_event_to_py", |b| {
        let event = test_trade_event();
        b.iter(|| {
            let py: PyMarketEvent = black_box(event.clone()).into();
            black_box(py)
        });
    });

    // Latency calculation
    group.bench_function("latency_micros", |b| {
        let py: PyMarketEvent = test_book_event().into();
        b.iter(|| {
            let latency = black_box(&py).latency_micros();
            black_box(latency)
        });
    });

    group.finish();
}

// =============================================================================
// BOOK SNAPSHOT CONVERSION BENCHMARKS
// =============================================================================

fn bench_book_snapshot_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_snapshot_conversions");

    // Snapshot with varying depths
    for depth in [10, 25, 50, 100] {
        group.throughput(Throughput::Elements(depth as u64 * 2)); // bids + asks

        group.bench_with_input(
            BenchmarkId::new("snapshot_to_py", depth),
            &depth,
            |b, &depth| {
                let snapshot = test_book_snapshot(depth);
                b.iter(|| {
                    let py: PyBookSnapshot = black_box(snapshot.clone()).into();
                    black_box(py)
                });
            },
        );
    }

    // Snapshot methods
    group.bench_function("snapshot_best_bid", |b| {
        let py: PyBookSnapshot = test_book_snapshot(50).into();
        b.iter(|| {
            let best = black_box(&py).best_bid();
            black_box(best)
        });
    });

    group.bench_function("snapshot_best_ask", |b| {
        let py: PyBookSnapshot = test_book_snapshot(50).into();
        b.iter(|| {
            let best = black_box(&py).best_ask();
            black_box(best)
        });
    });

    group.bench_function("snapshot_mid_price", |b| {
        let py: PyBookSnapshot = test_book_snapshot(50).into();
        b.iter(|| {
            let mid = black_box(&py).mid_price();
            black_box(mid)
        });
    });

    group.bench_function("snapshot_spread", |b| {
        let py: PyBookSnapshot = test_book_snapshot(50).into();
        b.iter(|| {
            let spread = black_box(&py).spread();
            black_box(spread)
        });
    });

    group.finish();
}

// =============================================================================
// ACCESSOR BENCHMARKS
// =============================================================================

fn bench_accessors(c: &mut Criterion) {
    let mut group = c.benchmark_group("accessors");

    // PyInstrument accessors
    let inst: PyInstrument = test_instrument().into();
    group.bench_function("instrument_base", |b| {
        b.iter(|| {
            let base = black_box(&inst).base();
            black_box(base)
        });
    });

    group.bench_function("instrument_symbol", |b| {
        b.iter(|| {
            let symbol = black_box(&inst).symbol();
            black_box(symbol)
        });
    });

    // PyPriceLevel accessors
    let level: PyPriceLevel = test_price_level().into();
    group.bench_function("price_level_price", |b| {
        b.iter(|| {
            let price = black_box(&level).price();
            black_box(price)
        });
    });

    group.bench_function("price_level_quantity", |b| {
        b.iter(|| {
            let qty = black_box(&level).quantity();
            black_box(qty)
        });
    });

    group.bench_function("price_level_quantity_as_float", |b| {
        b.iter(|| {
            let qty = black_box(&level).quantity_as_float();
            black_box(qty)
        });
    });

    group.bench_function("price_level_is_empty", |b| {
        b.iter(|| {
            let empty = black_box(&level).is_empty();
            black_box(empty)
        });
    });

    // PyExchange accessors
    let exchange = PyExchange::Deribit;
    group.bench_function("exchange_as_str", |b| {
        b.iter(|| {
            let s = black_box(exchange).as_str();
            black_box(s)
        });
    });

    // PySide accessors
    let side = PySide::Bid;
    group.bench_function("side_as_str", |b| {
        b.iter(|| {
            let s = black_box(side).as_str();
            black_box(s)
        });
    });

    group.finish();
}

// =============================================================================
// BATCH CONVERSION BENCHMARKS
// =============================================================================

fn bench_batch_conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_conversions");

    // Batch of price levels
    for count in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("price_levels_batch", count),
            &count,
            |b, &count| {
                let levels = test_price_levels(count);
                b.iter(|| {
                    let py: Vec<PyPriceLevel> = black_box(&levels)
                        .iter()
                        .map(|l| l.clone().into())
                        .collect();
                    black_box(py)
                });
            },
        );
    }

    // Batch of market events
    for count in [10, 50, 100] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("events_batch", count),
            &count,
            |b, &count| {
                let events: Vec<MarketEvent> = (0..count).map(|_| test_trade_event()).collect();

                b.iter(|| {
                    let py: Vec<PyMarketEvent> = black_box(&events)
                        .iter()
                        .map(|e| e.clone().into())
                        .collect();
                    black_box(py)
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// ORDERBOOK BENCHMARKS (Phase 5.2)
// =============================================================================

fn test_py_instrument() -> PyInstrument {
    test_instrument().into()
}

fn test_py_price_levels(count: usize) -> Vec<PyPriceLevel> {
    test_price_levels(count)
        .into_iter()
        .map(|l| l.into())
        .collect()
}

fn bench_orderbook_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_construction");

    // PyOrderBook::new
    group.bench_function("orderbook_new", |b| {
        let inst = test_py_instrument();
        b.iter(|| {
            let book = PyOrderBook::new(black_box(inst.clone()), None);
            black_box(book)
        });
    });

    // PyOrderBook::new with config
    group.bench_function("orderbook_new_with_config", |b| {
        let inst = test_py_instrument();
        let config = PyOrderBookConfig::new(50, 100, false, true);
        b.iter(|| {
            let book = PyOrderBook::new(black_box(inst.clone()), Some(black_box(config.clone())));
            black_box(book)
        });
    });

    // PyOrderBook::from_snapshot
    group.bench_function("orderbook_from_snapshot", |b| {
        let inst = test_py_instrument();
        let bids = test_py_price_levels(50);
        let asks = test_py_price_levels(50);
        b.iter(|| {
            let book = PyOrderBook::from_snapshot(
                black_box(inst.clone()),
                black_box(bids.clone()),
                black_box(asks.clone()),
                1703808000000000,
                None,
            );
            black_box(book)
        });
    });

    group.finish();
}

fn bench_orderbook_snapshots(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_snapshots");

    // apply_snapshot with varying depths
    for depth in [10, 25, 50, 100] {
        group.throughput(Throughput::Elements(depth as u64 * 2)); // bids + asks

        group.bench_with_input(
            BenchmarkId::new("apply_snapshot", depth),
            &depth,
            |b, &depth| {
                let inst = test_py_instrument();
                let bids = test_py_price_levels(depth);
                let asks = test_py_price_levels(depth);

                b.iter(|| {
                    let book = PyOrderBook::new(inst.clone(), None);
                    book.apply_snapshot(
                        black_box(bids.clone()),
                        black_box(asks.clone()),
                        1703808000000000,
                    );
                    black_box(book)
                });
            },
        );
    }

    // Export snapshot
    for depth in [10, 25, 50] {
        group.throughput(Throughput::Elements(depth as u64 * 2));

        group.bench_with_input(
            BenchmarkId::new("export_snapshot", depth),
            &depth,
            |b, &depth| {
                let inst = test_py_instrument();
                let book = PyOrderBook::new(inst, None);
                let bids = test_py_price_levels(depth);
                let asks = test_py_price_levels(depth);
                book.apply_snapshot(bids, asks, 1703808000000000);

                b.iter(|| {
                    let snap = black_box(&book).snapshot(depth);
                    black_box(snap)
                });
            },
        );
    }

    group.finish();
}

fn bench_orderbook_deltas(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_deltas");

    // apply_delta single level (bid side)
    group.bench_function("apply_delta_single_bid", |b| {
        let inst = test_py_instrument();
        let book = PyOrderBook::new(inst, None);
        let bids = test_py_price_levels(50);
        let asks = test_py_price_levels(50);
        book.apply_snapshot(bids, asks, 1703808000000000);

        let delta_levels: Vec<PyPriceLevel> =
            vec![PriceLevel::new(50025.0, dec!(2.0), 1703808001000000).into()];

        b.iter(|| {
            book.apply_delta(
                black_box(PySide::Bid),
                black_box(delta_levels.clone()),
                1703808001000000,
            );
        });
    });

    // apply_delta single level (ask side)
    group.bench_function("apply_delta_single_ask", |b| {
        let inst = test_py_instrument();
        let book = PyOrderBook::new(inst, None);
        let bids = test_py_price_levels(50);
        let asks = test_py_price_levels(50);
        book.apply_snapshot(bids, asks, 1703808000000000);

        let delta_levels: Vec<PyPriceLevel> =
            vec![PriceLevel::new(50075.0, dec!(3.0), 1703808001000000).into()];

        b.iter(|| {
            book.apply_delta(
                black_box(PySide::Ask),
                black_box(delta_levels.clone()),
                1703808001000000,
            );
        });
    });

    // apply_delta multiple levels
    for count in [5, 10, 20] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("apply_delta_batch_bid", count),
            &count,
            |b, &count| {
                let inst = test_py_instrument();
                let book = PyOrderBook::new(inst, None);
                let bids = test_py_price_levels(50);
                let asks = test_py_price_levels(50);
                book.apply_snapshot(bids, asks, 1703808000000000);

                let delta_levels: Vec<PyPriceLevel> = (0..count)
                    .map(|i| {
                        PriceLevel::new(50025.0 + i as f64, dec!(2.0), 1703808001000000).into()
                    })
                    .collect();

                b.iter(|| {
                    book.apply_delta(
                        black_box(PySide::Bid),
                        black_box(delta_levels.clone()),
                        1703808001000000,
                    );
                });
            },
        );
    }

    // update_level
    group.bench_function("update_level", |b| {
        let inst = test_py_instrument();
        let book = PyOrderBook::new(inst, None);
        let bids = test_py_price_levels(50);
        let asks = test_py_price_levels(50);
        book.apply_snapshot(bids, asks, 1703808000000000);

        let level: PyPriceLevel = PriceLevel::new(50025.0, dec!(5.0), 1703808001000000).into();

        b.iter(|| {
            book.update_level(black_box(PySide::Bid), black_box(level.clone()));
        });
    });

    // remove_level
    group.bench_function("remove_level", |b| {
        let inst = test_py_instrument();
        let book = PyOrderBook::new(inst, None);
        let bids = test_py_price_levels(50);
        let asks = test_py_price_levels(50);
        book.apply_snapshot(bids.clone(), asks, 1703808000000000);

        b.iter_batched(
            || {
                // Reset book before each iteration
                book.apply_snapshot(bids.clone(), test_py_price_levels(50), 1703808000000000);
            },
            |_| {
                book.remove_level(black_box(PySide::Bid), black_box(50000.0));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_orderbook_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_queries");

    // Setup a book with data
    let inst = test_py_instrument();
    let book = PyOrderBook::new(inst, None);
    let bids = test_py_price_levels(50);
    let asks = test_py_price_levels(50);
    book.apply_snapshot(bids, asks, 1703808000000000);

    // best_bid
    group.bench_function("best_bid", |b| {
        b.iter(|| {
            let best = black_box(&book).best_bid();
            black_box(best)
        });
    });

    // best_ask
    group.bench_function("best_ask", |b| {
        b.iter(|| {
            let best = black_box(&book).best_ask();
            black_box(best)
        });
    });

    // mid_price
    group.bench_function("mid_price", |b| {
        b.iter(|| {
            let mid = black_box(&book).mid_price();
            black_box(mid)
        });
    });

    // spread
    group.bench_function("spread", |b| {
        b.iter(|| {
            let spread = black_box(&book).spread();
            black_box(spread)
        });
    });

    // spread_bps
    group.bench_function("spread_bps", |b| {
        b.iter(|| {
            let bps = black_box(&book).spread_bps();
            black_box(bps)
        });
    });

    // bid_count / ask_count
    group.bench_function("bid_count", |b| {
        b.iter(|| {
            let count = black_box(&book).bid_count();
            black_box(count)
        });
    });

    group.bench_function("ask_count", |b| {
        b.iter(|| {
            let count = black_box(&book).ask_count();
            black_box(count)
        });
    });

    // total quantities
    group.bench_function("total_bid_quantity", |b| {
        b.iter(|| {
            let qty = black_box(&book).total_bid_quantity();
            black_box(qty)
        });
    });

    group.bench_function("total_ask_quantity", |b| {
        b.iter(|| {
            let qty = black_box(&book).total_ask_quantity();
            black_box(qty)
        });
    });

    group.finish();
}

fn bench_orderbook_top_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_top_levels");

    // Setup a book with data
    let inst = test_py_instrument();
    let book = PyOrderBook::new(inst, None);
    let bids = test_py_price_levels(100);
    let asks = test_py_price_levels(100);
    book.apply_snapshot(bids, asks, 1703808000000000);

    // top_bids with varying depths
    for depth in [5, 10, 25, 50] {
        group.throughput(Throughput::Elements(depth as u64));

        group.bench_with_input(BenchmarkId::new("top_bids", depth), &depth, |b, &depth| {
            b.iter(|| {
                let levels = black_box(&book).top_bids(black_box(depth));
                black_box(levels)
            });
        });

        group.bench_with_input(BenchmarkId::new("top_asks", depth), &depth, |b, &depth| {
            b.iter(|| {
                let levels = black_box(&book).top_asks(black_box(depth));
                black_box(levels)
            });
        });
    }

    // get_level
    group.bench_function("get_level_existing", |b| {
        b.iter(|| {
            let level = black_box(&book).get_level(black_box(PySide::Bid), black_box(50000.0));
            black_box(level)
        });
    });

    group.bench_function("get_level_missing", |b| {
        b.iter(|| {
            let level = black_box(&book).get_level(black_box(PySide::Bid), black_box(99999.0));
            black_box(level)
        });
    });

    group.finish();
}

fn bench_orderbook_misc(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_misc");

    // Setup a book with data
    let inst = test_py_instrument();
    let book = PyOrderBook::new(inst.clone(), None);
    let bids = test_py_price_levels(50);
    let asks = test_py_price_levels(50);
    book.apply_snapshot(bids, asks, 1703808000000000);

    // instrument (cached access)
    group.bench_function("instrument", |b| {
        b.iter(|| {
            let inst = black_box(&book).instrument();
            black_box(inst)
        });
    });

    // stats
    group.bench_function("stats", |b| {
        b.iter(|| {
            let stats = black_box(&book).stats();
            black_box(stats)
        });
    });

    // is_empty (alternative to __bool__)
    group.bench_function("is_empty", |b| {
        b.iter(|| {
            let empty = black_box(&book).is_empty();
            black_box(empty)
        });
    });

    // clear
    group.bench_function("clear", |b| {
        let fresh_bids = test_py_price_levels(50);
        let fresh_asks = test_py_price_levels(50);
        b.iter_batched(
            || {
                let book = PyOrderBook::new(inst.clone(), None);
                book.apply_snapshot(fresh_bids.clone(), fresh_asks.clone(), 1703808000000000);
                book
            },
            |book| {
                book.clear();
                black_box(book)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// =============================================================================
// STREAM BENCHMARKS (Phase 5.3)
// =============================================================================

fn bench_stream_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_config");

    // PyStreamConfig::new
    group.bench_function("config_new", |b| {
        let topics = vec!["market_data.deribit.btc_usd.book".to_string()];
        b.iter(|| {
            let config = PyStreamConfig::new(
                black_box(topics.clone()),
                black_box("bincode".to_string()),
                black_box("$".to_string()),
                black_box(5000),
                black_box(100),
                None,
                None,
            );
            black_box(config)
        });
    });

    // PyStreamConfig::new with consumer group
    group.bench_function("config_new_with_group", |b| {
        let topics = vec!["market_data.deribit.btc_usd.book".to_string()];
        b.iter(|| {
            let config = PyStreamConfig::new(
                black_box(topics.clone()),
                black_box("bincode".to_string()),
                black_box(">".to_string()),
                black_box(5000),
                black_box(100),
                Some("my_group".to_string()),
                Some("consumer_1".to_string()),
            );
            black_box(config)
        });
    });

    // Config getters
    let config = PyStreamConfig::new(
        vec!["topic1".to_string(), "topic2".to_string()],
        "bincode".to_string(),
        "$".to_string(),
        5000,
        100,
        Some("group".to_string()),
        Some("consumer".to_string()),
    );

    group.bench_function("config_topics", |b| {
        b.iter(|| {
            let topics = black_box(&config).topics();
            black_box(topics)
        });
    });

    group.bench_function("config_format", |b| {
        b.iter(|| {
            let format = black_box(&config).format();
            black_box(format)
        });
    });

    group.bench_function("config_start_id", |b| {
        b.iter(|| {
            let id = black_box(&config).start_id();
            black_box(id)
        });
    });

    group.bench_function("config_block_ms", |b| {
        b.iter(|| {
            let ms = black_box(&config).block_ms();
            black_box(ms)
        });
    });

    group.bench_function("config_count", |b| {
        b.iter(|| {
            let count = black_box(&config).count();
            black_box(count)
        });
    });

    group.bench_function("config_group_name", |b| {
        b.iter(|| {
            let name = black_box(&config).group_name();
            black_box(name)
        });
    });

    group.finish();
}

fn bench_flash_client(c: &mut Criterion) {
    let mut group = c.benchmark_group("flash_client");

    // PyFlashClient::new
    group.bench_function("client_new", |b| {
        b.iter(|| {
            let client = PyFlashClient::new(black_box("redis://localhost:6379".to_string()));
            black_box(client)
        });
    });

    // Create a client for subsequent benchmarks
    let client = PyFlashClient::new("redis://localhost:6379".to_string()).unwrap();

    // subscribe_one (creates iterator)
    group.bench_function("subscribe_one", |b| {
        b.iter(|| {
            let iter = black_box(&client).subscribe_one(black_box("topic".to_string()));
            iter.stop(); // Stop immediately to clean up
            black_box(iter)
        });
    });

    // subscribe with config
    group.bench_function("subscribe", |b| {
        let config = PyStreamConfig::new(
            vec!["topic".to_string()],
            "bincode".to_string(),
            "$".to_string(),
            5000,
            100,
            None,
            None,
        );
        b.iter(|| {
            let iter = black_box(&client).subscribe(black_box(config.clone()));
            iter.stop();
            black_box(iter)
        });
    });

    // stats getter
    group.bench_function("stats", |b| {
        b.iter(|| {
            let stats = black_box(&client).stats();
            black_box(stats)
        });
    });

    group.finish();
}

fn bench_stream_iterator(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_iterator");

    let client = PyFlashClient::new("redis://localhost:6379".to_string()).unwrap();
    let config = PyStreamConfig::new(
        vec![
            "topic1".to_string(),
            "topic2".to_string(),
            "topic3".to_string(),
        ],
        "json".to_string(),
        "0".to_string(),
        10000,
        50,
        None,
        None,
    );
    let iterator = client.subscribe(config);

    // is_active
    group.bench_function("is_active", |b| {
        b.iter(|| {
            let active = black_box(&iterator).is_active();
            black_box(active)
        });
    });

    // topic_count
    group.bench_function("topic_count", |b| {
        b.iter(|| {
            let count = black_box(&iterator).topic_count();
            black_box(count)
        });
    });

    // received_count
    group.bench_function("received_count", |b| {
        b.iter(|| {
            let count = black_box(&iterator).received_count();
            black_box(count)
        });
    });

    // config getter
    group.bench_function("config", |b| {
        b.iter(|| {
            let config = black_box(&iterator).config();
            black_box(config)
        });
    });

    // stop (measure atomics)
    group.bench_function("stop", |b| {
        b.iter_batched(
            || client.subscribe_one("topic".to_string()),
            |iter| {
                iter.stop();
                black_box(iter)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// =============================================================================
// CRITERION SETUP
// =============================================================================

criterion_group!(
    benches,
    bench_enum_conversions,
    bench_struct_conversions,
    bench_market_data_conversions,
    bench_market_event_conversions,
    bench_book_snapshot_conversions,
    bench_accessors,
    bench_batch_conversions,
    // OrderBook benchmarks (Phase 5.2)
    bench_orderbook_construction,
    bench_orderbook_snapshots,
    bench_orderbook_deltas,
    bench_orderbook_queries,
    bench_orderbook_top_levels,
    bench_orderbook_misc,
    // Stream benchmarks (Phase 5.3)
    bench_stream_config,
    bench_flash_client,
    bench_stream_iterator,
);

criterion_main!(benches);
