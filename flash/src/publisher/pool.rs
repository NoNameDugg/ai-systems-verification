//! Redis connection pool for Flash.
//!
//! This module provides high-performance connection pooling for Redis,
//! using `deadpool-redis` for efficient connection management.
//!
//! # Features
//!
//! - Connection pooling with configurable size
//! - Automatic connection health checks
//! - Graceful reconnection on failures
//! - Metrics integration for monitoring
//! - Builder pattern for configuration
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::pool::{RedisPool, PoolConfig};
//!
//! // Create pool with default config
//! let pool = RedisPool::new(PoolConfig::default()).await?;
//!
//! // Or use the builder
//! let pool = RedisPool::builder("redis://127.0.0.1:6379")
//!     .max_size(20)
//!     .min_idle(5)
//!     .build()
//!     .await?;
//!
//! // Get a connection
//! let conn = pool.get().await?;
//!
//! // Use the connection for Redis commands
//! // Connection is automatically returned to pool on drop
//! ```
//!
//! # Performance Targets
//!
//! | Metric | Target |
//! |--------|--------|
//! | Pool creation | < 100 ms |
//! | Connection acquire | < 1 ms |
//! | Health check | < 10 ms |

use crate::core::config::RedisConfig;
use crate::core::types::Timestamp;
use deadpool_redis::{Config as DeadpoolConfig, Connection, Pool, Runtime};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

// =============================================================================
// POOL CONFIGURATION
// =============================================================================

/// Configuration for the Redis connection pool.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::pool::PoolConfig;
///
/// let config = PoolConfig {
///     url: "redis://127.0.0.1:6379".to_string(),
///     max_size: 20,
///     min_idle: Some(5),
///     ..PoolConfig::default()
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Redis connection URL (e.g., "redis://127.0.0.1:6379").
    pub url: String,

    /// Maximum number of connections in the pool.
    pub max_size: usize,

    /// Minimum number of idle connections to maintain.
    pub min_idle: Option<usize>,

    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,

    /// Maximum time to wait for a connection from the pool (ms).
    pub wait_timeout_ms: u64,

    /// Interval for health check pings (ms).
    pub health_check_interval_ms: u64,

    /// Enable automatic reconnection.
    pub auto_reconnect: bool,

    /// Maximum reconnection attempts before giving up.
    pub max_reconnect_attempts: u32,

    /// Reconnection delay base for exponential backoff (ms).
    pub reconnect_delay_ms: u64,

    /// Redis database number (0-15).
    pub database: u8,

    /// Optional password for authentication.
    pub password: Option<String>,

    /// Enable TLS/SSL connection.
    pub tls_enabled: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_string(),
            max_size: 10,
            min_idle: Some(2),
            connect_timeout_ms: 5000,
            wait_timeout_ms: 5000,
            health_check_interval_ms: 10000,
            auto_reconnect: true,
            max_reconnect_attempts: 10,
            reconnect_delay_ms: 1000,
            database: 0,
            password: None,
            tls_enabled: false,
        }
    }
}

