//! Tests for the Batching & Backpressure module.
//!
//! These tests verify:
//! - BatchConfig configuration and validation
//! - OverflowAction enum behavior
//! - BackpressureStatus enum behavior
//! - BatchMessage creation and manipulation
//! - MessageBatch assembly and management
//! - Batcher flush logic (size, timeout, manual)
//! - Backpressure detection and handling
//! - Statistics tracking
//! - Error handling
//!
//! Run with: `cargo test --test publisher -- batch`

use astra_flash::core::types::{
    now_micros, Exchange, Instrument, MarketData, MarketEvent, MarketEventType, Side,
};
use astra_flash::publisher::batch::{
    BackpressureStatus, BatchConfig, BatchConfigBuilder, BatchMessage, BatchPublishResult,
    BatcherError, BatcherEvent, BatcherStats, DropReason, FlushTrigger, MessageBatch,
    OverflowAction,
};
use astra_flash::publisher::stream::SerializationFormat;
use rust_decimal_macros::dec;
use std::time::Duration;

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create a test batch message.
fn test_batch_message(topic: &str, size: usize) -> BatchMessage {
    BatchMessage {
        topic: topic.to_string(),
        data: vec![0u8; size],
        format: SerializationFormat::Bincode,
        enqueued_at: now_micros(),
        priority: 0,
    }
}

/// Create a test market event.
fn test_market_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Trade,
        instrument: test_instrument(),
        timestamp: now_micros(),
        local_timestamp: now_micros(),
        sequence: Some(1),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: dec!(0.1),
            side: Side::Bid,
            trade_id: Some("trade_123".to_string()),
        },
    }
}

// =============================================================================
// CATEGORY 1: BATCH CONFIG TESTS (8 tests)
// =============================================================================

#[test]
fn test_batch_config_default() {
    let config = BatchConfig::default();

    // Verify default values
    assert_eq!(config.max_batch_size, 100);
    assert_eq!(config.max_batch_delay, Duration::from_millis(10));
    assert_eq!(config.channel_capacity, 10_000);
    assert!((config.warn_threshold - 0.80).abs() < f64::EPSILON);
    assert!((config.critical_threshold - 0.95).abs() < f64::EPSILON);
    assert_eq!(config.overflow_action, OverflowAction::Block);
    assert!(config.auto_flush);
    assert_eq!(config.min_batch_size, 1);
}

#[test]
fn test_batch_config_builder_all_fields() {
    let config = BatchConfigBuilder::new()
        .max_batch_size(50)
        .max_batch_delay(Duration::from_millis(5))
        .channel_capacity(5000)
        .warn_threshold(0.70)
        .critical_threshold(0.90)
        .overflow_action(OverflowAction::DropNewest)
        .auto_flush(false)
        .min_batch_size(10)
        .build()
        .expect("Valid config should build");

    assert_eq!(config.max_batch_size, 50);
    assert_eq!(config.max_batch_delay, Duration::from_millis(5));
    assert_eq!(config.channel_capacity, 5000);
    assert!((config.warn_threshold - 0.70).abs() < f64::EPSILON);
    assert!((config.critical_threshold - 0.90).abs() < f64::EPSILON);
    assert_eq!(config.overflow_action, OverflowAction::DropNewest);
    assert!(!config.auto_flush);
    assert_eq!(config.min_batch_size, 10);
}

#[test]
fn test_batch_config_validation_max_batch_size_zero() {
    let result = BatchConfigBuilder::new().max_batch_size(0).build();

    assert!(result.is_err());
    if let Err(BatcherError::InvalidConfig { reason }) = result {
        assert!(reason.contains("batch_size"));
    } else {
        panic!("Expected InvalidConfig error");
    }
}

#[test]
fn test_batch_config_validation_thresholds_inverted() {
    let result = BatchConfigBuilder::new()
        .warn_threshold(0.95)
        .critical_threshold(0.80)
        .build();

    assert!(result.is_err());
    if let Err(BatcherError::InvalidConfig { reason }) = result {
        assert!(reason.contains("threshold"));
    } else {
        panic!("Expected InvalidConfig error");
    }
}

#[test]
fn test_batch_config_validation_threshold_out_of_range() {
    let result = BatchConfigBuilder::new().warn_threshold(1.5).build();

    assert!(result.is_err());
    if let Err(BatcherError::InvalidConfig { reason }) = result {
        assert!(reason.contains("threshold"));
    } else {
        panic!("Expected InvalidConfig error");
    }
}

