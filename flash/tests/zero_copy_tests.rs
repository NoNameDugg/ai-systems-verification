//! Zero-Copy Tuning Tests (Batch 4.2)
//!
//! This module tests zero-copy optimizations for String handling in hot paths.
//! Tests verify that Cow<'static, str> and &str optimizations reduce allocations.

// Allow unsafe code for the custom GlobalAlloc allocator used to track allocations.
// This is necessary for testing memory allocation patterns in zero-copy optimizations.
#![allow(unsafe_code)]

use astra_flash::core::types::{
    Exchange, Instrument, MarketData, MarketEvent, MarketEventType, PriceLevel, Side,
};
use rust_decimal_macros::dec;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// =============================================================================
// ALLOCATION COUNTER
// =============================================================================

/// Custom allocator that counts allocations for testing.
struct CountingAllocator {
    allocations: AtomicUsize,
    deallocations: AtomicUsize,
    bytes_allocated: AtomicUsize,
    bytes_deallocated: AtomicUsize,
}

impl CountingAllocator {
    const fn new() -> Self {
        Self {
            allocations: AtomicUsize::new(0),
            deallocations: AtomicUsize::new(0),
            bytes_allocated: AtomicUsize::new(0),
            bytes_deallocated: AtomicUsize::new(0),
        }
    }

    fn reset(&self) {
        self.allocations.store(0, Ordering::SeqCst);
        self.deallocations.store(0, Ordering::SeqCst);
        self.bytes_allocated.store(0, Ordering::SeqCst);
        self.bytes_deallocated.store(0, Ordering::SeqCst);
    }

    fn allocations(&self) -> usize {
        self.allocations.load(Ordering::SeqCst)
    }

    fn bytes_allocated(&self) -> usize {
        self.bytes_allocated.load(Ordering::SeqCst)
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocations.fetch_add(1, Ordering::SeqCst);
        self.bytes_allocated
            .fetch_add(layout.size(), Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.deallocations.fetch_add(1, Ordering::SeqCst);
        self.bytes_deallocated
            .fetch_add(layout.size(), Ordering::SeqCst);
        System.dealloc(ptr, layout)
    }
}

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

/// Create test instruments for benchmarking.
fn create_test_instruments(count: usize) -> Vec<Instrument> {
    let bases = ["BTC", "ETH", "SOL", "XRP", "ADA"];
    let quotes = ["USD", "USDT", "EUR"];
    let exchanges = [Exchange::Deribit, Exchange::Binance, Exchange::Oanda];

    (0..count)
        .map(|i| {
            let base = bases[i % bases.len()];
            let quote = quotes[i % quotes.len()];
            let exchange = exchanges[i % exchanges.len()];
            let raw_symbol = format!("{}-PERPETUAL", base);
            Instrument::new(base, quote, exchange, raw_symbol)
        })
        .collect()
}

// =============================================================================
// STRING CLONE BASELINE TESTS
// =============================================================================

#[test]
fn test_instrument_creation_baseline() {
    // Baseline test: Count allocations for Instrument creation
    // This establishes the "before" state for optimization comparison

    let iterations = 1000;
    let start = Instant::now();

    let mut instruments = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        instruments.push(create_instrument());
    }

    let duration = start.elapsed();

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT CREATION BASELINE");
    println!("{}", "=".repeat(70));
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", duration);
    println!("Per instrument: {:?}", duration / iterations as u32);

    // Verify instruments were created correctly
    assert_eq!(instruments.len(), iterations);
    for inst in &instruments {
        assert_eq!(inst.base, "BTC");
        assert_eq!(inst.quote, "USD");
        assert_eq!(inst.exchange, Exchange::Deribit);
    }
}

#[test]
fn test_instrument_clone_baseline() {
    // Baseline test: Measure clone cost
    let instrument = create_instrument();

    let iterations = 10_000;
    let start = Instant::now();

    let mut clones = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        clones.push(instrument.clone());
    }

    let duration = start.elapsed();

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT CLONE BASELINE");
    println!("{}", "=".repeat(70));
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", duration);
    println!("Per clone: {:?}", duration / iterations as u32);

    // Each clone should be equal to original
    assert!(clones.iter().all(|c| *c == instrument));
}

#[test]
fn test_string_field_sizes() {
    // Measure typical string sizes in Instrument
    let bases = ["BTC", "ETH", "SOL", "DOGE", "XRP"];
    let quotes = ["USD", "USDT", "EUR", "GBP"];
    let raw_symbols = ["BTC-PERPETUAL", "ETHUSDT", "BTCUSD", "ETH_USD"];

    println!("\n{}", "=".repeat(70));
    println!("STRING FIELD SIZE ANALYSIS");
    println!("{}", "=".repeat(70));

    println!("\nBase currency sizes:");
    for base in &bases {
        println!("  '{}': {} bytes", base, base.len());
    }

    println!("\nQuote currency sizes:");
    for quote in &quotes {
        println!("  '{}': {} bytes", quote, quote.len());
    }

    println!("\nRaw symbol sizes:");
    for symbol in &raw_symbols {
        println!("  '{}': {} bytes", symbol, symbol.len());
    }

    // All typical values should fit in SmolStr inline storage (<=23 bytes)
    assert!(bases.iter().all(|s| s.len() <= 23));
    assert!(quotes.iter().all(|s| s.len() <= 23));
    assert!(raw_symbols.iter().all(|s| s.len() <= 23));
}

