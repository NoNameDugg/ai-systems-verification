//! Common utilities for E2E testing.
//!
//! This module provides:
//! - Mock WebSocket server for simulating exchanges
//! - Test fixtures for generating test data
//! - Test consumer for verifying Redis output
//! - Test environment for isolated test execution

pub mod fixtures;
pub mod mock_server;
pub mod test_consumer;

pub use fixtures::*;
pub use mock_server::*;
pub use test_consumer::*;

use astra_flash::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// =============================================================================
// TEST ENVIRONMENT
// =============================================================================

/// Isolated test environment with unique resources.
///
/// Each E2E test should create its own TestEnvironment to ensure isolation.
/// Resources are cleaned up when the environment is dropped.
///
/// # Example
///
/// ```rust,ignore
/// #[tokio::test]
/// async fn test_something() {
///     let env = TestEnvironment::new().await;
///     // Use env.stream_prefix() for unique stream names
///     // env automatically cleans up on drop
/// }
/// ```
pub struct TestEnvironment {
    /// Unique test ID
    id: Uuid,
    /// Stream prefix for this test
    stream_prefix: String,
    /// Test start time
    start_time: std::time::Instant,
    /// Message counter for unique IDs
    message_counter: AtomicU64,
}

impl TestEnvironment {
    /// Create a new isolated test environment.
    pub async fn new() -> Self {
        let id = Uuid::new_v4();
        let stream_prefix = format!("test_{}", id.to_string().replace('-', "_"));

        Self {
            id,
            stream_prefix,
            start_time: std::time::Instant::now(),
            message_counter: AtomicU64::new(0),
        }
    }

    /// Get the unique test ID.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the stream prefix for this test.
    pub fn stream_prefix(&self) -> &str {
        &self.stream_prefix
    }

