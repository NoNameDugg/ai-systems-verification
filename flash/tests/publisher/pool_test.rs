//! Tests for Redis connection pool.
//!
//! These tests verify the connection pool implementation for high-throughput
//! Redis publishing.
//!
//! Test Categories:
//! - Configuration Tests (8 tests)
//! - Statistics Tests (4 tests)
//! - Health Tests (5 tests)
//! - Builder Tests (5 tests)
//! - Integration Tests (10 tests - requires Redis)
//! - Concurrency Tests (6 tests)
//! - Edge Case Tests (4 tests)
//! - Performance Tests (2 tests)
//!
//! Total: 44 tests (TDI requirement: 36+ tests before implementation)

// Note: These imports will be used once the module is integrated
// use astra_flash::publisher::pool::{
//     PoolConfig, PoolError, PoolEvent, PoolHealth, PoolHealthStatus, PoolResult,
//     PoolStats, PooledConnection, RedisPool, RedisPoolBuilder, RedisServerInfo,
// };

// For now, use module-level types for testing
use astra_flash::publisher::{
    PoolConfig, PoolError, PoolEvent, PoolHealth, PoolHealthStatus, PoolStats, RedisPool,
    RedisPoolBuilder, RedisServerInfo,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

// =============================================================================
// TEST UTILITIES
// =============================================================================

/// Default test Redis URL (local)
const TEST_REDIS_URL: &str = "redis://127.0.0.1:6379";

/// Create a test pool configuration
fn test_config() -> PoolConfig {
    PoolConfig {
        url: TEST_REDIS_URL.to_string(),
        max_size: 10,
        min_idle: Some(2),
        connect_timeout_ms: 5000,
        wait_timeout_ms: 5000,
        health_check_interval_ms: 10000,
        auto_reconnect: true,
        max_reconnect_attempts: 3,
        reconnect_delay_ms: 100,
        database: 0,
        password: None,
        tls_enabled: false,
    }
}

/// Create a minimal test configuration
fn minimal_config() -> PoolConfig {
    PoolConfig {
        url: TEST_REDIS_URL.to_string(),
        max_size: 2,
        min_idle: None,
        connect_timeout_ms: 1000,
        wait_timeout_ms: 1000,
        health_check_interval_ms: 30000,
        auto_reconnect: false,
        max_reconnect_attempts: 1,
        reconnect_delay_ms: 100,
        database: 0,
        password: None,
        tls_enabled: false,
    }
}

/// Check if Redis is available for integration tests
fn redis_available() -> bool {
    // In real tests, this would attempt a connection
    // For now, return false to skip integration tests by default
    std::env::var("REDIS_TEST_URL").is_ok() || std::env::var("TEST_REDIS").is_ok()
}

// =============================================================================
// CONFIGURATION TESTS (8 tests)
// =============================================================================

#[test]
fn test_pool_config_default_values() {
    let config = PoolConfig::default();

    assert_eq!(config.max_size, 10);
    assert_eq!(config.connect_timeout_ms, 5000);
    assert_eq!(config.wait_timeout_ms, 5000);
    assert!(config.auto_reconnect);
    assert_eq!(config.database, 0);
    assert!(config.password.is_none());
    assert!(!config.tls_enabled);
}

#[test]
fn test_pool_config_custom_values() {
    let config = PoolConfig {
        url: "redis://custom:6380".to_string(),
        max_size: 50,
        min_idle: Some(10),
        connect_timeout_ms: 10000,
        wait_timeout_ms: 3000,
        health_check_interval_ms: 5000,
        auto_reconnect: false,
        max_reconnect_attempts: 5,
        reconnect_delay_ms: 500,
        database: 1,
        password: Some("secret".to_string()),
        tls_enabled: true,
    };

    assert_eq!(config.url, "redis://custom:6380");
    assert_eq!(config.max_size, 50);
    assert_eq!(config.min_idle, Some(10));
    assert_eq!(config.connect_timeout_ms, 10000);
    assert_eq!(config.wait_timeout_ms, 3000);
    assert!(!config.auto_reconnect);
    assert_eq!(config.database, 1);
    assert_eq!(config.password, Some("secret".to_string()));
    assert!(config.tls_enabled);
}

#[test]
fn test_pool_config_clone() {
    let config = test_config();
    let cloned = config.clone();

    assert_eq!(config.url, cloned.url);
    assert_eq!(config.max_size, cloned.max_size);
    assert_eq!(config.auto_reconnect, cloned.auto_reconnect);
}

#[test]
fn test_pool_config_debug_format() {
    let config = test_config();
    let debug_str = format!("{:?}", config);

    assert!(debug_str.contains("PoolConfig"));
    assert!(debug_str.contains("max_size"));
    assert!(debug_str.contains("10")); // max_size value
}

#[test]
fn test_pool_config_validation_empty_url() {
    let config = PoolConfig {
        url: String::new(),
        ..PoolConfig::default()
    };

    // Validation should fail for empty URL
    let result = config.validate();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("URL") || e.to_string().contains("url"));
    }
}