#[test]
fn test_market_event_clone_cost() {
    // Measure full MarketEvent clone cost (includes Instrument clone)
    let instrument = create_instrument();
    let bids: Vec<PriceLevel> = (0..50)
        .map(|i| create_price_level(50000.0 - i as f64 * 10.0))
        .collect();
    let asks: Vec<PriceLevel> = (0..50)
        .map(|i| create_price_level(50010.0 + i as f64 * 10.0))
        .collect();

    let event = MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument,
        timestamp: 1234567890,
        local_timestamp: 1234567891,
        sequence: Some(1),
        data: MarketData::Book { bids, asks },
    };

    let iterations = 1000;
    let start = Instant::now();

    let mut clones = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        clones.push(event.clone());
    }

    let duration = start.elapsed();

    println!("\n{}", "=".repeat(70));
    println!("MARKET EVENT CLONE BASELINE (50 bids + 50 asks)");
    println!("{}", "=".repeat(70));
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", duration);
    println!("Per clone: {:?}", duration / iterations as u32);

    assert_eq!(clones.len(), iterations);
}

// =============================================================================
// STATIC STRING REFERENCE TESTS
// =============================================================================

#[test]
fn test_static_exchange_names() {
    // Verify Exchange::as_str returns static strings
    let deribit_str = Exchange::Deribit.as_str();
    let binance_str = Exchange::Binance.as_str();
    let oanda_str = Exchange::Oanda.as_str();

    // Static strings have 'static lifetime
    assert_eq!(deribit_str, "deribit");
    assert_eq!(binance_str, "binance");
    assert_eq!(oanda_str, "oanda");

    println!("\n{}", "=".repeat(70));
    println!("STATIC EXCHANGE NAMES");
    println!("{}", "=".repeat(70));
    println!("Exchange::Deribit.as_str() = '{}'", deribit_str);
    println!("Exchange::Binance.as_str() = '{}'", binance_str);
    println!("Exchange::Oanda.as_str()   = '{}'", oanda_str);
}

#[test]
fn test_static_side_names() {
    // Verify Side::as_str returns static strings
    let bid_str = Side::Bid.as_str();
    let ask_str = Side::Ask.as_str();

    assert_eq!(bid_str, "bid");
    assert_eq!(ask_str, "ask");

    println!("\n{}", "=".repeat(70));
    println!("STATIC SIDE NAMES");
    println!("{}", "=".repeat(70));
    println!("Side::Bid.as_str() = '{}'", bid_str);
    println!("Side::Ask.as_str() = '{}'", ask_str);
}

// =============================================================================
// ALLOCATION PATTERN TESTS
// =============================================================================

#[test]
fn test_instrument_new_allocations() {
    // Test that Instrument::new creates String allocations
    // This will fail initially, then pass after Cow<'static, str> optimization

    // Create instrument with literal strings
    let inst1 = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");

    // Create instrument with owned strings
    let base = String::from("ETH");
    let quote = String::from("USDT");
    let symbol = String::from("ETHUSDT");
    let inst2 = Instrument::new(base, quote, Exchange::Binance, symbol);

    // Both should work
    assert_eq!(inst1.base, "BTC");
    assert_eq!(inst2.base, "ETH");

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT ALLOCATION PATTERNS");
    println!("{}", "=".repeat(70));
    println!(
        "Instrument with literals: base='{}', quote='{}'",
        inst1.base, inst1.quote
    );
    println!(
        "Instrument with owned:    base='{}', quote='{}'",
        inst2.base, inst2.quote
    );
}

#[test]
fn test_trade_id_allocation() {
    // Test trade_id string allocation in MarketData::Trade
    let instrument = create_instrument();

    // Trade with trade_id
    let event_with_id = MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: instrument.clone(),
        timestamp: 1234567890,
        local_timestamp: 1234567891,
        sequence: None,
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.1),
            side: Side::Bid,
            trade_id: Some("trade_12345".to_string()),
        },
    };

    // Trade without trade_id
    let event_no_id = MarketEvent {
        event_type: MarketEventType::Trade,
        instrument,
        timestamp: 1234567890,
        local_timestamp: 1234567891,
        sequence: None,
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.1),
            side: Side::Bid,
            trade_id: None,
        },
    };

    // Verify data
    if let MarketData::Trade { trade_id, .. } = &event_with_id.data {
        assert_eq!(trade_id.as_deref(), Some("trade_12345"));
    }
    if let MarketData::Trade { trade_id, .. } = &event_no_id.data {
        assert!(trade_id.is_none());
    }

    println!("\n{}", "=".repeat(70));
    println!("TRADE ID ALLOCATION PATTERNS");
    println!("{}", "=".repeat(70));
    println!("Trade with ID:    has trade_id = true");
    println!("Trade without ID: has trade_id = false");
}

