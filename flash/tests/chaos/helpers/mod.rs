//! Chaos test helpers and utilities
//!
//! Provides infrastructure for chaos injection, mock factories,
//! and recovery validation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use astra_flash::book::{OrderBook, OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel};
use rust_decimal::Decimal;

// ============================================================================
// CHAOS CONFIGURATION
// ============================================================================

/// Configuration for chaos injection
#[derive(Debug, Clone)]
pub struct ChaosConfig {
    /// Probability of failure (0.0 - 1.0)
    pub failure_rate: f64,
    /// Delay injection range
    pub delay_range: (Duration, Duration),
    /// Whether to log chaos events
    pub verbose: bool,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            failure_rate: 0.1,
            delay_range: (Duration::from_millis(0), Duration::from_millis(100)),
            verbose: false,
        }
    }
}

// ============================================================================
// MOCK MESSAGE GENERATORS
// ============================================================================

/// Generate valid order book update message
pub fn generate_orderbook_message(sequence: u64, bid_price: f64, ask_price: f64) -> String {
    format!(
        r#"{{"type":"book","sequence":{},"bids":[[{},1.0]],"asks":[[{},1.0]],"timestamp":{}}}"#,
        sequence,
        bid_price,
        ask_price,
        chrono::Utc::now().timestamp_micros()
    )
}

/// Generate burst of valid messages
pub fn generate_message_burst(count: usize, start_sequence: u64) -> Vec<String> {
    (0..count)
        .map(|i| {
            let seq = start_sequence + i as u64;
            let bid = 100.0 + (i as f64 * 0.01);
            let ask = 101.0 + (i as f64 * 0.01);
            generate_orderbook_message(seq, bid, ask)
        })
        .collect()
}

/// Generate malformed JSON messages
pub fn generate_malformed_messages(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| match i % 5 {
            0 => "not json at all {{{".to_string(),
            1 => r#"{"incomplete": "#.to_string(),
            2 => r#"{"price": "NaN", "quantity": 1}"#.to_string(),
            3 => r#"{"price": -100, "quantity": -5}"#.to_string(),
            _ => r#"{"missing_required_fields": true}"#.to_string(),
        })
        .collect()
}

/// Generate messages with sequence gaps
pub fn generate_messages_with_gaps(count: usize, gap_probability: f64) -> Vec<(u64, String)> {
    let mut sequence = 0u64;
    let mut messages = Vec::with_capacity(count);

    for _ in 0..count {
        // Randomly skip sequences to create gaps
        if rand::random::<f64>() < gap_probability {
            sequence += rand::random::<u64>() % 10 + 2; // Skip 2-11 sequences
        } else {
            sequence += 1;
        }

        let bid = 100.0 + (sequence as f64 * 0.01);
        let ask = 101.0 + (sequence as f64 * 0.01);
        messages.push((sequence, generate_orderbook_message(sequence, bid, ask)));
    }

    messages
}

// ============================================================================
// TEST FIXTURES
// ============================================================================

/// Create a test instrument
pub fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-USD")
}

/// Create a test order book with initial data
pub fn test_orderbook_with_data(levels: usize) -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let now = chrono::Utc::now().timestamp_micros();

    let bids: Vec<PriceLevel> = (0..levels)
        .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
        .collect();

    let asks: Vec<PriceLevel> = (0..levels)
        .map(|i| PriceLevel::new(101.0 + i as f64, Decimal::new(10, 0), now))
        .collect();

    book.apply_snapshot(bids, asks, now);
    book
}

/// Create a thread-safe order book for concurrent tests
pub fn thread_safe_orderbook(levels: usize) -> ThreadSafeOrderBook {
    ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default())
}

// ============================================================================
// RECOVERY METRICS
// ============================================================================

/// Metrics for tracking recovery behavior
#[derive(Debug, Default)]
pub struct RecoveryMetrics {
    /// Time from failure to recovery
    pub recovery_time: Option<Duration>,
    /// Messages processed during chaos
    pub messages_processed: AtomicU64,
    /// Messages lost during chaos
    pub messages_lost: AtomicU64,
    /// Recovery attempts made
    pub recovery_attempts: AtomicU64,
    /// Start time for tracking
    failure_start: Option<Instant>,
}