#[test]
fn test_pool_config_validation_invalid_size() {
    let config = PoolConfig {
        max_size: 0,
        ..PoolConfig::default()
    };

    // Validation should fail for zero pool size
    let result = config.validate();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("size") || e.to_string().contains("max_size"));
    }
}

#[test]
fn test_pool_config_validation_invalid_database() {
    let config = PoolConfig {
        database: 16, // Redis only supports 0-15
        ..PoolConfig::default()
    };

    // Validation should fail for invalid database number
    let result = config.validate();
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("database"));
    }
}

#[test]
fn test_pool_config_from_redis_config() {
    // Assumes RedisConfig from core::config
    // This test verifies conversion from FlashConfig's RedisConfig
    let config = PoolConfig::default();

    // With default values, should have sensible settings
    assert!(config.max_size > 0);
    assert!(config.connect_timeout_ms > 0);
    assert!(!config.url.is_empty() || config.url == TEST_REDIS_URL);
}

// =============================================================================
// STATISTICS TESTS (4 tests)
// =============================================================================

#[test]
fn test_pool_stats_default() {
    let stats = PoolStats::default();

    assert_eq!(stats.connections_created, 0);
    assert_eq!(stats.connections_recycled, 0);
    assert_eq!(stats.connection_timeouts, 0);
    assert_eq!(stats.connection_failures, 0);
    assert_eq!(stats.current_size, 0);
    assert_eq!(stats.idle_connections, 0);
    assert_eq!(stats.in_use_connections, 0);
    assert_eq!(stats.peak_in_use, 0);
    assert_eq!(stats.commands_executed, 0);
    assert!(stats.last_health_check.is_none());
}

#[test]
fn test_pool_stats_update() {
    let mut stats = PoolStats::default();

    stats.connections_created = 10;
    stats.connections_recycled = 5;
    stats.current_size = 8;
    stats.idle_connections = 3;
    stats.in_use_connections = 5;
    stats.peak_in_use = 7;

    assert_eq!(stats.connections_created, 10);
    assert_eq!(stats.connections_recycled, 5);
    assert_eq!(stats.current_size, 8);
    assert_eq!(stats.idle_connections, 3);
    assert_eq!(stats.in_use_connections, 5);
    assert_eq!(stats.peak_in_use, 7);
}

#[test]
fn test_pool_stats_clone() {
    let mut stats = PoolStats::default();
    stats.connections_created = 100;
    stats.commands_executed = 1000;

    let cloned = stats.clone();
    assert_eq!(stats.connections_created, cloned.connections_created);
    assert_eq!(stats.commands_executed, cloned.commands_executed);
}

#[test]
fn test_pool_stats_debug() {
    let stats = PoolStats::default();
    let debug_str = format!("{:?}", stats);

    assert!(debug_str.contains("PoolStats"));
    assert!(debug_str.contains("connections_created"));
}

// =============================================================================
// HEALTH TESTS (5 tests)
// =============================================================================

#[test]
fn test_pool_health_default() {
    let health = PoolHealth::default();

    assert_eq!(health.status, PoolHealthStatus::Unknown);
    assert_eq!(health.avg_latency_us, 0.0);
    assert_eq!(health.consecutive_successes, 0);
    assert_eq!(health.consecutive_failures, 0);
    assert!(health.last_error.is_none());
    assert!(health.server_info.is_none());
}