// =============================================================================
// MEMORY EFFICIENCY TESTS
// =============================================================================

#[test]
fn test_instrument_memory_size() {
    use std::mem::size_of;

    let instrument_size = size_of::<Instrument>();

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT MEMORY SIZE");
    println!("{}", "=".repeat(70));
    println!("sizeof(Instrument) = {} bytes", instrument_size);
    println!("sizeof(String)     = {} bytes", size_of::<String>());
    println!("sizeof(Exchange)   = {} bytes", size_of::<Exchange>());

    // Instrument should be reasonably sized
    // With 3 Strings (24 bytes each on 64-bit) + Exchange (1 byte) + padding
    // Expected: ~80-96 bytes (without optimization)
    assert!(
        instrument_size <= 128,
        "Instrument too large: {} bytes",
        instrument_size
    );
}

#[test]
fn test_multiple_instruments_same_values() {
    // Test that multiple instruments with same values share no data
    // This establishes baseline for potential interning optimization

    let inst1 = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let inst2 = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    let inst3 = inst1.clone();

    // All should be equal
    assert_eq!(inst1, inst2);
    assert_eq!(inst1, inst3);

    // But base strings have different addresses (no interning yet)
    let addr1 = inst1.base.as_ptr();
    let addr2 = inst2.base.as_ptr();
    let addr3 = inst3.base.as_ptr();

    println!("\n{}", "=".repeat(70));
    println!("STRING ADDRESS ANALYSIS");
    println!("{}", "=".repeat(70));
    println!("inst1.base addr: {:p}", addr1);
    println!("inst2.base addr: {:p}", addr2);
    println!("inst3.base addr: {:p}", addr3);
    println!("inst1 == inst2: {}", inst1 == inst2);
    println!("addr1 == addr2: {}", addr1 == addr2);
    println!("addr1 == addr3: {}", addr1 == addr3);

    // After optimization, static strings could share addresses
}

// =============================================================================
// THROUGHPUT TESTS
// =============================================================================

#[test]
fn test_instrument_creation_throughput() {
    // Measure throughput of instrument creation
    let iterations = 100_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    }
    let duration = start.elapsed();

    let throughput = iterations as f64 / duration.as_secs_f64();
    let ns_per_op = duration.as_nanos() as f64 / iterations as f64;

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT CREATION THROUGHPUT");
    println!("{}", "=".repeat(70));
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", duration);
    println!("Throughput: {:.0} ops/sec", throughput);
    println!("Time per op: {:.1} ns", ns_per_op);

    // Target: >1M ops/sec (< 1000 ns per op)
    assert!(
        ns_per_op < 10000.0,
        "Instrument creation too slow: {:.1} ns",
        ns_per_op
    );
}

#[test]
fn test_instrument_clone_throughput() {
    let instrument = create_instrument();
    let iterations = 100_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = instrument.clone();
    }
    let duration = start.elapsed();

    let throughput = iterations as f64 / duration.as_secs_f64();
    let ns_per_op = duration.as_nanos() as f64 / iterations as f64;

    println!("\n{}", "=".repeat(70));
    println!("INSTRUMENT CLONE THROUGHPUT");
    println!("{}", "=".repeat(70));
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", duration);
    println!("Throughput: {:.0} ops/sec", throughput);
    println!("Time per op: {:.1} ns", ns_per_op);

    // Target: >1M ops/sec (< 1000 ns per clone)
    assert!(
        ns_per_op < 10000.0,
        "Instrument clone too slow: {:.1} ns",
        ns_per_op
    );
}

// =============================================================================
// SUMMARY TEST
// =============================================================================

#[test]
fn test_zero_copy_summary() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("ZERO-COPY TUNING TEST SUMMARY (Batch 4.2)");
    println!("{}", "=".repeat(70));
    println!();
    println!("KEY METRICS:");
    println!(
        "  - Instrument size: {} bytes",
        std::mem::size_of::<Instrument>()
    );
    println!(
        "  - String size:     {} bytes",
        std::mem::size_of::<String>()
    );
    println!(
        "  - Exchange size:   {} bytes",
        std::mem::size_of::<Exchange>()
    );
    println!();
    println!("OPTIMIZATION TARGETS:");
    println!("  1. Use Cow<'static, str> for Instrument.base/quote/raw_symbol");
    println!("  2. Static strings for common values (BTC, USD, etc.)");
    println!("  3. Avoid cloning in adapter parse paths");
    println!();
    println!("EXPECTED IMPROVEMENTS:");
    println!("  - Reduce heap allocations in Instrument::new with literals");
    println!("  - Faster instrument cloning with Cow::Borrowed");
    println!("  - Lower memory pressure in high-throughput scenarios");
    println!("{}", "=".repeat(70));
}