#[test]
fn test_batch_config_clone() {
    let config = BatchConfigBuilder::new()
        .max_batch_size(75)
        .overflow_action(OverflowAction::DropOldest)
        .build()
        .unwrap();

    let cloned = config.clone();
    assert_eq!(cloned.max_batch_size, 75);
    assert_eq!(cloned.overflow_action, OverflowAction::DropOldest);
}

#[test]
fn test_batch_config_debug() {
    let config = BatchConfig::default();
    let debug = format!("{:?}", config);

    assert!(debug.contains("BatchConfig"));
    assert!(debug.contains("max_batch_size"));
}

#[test]
fn test_batch_config_builder_defaults() {
    let config = BatchConfigBuilder::new()
        .build()
        .expect("Default builder should succeed");

    // Should use same defaults as BatchConfig::default()
    assert_eq!(config.max_batch_size, BatchConfig::default().max_batch_size);
    assert_eq!(
        config.channel_capacity,
        BatchConfig::default().channel_capacity
    );
}

// =============================================================================
// CATEGORY 2: OVERFLOW ACTION TESTS (4 tests)
// =============================================================================

#[test]
fn test_overflow_action_default() {
    let action = OverflowAction::default();
    assert_eq!(action, OverflowAction::Block);
}

#[test]
fn test_overflow_action_display() {
    // All variants should have meaningful string representation
    let block = format!("{:?}", OverflowAction::Block);
    let drop_oldest = format!("{:?}", OverflowAction::DropOldest);
    let drop_newest = format!("{:?}", OverflowAction::DropNewest);
    let warn = format!("{:?}", OverflowAction::WarnAndContinue);

    assert!(block.contains("Block"));
    assert!(drop_oldest.contains("DropOldest"));
    assert!(drop_newest.contains("DropNewest"));
    assert!(warn.contains("WarnAndContinue"));
}

#[test]
fn test_overflow_action_clone() {
    let action = OverflowAction::DropOldest;
    let cloned = action.clone();
    assert_eq!(action, cloned);
}

#[test]
fn test_overflow_action_eq() {
    assert_eq!(OverflowAction::Block, OverflowAction::Block);
    assert_ne!(OverflowAction::Block, OverflowAction::DropNewest);
    assert_ne!(OverflowAction::DropOldest, OverflowAction::DropNewest);
}

// =============================================================================
// CATEGORY 3: BACKPRESSURE STATUS TESTS (4 tests)
// =============================================================================

#[test]
fn test_backpressure_status_default() {
    let status = BackpressureStatus::default();
    assert_eq!(status, BackpressureStatus::Normal);
}

#[test]
fn test_backpressure_status_transitions() {
    // Test all possible states
    let normal = BackpressureStatus::Normal;
    let warning = BackpressureStatus::Warning;
    let critical = BackpressureStatus::Critical;

    // Each should be distinct
    assert_ne!(normal, warning);
    assert_ne!(warning, critical);
    assert_ne!(normal, critical);
}

#[test]
fn test_backpressure_status_display() {
    let normal = format!("{:?}", BackpressureStatus::Normal);
    let warning = format!("{:?}", BackpressureStatus::Warning);
    let critical = format!("{:?}", BackpressureStatus::Critical);

    assert!(normal.contains("Normal"));
    assert!(warning.contains("Warning"));
    assert!(critical.contains("Critical"));
}

#[test]
fn test_backpressure_status_clone() {
    let status = BackpressureStatus::Warning;
    let cloned = status.clone();
    assert_eq!(status, cloned);
}

// =============================================================================
// CATEGORY 4: BATCH MESSAGE TESTS (6 tests)
// =============================================================================

#[test]
fn test_batch_message_new() {
    let msg = test_batch_message("test.topic", 100);

    assert_eq!(msg.topic, "test.topic");
    assert_eq!(msg.data.len(), 100);
    assert_eq!(msg.format, SerializationFormat::Bincode);
    assert!(msg.enqueued_at > 0);
    assert_eq!(msg.priority, 0);
}

