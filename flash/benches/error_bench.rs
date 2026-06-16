//! Benchmarks for Flash error handling.
//!
//! # Performance Targets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Error creation | < 100 ns |
//! | Error display | < 500 ns |
//! | is_recoverable() | < 10 ns |
//! | error_code() | < 10 ns |
//! | severity() | < 10 ns |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench --bench error_bench
//! ```

use astra_flash::core::error::{ErrorSeverity, FlashError};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// =============================================================================
// ERROR CREATION BENCHMARKS
// =============================================================================

/// Benchmark ConnectionFailed error creation.
fn bench_connection_failed_new(c: &mut Criterion) {
    c.bench_function("FlashError::ConnectionFailed::new", |b| {
        b.iter(|| FlashError::ConnectionFailed(black_box("wss://example.com".to_string())));
    });
}

/// Benchmark Disconnected error creation (struct variant).
fn bench_disconnected_new(c: &mut Criterion) {
    c.bench_function("FlashError::Disconnected::new", |b| {
        b.iter(|| FlashError::Disconnected {
            reason: black_box("Server closed".to_string()),
            will_retry: black_box(true),
        });
    });
}

/// Benchmark SequenceGap error creation.
fn bench_sequence_gap_new(c: &mut Criterion) {
    c.bench_function("FlashError::SequenceGap::new", |b| {
        b.iter(|| FlashError::SequenceGap {
            expected: black_box(100),
            actual: black_box(105),
        });
    });
}

/// Benchmark InvalidPrice error creation.
fn bench_invalid_price_new(c: &mut Criterion) {
    c.bench_function("FlashError::InvalidPrice::new", |b| {
        b.iter(|| FlashError::InvalidPrice {
            price: black_box(-1.0),
            reason: black_box("Price cannot be negative".to_string()),
        });
    });
}

/// Benchmark RateLimited error creation.
fn bench_rate_limited_new(c: &mut Criterion) {
    c.bench_function("FlashError::RateLimited::new", |b| {
        b.iter(|| FlashError::RateLimited {
            exchange: black_box("binance".to_string()),
            retry_after_ms: black_box(1000),
        });
    });
}

// =============================================================================
// ERROR DISPLAY BENCHMARKS
// =============================================================================

/// Benchmark error Display trait (to_string).
fn bench_error_display(c: &mut Criterion) {
    let err = FlashError::SequenceGap {
        expected: 100,
        actual: 105,
    };

    c.bench_function("FlashError::to_string", |b| {
        b.iter(|| black_box(&err).to_string());
    });
}

/// Benchmark complex error Display.
fn bench_error_display_complex(c: &mut Criterion) {
    let err = FlashError::ExchangeError {
        exchange: "deribit".to_string(),
        code: 10001,
        message: "Instrument not found".to_string(),
    };

    c.bench_function("FlashError::to_string (complex)", |b| {
        b.iter(|| black_box(&err).to_string());
    });
}

// =============================================================================
// HELPER METHOD BENCHMARKS
// =============================================================================

/// Benchmark is_recoverable() method.
fn bench_is_recoverable(c: &mut Criterion) {
    let err = FlashError::ConnectionFailed("test".to_string());

    c.bench_function("FlashError::is_recoverable", |b| {
        b.iter(|| black_box(&err).is_recoverable());
    });
}

/// Benchmark is_retryable() method (alias).
fn bench_is_retryable(c: &mut Criterion) {
    let err = FlashError::RateLimited {
        exchange: "binance".to_string(),
        retry_after_ms: 1000,
    };

    c.bench_function("FlashError::is_retryable", |b| {
        b.iter(|| black_box(&err).is_retryable());
    });
}

/// Benchmark requires_snapshot() method.
fn bench_requires_snapshot(c: &mut Criterion) {
    let err = FlashError::SequenceGap {
        expected: 100,
        actual: 105,
    };

    c.bench_function("FlashError::requires_snapshot", |b| {
        b.iter(|| black_box(&err).requires_snapshot());
    });
}

/// Benchmark error_code() method.
fn bench_error_code(c: &mut Criterion) {
    let err = FlashError::ConnectionFailed("test".to_string());

    c.bench_function("FlashError::error_code", |b| {
        b.iter(|| black_box(&err).error_code());
    });
}

/// Benchmark severity() method.
fn bench_severity(c: &mut Criterion) {
    let err = FlashError::ConnectionFailed("test".to_string());

    c.bench_function("FlashError::severity", |b| {
        b.iter(|| black_box(&err).severity());
    });
}

// =============================================================================
// SEVERITY DISPLAY BENCHMARKS
// =============================================================================

/// Benchmark ErrorSeverity Display.
fn bench_severity_display(c: &mut Criterion) {
    let severity = ErrorSeverity::Error;

    c.bench_function("ErrorSeverity::to_string", |b| {
        b.iter(|| black_box(severity).to_string());
    });
}

// =============================================================================
// ERROR MATCHING BENCHMARKS
// =============================================================================

/// Benchmark pattern matching on error variants.
fn bench_error_match(c: &mut Criterion) {
    let err = FlashError::SequenceGap {
        expected: 100,
        actual: 105,
    };

    c.bench_function("FlashError::match", |b| {
        b.iter(|| match black_box(&err) {
            FlashError::SequenceGap { expected, actual } => (*expected, *actual),
            _ => (0, 0),
        });
    });
}

// =============================================================================
// FROM TRAIT BENCHMARKS
// =============================================================================

/// Benchmark From<serde_json::Error> conversion.
fn bench_from_json_error(c: &mut Criterion) {
    let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();

    c.bench_function("FlashError::from(serde_json::Error)", |b| {
        // We need to clone the error each iteration since From consumes it
        b.iter(|| {
            let err = serde_json::from_str::<i32>("not a number").unwrap_err();
            let _: FlashError = black_box(err).into();
        });
    });
}

/// Benchmark From<std::io::Error> conversion.
fn bench_from_io_error(c: &mut Criterion) {
    c.bench_function("FlashError::from(std::io::Error)", |b| {
        b.iter(|| {
            let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
            let _: FlashError = black_box(err).into();
        });
    });
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    creation,
    bench_connection_failed_new,
    bench_disconnected_new,
    bench_sequence_gap_new,
    bench_invalid_price_new,
    bench_rate_limited_new,
);

criterion_group!(
    display,
    bench_error_display,
    bench_error_display_complex,
    bench_severity_display,
);

criterion_group!(
    methods,
    bench_is_recoverable,
    bench_is_retryable,
    bench_requires_snapshot,
    bench_error_code,
    bench_severity,
);

criterion_group!(matching, bench_error_match,);

criterion_group!(conversions, bench_from_json_error, bench_from_io_error,);

criterion_main!(creation, display, methods, matching, conversions);