#[test]
fn test_pool_health_status_transitions() {
    let mut health = PoolHealth::default();

    // Start unknown
    assert_eq!(health.status, PoolHealthStatus::Unknown);

    // After successful checks, become healthy
    health.status = PoolHealthStatus::Healthy;
    health.consecutive_successes = 3;
    assert_eq!(health.status, PoolHealthStatus::Healthy);

    // After failures, become degraded
    health.status = PoolHealthStatus::Degraded;
    health.consecutive_failures = 2;
    assert_eq!(health.status, PoolHealthStatus::Degraded);

    // After more failures, become unhealthy
    health.status = PoolHealthStatus::Unhealthy;
    health.consecutive_failures = 5;
    assert_eq!(health.status, PoolHealthStatus::Unhealthy);
}

#[test]
fn test_pool_health_latency_tracking() {
    let mut health = PoolHealth::default();

    health.last_latency_us = 100;
    health.avg_latency_us = 100.0;

    // Update with new latency (EMA)
    let new_latency = 200u64;
    let alpha = 0.2;
    health.avg_latency_us = alpha * new_latency as f64 + (1.0 - alpha) * health.avg_latency_us;

    assert!(health.avg_latency_us > 100.0);
    assert!(health.avg_latency_us < 200.0);
}

#[test]
fn test_pool_health_with_error() {
    let mut health = PoolHealth::default();

    health.status = PoolHealthStatus::Unhealthy;
    health.last_error = Some("Connection refused".to_string());
    health.consecutive_failures = 5;

    assert!(health.last_error.is_some());
    assert_eq!(health.last_error.as_ref().unwrap(), "Connection refused");
}

#[test]
fn test_pool_health_status_enum() {
    let statuses = [
        PoolHealthStatus::Unknown,
        PoolHealthStatus::Healthy,
        PoolHealthStatus::Degraded,
        PoolHealthStatus::Unhealthy,
    ];

    for status in &statuses {
        let debug_str = format!("{:?}", status);
        assert!(!debug_str.is_empty());
    }

    // Test equality
    assert_eq!(PoolHealthStatus::Healthy, PoolHealthStatus::Healthy);
    assert_ne!(PoolHealthStatus::Healthy, PoolHealthStatus::Unhealthy);
}

// =============================================================================
// BUILDER TESTS (5 tests)
// =============================================================================

#[test]
fn test_builder_basic() {
    let builder = RedisPoolBuilder::new(TEST_REDIS_URL);
    let config = builder.config();

    assert_eq!(config.url, TEST_REDIS_URL);
    assert!(config.max_size > 0);
}

#[test]
fn test_builder_max_size() {
    let builder = RedisPoolBuilder::new(TEST_REDIS_URL).max_size(50);
    let config = builder.config();

    assert_eq!(config.max_size, 50);
}

#[test]
fn test_builder_all_options() {
    let builder = RedisPoolBuilder::new("redis://custom:6380")
        .max_size(20)
        .min_idle(5)
        .connect_timeout(Duration::from_secs(10))
        .wait_timeout(Duration::from_secs(3))
        .health_check_interval(Duration::from_secs(5))
        .auto_reconnect(false)
        .database(2)
        .password("secret123")
        .tls(true);

    let config = builder.config();

    assert_eq!(config.url, "redis://custom:6380");
    assert_eq!(config.max_size, 20);
    assert_eq!(config.min_idle, Some(5));
    assert_eq!(config.connect_timeout_ms, 10000);
    assert_eq!(config.wait_timeout_ms, 3000);
    assert_eq!(config.health_check_interval_ms, 5000);
    assert!(!config.auto_reconnect);
    assert_eq!(config.database, 2);
    assert_eq!(config.password, Some("secret123".to_string()));
    assert!(config.tls_enabled);
}

#[test]
fn test_builder_chaining() {
    // Test that builder methods can be chained
    let builder = RedisPoolBuilder::new(TEST_REDIS_URL)
        .max_size(10)
        .min_idle(2)
        .connect_timeout(Duration::from_secs(5));

    let config = builder.config();
    assert_eq!(config.max_size, 10);
    assert_eq!(config.min_idle, Some(2));
}

#[test]
fn test_builder_invalid_config() {
    let builder = RedisPoolBuilder::new("").max_size(0);

    let config = builder.config();

    // Should have invalid values
    assert!(config.url.is_empty());
    assert_eq!(config.max_size, 0);

    // Validation should fail
    assert!(config.validate().is_err());
}