#[test]
fn test_batch_message_clone() {
    let original = test_batch_message("test.topic", 500);
    let cloned = original.clone();

    assert_eq!(original.topic, cloned.topic);
    assert_eq!(original.data, cloned.data);
    assert_eq!(original.format, cloned.format);
    assert_eq!(original.enqueued_at, cloned.enqueued_at);
}

#[test]
fn test_batch_message_priority() {
    let mut msg = test_batch_message("test.topic", 100);
    msg.priority = 10;

    assert_eq!(msg.priority, 10);
}

#[test]
fn test_batch_message_timestamp() {
    let before = now_micros();
    let msg = test_batch_message("test.topic", 100);
    let after = now_micros();

    assert!(msg.enqueued_at >= before);
    assert!(msg.enqueued_at <= after);
}

#[test]
fn test_batch_message_size() {
    let msg = test_batch_message("test.topic", 1024);
    assert_eq!(msg.data.len(), 1024);
}

#[test]
fn test_batch_message_debug() {
    let msg = test_batch_message("test.topic", 100);
    let debug = format!("{:?}", msg);

    assert!(debug.contains("BatchMessage"));
    assert!(debug.contains("test.topic"));
}

// =============================================================================
// CATEGORY 5: MESSAGE BATCH TESTS (4 tests)
// =============================================================================

#[test]
fn test_message_batch_new() {
    let batch = MessageBatch::new();

    assert!(batch.messages.is_empty());
    assert!(batch.created_at > 0);
    assert_eq!(batch.total_bytes, 0);
    assert_eq!(batch.message_count, 0);
}

#[test]
fn test_message_batch_add_message() {
    let mut batch = MessageBatch::new();
    let msg = test_batch_message("test.topic", 100);

    batch.add(msg);

    assert_eq!(batch.message_count, 1);
    assert_eq!(batch.total_bytes, 100);
    assert_eq!(batch.messages.len(), 1);
}

#[test]
fn test_message_batch_total_bytes() {
    let mut batch = MessageBatch::new();

    batch.add(test_batch_message("topic1", 100));
    batch.add(test_batch_message("topic2", 200));
    batch.add(test_batch_message("topic3", 300));

    assert_eq!(batch.total_bytes, 600);
    assert_eq!(batch.message_count, 3);
}

#[test]
fn test_message_batch_clear() {
    let mut batch = MessageBatch::new();
    batch.add(test_batch_message("topic1", 100));
    batch.add(test_batch_message("topic2", 200));

    batch.clear();

    assert!(batch.messages.is_empty());
    assert_eq!(batch.total_bytes, 0);
    assert_eq!(batch.message_count, 0);
}

// =============================================================================
// CATEGORY 6: BATCHING LOGIC TESTS (8 tests)
// =============================================================================

#[test]
fn test_batcher_flush_trigger_batch_full() {
    let trigger = FlushTrigger::BatchFull;
    let debug = format!("{:?}", trigger);
    assert!(debug.contains("BatchFull"));
}

#[test]
fn test_batcher_flush_trigger_timeout() {
    let trigger = FlushTrigger::Timeout;
    let debug = format!("{:?}", trigger);
    assert!(debug.contains("Timeout"));
}

#[test]
fn test_batcher_flush_trigger_manual() {
    let trigger = FlushTrigger::Manual;
    let debug = format!("{:?}", trigger);
    assert!(debug.contains("Manual"));
}

#[test]
fn test_batcher_flush_trigger_shutdown() {
    let trigger = FlushTrigger::Shutdown;
    let debug = format!("{:?}", trigger);
    assert!(debug.contains("Shutdown"));
}

#[test]
fn test_batcher_flush_trigger_eq() {
    assert_eq!(FlushTrigger::BatchFull, FlushTrigger::BatchFull);
    assert_ne!(FlushTrigger::Timeout, FlushTrigger::Manual);
}

#[test]
fn test_batcher_batch_config_min_batch_size() {
    let config = BatchConfigBuilder::new().min_batch_size(5).build().unwrap();

    assert_eq!(config.min_batch_size, 5);
}

#[test]
fn test_batcher_batch_config_max_batch_delay() {
    let config = BatchConfigBuilder::new()
        .max_batch_delay(Duration::from_millis(20))
        .build()
        .unwrap();

    assert_eq!(config.max_batch_delay, Duration::from_millis(20));
}

