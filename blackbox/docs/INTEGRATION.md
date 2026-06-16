# BlackBox Integration Guide

**Version:** 1.0.0
**Last Updated:** 2026-01-06

---

## Overview

This guide explains how to integrate BlackBox with the host trading engine and other components.

---

## Table of Contents

1. [Architecture](#architecture)
2. [Dependency Setup](#dependency-setup)
3. [TAP Point Integration](#tap-point-integration)
4. [Feature Flag Configuration](#feature-flag-configuration)
5. [Type Conversions](#type-conversions)
6. [Testing Integration](#testing-integration)

---

## Architecture

### Dependency Chain

```
blackbox-types  <──  blackbox  <──  trading-engine
     │                  │                   │
   (Core)           (Journal)          (Trading)
   Types              Replay             System
```

The `blackbox-types` crate provides shared types to break circular dependencies between BlackBox and the host engine.

### TAP Point Locations

```
┌─────────────────────────────────────────────────────────────┐
│                     Host Trading Engine                       │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐    │
│  │ Connector   │────▶│ OrderBook   │────▶│ OrderMgr    │    │
│  │ + TAP #1    │     │ + TAP #2    │     │ + TAP #3    │    │
│  │ (Ingress)   │     │ (Internal)  │     │ (Egress)    │    │
│  └──────┬──────┘     └──────┬──────┘     └──────┬──────┘    │
│         │                   │                   │            │
│         ▼                   ▼                   ▼            │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                    JournalWriter                       │  │
│  │                   (blackbox crate)                    │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## Dependency Setup

### 1. Workspace Configuration

In your workspace `Cargo.toml`:

```toml
[workspace]
members = ["crates/*"]

[workspace.dependencies]
# Shared types crate
blackbox-types = { path = "crates/blackbox-types" }

# BlackBox crate
blackbox = { path = "crates/blackbox" }
```

### 2. Host Engine Cargo.toml

```toml
[package]
name = "trading-engine"
version = "1.0.0"

[dependencies]
# Core dependencies
tokio = { version = "1", features = ["full"] }

# Shared types (always needed)
blackbox-types = { workspace = true }

# Optional BlackBox integration
blackbox = { workspace = true, optional = true }

[features]
default = []
blackbox = ["dep:blackbox"]
```

### 3. Verify Dependencies

```bash
# Check default build (no blackbox)
cargo check --package trading-engine

# Check with blackbox
cargo check --package trading-engine --features blackbox

# Ensure no circular dependencies
cargo tree --package trading-engine
```

---

## TAP Point Integration

### TAP-1: Ingress (connector.rs)

Record raw WebSocket frames at the entry point:

```rust
// src/network/connector.rs

use blackbox_types::{Exchange, Timestamp};

#[cfg(feature = "blackbox")]
use blackbox::tap::Tap;

pub struct Connector<T: Tap = NullTap> {
    config: ConnectorConfig,
    metrics: Arc<Metrics>,
    #[cfg(feature = "blackbox")]
    tap: T,
}

impl<T: Tap> Connector<T> {
    /// Create connector with blackbox tap.
    #[cfg(feature = "blackbox")]
    pub fn with_tap(config: ConnectorConfig, metrics: Arc<Metrics>, tap: T) -> Self {
        Self { config, metrics, tap }
    }

    async fn handle_message(&self, frame: &[u8], exchange: Exchange) {
        let timestamp = Timestamp::from_micros(chrono::Utc::now().timestamp_micros());

        // TAP-1: Record raw ingress BEFORE processing
        #[cfg(feature = "blackbox")]
        self.tap.record_ingress(exchange, frame, timestamp);

        // Normal processing continues...
        let message = parse_message(frame)?;
        self.process(message).await;
    }
}
```

### TAP-2: Internal (orderbook.rs)

Record state changes in the order book:

```rust
// src/market/orderbook.rs

use blackbox::tap::Tap;

pub struct OrderBook<T: Tap = NullTap> {
    bids: BTreeMap<Price, Level>,
    asks: BTreeMap<Price, Level>,
    #[cfg(feature = "blackbox")]
    tap: T,
}

impl<T: Tap> OrderBook<T> {
    pub fn apply_snapshot(&mut self, snapshot: BookSnapshot) {
        // Apply the snapshot
        self.bids = snapshot.bids;
        self.asks = snapshot.asks;

        // TAP-2: Record internal state change
        #[cfg(feature = "blackbox")]
        {
            let payload = bincode::serialize(&snapshot).unwrap();
            self.tap.record_internal(
                0x0010, // BOOK_SNAPSHOT event type
                &payload,
                snapshot.timestamp,
            );
        }
    }

    pub fn apply_delta(&mut self, delta: BookDelta) {
        // Apply delta updates...

        // TAP-2: Record delta
        #[cfg(feature = "blackbox")]
        {
            let payload = bincode::serialize(&delta).unwrap();
            self.tap.record_internal(
                0x0011, // BOOK_DELTA event type
                &payload,
                delta.timestamp,
            );
        }
    }
}
```

### TAP-3: Egress (order/manager.rs)

Record outbound order submissions:

```rust
// src/order/manager.rs

use blackbox::tap::Tap;

pub struct OrderManager<T: Tap = NullTap> {
    pending_orders: HashMap<OrderId, Order>,
    #[cfg(feature = "blackbox")]
    tap: T,
}

impl<T: Tap> OrderManager<T> {
    pub fn submit_order(&mut self, request: OrderRequest) -> OrderId {
        let order_id = self.generate_id();
        let timestamp = self.clock.now();

        // TAP-3: Record egress BEFORE sending to exchange
        #[cfg(feature = "blackbox")]
        {
            let payload = serde_json::to_vec(&request).unwrap();
            self.tap.record_egress(
                request.exchange,
                &payload,
                timestamp,
            );
        }

        // Send to exchange...
        self.exchange_client.submit(request).await?;

        order_id
    }
}
```

---

## Feature Flag Configuration

### Module-Level Gating

Create a `blackbox.rs` module for integration utilities:

```rust
// src/blackbox.rs

//! BlackBox integration module.
//!
//! Only compiled when `blackbox` feature is enabled.

pub use blackbox::tap::{JournalTap, NullTap, Tap};
pub use blackbox_types::{Exchange as BlackBoxExchange, Timestamp as BlackBoxTimestamp};

/// Event types for internal tap recording.
pub mod event_types {
    pub const BOOK_SNAPSHOT: u16 = 0x0010;
    pub const BOOK_DELTA: u16 = 0x0011;
    pub const STATE_CHANGE: u16 = 0x0030;
}

/// Convert the host engine's Exchange to BlackBox Exchange.
#[inline]
pub fn to_blackbox_exchange(exchange: crate::Exchange) -> BlackBoxExchange {
    match exchange {
        crate::Exchange::Deribit => BlackBoxExchange::Deribit,
        crate::Exchange::Binance => BlackBoxExchange::Binance,
        crate::Exchange::Oanda => BlackBoxExchange::Unknown,
    }
}

/// Convert the host engine's Timestamp to BlackBox Timestamp.
#[inline]
pub fn to_blackbox_timestamp(timestamp: i64) -> BlackBoxTimestamp {
    BlackBoxTimestamp::from_micros(timestamp)
}
```

### lib.rs Exports

```rust
// src/lib.rs

#[cfg(feature = "blackbox")]
pub mod blackbox;

// Re-export for convenience
#[cfg(feature = "blackbox")]
pub use blackbox::{JournalTap, NullTap, Tap};
```

### Conditional Compilation Pattern

```rust
// Pattern 1: Compile-time feature flag
#[cfg(feature = "blackbox")]
fn do_recording(tap: &impl Tap, data: &[u8]) {
    tap.record_ingress(Exchange::Binance, data, timestamp);
}

#[cfg(not(feature = "blackbox"))]
fn do_recording(_tap: &NullTap, _data: &[u8]) {
    // No-op when blackbox disabled
}

// Pattern 2: Generic with default NullTap
pub struct Engine<T: Tap = NullTap> {
    tap: T,
}

// Pattern 3: TapExt trait for ergonomic host-engine types
use crate::blackbox::TapExt;

tap.record_engine_ingress(engine_exchange, data, engine_timestamp);
```

---

## Type Conversions

### Exchange Conversion

| Engine Exchange | BlackBox Exchange |
|----------------|-------------------|
| `Deribit` | `Deribit` |
| `Binance` | `Binance` |
| `Oanda` | `Unknown` |

```rust
impl From<EngineExchange> for BlackBoxExchange {
    fn from(e: EngineExchange) -> Self {
        match e {
            EngineExchange::Deribit => BlackBoxExchange::Deribit,
            EngineExchange::Binance => BlackBoxExchange::Binance,
            EngineExchange::Oanda => BlackBoxExchange::Unknown,
        }
    }
}
```

### Timestamp Conversion

Both systems use microseconds since Unix epoch:

```rust
// The host engine uses i64 directly
let engine_ts: i64 = 1704067200_000_000;

// BlackBox uses Timestamp wrapper
let bb_ts = Timestamp::from_micros(engine_ts);

// Convert back
let engine_ts_back = bb_ts.as_micros();
```

---

## Testing Integration

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use blackbox::tap::NullTap;

    #[test]
    fn test_orderbook_with_null_tap() {
        let book = OrderBook::with_tap(config, NullTap);

        // NullTap is zero-overhead
        book.apply_snapshot(snapshot);

        assert!(book.best_bid().is_some());
    }
}
```

### Integration Tests

```rust
#[cfg(all(test, feature = "blackbox"))]
mod integration_tests {
    use blackbox::journal::{JournalWriter, WriterConfig};
    use blackbox::tap::JournalTap;
    use tempfile::tempdir;

    #[test]
    fn test_full_recording_workflow() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.journal");

        // Create journal
        let config = WriterConfig::default();
        let writer = JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        // Create components with tap
        let connector = Connector::with_tap(config, metrics, tap.clone());
        let orderbook = OrderBook::with_tap(config, tap.clone());
        let order_mgr = OrderManager::with_tap(config, tap);

        // Simulate trading...

        // Verify journal was written
        drop(tap);
        let reader = JournalReader::open(&path).unwrap();
        assert!(reader.record_count() > 0);
    }
}
```

### Benchmark Tests

```rust
#[cfg(all(test, feature = "blackbox"))]
mod benches {
    use criterion::{black_box, Criterion};

    pub fn bench_tap_overhead(c: &mut Criterion) {
        let tap = NullTap;

        c.bench_function("null_tap_overhead", |b| {
            b.iter(|| {
                tap.record_ingress(
                    black_box(Exchange::Binance),
                    black_box(b"data"),
                    black_box(Timestamp::from_micros(0)),
                );
            });
        });
    }
}
```

---

## Production Deployment

### 1. Build Configuration

```bash
# Development (with recording)
cargo build --features blackbox

# Production (no recording overhead)
cargo build --release

# Production (with recording capability)
cargo build --release --features blackbox
```

### 2. Runtime Configuration

```rust
fn main() {
    let recording_enabled = std::env::var("BLACKBOX_ENABLED")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let tap: Box<dyn Tap> = if recording_enabled {
        let writer = JournalWriter::new(journal_path, config)?;
        Box::new(JournalTap::new(writer))
    } else {
        Box::new(NullTap)
    };

    let engine = TradingEngine::with_tap(config, tap);
    engine.run().await;
}
```

### 3. Monitoring

```bash
# Check journal file growth
watch -n 1 'ls -lh /var/blackbox/journals/*.journal'

# Monitor with CLI
blackbox stats /var/blackbox/journals/current.journal
```

---

## Troubleshooting

### Common Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| Compile error with `blackbox` feature | Missing dependency | Add `blackbox = { workspace = true, optional = true }` |
| Type mismatch errors | Different Exchange enums | Use conversion functions in `blackbox.rs` |
| Zero records in journal | Tap not connected | Verify tap is passed to components |
| Large journal files | Too much data | Filter what's recorded, increase rotation |

### Verification Commands

```bash
# Test feature flag
cargo check --package trading-engine
cargo check --package trading-engine --features blackbox

# Run all tests
cargo test --workspace
cargo test --workspace --features blackbox

# Verify no circular deps
cargo tree --package trading-engine --no-dedupe | head -50
```

---

## See Also

- [USAGE.md](USAGE.md) - User guide
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) - Common issues
- [PERFORMANCE.md](PERFORMANCE.md) - Performance specs
- [schemas/blackbox-v1.0.xml](../schemas/blackbox-v1.0.xml) - Binary format / SBE schema

---

*Generated for BlackBox v1.0.0*
