//! End-to-End Pipeline Tests for Flash.
//!
//! This module contains comprehensive E2E tests validating the complete data flow:
//!
//! ```text
//! [Exchange WebSocket] -> [Exchange Adapter] -> [OrderBook] -> [Redis Publisher]
//! ```
//!
//! # Test Categories
//!
//! - **Pipeline Tests**: Full data flow validation (12 tests)
//! - **Multi-Exchange Tests**: Concurrent exchange handling (8 tests)
//! - **Failure Tests**: Error handling and recovery (10 tests)
//! - **Performance Tests**: Latency and throughput validation (6 tests)
//!
//! # Running E2E Tests
//!
//! ```bash
//! # Run all E2E tests
//! cargo test --test e2e
//!
//! # Run with real Redis (set environment variable)
//! E2E_REDIS_URL=redis://localhost:6379 cargo test --test e2e
//!
//! # Run performance tests only
//! cargo test --test e2e performance
//! ```

pub mod common;
pub mod failure_test;
pub mod multi_exchange_test;
pub mod performance_test;
pub mod pipeline_test;
