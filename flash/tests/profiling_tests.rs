//! Profiling Tests for Flash (Batch 4.1).
//!
//! These tests analyze CPU hotspots and heap allocation patterns.
//!
//! # Running
//!
//! ```bash
//! cargo test --test profiling_tests -- --nocapture
//! ```
//!
//! # Expected CPU Hotspots (Ranked by Impact)
//!
//! | Rank | Operation | Est. CPU% | Notes |
//! |------|-----------|-----------|-------|
//! | 1 | JSON parsing | 30-40% | serde_json allocations |
//! | 2 | Order book updates | 20-30% | BTreeMap operations |
//! | 3 | Serialization | 15-25% | JSON/Bincode output |
//! | 4 | Lock acquisition | 5-10% | RwLock overhead |
//!
//! # Expected Allocation Patterns
//!
//! | Operation | Expected Allocs | Notes |
//! |-----------|-----------------|-------|
//! | PriceLevel::new | 0-1 | Decimal may heap-allocate |
//! | OrderBook::update_level | 0-1 | Only if new entry |
//! | BookSnapshot::new | 2 | bids + asks Vecs |
//! | JSON parse | Many | String allocations |
//! | Bincode serialize | 1 | Output buffer |

use astra_flash::book::{OrderBook, OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel, Side};
use astra_flash::network::adapters::{DeribitAdapter, ExchangeAdapter, OandaAdapter};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::time::Instant;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

fn level(price: f64, quantity: Decimal, timestamp: i64) -> PriceLevel {
    PriceLevel::new(price, quantity, timestamp)
}

fn create_bids(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price - (i as f64 * 0.5), dec!(1), now))
        .collect()
}

fn create_asks(start_price: f64, count: usize) -> Vec<PriceLevel> {
    let now = chrono::Utc::now().timestamp_micros();
    (0..count)
        .map(|i| level(start_price + (i as f64 * 0.5), dec!(1), now))
        .collect()
}

fn create_book(n: usize) -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    book.apply_snapshot(create_bids(50000.0, n), create_asks(50001.0, n), 0);
    book
}

// =============================================================================
// TEST MESSAGES
// =============================================================================

const DERIBIT_DELTA: &str = r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"book.BTC-PERPETUAL.raw","data":{"type":"change","timestamp":1703836800000,"instrument_name":"BTC-PERPETUAL","change_id":12345679,"bids":[["change",42000.0,10.5]],"asks":[["change",42001.0,8.0]]}}}"#;

const DERIBIT_SNAPSHOT: &str = r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"book.BTC-PERPETUAL.raw","data":{"type":"snapshot","timestamp":1703836800000,"instrument_name":"BTC-PERPETUAL","change_id":12345678,"bids":[["new",42000.0,10.5],["new",41999.5,5.0],["new",41999.0,3.0],["new",41998.5,2.5],["new",41998.0,2.0]],"asks":[["new",42001.0,8.0],["new",42001.5,4.0],["new",42002.0,3.5],["new",42002.5,3.0],["new",42003.0,2.5]]}}}"#;

const OANDA_PRICE: &str = r#"{"type":"PRICE","time":"2023-12-29T12:00:00.123456789Z","instrument":"EUR_USD","bids":[{"price":"1.10500","liquidity":1000000},{"price":"1.10495","liquidity":2000000}],"asks":[{"price":"1.10505","liquidity":1000000},{"price":"1.10510","liquidity":2000000}]}"#;

// =============================================================================
// CPU HOTSPOT ANALYSIS
// =============================================================================

