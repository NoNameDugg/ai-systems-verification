//! Chaos and Resilience Testing Module
//!
//! This module provides comprehensive chaos engineering tests for Flash.
//! Tests validate system behavior under adverse conditions including:
//!
//! - Network failures and degradation
//! - Redis unavailability and timeouts
//! - Resource pressure (memory, CPU)
//! - Message chaos (malformed, bursts, sequences)
//! - Timing chaos (clock skew, timeouts)
//! - Combined failure scenarios
//!
//! # Test Categories
//!
//! | Category | Count | Purpose |
//! |----------|-------|---------|
//! | Network | 12 | WebSocket disconnect, reconnect, degradation |
//! | Redis | 10 | Connection failure, recovery, timeout |
//! | Resource | 8 | Memory pressure, CPU, channel capacity |
//! | Message | 10 | Malformed JSON, bursts, sequence gaps |
//! | Timing | 4 | Clock skew, deadline exceeded |
//! | Combined | 4 | Multi-failure scenarios |
//!
//! # Running Chaos Tests
//!
//! ```bash
//! # Run all chaos tests
//! cargo test --test chaos_tests
//!
//! # Run specific category
//! cargo test --test chaos_tests network
//! cargo test --test chaos_tests redis
//! ```

pub mod combined_chaos_test;
pub mod helpers;
pub mod message_chaos_test;
pub mod network_chaos_test;
pub mod redis_chaos_test;
pub mod resource_chaos_test;
pub mod timing_chaos_test;