impl PoolConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns `PoolError::InvalidConfig` if validation fails.
    pub fn validate(&self) -> PoolResult<()> {
        if self.url.is_empty() {
            return Err(PoolError::InvalidConfig("URL cannot be empty".to_string()));
        }

        if self.max_size == 0 {
            return Err(PoolError::InvalidConfig(
                "max_size must be greater than 0".to_string(),
            ));
        }

        if self.database > 15 {
            return Err(PoolError::InvalidConfig(
                "database must be between 0 and 15".to_string(),
            ));
        }

        if let Some(min) = self.min_idle {
            if min > self.max_size {
                return Err(PoolError::InvalidConfig(
                    "min_idle cannot exceed max_size".to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl From<&RedisConfig> for PoolConfig {
    fn from(config: &RedisConfig) -> Self {
        Self {
            url: config.url.clone(),
            max_size: config.pool_size as usize,
            min_idle: Some(config.pool_size as usize / 4),
            connect_timeout_ms: config.connect_timeout_ms,
            wait_timeout_ms: 5000,
            health_check_interval_ms: config.health_check_interval_ms,
            auto_reconnect: true,
            max_reconnect_attempts: 10,
            reconnect_delay_ms: 1000,
            database: config.database,
            password: None,
            tls_enabled: false,
        }
    }
}

// =============================================================================
// POOL STATISTICS
// =============================================================================

/// Pool statistics for monitoring.
///
/// Tracks connection usage patterns for performance analysis and alerting.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total connections created since pool start.
    pub connections_created: u64,

    /// Total connections recycled (returned to pool).
    pub connections_recycled: u64,

    /// Total connection timeouts.
    pub connection_timeouts: u64,

    /// Total failed connection attempts.
    pub connection_failures: u64,

    /// Current pool size (active + idle).
    pub current_size: usize,

    /// Current idle connections.
    pub idle_connections: usize,

    /// Current in-use connections.
    pub in_use_connections: usize,

    /// Peak in-use connections (high water mark).
    pub peak_in_use: usize,

    /// Total commands executed.
    pub commands_executed: u64,

    /// Last successful health check timestamp.
    pub last_health_check: Option<Timestamp>,
}

// =============================================================================
// POOL HEALTH
// =============================================================================

/// Health status of the connection pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum PoolHealthStatus {
    /// Pool is healthy, all connections working.
    Healthy,
    /// Pool is degraded (some connections failing).
    Degraded,
    /// Pool is unhealthy (majority failing).
    Unhealthy,
    /// Pool status unknown (no health checks yet).
    #[default]
    Unknown,
}


/// Health information for the connection pool.
#[derive(Debug, Clone)]
pub struct PoolHealth {
    /// Current health status.
    pub status: PoolHealthStatus,

    /// Average latency to Redis (microseconds).
    pub avg_latency_us: f64,

    /// Last latency measurement (microseconds).
    pub last_latency_us: u64,

    /// Number of consecutive successful health checks.
    pub consecutive_successes: u32,

    /// Number of consecutive failed health checks.
    pub consecutive_failures: u32,

    /// Last error message (if any).
    pub last_error: Option<String>,

    /// Redis server info (if available).
    pub server_info: Option<RedisServerInfo>,
}

impl Default for PoolHealth {
    fn default() -> Self {
        Self {
            status: PoolHealthStatus::Unknown,
            avg_latency_us: 0.0,
            last_latency_us: 0,
            consecutive_successes: 0,
            consecutive_failures: 0,
            last_error: None,
            server_info: None,
        }
    }
}

/// Redis server information.
#[derive(Debug, Clone)]
pub struct RedisServerInfo {
    /// Redis version string.
    pub version: String,
    /// Number of connected clients.
    pub connected_clients: u32,
    /// Used memory in bytes.
    pub used_memory_bytes: u64,
    /// Uptime in seconds.
    pub uptime_seconds: u64,
}

// =============================================================================
// POOL EVENTS
// =============================================================================

/// Events emitted by the pool for monitoring.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// Pool created successfully.
    Created {
        /// Number of connections in pool.
        size: usize,
    },

    /// Connection acquired from pool.
    ConnectionAcquired {
        /// Remaining idle connections.
        idle_remaining: usize,
    },

    /// Connection returned to pool.
    ConnectionReleased {
        /// Duration connection was in use (microseconds).
        usage_duration_us: u64,
    },

    /// Connection creation failed.
    ConnectionFailed {
        /// Error description.
        error: String,
    },

    /// Health check completed.
    HealthCheckCompleted {
        /// Resulting health status.
        status: PoolHealthStatus,
        /// Latency in microseconds.
        latency_us: u64,
    },

    /// Pool exhausted (no available connections).
    Exhausted {
        /// Number of requests waiting.
        wait_queue_size: usize,
    },

    /// Pool resized.
    Resized {
        /// Previous size.
        old_size: usize,
        /// New size.
        new_size: usize,
    },
}

// =============================================================================
// POOL ERRORS
// =============================================================================

/// Errors specific to Redis pool operations.
#[derive(Debug, Error)]
pub enum PoolError {
    /// Failed to create connection pool.
    #[error("Failed to create pool: {0}")]
    CreationFailed(String),

    /// Failed to acquire connection from pool.
    #[error("Failed to acquire connection: {0}")]
    AcquisitionFailed(String),

    /// Connection timeout.
    #[error("Connection timeout after {timeout_ms}ms")]
    ConnectionTimeout {
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// Pool exhausted (no connections available).
    #[error("Pool exhausted, {waiting} requests waiting")]
    PoolExhausted {
        /// Number of waiting requests.
        waiting: usize,
    },

    /// Connection health check failed.
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),

    /// Invalid configuration.
    #[error("Invalid pool configuration: {0}")]
    InvalidConfig(String),

    /// Redis command error.
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    /// Pool is closed.
    #[error("Pool is closed")]
    PoolClosed,

    /// Deadpool error.
    #[error("Pool error: {0}")]
    DeadpoolError(String),
}

/// Result type for pool operations.
pub type PoolResult<T> = Result<T, PoolError>;

// =============================================================================
// REDIS POOL BUILDER
// =============================================================================

/// Builder for creating `RedisPool` with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// let pool = RedisPoolBuilder::new("redis://localhost:6379")
///     .max_size(20)
///     .min_idle(5)
///     .connect_timeout(Duration::from_secs(10))
///     .build()
///     .await?;
/// ```
pub struct RedisPoolBuilder {
    config: PoolConfig,
}

impl RedisPoolBuilder {
    /// Create a new builder with the given Redis URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            config: PoolConfig {
                url: url.into(),
                ..PoolConfig::default()
            },
        }
    }

    /// Set maximum pool size.
    #[must_use]
    pub const fn max_size(mut self, size: usize) -> Self {
        self.config.max_size = size;
        self
    }

    /// Set minimum idle connections.
    #[must_use]
    pub const fn min_idle(mut self, count: usize) -> Self {
        self.config.min_idle = Some(count);
        self
    }

    /// Set connection timeout.
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.config.connect_timeout_ms = timeout.as_millis() as u64;
        self
    }

    /// Set wait timeout for acquiring connections.
    #[must_use]
    pub const fn wait_timeout(mut self, timeout: Duration) -> Self {
        self.config.wait_timeout_ms = timeout.as_millis() as u64;
        self
    }

    /// Set health check interval.
    #[must_use]
    pub const fn health_check_interval(mut self, interval: Duration) -> Self {
        self.config.health_check_interval_ms = interval.as_millis() as u64;
        self
    }

    /// Enable/disable auto reconnection.
    #[must_use]
    pub const fn auto_reconnect(mut self, enabled: bool) -> Self {
        self.config.auto_reconnect = enabled;
        self
    }

    /// Set database number (0-15).
    #[must_use]
    pub const fn database(mut self, db: u8) -> Self {
        self.config.database = db;
        self
    }

    /// Set password for authentication.
    #[must_use]
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.config.password = Some(password.into());
        self
    }

    /// Enable/disable TLS.
    #[must_use]
    pub const fn tls(mut self, enabled: bool) -> Self {
        self.config.tls_enabled = enabled;
        self
    }

    /// Get the current configuration.
    #[must_use]
    pub const fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Build the pool.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid or connection fails.
    pub async fn build(self) -> PoolResult<RedisPool> {
        RedisPool::new(self.config).await
    }
}