impl RecoveryMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_failure_start(&mut self) {
        self.failure_start = Some(Instant::now());
    }

    pub fn record_recovery(&mut self) {
        if let Some(start) = self.failure_start {
            self.recovery_time = Some(start.elapsed());
        }
        self.recovery_attempts.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_message_processed(&self) {
        self.messages_processed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_message_lost(&self) {
        self.messages_lost.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_processed(&self) -> u64 {
        self.messages_processed.load(Ordering::SeqCst)
    }

    pub fn get_lost(&self) -> u64 {
        self.messages_lost.load(Ordering::SeqCst)
    }

    pub fn get_recovery_time_ms(&self) -> Option<u64> {
        self.recovery_time.map(|d| d.as_millis() as u64)
    }
}

// ============================================================================
// CHAOS SIMULATION HELPERS
// ============================================================================

/// Simulate network delay
pub async fn simulate_network_delay(min_ms: u64, max_ms: u64) {
    let delay = if max_ms > min_ms {
        min_ms + rand::random::<u64>() % (max_ms - min_ms)
    } else {
        min_ms
    };
    tokio::time::sleep(Duration::from_millis(delay)).await;
}

/// Simulate intermittent failure
pub fn should_fail(failure_rate: f64) -> bool {
    rand::random::<f64>() < failure_rate
}

/// Simulate memory pressure by allocating buffers
pub fn allocate_memory_pressure(size_mb: usize) -> Vec<u8> {
    vec![0u8; size_mb * 1024 * 1024]
}

// ============================================================================
// VALIDATION HELPERS
// ============================================================================

/// Validate order book state consistency
pub fn validate_orderbook_consistency(book: &OrderBook) -> bool {
    // Check best bid < best ask (no crossed book)
    if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
        if bid.price >= ask.price {
            return false;
        }
    }

    // Check all bid prices are positive
    for bid in book.top_bids(100) {
        if bid.price <= 0.0 || bid.price.is_nan() || bid.price.is_infinite() {
            return false;
        }
    }

    // Check all ask prices are positive
    for ask in book.top_asks(100) {
        if ask.price <= 0.0 || ask.price.is_nan() || ask.price.is_infinite() {
            return false;
        }
    }

    true
}

/// Validate recovery completed within time limit
pub fn validate_recovery_time(metrics: &RecoveryMetrics, max_ms: u64) -> bool {
    metrics
        .get_recovery_time_ms()
        .map(|t| t <= max_ms)
        .unwrap_or(false)
}

/// Validate no message loss
pub fn validate_no_message_loss(metrics: &RecoveryMetrics) -> bool {
    metrics.get_lost() == 0
}

// ============================================================================
// STRESS TEST UTILITIES
// ============================================================================

/// Run a stress test with configurable parameters
pub async fn run_stress_test<F, Fut>(
    iterations: usize,
    concurrency: usize,
    operation: F,
) -> StressTestResults
where
    F: Fn(usize) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Duration, String>> + Send,
{
    let operation = Arc::new(operation);
    let _handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let start = Instant::now();
    let success_count = Arc::new(AtomicU64::new(0));
    let failure_count = Arc::new(AtomicU64::new(0));
    let total_latency = Arc::new(AtomicU64::new(0));

    for batch in 0..(iterations / concurrency) {
        let mut batch_handles = Vec::new();

        for i in 0..concurrency {
            let op = Arc::clone(&operation);
            let success = Arc::clone(&success_count);
            let failure = Arc::clone(&failure_count);
            let latency = Arc::clone(&total_latency);
            let iteration = batch * concurrency + i;

            batch_handles.push(tokio::spawn(async move {
                match op(iteration).await {
                    Ok(duration) => {
                        success.fetch_add(1, Ordering::SeqCst);
                        latency.fetch_add(duration.as_micros() as u64, Ordering::SeqCst);
                    },
                    Err(_) => {
                        failure.fetch_add(1, Ordering::SeqCst);
                    },
                }
            }));
        }

        for handle in batch_handles {
            let _ = handle.await;
        }
    }

    let total_time = start.elapsed();
    let successes = success_count.load(Ordering::SeqCst);
    let failures = failure_count.load(Ordering::SeqCst);
    let total_lat = total_latency.load(Ordering::SeqCst);

    StressTestResults {
        total_iterations: iterations as u64,
        successful: successes,
        failed: failures,
        total_duration: total_time,
        avg_latency_us: if successes > 0 {
            total_lat / successes
        } else {
            0
        },
        throughput: successes as f64 / total_time.as_secs_f64(),
    }
}

/// Results from a stress test
#[derive(Debug)]
pub struct StressTestResults {
    pub total_iterations: u64,
    pub successful: u64,
    pub failed: u64,
    pub total_duration: Duration,
    pub avg_latency_us: u64,
    pub throughput: f64,
}

impl StressTestResults {
    pub fn success_rate(&self) -> f64 {
        if self.total_iterations > 0 {
            self.successful as f64 / self.total_iterations as f64
        } else {
            0.0
        }
    }
}