/// Measure and report CPU hotspot distribution.
#[test]
fn test_cpu_hotspot_distribution() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("CPU HOTSPOT ANALYSIS - Flash");
    println!("{}", "=".repeat(70));

    let iterations = 10_000;

    // 1. JSON Parsing
    let deribit = DeribitAdapter::default();
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = deribit.parse_message(DERIBIT_DELTA);
    }
    let parse_time = start.elapsed();

    // 2. Order Book Operations
    let mut book = create_book(50);
    let update = level(49999.0, dec!(5), 0);
    let start = Instant::now();
    for _ in 0..iterations {
        book.update_level(Side::Bid, update.clone());
    }
    let update_time = start.elapsed();

    // 3. Snapshot Creation
    let book = create_book(50);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = book.to_snapshot(50);
    }
    let snapshot_time = start.elapsed();

    // 4. JSON Serialization
    let snapshot = book.to_snapshot(50);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&snapshot);
    }
    let json_ser_time = start.elapsed();

    // 5. Bincode Serialization
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = bincode::serialize(&snapshot);
    }
    let bincode_ser_time = start.elapsed();

    // 6. Best Bid Lookup
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = book.best_bid();
    }
    let lookup_time = start.elapsed();

    // Calculate percentages
    let total = parse_time + update_time + snapshot_time + json_ser_time + bincode_ser_time;

    println!("\nOperation Timing ({}K iterations):", iterations / 1000);
    println!("{:-<70}", "");
    println!(
        "{:<30} {:>12} {:>10} {:>10}",
        "Operation", "Total (ms)", "Per-op", "% of Total"
    );
    println!("{:-<70}", "");

    let ops = [
        ("JSON Parsing", parse_time),
        ("Order Book Update", update_time),
        ("Snapshot Creation", snapshot_time),
        ("JSON Serialization", json_ser_time),
        ("Bincode Serialization", bincode_ser_time),
        ("Best Bid Lookup", lookup_time),
    ];

    for (name, time) in &ops {
        let per_op = time.as_nanos() / iterations as u128;
        let pct = (time.as_nanos() as f64 / total.as_nanos() as f64) * 100.0;
        println!(
            "{:<30} {:>10.2} ms {:>8} ns {:>9.1}%",
            name,
            time.as_secs_f64() * 1000.0,
            per_op,
            pct
        );
    }
    println!("{:-<70}", "");

    println!("\nHOTSPOT RANKING:");
    let mut ranked: Vec<_> = ops.iter().collect();
    ranked.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    for (i, (name, time)) in ranked.iter().enumerate() {
        let per_op = time.as_nanos() / iterations as u128;
        println!("  {}. {} ({} ns/op)", i + 1, name, per_op);
    }
}

/// Analyze JSON parsing - typically the biggest hotspot.
#[test]
fn test_hotspot_json_parsing() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("HOTSPOT #1: JSON PARSING ANALYSIS");
    println!("{}", "=".repeat(70));

    let deribit = DeribitAdapter::default();
    let oanda = OandaAdapter::new("test");
    let iterations = 10_000;

    // Raw JSON parsing
    let start = Instant::now();
    for _ in 0..iterations {
        let _: serde_json::Value = serde_json::from_str(DERIBIT_DELTA).unwrap();
    }
    let raw_time = start.elapsed();

    // Full adapter parsing (includes type conversion)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = deribit.parse_message(DERIBIT_DELTA);
    }
    let adapter_time = start.elapsed();

    // Snapshot parsing (larger message)
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = deribit.parse_message(DERIBIT_SNAPSHOT);
    }
    let snapshot_time = start.elapsed();

    // OANDA parsing
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = oanda.parse_message(OANDA_PRICE);
    }
    let oanda_time = start.elapsed();

    println!("\nJSON Parsing Breakdown ({}K iterations):", iterations / 1000);
    println!("{:-<60}", "");
    println!("{:<35} {:>12} {:>10}", "Operation", "Total (ms)", "Per-op");
    println!("{:-<60}", "");
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Raw serde_json (delta)",
        raw_time.as_secs_f64() * 1000.0,
        raw_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Deribit adapter (delta)",
        adapter_time.as_secs_f64() * 1000.0,
        adapter_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Deribit adapter (snapshot 5 lvl)",
        snapshot_time.as_secs_f64() * 1000.0,
        snapshot_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "OANDA adapter (price)",
        oanda_time.as_secs_f64() * 1000.0,
        oanda_time.as_nanos() / iterations as u128
    );
    println!("{:-<60}", "");

    let overhead = adapter_time.as_nanos() - raw_time.as_nanos();
    println!(
        "\nAdapter overhead vs raw JSON: {} ns ({:.1}%)",
        overhead / iterations as u128,
        (overhead as f64 / raw_time.as_nanos() as f64) * 100.0
    );

    println!("\nRecommendations:");
    println!("  - Consider simd-json for SIMD-accelerated parsing");
    println!("  - Pre-validate message type before full parse");
    println!("  - Use zero-copy parsing where possible");
}