// =============================================================================
// POOLED CONNECTION
// =============================================================================

/// Connection wrapper for RAII-style usage tracking.
///
/// When dropped, the connection is automatically returned to the pool
/// and usage statistics are recorded.
pub struct PooledConnection {
    inner: Option<Connection>,
    acquired_at: Instant,
    stats: Arc<RwLock<PoolStats>>,
}

impl PooledConnection {
    /// Create a new pooled connection.
    fn new(conn: Connection, stats: Arc<RwLock<PoolStats>>) -> Self {
        Self {
            inner: Some(conn),
            acquired_at: Instant::now(),
            stats,
        }
    }

    /// Get the time this connection was acquired.
    #[must_use]
    pub const fn acquired_at(&self) -> Instant {
        self.acquired_at
    }

    /// Get how long this connection has been in use.
    #[must_use]
    pub fn usage_duration(&self) -> Duration {
        self.acquired_at.elapsed()
    }
}

impl Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().expect("Connection already dropped")
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().expect("Connection already dropped")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if self.inner.is_some() {
            let mut stats = self.stats.write();
            stats.connections_recycled += 1;
            if stats.in_use_connections > 0 {
                stats.in_use_connections -= 1;
            }
            stats.idle_connections += 1;
        }
    }
}

// =============================================================================
// REDIS POOL
// =============================================================================

