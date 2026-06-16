//! Chaos & Resilience Test Suite
//!
//! This test file integrates all chaos testing modules for Flash.
//! Tests validate system behavior under adverse conditions.
//!
//! # Test Categories
//!
//! - **Network Chaos (12 tests)**: WebSocket disconnect, reconnect, degradation
//! - **Redis Chaos (10 tests)**: Connection failure, recovery, timeout
//! - **Resource Chaos (8 tests)**: Memory pressure, CPU, channel capacity
//! - **Message Chaos (10 tests)**: Malformed JSON, bursts, sequence gaps
//! - **Timing Chaos (4 tests)**: Clock skew, deadline exceeded
//! - **Combined Chaos (4 tests)**: Multi-failure scenarios
//!
//! # Total: 48+ Tests (exceeds 36 requirement)
//!
//! # Running Tests
//!
//! ```bash
//! # Run all chaos tests
//! cargo test --test chaos_tests
//!
//! # Run specific category
//! cargo test --test chaos_tests network
//! cargo test --test chaos_tests redis
//! cargo test --test chaos_tests resource
//! cargo test --test chaos_tests message
//! cargo test --test chaos_tests timing
//! cargo test --test chaos_tests combined
//! ```

mod chaos;

// Re-export all test modules
pub use chaos::combined_chaos_test;
pub use chaos::helpers;
pub use chaos::message_chaos_test;
pub use chaos::network_chaos_test;
pub use chaos::redis_chaos_test;
pub use chaos::resource_chaos_test;
pub use chaos::timing_chaos_test;