/// Analyze order book operations.
#[test]
fn test_hotspot_orderbook_operations() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("HOTSPOT #2: ORDER BOOK OPERATIONS");
    println!("{}", "=".repeat(70));

    let iterations = 100_000;

    // Update level
    let mut book = create_book(50);
    let update = level(49999.0, dec!(5), 0);
    let start = Instant::now();
    for _ in 0..iterations {
        book.update_level(Side::Bid, update.clone());
    }
    let update_time = start.elapsed();

    // Insert new level
    let mut book = create_book(50);
    let start = Instant::now();
    for i in 0..iterations {
        let new_level = level(49000.0 + (i as f64 * 0.01), dec!(1), 0);
        book.update_level(Side::Bid, new_level);
    }
    let insert_time = start.elapsed();

    // Delete level (zero quantity)
    let mut book = create_book(1000);
    let start = Instant::now();
    for i in 0..(iterations.min(1000)) {
        let del = level(50000.0 - (i as f64 * 0.5), dec!(0), 0);
        book.update_level(Side::Bid, del);
    }
    let delete_time = start.elapsed();
    let delete_iters = iterations.min(1000);

    // Best bid lookup
    let book = create_book(50);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = book.best_bid();
    }
    let lookup_time = start.elapsed();

    // Mid price calculation
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = book.mid_price();
    }
    let mid_time = start.elapsed();

    println!(
        "\nOrder Book Operations ({}K iterations):",
        iterations / 1000
    );
    println!("{:-<60}", "");
    println!("{:<35} {:>12} {:>10}", "Operation", "Total (ms)", "Per-op");
    println!("{:-<60}", "");
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Update existing level",
        update_time.as_secs_f64() * 1000.0,
        update_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Insert new level",
        insert_time.as_secs_f64() * 1000.0,
        insert_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Delete level (zero qty)",
        delete_time.as_secs_f64() * 1000.0,
        delete_time.as_nanos() / delete_iters as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Best bid lookup",
        lookup_time.as_secs_f64() * 1000.0,
        lookup_time.as_nanos() / iterations as u128
    );
    println!(
        "{:<35} {:>10.2} ms {:>8} ns",
        "Mid price calculation",
        mid_time.as_secs_f64() * 1000.0,
        mid_time.as_nanos() / iterations as u128
    );
    println!("{:-<60}", "");

    println!("\nRecommendations:");
    println!("  - BTreeMap operations are O(log n) - good for 50 levels");
    println!("  - Consider level caching for best bid/ask");
    println!("  - Batch updates where possible");
}

// =============================================================================
// ALLOCATION ANALYSIS
// =============================================================================

/// Analyze memory allocation patterns.
#[test]
fn test_allocation_analysis() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("ALLOCATION ANALYSIS");
    println!("{}", "=".repeat(70));

    // Size analysis
    println!("\nType Sizes (stack allocation):");
    println!("{:-<50}", "");
    println!("{:<30} {:>15}", "Type", "Size (bytes)");
    println!("{:-<50}", "");
    println!(
        "{:<30} {:>15}",
        "PriceLevel",
        std::mem::size_of::<PriceLevel>()
    );
    println!(
        "{:<30} {:>15}",
        "Decimal",
        std::mem::size_of::<rust_decimal::Decimal>()
    );
    println!(
        "{:<30} {:>15}",
        "Instrument",
        std::mem::size_of::<Instrument>()
    );
    println!(
        "{:<30} {:>15}",
        "OrderBook",
        std::mem::size_of::<OrderBook>()
    );
    println!(
        "{:<30} {:>15}",
        "ThreadSafeOrderBook",
        std::mem::size_of::<ThreadSafeOrderBook>()
    );
    println!("{:-<50}", "");

    // Serialization size analysis
    let book = create_book(50);

    println!("\nSerialized Sizes by Depth:");
    println!("{:-<60}", "");
    println!(
        "{:<15} {:>12} {:>12} {:>15}",
        "Depth", "JSON", "Bincode", "Compression"
    );
    println!("{:-<60}", "");

    for depth in [10, 25, 50] {
        let snapshot = book.to_snapshot(depth);
        let json_size = serde_json::to_vec(&snapshot).unwrap().len();
        let bincode_size = bincode::serialize(&snapshot).unwrap().len();
        let ratio = json_size as f64 / bincode_size as f64;

        println!(
            "{:<15} {:>10} B {:>10} B {:>14.1}x",
            format!("{} levels", depth),
            json_size,
            bincode_size,
            ratio
        );
    }
    println!("{:-<60}", "");

    // Bandwidth estimation
    let snapshot_50 = book.to_snapshot(50);
    let json_50 = serde_json::to_vec(&snapshot_50).unwrap().len();
    let bincode_50 = bincode::serialize(&snapshot_50).unwrap().len();

    println!("\nBandwidth Estimation (50K msg/sec):");
    println!(
        "  JSON:    {:.1} MB/sec",
        (50_000.0 * json_50 as f64) / 1_000_000.0
    );
    println!(
        "  Bincode: {:.1} MB/sec",
        (50_000.0 * bincode_50 as f64) / 1_000_000.0
    );
    println!(
        "  Savings: {:.1} MB/sec",
        (50_000.0 * (json_50 - bincode_50) as f64) / 1_000_000.0
    );
}

