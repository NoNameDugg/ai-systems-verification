//! Publisher module - Redis stream publishing for Flash.
//!
//! This module handles high-throughput message publishing to Redis:
//!
//! - [`pool`] - Redis connection pool management
//! - [`stream`] - Stream operations (XADD) with multi-format serialization
//! - [`batch`] - Message batching for throughput optimization (Part 4.3)
//! - [`topics`] - Topic naming and routing (Part 4.4)
//! - [`dual`] - Dual output publishing (Batch 3.2)
//! - [`backpressure`] - Non-blocking sends with DROP policy (Batch 3.3)
//!
//! # Phase 4 Implementation
//!
//! This module is implemented in Phase 4 of the TDI roadmap:
//!
//! - Part 4.1: Redis Connection Pool (COMPLETE)
//! - Part 4.2: Stream Publisher (XADD) (COMPLETE)
//! - Part 4.3: Batching & Backpressure (COMPLETE)
//! - Part 4.4: Topic Routing (COMPLETE)
//!
//! # Batch 3.2: Dual Output
//!
//! The dual publisher writes to both streams:
//!
//! ```text
//! market:orderbook:{symbol}                    → Gateway UI (OrderBookSnapshot)
//! astra:signals:flash:{exchange}:{symbol}     → Fusion engine (AlphaSignal)
//! ```
//!
//! # Topic Naming Convention
//!
//! ```text
//! market_data.{exchange}.{base}_{quote}.book    → Full order book
//! market_data.{exchange}.{base}_{quote}.trade   → Trades
//! market_data.{exchange}.{base}_{quote}.ticker  → Ticker updates
//! system.flash.health                           → Health heartbeat
//! ```
//!
//! # Serialization Formats (RED TEAM)
//!
//! | Format | Use Case | Performance |
//! |--------|----------|-------------|
//! | JSON | Debug, external APIs | ~12 μs |
//! | Bincode | Production internal | ~0.4 μs |
//! | Rkyv | Zero-copy Python | ~0.3 μs |
//!
//! # Performance Targets
//!
//! | Metric | Target |
//! |--------|--------|
//! | Connection acquire | < 1 ms |
//! | Single publish | < 100 μs |
//! | Batch publish (100) | < 1 ms |
//! | Throughput | > 50K msg/s |
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::{RedisPool, StreamPublisher, SerializationFormat};
//! use std::sync::Arc;
//!
//! // Create connection pool
//! let pool = Arc::new(RedisPool::builder("redis://127.0.0.1:6379")
//!     .max_size(20)
//!     .build()
//!     .await?);
//!
//! // Create stream publisher
//! let publisher = StreamPublisher::builder(Arc::clone(&pool))
//!     .format(SerializationFormat::Bincode)
//!     .build(pool);
//!
//! // Publish book snapshot
//! let result = publisher.publish_book(&snapshot).await?;
//! println!("Published: {} in {}μs", result.message_id, result.latency_us);
//! ```

// Part 4.1: Redis Connection Pool
pub mod pool;

// Part 4.2: Stream Publisher (XADD)
pub mod stream;

// Part 4.3: Batching & Backpressure
pub mod batch;

// Part 4.4: Topic Routing
pub mod topics;

// Batch 3.2: Dual Output Publisher
pub mod dual;

// Batch 3.3: Backpressure Handling
pub mod backpressure;

// Batch 4.2: Zero-Copy Tuning - Serialization Buffer
pub mod buffer;

// Re-exports for convenience - Pool types
pub use pool::{
    PoolConfig, PoolError, PoolEvent, PoolHealth, PoolHealthStatus, PoolResult, PoolStats,
    PooledConnection, RedisPool, RedisPoolBuilder, RedisServerInfo,
};

// Re-exports for convenience - Stream types
pub use stream::{
    PublishResult, SerializationFormat, StreamError, StreamPublisher, StreamPublisherBuilder,
    StreamPublisherConfig, StreamResult, StreamStats, TopicBuilder, TopicType,
};

// Re-exports for convenience - Batch types
pub use batch::{
    BackpressureMonitor, BackpressureStatus, BatchAccumulator, BatchConfig, BatchConfigBuilder,
    BatchMessage, BatchPublishResult, BatcherError, BatcherEvent, BatcherResult, BatcherStats,
    DropReason, FlushTrigger, MessageBatch, OverflowAction,
};

// Re-exports for convenience - Topic routing types
pub use topics::{
    PatternSegment, RouterConfig, RouterConfigBuilder, RoutingAction, RoutingError, RoutingFilter,
    RoutingFilterBuilder, RoutingResult, RoutingRule, RoutingRuleBuilder, RoutingStats, RuleId,
    TemplateSegment, TopicPattern, TopicRouter, TopicRouterBuilder, TopicTemplate,
};

// Re-exports for convenience - Dual publisher types
pub use dual::{
    DualPublishError, DualPublishResult, DualPublisher, DualPublisherBuilder, DualPublisherConfig,
    DualStats, DEFAULT_ORDERBOOK_PREFIX,
};

// Re-exports for convenience - Backpressure types (Batch 3.3)
pub use backpressure::{
    send_with_backpressure, BackpressureConfig, BackpressureConfigBuilder, BackpressureMetrics,
    BackpressurePolicy, BackpressureResult, BackpressureSendError, BackpressureSender,
};

// Re-exports for convenience - Serialization buffer types (Batch 4.2)
pub use buffer::{
    size_hints, thread_buffer_stats, with_bincode_buffer, with_json_buffer, BufferStats,
    SerializationBuffer, DEFAULT_BUFFER_CAPACITY, MAX_BUFFER_CAPACITY,
};