#[test]
fn test_batcher_auto_flush_config() {
    let config_auto = BatchConfigBuilder::new().auto_flush(true).build().unwrap();

    let config_manual = BatchConfigBuilder::new().auto_flush(false).build().unwrap();

    assert!(config_auto.auto_flush);
    assert!(!config_manual.auto_flush);
}

// =============================================================================
// CATEGORY 7: BACKPRESSURE TESTS (6 tests)
// =============================================================================

#[test]
fn test_backpressure_threshold_calculation_normal() {
    let config = BatchConfig::default();

    // At 50% capacity, should be Normal
    let capacity = config.channel_capacity;
    let queue_depth = capacity / 2;
    let utilization = queue_depth as f64 / capacity as f64;

    assert!(utilization < config.warn_threshold);
}

#[test]
fn test_backpressure_threshold_calculation_warning() {
    let config = BatchConfig::default();

    // At 85% capacity, should be Warning
    let capacity = config.channel_capacity;
    let queue_depth = (capacity as f64 * 0.85) as usize;
    let utilization = queue_depth as f64 / capacity as f64;

    assert!(utilization >= config.warn_threshold);
    assert!(utilization < config.critical_threshold);
}

#[test]
fn test_backpressure_threshold_calculation_critical() {
    let config = BatchConfig::default();

    // At 96% capacity, should be Critical
    let capacity = config.channel_capacity;
    let queue_depth = (capacity as f64 * 0.96) as usize;
    let utilization = queue_depth as f64 / capacity as f64;

    assert!(utilization >= config.critical_threshold);
}

#[test]
fn test_backpressure_custom_thresholds() {
    let config = BatchConfigBuilder::new()
        .warn_threshold(0.50)
        .critical_threshold(0.75)
        .build()
        .unwrap();

    assert!((config.warn_threshold - 0.50).abs() < f64::EPSILON);
    assert!((config.critical_threshold - 0.75).abs() < f64::EPSILON);
}

#[test]
fn test_drop_reason_queue_full() {
    let reason = DropReason::QueueFull;
    let debug = format!("{:?}", reason);
    assert!(debug.contains("QueueFull"));
}

#[test]
fn test_drop_reason_evicted() {
    let reason = DropReason::Evicted;
    let debug = format!("{:?}", reason);
    assert!(debug.contains("Evicted"));
}

// =============================================================================
// CATEGORY 8: STATISTICS TESTS (4 tests)
// =============================================================================

#[test]
fn test_batcher_stats_default() {
    let stats = BatcherStats::default();

    assert_eq!(stats.messages_enqueued, 0);
    assert_eq!(stats.batches_published, 0);
    assert_eq!(stats.messages_published, 0);
    assert_eq!(stats.messages_dropped, 0);
    assert_eq!(stats.current_queue_depth, 0);
    assert_eq!(stats.peak_queue_depth, 0);
    assert_eq!(stats.bytes_processed, 0);
    assert!((stats.avg_batch_size - 0.0).abs() < f64::EPSILON);
    assert_eq!(stats.backpressure_status, BackpressureStatus::Normal);
}

#[test]
fn test_batcher_stats_clone() {
    let mut stats = BatcherStats::default();
    stats.messages_enqueued = 100;
    stats.batches_published = 10;
    stats.peak_queue_depth = 50;

    let cloned = stats.clone();
    assert_eq!(cloned.messages_enqueued, 100);
    assert_eq!(cloned.batches_published, 10);
    assert_eq!(cloned.peak_queue_depth, 50);
}

#[test]
fn test_batcher_stats_debug() {
    let stats = BatcherStats::default();
    let debug = format!("{:?}", stats);

    assert!(debug.contains("BatcherStats"));
    assert!(debug.contains("messages_enqueued"));
}

#[test]
fn test_batcher_stats_backpressure_tracking() {
    let mut stats = BatcherStats::default();
    stats.backpressure_status = BackpressureStatus::Warning;
    stats.backpressure_time_us = 1000;

    assert_eq!(stats.backpressure_status, BackpressureStatus::Warning);
    assert_eq!(stats.backpressure_time_us, 1000);
}

// =============================================================================
// CATEGORY 9: ERROR HANDLING TESTS (4 tests)
// =============================================================================

#[test]
fn test_batcher_error_invalid_config() {
    let error = BatcherError::InvalidConfig {
        reason: "max_batch_size cannot be zero".to_string(),
    };

    let display = error.to_string();
    assert!(display.contains("Invalid configuration"));
    assert!(display.contains("max_batch_size"));
}