/// Analyze allocation-heavy operations.
#[test]
fn test_allocation_heavy_operations() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("ALLOCATION-HEAVY OPERATIONS");
    println!("{}", "=".repeat(70));

    println!("\nAllocation Sources by Operation:");
    println!("{:-<70}", "");
    println!("{:<30} {:<40}", "Operation", "Allocation Sources");
    println!("{:-<70}", "");

    // Parse operations - note this may fail depending on adapter state
    let deribit = DeribitAdapter::default();
    match deribit.parse_message(DERIBIT_DELTA) {
        Ok(events) => {
            println!(
                "{:<30} {:<40}",
                "JSON parsing", "serde_json::Value tree, strings"
            );
            println!(
                "{:<30} {:<40}",
                "Adapter parsing",
                format!("+ Instrument (4 strings), Vec<Event>({})", events.len())
            );
        }
        Err(e) => {
            println!(
                "{:<30} {:<40}",
                "JSON parsing", "serde_json::Value tree, strings"
            );
            println!(
                "{:<30} {:<40}",
                "Adapter parsing",
                format!("(parse error: {})", e)
            );
        }
    }

    // Snapshot
    let book = create_book(50);
    let snapshot = book.to_snapshot(50);
    println!(
        "{:<30} {:<40}",
        "Snapshot creation",
        format!(
            "Vec<PriceLevel> x2 ({} + {} items)",
            snapshot.bids.len(),
            snapshot.asks.len()
        )
    );

    // Serialization
    let json_bytes = serde_json::to_vec(&snapshot).unwrap();
    let bincode_bytes = bincode::serialize(&snapshot).unwrap();
    println!(
        "{:<30} {:<40}",
        "JSON serialize",
        format!("Vec<u8> ({} bytes)", json_bytes.len())
    );
    println!(
        "{:<30} {:<40}",
        "Bincode serialize",
        format!("Vec<u8> ({} bytes)", bincode_bytes.len())
    );

    // Clone
    let _cloned = snapshot.clone();
    println!(
        "{:<30} {:<40}",
        "Snapshot clone", "Deep copy of all fields"
    );

    println!("{:-<70}", "");

    println!("\nOptimization Recommendations:");
    println!("  1. Use object pooling for BookSnapshot");
    println!("  2. Pre-allocate serialization buffers");
    println!("  3. Consider arena allocators for parsing");
    println!("  4. Use Cow<str> for instrument fields");
    println!("  5. Evaluate simd-json for faster parsing");
}

// =============================================================================
// THREAD-SAFE OVERHEAD ANALYSIS
// =============================================================================