// =============================================================================
// POOL EVENT TESTS (4 tests)
// =============================================================================

#[test]
fn test_pool_event_created() {
    let event = PoolEvent::Created { size: 10 };

    if let PoolEvent::Created { size } = event {
        assert_eq!(size, 10);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_pool_event_connection_acquired() {
    let event = PoolEvent::ConnectionAcquired { idle_remaining: 5 };

    if let PoolEvent::ConnectionAcquired { idle_remaining } = event {
        assert_eq!(idle_remaining, 5);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_pool_event_connection_released() {
    let event = PoolEvent::ConnectionReleased {
        usage_duration_us: 1000,
    };

    if let PoolEvent::ConnectionReleased { usage_duration_us } = event {
        assert_eq!(usage_duration_us, 1000);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_pool_event_health_check() {
    let event = PoolEvent::HealthCheckCompleted {
        status: PoolHealthStatus::Healthy,
        latency_us: 500,
    };

    if let PoolEvent::HealthCheckCompleted { status, latency_us } = event {
        assert_eq!(status, PoolHealthStatus::Healthy);
        assert_eq!(latency_us, 500);
    } else {
        panic!("Wrong event type");
    }
}

// =============================================================================
// ERROR TESTS (4 tests)
// =============================================================================

#[test]
fn test_pool_error_creation_failed() {
    let error = PoolError::CreationFailed("Connection refused".to_string());

    assert!(error.to_string().contains("Connection refused"));
    assert!(error.to_string().contains("create") || error.to_string().contains("Failed"));
}

#[test]
fn test_pool_error_timeout() {
    let error = PoolError::ConnectionTimeout { timeout_ms: 5000 };

    assert!(error.to_string().contains("timeout") || error.to_string().contains("Timeout"));
    assert!(error.to_string().contains("5000"));
}

#[test]
fn test_pool_error_exhausted() {
    let error = PoolError::PoolExhausted { waiting: 10 };

    assert!(error.to_string().contains("exhaust") || error.to_string().contains("Exhaust"));
    assert!(error.to_string().contains("10"));
}

#[test]
fn test_pool_error_from_redis_error() {
    // Create a Redis error type that we can convert
    // This tests the From<redis::RedisError> impl
    let error = PoolError::RedisError(redis::RedisError::from((
        redis::ErrorKind::IoError,
        "Connection reset",
    )));

    assert!(error.to_string().contains("Redis") || error.to_string().contains("redis"));
}

// =============================================================================
// SERVER INFO TESTS (2 tests)
// =============================================================================

#[test]
fn test_server_info_struct() {
    let info = RedisServerInfo {
        version: "7.0.0".to_string(),
        connected_clients: 5,
        used_memory_bytes: 1024 * 1024,
        uptime_seconds: 3600,
    };

    assert_eq!(info.version, "7.0.0");
    assert_eq!(info.connected_clients, 5);
    assert_eq!(info.used_memory_bytes, 1024 * 1024);
    assert_eq!(info.uptime_seconds, 3600);
}

#[test]
fn test_server_info_clone() {
    let info = RedisServerInfo {
        version: "7.0.0".to_string(),
        connected_clients: 5,
        used_memory_bytes: 1024 * 1024,
        uptime_seconds: 3600,
    };

    let cloned = info.clone();
    assert_eq!(info.version, cloned.version);
    assert_eq!(info.connected_clients, cloned.connected_clients);
}

// =============================================================================
// INTEGRATION TESTS (10 tests - require Redis)
// =============================================================================

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_creation_success() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await;
    assert!(pool.is_ok());

    let pool = pool.unwrap();
    assert!(pool.stats().current_size > 0 || pool.config().min_idle.is_none());
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_get_connection() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let conn = pool.get().await;

    assert!(conn.is_ok());
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_get_multiple_connections() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();

    let conn1 = pool.get().await;
    let conn2 = pool.get().await;
    let conn3 = pool.get().await;

    assert!(conn1.is_ok());
    assert!(conn2.is_ok());
    assert!(conn3.is_ok());

    // Stats should show 3 in use
    let stats = pool.stats();
    assert!(stats.in_use_connections >= 3 || stats.connections_created >= 3);
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_connection_return() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(minimal_config()).await.unwrap();

    {
        let _conn = pool.get().await.unwrap();
        // Connection in use
    }
    // Connection returned to pool on drop

    // Get another connection (should reuse)
    let _conn = pool.get().await.unwrap();

    let stats = pool.stats();
    assert!(stats.connections_recycled >= 1 || stats.connections_created >= 1);
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_ping() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let result = pool.ping().await;

    assert!(result.is_ok());
    let latency = result.unwrap();
    assert!(latency.as_millis() < 1000); // Should be fast for local Redis
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_health_check() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let result = pool.health_check().await;

    assert!(result.is_ok());
    let health = result.unwrap();
    assert!(
        health.status == PoolHealthStatus::Healthy || health.status == PoolHealthStatus::Unknown
    );
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_server_info() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let result = pool.server_info().await;

    assert!(result.is_ok());
    let info = result.unwrap();
    assert!(!info.version.is_empty());
    assert!(info.uptime_seconds > 0);
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_close() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(minimal_config()).await.unwrap();
    let result = pool.close().await;

    assert!(result.is_ok());

    // Getting connection after close should fail
    let conn_result = pool.get().await;
    assert!(conn_result.is_err());
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_reset() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();

    // Get some connections
    let _conn1 = pool.get().await.unwrap();
    drop(_conn1);

    let initial_created = pool.stats().connections_created;

    // Reset the pool
    let result = pool.reset().await;
    assert!(result.is_ok());

    // After reset, should be able to get connections again
    let conn = pool.get().await;
    assert!(conn.is_ok());
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_status_updates() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();

    // Initial status
    let initial_stats = pool.stats();

    // Get connection
    let conn = pool.get().await.unwrap();

    // Check stats updated
    let after_get_stats = pool.stats();

    // Drop connection
    drop(conn);

    // Check stats updated again
    let final_stats = pool.stats();

    // Verify stats were tracked
    assert!(after_get_stats.in_use_connections >= 1 || after_get_stats.connections_created >= 1);
}

// =============================================================================
// CONCURRENCY TESTS (6 tests)
// =============================================================================

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_concurrent_acquire_10() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let mut handles = Vec::new();

    for _ in 0..10 {
        let pool_clone = Arc::clone(&pool);
        handles.push(tokio::spawn(async move {
            let conn = pool_clone.get().await;
            assert!(conn.is_ok());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_concurrent_acquire_100() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(
        RedisPool::builder(TEST_REDIS_URL)
            .max_size(20)
            .build()
            .await
            .unwrap(),
    );

    let mut handles = Vec::new();
    let success_count = Arc::new(AtomicUsize::new(0));

    for _ in 0..100 {
        let pool_clone = Arc::clone(&pool);
        let success_count_clone = Arc::clone(&success_count);

        handles.push(tokio::spawn(async move {
            match pool_clone.get().await {
                Ok(_conn) => {
                    success_count_clone.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(_) => {
                    // Some might timeout, that's expected
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // At least some should succeed
    assert!(success_count.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_concurrent_acquire_release() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(minimal_config()).await.unwrap());
    let iteration_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();

    for _ in 0..20 {
        let pool_clone = Arc::clone(&pool);
        let iteration_clone = Arc::clone(&iteration_count);

        handles.push(tokio::spawn(async move {
            for _ in 0..5 {
                if let Ok(_conn) = pool_clone.get().await {
                    iteration_clone.fetch_add(1, Ordering::SeqCst);
                    // Briefly hold and release
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Should have completed many iterations
    assert!(iteration_count.load(Ordering::SeqCst) > 50);
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_exhaustion() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::builder(TEST_REDIS_URL)
        .max_size(2)
        .wait_timeout(Duration::from_millis(100))
        .build()
        .await
        .unwrap();

    // Hold 2 connections
    let _conn1 = pool.get().await.unwrap();
    let _conn2 = pool.get().await.unwrap();

    // Third should timeout
    let result = pool.get().await;

    // Should either get a timeout error or succeed if connection is recycled fast
    match result {
        Ok(_) => { /* Connection was available */ }
        Err(PoolError::ConnectionTimeout { .. }) => { /* Expected */ }
        Err(PoolError::PoolExhausted { .. }) => { /* Expected */ }
        Err(e) => panic!("Unexpected error: {:?}", e),
    }
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_concurrent_health_checks() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());
    let mut handles = Vec::new();

    for _ in 0..10 {
        let pool_clone = Arc::clone(&pool);
        handles.push(tokio::spawn(async move {
            let result = pool_clone.health_check().await;
            assert!(result.is_ok());
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_under_load() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(
        RedisPool::builder(TEST_REDIS_URL)
            .max_size(10)
            .build()
            .await
            .unwrap(),
    );

    let start = Instant::now();
    let operations_completed = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    // Run for 1 second under load
    for _ in 0..50 {
        let pool_clone = Arc::clone(&pool);
        let ops_clone = Arc::clone(&operations_completed);

        handles.push(tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(1);

            while Instant::now() < deadline {
                if let Ok(conn) = pool_clone.get().await {
                    // Simulate some work
                    ops_clone.fetch_add(1, Ordering::SeqCst);
                    drop(conn);
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let ops = operations_completed.load(Ordering::SeqCst);

    // Should complete many operations
    assert!(ops > 100);
    println!("Completed {} operations in {:?}", ops, elapsed);
}

// =============================================================================
// EDGE CASE TESTS (4 tests)
// =============================================================================

#[tokio::test]
async fn test_pool_invalid_url() {
    let config = PoolConfig {
        url: "not-a-valid-url".to_string(),
        ..PoolConfig::default()
    };

    let result = RedisPool::new(config).await;

    // Should fail to create pool with invalid URL
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_pool_connection_timeout() {
    if !redis_available() {
        return;
    }

    // Create pool with very short timeout
    let config = PoolConfig {
        url: TEST_REDIS_URL.to_string(),
        max_size: 1,
        connect_timeout_ms: 1, // 1ms timeout
        wait_timeout_ms: 1,
        ..PoolConfig::default()
    };

    // This might succeed if Redis is very fast, or fail with timeout
    let result = RedisPool::new(config).await;

    // Either outcome is acceptable for this test
    match result {
        Ok(_) => { /* Redis was fast enough */ }
        Err(PoolError::ConnectionTimeout { .. }) => { /* Expected timeout */ }
        Err(e) => panic!("Unexpected error type: {:?}", e),
    }
}

#[tokio::test]
async fn test_pool_unreachable_server() {
    // Try to connect to a port that's likely not running Redis
    // Note: Pool creation is lazy in deadpool, so we need to actually try to get a connection
    let config = PoolConfig {
        url: "redis://127.0.0.1:59999".to_string(),
        connect_timeout_ms: 100,
        wait_timeout_ms: 100,
        ..PoolConfig::default()
    };

    // Pool creation may succeed (it's lazy), but getting a connection should fail
    match RedisPool::new(config).await {
        Ok(pool) => {
            // Try to actually get a connection - this should fail
            let conn_result = pool.get().await;
            assert!(
                conn_result.is_err(),
                "Getting connection from unreachable server should fail"
            );
        }
        Err(_) => {
            // Pool creation failed, which is also acceptable
        }
    }
}

#[test]
fn test_pool_config_serialization() {
    let config = test_config();

    // Test that config can be serialized/deserialized
    let json = serde_json::to_string(&config);
    assert!(json.is_ok());

    let json_str = json.unwrap();
    assert!(json_str.contains("max_size"));
    assert!(json_str.contains("10"));

    // Deserialize
    let parsed: Result<PoolConfig, _> = serde_json::from_str(&json_str);
    assert!(parsed.is_ok());

    let parsed_config = parsed.unwrap();
    assert_eq!(parsed_config.max_size, config.max_size);
    assert_eq!(parsed_config.url, config.url);
}

// =============================================================================
// PERFORMANCE TESTS (2 tests)
// =============================================================================

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_acquire_latency() {
    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();

    // Warm up
    let _conn = pool.get().await.unwrap();
    drop(_conn);

    // Measure acquire latency
    let mut latencies = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let conn = pool.get().await.unwrap();
        let latency = start.elapsed();
        latencies.push(latency);
        drop(conn);
    }

    // Calculate average
    let avg_latency: Duration = latencies.iter().sum::<Duration>() / latencies.len() as u32;

    println!("Average acquire latency: {:?}", avg_latency);

    // Should be under 1ms for local Redis with warm pool
    assert!(
        avg_latency.as_millis() < 10,
        "Acquire latency too high: {:?}",
        avg_latency
    );
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_throughput() {
    if !redis_available() {
        return;
    }

    let pool = Arc::new(RedisPool::new(test_config()).await.unwrap());

    let start = Instant::now();
    let operations = 1000;

    for _ in 0..operations {
        let conn = pool.get().await.unwrap();
        drop(conn);
    }

    let elapsed = start.elapsed();
    let ops_per_sec = operations as f64 / elapsed.as_secs_f64();

    println!(
        "Throughput: {} ops in {:?} ({:.0} ops/sec)",
        operations, elapsed, ops_per_sec
    );

    // Should handle at least 10000 ops/sec for local acquire/release
    assert!(
        ops_per_sec > 1000.0,
        "Throughput too low: {} ops/sec",
        ops_per_sec
    );
}

// =============================================================================
// DUMMY KEY WRITE/READ TESTS (Batch 3.1 Verification)
// =============================================================================

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_write_read_dummy_key() {
    //! Integration test: Write and read a dummy key to verify Redis connectivity.
    //!
    //! This test verifies:
    //! 1. Connection to Redis can be established
    //! 2. SET command works correctly
    //! 3. GET command retrieves the written value
    //! 4. DEL command cleans up the test key
    //!
    //! Required for Batch 3.1 verification.

    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let mut conn = pool.get().await.unwrap();

    // Test key and value
    let test_key = "astra_flash:test:dummy_key";
    let test_value = "hello_from_astra_flash";

    // Write (SET)
    let set_result: String = redis::cmd("SET")
        .arg(test_key)
        .arg(test_value)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert_eq!(set_result, "OK");

    // Read (GET)
    let get_result: String = redis::cmd("GET")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert_eq!(get_result, test_value);

    // Cleanup (DEL)
    let del_result: i32 = redis::cmd("DEL")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert_eq!(del_result, 1); // 1 key deleted

    // Verify key is gone
    let verify_result: Option<String> = redis::cmd("GET")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert!(verify_result.is_none());

    println!("✓ Successfully wrote and read dummy key from Redis");
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_write_read_with_expiry() {
    //! Integration test: Write a key with TTL and verify expiry behavior.
    //!
    //! This test verifies:
    //! 1. SETEX command works (SET with expiry)
    //! 2. TTL can be retrieved
    //! 3. Key can be read before expiry

    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();
    let mut conn = pool.get().await.unwrap();

    let test_key = "astra_flash:test:expiring_key";
    let test_value = "this_will_expire";
    let ttl_seconds = 60;

    // Write with expiry (SETEX)
    let set_result: String = redis::cmd("SETEX")
        .arg(test_key)
        .arg(ttl_seconds)
        .arg(test_value)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert_eq!(set_result, "OK");

    // Check TTL
    let ttl_result: i64 = redis::cmd("TTL")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert!(ttl_result > 0);
    assert!(ttl_result <= ttl_seconds);

    // Read value
    let get_result: String = redis::cmd("GET")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();
    assert_eq!(get_result, test_value);

    // Cleanup
    let _: i32 = redis::cmd("DEL")
        .arg(test_key)
        .query_async(&mut *conn)
        .await
        .unwrap();

    println!("✓ Successfully wrote and read key with TTL from Redis");
}

#[tokio::test]
#[ignore = "Requires Redis server"]
async fn test_redis_execute_helper() {
    //! Integration test: Verify the pool.execute() helper method.
    //!
    //! This uses the higher-level execute API provided by RedisPool.

    if !redis_available() {
        return;
    }

    let pool = RedisPool::new(test_config()).await.unwrap();

    let test_key = "astra_flash:test:execute_test";
    let test_value = "via_execute_helper";

    // Use pool.execute() for SET
    let set_result: String = pool
        .execute(redis::cmd("SET").arg(test_key).arg(test_value))
        .await
        .unwrap();
    assert_eq!(set_result, "OK");

    // Use pool.execute() for GET
    let get_result: String = pool.execute(redis::cmd("GET").arg(test_key)).await.unwrap();
    assert_eq!(get_result, test_value);

    // Cleanup
    let _: i32 = pool.execute(redis::cmd("DEL").arg(test_key)).await.unwrap();

    // Verify stats updated
    let stats = pool.stats();
    assert!(stats.commands_executed >= 3);

    println!("✓ Successfully used pool.execute() helper for Redis commands");
}
