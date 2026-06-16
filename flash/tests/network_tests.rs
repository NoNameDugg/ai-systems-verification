//! Network Integration Tests for Flash.
//!
//! This is the main entry point for network tests including:
//! - Part 2.1: Connection Manager (`connector_test.rs`)
//! - Part 2.2: Heartbeat & Health (`heartbeat_test.rs`)
//! - Part 2.3: Reconnection Logic (`reconnect_test.rs`)
//! - Part 2.4: Exchange Adapters (`adapters_test.rs`)
//!
//! Run with:
//!
//! ```bash
//! cargo test --test network_tests
//! ```

// Include the network module directory
mod network;