/// Analyze thread-safe operation overhead.
#[test]
fn test_thread_safe_overhead() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("THREAD-SAFE OVERHEAD ANALYSIS");
    println!("{}", "=".repeat(70));

    let iterations = 100_000;

    // Raw operations
    let raw_book = create_book(50);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = raw_book.best_bid();
    }
    let raw_lookup = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = raw_book.to_snapshot(50);
    }
    let raw_snapshot = start.elapsed();

    // Thread-safe operations
    let ts_book = ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default());
    ts_book.apply_snapshot(create_bids(50000.0, 50), create_asks(50001.0, 50), 0);

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = ts_book.best_bid();
    }
    let ts_lookup = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = ts_book.snapshot(50);
    }
    let ts_snapshot = start.elapsed();

    println!(
        "\nThread-Safe Overhead ({}K iterations):",
        iterations / 1000
    );
    println!("{:-<70}", "");
    println!(
        "{:<25} {:>12} {:>12} {:>15}",
        "Operation", "Raw (ns)", "TS (ns)", "Overhead"
    );
    println!("{:-<70}", "");

    let raw_lookup_ns = raw_lookup.as_nanos() / iterations as u128;
    let ts_lookup_ns = ts_lookup.as_nanos() / iterations as u128;
    let lookup_overhead = if raw_lookup_ns > 0 {
        format!(
            "+{:.0}%",
            ((ts_lookup_ns as f64 / raw_lookup_ns as f64) - 1.0) * 100.0
        )
    } else {
        "N/A".to_string()
    };

    println!(
        "{:<25} {:>12} {:>12} {:>15}",
        "best_bid lookup", raw_lookup_ns, ts_lookup_ns, lookup_overhead
    );

    let raw_snap_ns = raw_snapshot.as_nanos() / iterations as u128;
    let ts_snap_ns = ts_snapshot.as_nanos() / iterations as u128;
    let snap_overhead = format!(
        "+{:.0}%",
        ((ts_snap_ns as f64 / raw_snap_ns as f64) - 1.0) * 100.0
    );

    println!(
        "{:<25} {:>12} {:>12} {:>15}",
        "snapshot (50 levels)", raw_snap_ns, ts_snap_ns, snap_overhead
    );

    println!("{:-<70}", "");

    println!("\nNotes:");
    println!("  - parking_lot::RwLock is faster than std::RwLock");
    println!("  - Read locks have minimal overhead (~10-20ns)");
    println!("  - Consider lock-free reads for hot paths");
}

// =============================================================================
// PROFILING SUMMARY
// =============================================================================

/// Generate comprehensive profiling summary.
#[test]
fn test_profiling_summary() {
    println!("\n");
    println!("{}", "=".repeat(70));
    println!("PROFILING SUMMARY - Flash Batch 4.1");
    println!("{}", "=".repeat(70));

    println!("\n## CPU HOTSPOTS (Estimated Distribution)");
    println!();
    println!("  1. JSON Parsing ............... ~35-40% of CPU");
    println!("     - serde_json deserialization");
    println!("     - String allocations");
    println!("     - Type conversion in adapters");
    println!();
    println!("  2. Order Book Operations ...... ~25-30% of CPU");
    println!("     - BTreeMap insert/update O(log n)");
    println!("     - Decimal arithmetic");
    println!("     - Snapshot Vec creation");
    println!();
    println!("  3. Serialization .............. ~15-20% of CPU");
    println!("     - JSON output for Gateway");
    println!("     - Bincode for internal use");
    println!("     - Buffer allocation");
    println!();
    println!("  4. Thread Synchronization ..... ~5-10% of CPU");
    println!("     - RwLock acquisition");
    println!("     - Atomic operations");
    println!();
    println!("  5. Network I/O ................ ~5-10% of CPU");
    println!("     - WebSocket message handling");
    println!("     - Redis XADD operations");

    println!("\n## MEMORY ALLOCATION HOTSPOTS");
    println!();
    println!("  High Allocation Frequency:");
    println!("    - JSON parsing (many small strings)");
    println!("    - Snapshot creation (Vec<PriceLevel>)");
    println!("    - Serialization buffers");
    println!();
    println!("  Low Allocation Frequency:");
    println!("    - best_bid/best_ask (reference return)");
    println!("    - mid_price/spread (numeric only)");
    println!("    - Level updates (in-place)");

    println!("\n## OPTIMIZATION RECOMMENDATIONS");
    println!();
    println!("  Immediate Impact:");
    println!("    1. Use simd-json for SIMD-accelerated parsing");
    println!("    2. Pool BookSnapshot objects");
    println!("    3. Pre-allocate serialization buffers");
    println!();
    println!("  Medium-Term:");
    println!("    4. Implement message type pre-detection");
    println!("    5. Consider arena allocators for parsing");
    println!("    6. Batch order book updates");
    println!();
    println!("  Long-Term:");
    println!("    7. Evaluate rkyv for zero-copy deserialization");
    println!("    8. Consider lock-free data structures");
    println!("    9. Profile Redis client operations");

    println!("\n## PERFORMANCE TARGETS");
    println!();
    println!("  | Metric                | Budget    | Current  |");
    println!("  |-----------------------|-----------|----------|");
    println!("  | Internal Latency p95  | < 50 us   | TBD      |");
    println!("  | Internal Latency p99  | < 100 us  | TBD      |");
    println!("  | Throughput            | > 50K/s   | TBD      |");
    println!("  | JSON Parse (delta)    | < 5 us    | TBD      |");
    println!("  | Bincode Serialize     | < 1 us    | TBD      |");

    println!("\n{}", "=".repeat(70));
}