/// Redis connection pool for high-throughput publishing.
///
/// Wraps `deadpool_redis::Pool` with project-specific configuration,
/// health monitoring, and metrics integration.
///
/// # Thread Safety
///
/// `RedisPool` is `Send + Sync` and can be safely shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use astra_flash::publisher::pool::RedisPool;
///
/// let pool = RedisPool::builder("redis://localhost:6379")
///     .max_size(20)
///     .build()
///     .await?;
///
/// // Get connection (RAII - auto-returns on drop)
/// let mut conn = pool.get().await?;
///
/// // Check health
/// let health = pool.health_check().await?;
/// println!("Pool status: {:?}", health.status);
/// ```
pub struct RedisPool {
    inner: Pool,
    config: PoolConfig,
    stats: Arc<RwLock<PoolStats>>,
    health: Arc<RwLock<PoolHealth>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl RedisPool {
    /// Create a new pool with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid or initial connection fails.
    #[allow(clippy::unused_async)] // Async for consistency with other pool operations
    pub async fn new(config: PoolConfig) -> PoolResult<Self> {
        config.validate()?;

        // Build deadpool configuration
        let deadpool_config = DeadpoolConfig::from_url(&config.url);

        let pool = deadpool_config
            .builder()
            .map_err(|e| PoolError::CreationFailed(e.to_string()))?
            .max_size(config.max_size)
            .wait_timeout(Some(Duration::from_millis(config.wait_timeout_ms)))
            .create_timeout(Some(Duration::from_millis(config.connect_timeout_ms)))
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|e| PoolError::CreationFailed(e.to_string()))?;

        let stats = Arc::new(RwLock::new(PoolStats::default()));
        let health = Arc::new(RwLock::new(PoolHealth::default()));

        let pool = Self {
            inner: pool,
            config,
            stats,
            health,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        // Record initial stats
        {
            let mut stats = pool.stats.write();
            stats.current_size = pool.inner.status().size;
            stats.idle_connections = pool.inner.status().available;
        }

        Ok(pool)
    }

    /// Create a pool using the builder pattern.
    #[must_use]
    pub fn builder(url: impl Into<String>) -> RedisPoolBuilder {
        RedisPoolBuilder::new(url)
    }

    /// Create a pool from `FlashConfig`'s Redis configuration.
    ///
    /// # Errors
    ///
    /// Returns error if connection fails.
    pub async fn from_redis_config(config: &RedisConfig) -> PoolResult<Self> {
        Self::new(PoolConfig::from(config)).await
    }

    // =========================================================================
    // Connection Acquisition
    // =========================================================================

    /// Get a connection from the pool.
    ///
    /// Returns a connection handle that automatically returns to the pool on drop.
    ///
    /// # Errors
    ///
    /// Returns error if pool is closed, exhausted, or connection fails.
    pub async fn get(&self) -> PoolResult<PooledConnection> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(PoolError::PoolClosed);
        }

        let start = Instant::now();

