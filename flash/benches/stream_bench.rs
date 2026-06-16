//! Benchmarks for Redis Stream Publisher.
//!
//! Run with: `cargo bench --bench stream_bench`
//!
//! Note: Integration benchmarks require a Redis server and are disabled by default.
//! Set `REDIS_BENCH=1` environment variable to enable them.

use astra_flash::book::{OrderBook, OrderBookConfig};
use astra_flash::core::types::{
    now_micros, Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side,
};
use astra_flash::publisher::stream::{
    PublishResult, SerializationFormat, StreamError, StreamPublisher, StreamPublisherBuilder,
    StreamPublisherConfig, StreamStats, TopicBuilder, TopicType,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal_macros::dec;
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create test price levels.
fn test_levels(count: usize, base_price: f64) -> Vec<PriceLevel> {
    (0..count)
        .map(|i| PriceLevel::new(base_price + (i as f64 * 10.0), dec!(1.5), now_micros()))
        .collect()
}

/// Create a test book snapshot with specified depth.
fn test_book_snapshot(depth: usize) -> astra_flash::book::BookSnapshot {
    let instrument = test_instrument();
    let config = OrderBookConfig::default();
    let mut book = OrderBook::new(instrument, config);

    let bids: Vec<PriceLevel> = (0..depth)
        .map(|i| PriceLevel::new(50000.0 - (i as f64 * 10.0), dec!(1.5), now_micros()))
        .collect();
    let asks: Vec<PriceLevel> = (0..depth)
        .map(|i| PriceLevel::new(50010.0 + (i as f64 * 10.0), dec!(1.5), now_micros()))
        .collect();

    book.apply_snapshot(bids, asks, now_micros());
    book.to_snapshot(depth)
}

/// Create a test market event.
fn test_market_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: test_instrument(),
        timestamp: now_micros(),
        local_timestamp: now_micros(),
        sequence: Some(1),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.1),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    }
}

// =============================================================================
// SERIALIZATION FORMAT BENCHMARKS
// =============================================================================

/// Benchmark SerializationFormat default creation.
fn bench_format_default(c: &mut Criterion) {
    c.bench_function("SerializationFormat::default", |b| {
        b.iter(|| {
            let format = SerializationFormat::default();
            black_box(format)
        });
    });
}

/// Benchmark SerializationFormat as_str.
fn bench_format_as_str(c: &mut Criterion) {
    let format = SerializationFormat::Bincode;

    c.bench_function("SerializationFormat::as_str", |b| {
        b.iter(|| {
            let s = format.as_str();
            black_box(s)
        });
    });
}

// =============================================================================
// CONFIGURATION BENCHMARKS
// =============================================================================

/// Benchmark config default creation.
fn bench_config_default(c: &mut Criterion) {
    c.bench_function("StreamPublisherConfig::default", |b| {
        b.iter(|| {
            let config = StreamPublisherConfig::default();
            black_box(config)
        });
    });
}

/// Benchmark config cloning.
fn bench_config_clone(c: &mut Criterion) {
    let config = StreamPublisherConfig::default();

    c.bench_function("StreamPublisherConfig::clone", |b| {
        b.iter(|| {
            let cloned = config.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark config validation.
fn bench_config_validate(c: &mut Criterion) {
    let config = StreamPublisherConfig::default();

    c.bench_function("StreamPublisherConfig::validate", |b| {
        b.iter(|| {
            let result = config.validate();
            black_box(result)
        });
    });
}

// =============================================================================
// TOPIC BUILDER BENCHMARKS
// =============================================================================

/// Benchmark topic builder creation.
fn bench_topic_builder_new(c: &mut Criterion) {
    c.bench_function("TopicBuilder::new", |b| {
        b.iter(|| {
            let builder = TopicBuilder::new("market_data");
            black_box(builder)
        });
    });
}

/// Benchmark market data topic generation.
fn bench_topic_market_data(c: &mut Criterion) {
    let builder = TopicBuilder::new("market_data");

    c.bench_function("TopicBuilder::market_data", |b| {
        b.iter(|| {
            let topic = builder.market_data(Exchange::Deribit, "BTC", "USD", TopicType::Book);
            black_box(topic)
        });
    });
}

/// Benchmark topic from instrument.
fn bench_topic_for_instrument(c: &mut Criterion) {
    let builder = TopicBuilder::new("market_data");
    let instrument = test_instrument();

    c.bench_function("TopicBuilder::for_instrument", |b| {
        b.iter(|| {
            let topic = builder.for_instrument(&instrument, TopicType::Book);
            black_box(topic)
        });
    });
}

/// Benchmark system topic generation.
fn bench_topic_system(c: &mut Criterion) {
    let builder = TopicBuilder::new("market_data");

    c.bench_function("TopicBuilder::system", |b| {
        b.iter(|| {
            let topic = builder.system("health");
            black_box(topic)
        });
    });
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

/// Benchmark book serialization with JSON.
fn bench_serialize_book_json(c: &mut Criterion) {
    let snapshot = test_book_snapshot(50);

    c.bench_function("serialize_book_json_50", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);
            black_box(data)
        });
    });
}

/// Benchmark book serialization with Bincode.
fn bench_serialize_book_bincode(c: &mut Criterion) {
    let snapshot = test_book_snapshot(50);

    c.bench_function("serialize_book_bincode_50", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);
            black_box(data)
        });
    });
}

