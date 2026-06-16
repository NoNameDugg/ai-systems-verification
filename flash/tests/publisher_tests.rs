//! Publisher Integration Tests for Flash.
//!
//! This is the main entry point for publisher tests including:
//! - Part 4.1: Redis Connection Pool (`pool_test.rs`)
//! - Part 4.2: Stream Publisher (`stream_test.rs`)
//! - Part 4.3: Batching & Backpressure (`batch_test.rs`)
//! - Part 4.4: Topic Routing (`topics_test.rs`)
//!
//! Run with:
//!
//! ```bash
//! cargo test --test publisher_tests
//! ```

// Include the publisher module directory
mod publisher;