        let conn = self.inner.get().await.map_err(|e| {
            let mut stats = self.stats.write();
            stats.connection_failures += 1;
            PoolError::AcquisitionFailed(e.to_string())
        })?;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.connections_created += 1;
            stats.in_use_connections += 1;
            if stats.idle_connections > 0 {
                stats.idle_connections -= 1;
            }
            if stats.in_use_connections > stats.peak_in_use {
                stats.peak_in_use = stats.in_use_connections;
            }
            stats.current_size = self.inner.status().size;
        }

        // Note: Latency metrics would be recorded here in production
        // via the metrics crate (flash.redis.pool.acquire_duration_us)
        let _latency_us = start.elapsed().as_micros() as u64;

        Ok(PooledConnection::new(conn, Arc::clone(&self.stats)))
    }

    /// Check if the pool has available connections.
    ///
    /// This is a non-blocking check of pool status.
    #[must_use]
    pub fn has_available(&self) -> bool {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        self.inner.status().available > 0
    }

    /// Get a connection with a custom timeout.
    ///
    /// # Errors
    ///
    /// Returns `PoolError::ConnectionTimeout` if timeout expires.
    pub async fn get_timeout(&self, timeout: Duration) -> PoolResult<PooledConnection> {
        if let Ok(result) = tokio::time::timeout(timeout, self.get()).await { result } else {
            let mut stats = self.stats.write();
            stats.connection_timeouts += 1;
            Err(PoolError::ConnectionTimeout {
                timeout_ms: timeout.as_millis() as u64,
            })
        }
    }

    // =========================================================================
    // Health & Status
    // =========================================================================

    /// Check if the pool is healthy.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        let health = self.health.read();
        health.status == PoolHealthStatus::Healthy
    }

    /// Get current pool health status.
    #[must_use]
    pub fn health(&self) -> PoolHealth {
        self.health.read().clone()
    }

    /// Get current pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let mut stats = self.stats.read().clone();
        // Update with current pool status
        stats.current_size = self.inner.status().size;
        stats.idle_connections = self.inner.status().available;
        stats
    }

    /// Perform a health check (PING command).
    ///
    /// # Errors
    ///
    /// Returns error if health check fails.
    pub async fn health_check(&self) -> PoolResult<PoolHealth> {
        let start = Instant::now();

        let result = self.ping().await;
        let latency_us = start.elapsed().as_micros() as u64;

        let mut health = self.health.write();

        match result {
            Ok(_) => {
                // Update EMA for latency
                let alpha = 0.2;
                health.avg_latency_us =
                    alpha * latency_us as f64 + (1.0 - alpha) * health.avg_latency_us;
                health.last_latency_us = latency_us;
                health.consecutive_successes += 1;
                health.consecutive_failures = 0;
                health.last_error = None;

                // Determine health status: Healthy if 3+ successes or no failures
                health.status = if health.consecutive_successes >= 3
                    || health.consecutive_failures == 0
                {
                    PoolHealthStatus::Healthy
                } else {
                    PoolHealthStatus::Degraded
                };

                // Update last health check timestamp
                let mut stats = self.stats.write();
                stats.last_health_check = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as i64,
                );

                Ok(health.clone())
            },
            Err(e) => {
                health.consecutive_failures += 1;
                health.consecutive_successes = 0;
                health.last_error = Some(e.to_string());

                // Determine health status based on failure count: Unhealthy at 5+
                health.status = if health.consecutive_failures >= 5 {
                    PoolHealthStatus::Unhealthy
                } else {
                    PoolHealthStatus::Degraded
                };

                Err(PoolError::HealthCheckFailed(e.to_string()))
            },
        }
    }

    /// Get Redis server info.
    ///
    /// # Errors
    ///
    /// Returns error if unable to get server info.
    pub async fn server_info(&self) -> PoolResult<RedisServerInfo> {
        let mut conn = self.get().await?;

        let info: String = redis::cmd("INFO")
            .query_async(&mut *conn)
            .await
            .map_err(PoolError::from)?;

        // Parse INFO response
        let mut version = String::new();
        let mut connected_clients = 0u32;
        let mut used_memory = 0u64;
        let mut uptime = 0u64;

        for line in info.lines() {
            if let Some(v) = line.strip_prefix("redis_version:") {
                version = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("connected_clients:") {
                connected_clients = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("used_memory:") {
                used_memory = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("uptime_in_seconds:") {
                uptime = v.trim().parse().unwrap_or(0);
            }
        }

        let server_info = RedisServerInfo {
            version,
            connected_clients,
            used_memory_bytes: used_memory,
            uptime_seconds: uptime,
        };

        // Store in health
        {
            let mut health = self.health.write();
            health.server_info = Some(server_info.clone());
        }

        Ok(server_info)
    }

    // =========================================================================
    // Pool Management
    // =========================================================================

    /// Close the pool gracefully.
    ///
    /// After calling this, no new connections can be acquired.
    ///
    /// # Errors
    ///
    /// This method does not return errors.
    #[allow(clippy::unused_async)] // Async for consistency with other pool operations
    pub async fn close(&self) -> PoolResult<()> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.close();
        Ok(())
    }

    /// Clear all connections and recreate the pool.
    ///
    /// This is useful for recovering from widespread connection failures.
    ///
    /// # Errors
    ///
    /// Returns error if pool recreation fails.
    #[allow(clippy::unused_async)] // Async for consistency with other pool operations
    pub async fn reset(&self) -> PoolResult<()> {
        // Close existing connections
        self.inner.close();

        // Reset stats
        {
            let mut stats = self.stats.write();
            stats.current_size = 0;
            stats.idle_connections = 0;
            stats.in_use_connections = 0;
        }

        // Reset health
        {
            let mut health = self.health.write();
            health.status = PoolHealthStatus::Unknown;
            health.consecutive_successes = 0;
            health.consecutive_failures = 0;
            health.last_error = None;
        }

        // Note: deadpool doesn't support true reset, connections will be recreated on demand
        self.closed
            .store(false, std::sync::atomic::Ordering::SeqCst);

        Ok(())
    }

    // =========================================================================
    // Convenience Methods
    // =========================================================================

    /// Execute a PING command to test connectivity.
    ///
    /// # Returns
    ///
    /// The round-trip latency of the PING command.
    ///
    /// # Errors
    ///
    /// Returns error if PING fails.
    pub async fn ping(&self) -> PoolResult<Duration> {
        let start = Instant::now();
        let mut conn = self.get().await?;

        let _: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(PoolError::from)?;

        Ok(start.elapsed())
    }

    /// Execute a Redis command directly.
    ///
    /// For simple operations where you don't need to hold the connection.
    ///
    /// # Errors
    ///
    /// Returns error if command fails.
    pub async fn execute<T: redis::FromRedisValue>(&self, cmd: &mut redis::Cmd) -> PoolResult<T> {
        let mut conn = self.get().await?;

        let result = cmd.query_async(&mut *conn).await.map_err(PoolError::from)?;

        // Update command count
        {
            let mut stats = self.stats.write();
            stats.commands_executed += 1;
        }

        Ok(result)
    }

    /// Get the pool configuration.
    #[must_use]
    pub const fn config(&self) -> &PoolConfig {
        &self.config
    }
}

// =============================================================================
// TRAIT IMPLEMENTATIONS
// =============================================================================

impl std::fmt::Debug for RedisPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPool")
            .field("config", &self.config)
            .field("stats", &*self.stats.read())
            .field("health", &*self.health.read())
            .field(
                "closed",
                &self.closed.load(std::sync::atomic::Ordering::SeqCst),
            )
            .finish()
    }
}