/// Benchmark event serialization with JSON.
fn bench_serialize_event_json(c: &mut Criterion) {
    let event = test_market_event();

    c.bench_function("serialize_event_json", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_event(&event, SerializationFormat::Json);
            black_box(data)
        });
    });
}

/// Benchmark event serialization with Bincode.
fn bench_serialize_event_bincode(c: &mut Criterion) {
    let event = test_market_event();

    c.bench_function("serialize_event_bincode", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_event(&event, SerializationFormat::Bincode);
            black_box(data)
        });
    });
}

/// Benchmark book serialization with different sizes.
fn bench_serialize_book_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize_book_bincode");

    for depth in [10, 25, 50, 100].iter() {
        let snapshot = test_book_snapshot(*depth);

        group.bench_with_input(BenchmarkId::from_parameter(depth), depth, |b, _| {
            b.iter(|| {
                let data = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);
                black_box(data)
            });
        });
    }

    group.finish();
}

/// Benchmark JSON vs Bincode comparison.
fn bench_serialize_format_comparison(c: &mut Criterion) {
    let snapshot = test_book_snapshot(50);
    let mut group = c.benchmark_group("serialize_book_format");

    group.bench_function("json", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Json);
            black_box(data)
        });
    });

    group.bench_function("bincode", |b| {
        b.iter(|| {
            let data = StreamPublisher::serialize_book(&snapshot, SerializationFormat::Bincode);
            black_box(data)
        });
    });

    group.finish();
}

// =============================================================================
// STATISTICS BENCHMARKS
// =============================================================================

/// Benchmark stats default creation.
fn bench_stats_default(c: &mut Criterion) {
    c.bench_function("StreamStats::default", |b| {
        b.iter(|| {
            let stats = StreamStats::default();
            black_box(stats)
        });
    });
}

/// Benchmark stats cloning.
fn bench_stats_clone(c: &mut Criterion) {
    let mut stats = StreamStats::default();
    stats.messages_published = 1000;
    stats.bytes_serialized = 1_000_000;
    stats.xadd_commands = 500;
    stats.avg_publish_latency_us = 50.5;
    stats.last_publish = Some(now_micros());

    c.bench_function("StreamStats::clone", |b| {
        b.iter(|| {
            let cloned = stats.clone();
            black_box(cloned)
        });
    });
}

/// Benchmark stats update latency.
fn bench_stats_update_latency(c: &mut Criterion) {
    let mut stats = StreamStats::default();

    c.bench_function("StreamStats::update_latency", |b| {
        b.iter(|| {
            stats.update_latency(black_box(50.0));
        });
    });
}

/// Benchmark stats reset.
fn bench_stats_reset(c: &mut Criterion) {
    let mut stats = StreamStats::default();
    stats.messages_published = 1000;
    stats.bytes_serialized = 1_000_000;

    c.bench_function("StreamStats::reset", |b| {
        b.iter(|| {
            stats.messages_published = 1000;
            stats.bytes_serialized = 1_000_000;
            stats.reset();
            black_box(&stats);
        });
    });
}

// =============================================================================
// BUILDER BENCHMARKS
// =============================================================================

/// Benchmark builder default creation.
fn bench_builder_default(c: &mut Criterion) {
    c.bench_function("StreamPublisherBuilder::default", |b| {
        b.iter(|| {
            let builder = StreamPublisherBuilder::default();
            black_box(builder)
        });
    });
}

/// Benchmark builder with full configuration.
fn bench_builder_full(c: &mut Criterion) {
    c.bench_function("StreamPublisherBuilder::full_config", |b| {
        b.iter(|| {
            let builder = StreamPublisherBuilder::default()
                .format(SerializationFormat::Bincode)
                .max_stream_length(100_000)
                .approximate_trimming(true)
                .include_timestamp(true)
                .topic_prefix("market_data")
                .data_field("data")
                .format_field("format");
            black_box(builder)
        });
    });
}