    /// Get elapsed time since test start.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Generate a unique message ID for this test.
    pub fn next_message_id(&self) -> u64 {
        self.message_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Build a stream name for this test.
    pub fn stream_name(
        &self,
        exchange: Exchange,
        base: &str,
        quote: &str,
        data_type: &str,
    ) -> String {
        format!(
            "{}.market_data.{:?}.{}_{}.{}",
            self.stream_prefix,
            exchange,
            base.to_lowercase(),
            quote.to_lowercase(),
            data_type
        )
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Log test duration
        let duration = self.start_time.elapsed();
        eprintln!("[E2E] Test {} completed in {:?}", self.id, duration);
    }
}

// =============================================================================
// TEST CONFIGURATION
// =============================================================================

/// E2E test configuration loaded from environment.
#[derive(Debug, Clone)]
pub struct E2EConfig {
    /// Redis URL (None = use mock)
    pub redis_url: Option<String>,
    /// Test timeout
    pub test_timeout: Duration,
    /// Performance test message count
    pub perf_message_count: usize,
    /// Performance test duration
    pub perf_duration: Duration,
    /// Chaos disconnect probability
    pub chaos_disconnect_prob: f64,
    /// Chaos malformed probability
    pub chaos_malformed_prob: f64,
}

impl Default for E2EConfig {
    fn default() -> Self {
        Self {
            redis_url: std::env::var("E2E_REDIS_URL").ok(),
            test_timeout: Duration::from_secs(
                std::env::var("E2E_TEST_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30),
            ),
            perf_message_count: std::env::var("E2E_PERF_MESSAGE_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10_000),
            perf_duration: Duration::from_secs(
                std::env::var("E2E_PERF_DURATION_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(10),
            ),
            chaos_disconnect_prob: std::env::var("E2E_CHAOS_DISCONNECT_PROB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.01),
            chaos_malformed_prob: std::env::var("E2E_CHAOS_MALFORMED_PROB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.001),
        }
    }
}

// =============================================================================
// LATENCY STATISTICS
// =============================================================================

/// Latency statistics for E2E measurements.
#[derive(Debug, Clone, Default)]
pub struct LatencyStats {
    /// Minimum latency in microseconds
    pub min_us: i64,
    /// Maximum latency in microseconds
    pub max_us: i64,
    /// Average latency in microseconds
    pub avg_us: f64,
    /// Median (p50) latency in microseconds
    pub p50_us: i64,
    /// 95th percentile latency in microseconds
    pub p95_us: i64,
    /// 99th percentile latency in microseconds
    pub p99_us: i64,
    /// Sample count
    pub count: usize,
}

impl LatencyStats {
    /// Calculate statistics from a vector of latency samples.
    pub fn from_samples(samples: &mut [i64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        samples.sort();
        let count = samples.len();
        let sum: i64 = samples.iter().sum();

        Self {
            min_us: samples[0],
            max_us: samples[count - 1],
            avg_us: sum as f64 / count as f64,
            p50_us: samples[count / 2],
            p95_us: samples[(count as f64 * 0.95) as usize],
            p99_us: samples[(count as f64 * 0.99) as usize],
            count,
        }
    }

    /// Check if p95 latency is within budget.
    pub fn p95_within_budget(&self, budget_us: i64) -> bool {
        self.p95_us <= budget_us
    }

    /// Check if p99 latency is within budget.
    pub fn p99_within_budget(&self, budget_us: i64) -> bool {
        self.p99_us <= budget_us
    }
}

// =============================================================================
// THROUGHPUT STATISTICS
// =============================================================================

/// Throughput statistics for performance tests.
#[derive(Debug, Clone, Default)]
pub struct ThroughputStats {
    /// Total messages processed
    pub total_messages: u64,
    /// Duration in seconds
    pub duration_secs: f64,
    /// Messages per second
    pub messages_per_sec: f64,
    /// Bytes processed
    pub total_bytes: u64,
    /// Megabytes per second
    pub mb_per_sec: f64,
}

impl ThroughputStats {
    /// Calculate throughput from message count and duration.
    pub fn from_count_and_duration(messages: u64, bytes: u64, duration: Duration) -> Self {
        let duration_secs = duration.as_secs_f64();
        let messages_per_sec = if duration_secs > 0.0 {
            messages as f64 / duration_secs
        } else {
            0.0
        };
        let mb_per_sec = if duration_secs > 0.0 {
            (bytes as f64 / 1_048_576.0) / duration_secs
        } else {
            0.0
        };

        Self {
            total_messages: messages,
            duration_secs,
            messages_per_sec,
            total_bytes: bytes,
            mb_per_sec,
        }
    }

    /// Check if throughput meets minimum requirement.
    pub fn meets_minimum(&self, min_msg_per_sec: f64) -> bool {
        self.messages_per_sec >= min_msg_per_sec
    }
}

// =============================================================================
// ASSERTION HELPERS
// =============================================================================

/// Assert that a value is within expected range.
#[macro_export]
macro_rules! assert_in_range {
    ($value:expr, $min:expr, $max:expr) => {
        assert!(
            $value >= $min && $value <= $max,
            "Expected {} to be in range [{}, {}], but was {}",
            stringify!($value),
            $min,
            $max,
            $value
        );
    };
}

/// Assert that latency is within budget.
#[macro_export]
macro_rules! assert_latency_within {
    ($stats:expr, $budget_us:expr) => {
        assert!(
            $stats.p95_within_budget($budget_us),
            "P95 latency {}μs exceeds budget {}μs",
            $stats.p95_us,
            $budget_us
        );
    };
}

/// Assert that throughput meets minimum.
#[macro_export]
macro_rules! assert_throughput_above {
    ($stats:expr, $min_msg_per_sec:expr) => {
        assert!(
            $stats.meets_minimum($min_msg_per_sec),
            "Throughput {:.0} msg/sec below minimum {} msg/sec",
            $stats.messages_per_sec,
            $min_msg_per_sec
        );
    };
}