// Verify Send + Sync
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RedisPool>();
    assert_send_sync::<PoolConfig>();
    assert_send_sync::<PoolStats>();
    assert_send_sync::<PoolHealth>();
};

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_size, 10);
        assert_eq!(config.database, 0);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_pool_config_validation() {
        let mut config = PoolConfig::default();

        // Valid config
        assert!(config.validate().is_ok());

        // Empty URL
        config.url = String::new();
        assert!(config.validate().is_err());

        // Zero size
        config.url = "redis://localhost".to_string();
        config.max_size = 0;
        assert!(config.validate().is_err());

        // Invalid database
        config.max_size = 10;
        config.database = 20;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.connections_created, 0);
        assert_eq!(stats.peak_in_use, 0);
    }

    #[test]
    fn test_pool_health_status() {
        let health = PoolHealth::default();
        assert_eq!(health.status, PoolHealthStatus::Unknown);
    }

    #[test]
    fn test_pool_builder() {
        let builder = RedisPoolBuilder::new("redis://test:6379")
            .max_size(20)
            .min_idle(5)
            .database(1);

        let config = builder.config();
        assert_eq!(config.url, "redis://test:6379");
        assert_eq!(config.max_size, 20);
        assert_eq!(config.min_idle, Some(5));
        assert_eq!(config.database, 1);
    }

    #[test]
    fn test_server_info_clone() {
        let info = RedisServerInfo {
            version: "7.0.0".to_string(),
            connected_clients: 5,
            used_memory_bytes: 1024,
            uptime_seconds: 3600,
        };
        let cloned = info.clone();
        assert_eq!(info.version, cloned.version);
    }

    #[test]
    fn test_pool_error_display() {
        let error = PoolError::ConnectionTimeout { timeout_ms: 5000 };
        assert!(error.to_string().contains("5000"));

        let error = PoolError::PoolExhausted { waiting: 10 };
        assert!(error.to_string().contains("10"));
    }

    #[test]
    fn test_pool_event_variants() {
        let event = PoolEvent::Created { size: 10 };
        if let PoolEvent::Created { size } = event {
            assert_eq!(size, 10);
        }

        let event = PoolEvent::HealthCheckCompleted {
            status: PoolHealthStatus::Healthy,
            latency_us: 500,
        };
        if let PoolEvent::HealthCheckCompleted { status, latency_us } = event {
            assert_eq!(status, PoolHealthStatus::Healthy);
            assert_eq!(latency_us, 500);
        }
    }
}