// =============================================================================
// PUBLISH RESULT BENCHMARKS
// =============================================================================

/// Benchmark publish result creation.
fn bench_publish_result_create(c: &mut Criterion) {
    c.bench_function("PublishResult::new", |b| {
        b.iter(|| {
            let result = PublishResult {
                message_id: "1234567890-0".to_string(),
                topic: "market_data.deribit.btc_usd.book".to_string(),
                data_size: 2048,
                latency_us: 50,
            };
            black_box(result)
        });
    });
}

/// Benchmark publish result cloning.
fn bench_publish_result_clone(c: &mut Criterion) {
    let result = PublishResult {
        message_id: "1234567890-0".to_string(),
        topic: "market_data.deribit.btc_usd.book".to_string(),
        data_size: 2048,
        latency_us: 50,
    };

    c.bench_function("PublishResult::clone", |b| {
        b.iter(|| {
            let cloned = result.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// ERROR BENCHMARKS
// =============================================================================

/// Benchmark error creation.
fn bench_error_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("StreamError");

    group.bench_function("SerializationFailed", |b| {
        b.iter(|| {
            let error = StreamError::SerializationFailed {
                format: SerializationFormat::Json,
                reason: "Invalid UTF-8".to_string(),
            };
            black_box(error)
        });
    });

    group.bench_function("XaddFailed", |b| {
        b.iter(|| {
            let error = StreamError::XaddFailed {
                topic: "market_data.deribit.btc_usd.book".to_string(),
                reason: "Connection refused".to_string(),
            };
            black_box(error)
        });
    });

    group.bench_function("InvalidTopic", |b| {
        b.iter(|| {
            let error = StreamError::InvalidTopic("invalid..topic".to_string());
            black_box(error)
        });
    });

    group.finish();
}

/// Benchmark error display.
fn bench_error_display(c: &mut Criterion) {
    let error = StreamError::XaddFailed {
        topic: "market_data.deribit.btc_usd.book".to_string(),
        reason: "Connection refused".to_string(),
    };

    c.bench_function("StreamError::to_string", |b| {
        b.iter(|| {
            let s = error.to_string();
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
            let format_size = size_of::<SerializationFormat>();
            let config_size = size_of::<StreamPublisherConfig>();
            let stats_size = size_of::<StreamStats>();
            let result_size = size_of::<PublishResult>();
            let topic_type_size = size_of::<TopicType>();

            black_box((
                format_size,
                config_size,
                stats_size,
                result_size,
                topic_type_size,
            ))
        });
    });

    // Print sizes for reference
    println!("\nType sizes:");
    println!(
        "  SerializationFormat: {} bytes",
        std::mem::size_of::<SerializationFormat>()
    );
    println!(
        "  StreamPublisherConfig: {} bytes",
        std::mem::size_of::<StreamPublisherConfig>()
    );
    println!(
        "  StreamStats: {} bytes",
        std::mem::size_of::<StreamStats>()
    );
    println!(
        "  PublishResult: {} bytes",
        std::mem::size_of::<PublishResult>()
    );
    println!("  TopicType: {} bytes", std::mem::size_of::<TopicType>());
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(format_benchmarks, bench_format_default, bench_format_as_str,);

criterion_group!(
    config_benchmarks,
    bench_config_default,
    bench_config_clone,
    bench_config_validate,
);

criterion_group!(
    topic_benchmarks,
    bench_topic_builder_new,
    bench_topic_market_data,
    bench_topic_for_instrument,
    bench_topic_system,
);

criterion_group!(
    serialization_benchmarks,
    bench_serialize_book_json,
    bench_serialize_book_bincode,
    bench_serialize_event_json,
    bench_serialize_event_bincode,
    bench_serialize_book_sizes,
    bench_serialize_format_comparison,
);

criterion_group!(
    stats_benchmarks,
    bench_stats_default,
    bench_stats_clone,
    bench_stats_update_latency,
    bench_stats_reset,
);

criterion_group!(
    builder_benchmarks,
    bench_builder_default,
    bench_builder_full,
);

criterion_group!(
    result_benchmarks,
    bench_publish_result_create,
    bench_publish_result_clone,
);

criterion_group!(error_benchmarks, bench_error_creation, bench_error_display,);

criterion_group!(misc_benchmarks, bench_type_sizes,);

criterion_main!(
    format_benchmarks,
    config_benchmarks,
    topic_benchmarks,
    serialization_benchmarks,
    stats_benchmarks,
    builder_benchmarks,
    result_benchmarks,
    error_benchmarks,
    misc_benchmarks,
);