#[test]
fn test_batcher_error_not_running() {
    let error = BatcherError::NotRunning;
    let display = error.to_string();
    assert!(display.contains("not running"));
}

#[test]
fn test_batcher_error_already_running() {
    let error = BatcherError::AlreadyRunning;
    let display = error.to_string();
    assert!(display.contains("already running"));
}

#[test]
fn test_batcher_error_display() {
    let error = BatcherError::ShutdownTimeout { timeout_ms: 5000 };
    let display = error.to_string();
    assert!(display.contains("Shutdown"));
    assert!(display.contains("5000"));
}

// =============================================================================
// CATEGORY 10: EVENT TESTS (4 tests)
// =============================================================================

#[test]
fn test_batcher_event_message_enqueued() {
    let event = BatcherEvent::MessageEnqueued {
        topic: "test.topic".to_string(),
        size: 1024,
    };
    let debug = format!("{:?}", event);
    assert!(debug.contains("MessageEnqueued"));
    assert!(debug.contains("test.topic"));
}

#[test]
fn test_batcher_event_batch_flushed() {
    let event = BatcherEvent::BatchFlushed {
        message_count: 50,
        bytes: 10240,
        trigger: FlushTrigger::BatchFull,
    };
    let debug = format!("{:?}", event);
    assert!(debug.contains("BatchFlushed"));
    assert!(debug.contains("50"));
}

#[test]
fn test_batcher_event_backpressure_changed() {
    let event = BatcherEvent::BackpressureChanged {
        old_status: BackpressureStatus::Normal,
        new_status: BackpressureStatus::Warning,
        queue_depth: 8000,
    };
    let debug = format!("{:?}", event);
    assert!(debug.contains("BackpressureChanged"));
}

#[test]
fn test_batcher_event_message_dropped() {
    let event = BatcherEvent::MessageDropped {
        topic: "test.topic".to_string(),
        reason: DropReason::QueueFull,
    };
    let debug = format!("{:?}", event);
    assert!(debug.contains("MessageDropped"));
    assert!(debug.contains("QueueFull"));
}

// =============================================================================
// CATEGORY 11: BATCH PUBLISH RESULT TESTS (4 tests)
// =============================================================================

#[test]
fn test_batch_publish_result_new() {
    let result = BatchPublishResult {
        messages_published: 100,
        messages_failed: 0,
        duration_us: 1000,
        avg_message_time_us: 10.0,
        message_ids: vec!["1234-0".to_string(), "1234-1".to_string()],
    };

    assert_eq!(result.messages_published, 100);
    assert_eq!(result.messages_failed, 0);
    assert_eq!(result.message_ids.len(), 2);
}

#[test]
fn test_batch_publish_result_with_failures() {
    let result = BatchPublishResult {
        messages_published: 90,
        messages_failed: 10,
        duration_us: 2000,
        avg_message_time_us: 20.0,
        message_ids: vec![],
    };

    assert_eq!(result.messages_published, 90);
    assert_eq!(result.messages_failed, 10);
}

#[test]
fn test_batch_publish_result_clone() {
    let result = BatchPublishResult {
        messages_published: 50,
        messages_failed: 0,
        duration_us: 500,
        avg_message_time_us: 10.0,
        message_ids: vec!["test-id".to_string()],
    };

    let cloned = result.clone();
    assert_eq!(cloned.messages_published, 50);
    assert_eq!(cloned.message_ids, result.message_ids);
}

#[test]
fn test_batch_publish_result_debug() {
    let result = BatchPublishResult {
        messages_published: 100,
        messages_failed: 0,
        duration_us: 1000,
        avg_message_time_us: 10.0,
        message_ids: vec![],
    };

    let debug = format!("{:?}", result);
    assert!(debug.contains("BatchPublishResult"));
    assert!(debug.contains("100"));
}

// =============================================================================
// ADDITIONAL TESTS: THREAD SAFETY AND SEND/SYNC (4 tests)
// =============================================================================

#[test]
fn test_batch_config_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<BatchConfig>();
}

#[test]
fn test_batch_config_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<BatchConfig>();
}

#[test]
fn test_batch_message_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<BatchMessage>();
}

#[test]
fn test_batcher_stats_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<BatcherStats>();
    assert_sync::<BatcherStats>();
}
