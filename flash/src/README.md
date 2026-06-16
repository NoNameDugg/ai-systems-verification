# Source Code

Rust implementation of Flash - ultra-low latency market data adapter.

## Module Structure

| Module | Purpose |
|--------|---------|
| `bindings/` | PyO3 Python bindings |
| `book/` | Order book engine with BTreeMap + cached best prices |
| `core/` | Configuration, error handling, metrics, types |
| `fusion/` | AlphaSignal generation for a downstream decision-aggregation engine |
| `gateway/` | Gateway output formatting |
| `network/` | WebSocket connectors and exchange adapters |
| `parsing/` | SIMD-accelerated JSON parsing |
| `publisher/` | Redis Streams publishing with backpressure |
| `order/` | Order representation |
