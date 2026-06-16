//! Production Test Helpers
//!
//! Common utilities for production readiness testing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use astra_flash::book::{OrderBook, OrderBookConfig, ThreadSafeOrderBook};
use astra_flash::core::types::{Exchange, Instrument, PriceLevel};
use rust_decimal::Decimal;

// ============================================================================
// TEST FIXTURES
// ============================================================================

/// Create a test instrument for production testing
pub fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-USD")
}

/// Create an order book with test data
pub fn test_orderbook() -> OrderBook {
    let mut book = OrderBook::new(test_instrument(), OrderBookConfig::default());
    let now = chrono::Utc::now().timestamp_micros();

    let bids: Vec<PriceLevel> = (0..10)
        .map(|i| PriceLevel::new(100.0 - i as f64, Decimal::new(10, 0), now))
        .collect();
    let asks: Vec<PriceLevel> = (0..10)
        .map(|i| PriceLevel::new(101.0 + i as f64, Decimal::new(10, 0), now))
        .collect();

    book.apply_snapshot(bids, asks, now);
    book
}

/// Create a thread-safe order book
pub fn thread_safe_orderbook() -> ThreadSafeOrderBook {
    ThreadSafeOrderBook::new(test_instrument(), OrderBookConfig::default())
}

// ============================================================================
// CONFIGURATION HELPERS
// ============================================================================

/// Mock configuration for testing
#[derive(Debug, Clone)]
pub struct MockConfig {
    pub exchange: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub redis_url: String,
    pub max_connections: usize,
    pub timeout_ms: u64,
    pub log_level: String,
    pub metrics_port: u16,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            exchange: "deribit".to_string(),
            api_key: None,
            api_secret: None,
            redis_url: "redis://localhost:6379".to_string(),
            max_connections: 10,
            timeout_ms: 5000,
            log_level: "info".to_string(),
            metrics_port: 9090,
        }
    }
}

impl MockConfig {
    /// Create a valid configuration
    pub fn valid() -> Self {
        Self::default()
    }

    /// Create an invalid configuration
    pub fn invalid() -> Self {
        Self {
            exchange: "".to_string(),                 // Invalid: empty
            redis_url: "not-a-valid-url".to_string(), // Invalid: bad URL
            max_connections: 0,                       // Invalid: zero connections
            timeout_ms: 0,                            // Invalid: zero timeout
            ..Default::default()
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.exchange.is_empty() {
            errors.push("Exchange cannot be empty".to_string());
        }

        if !self.redis_url.starts_with("redis://") {
            errors.push("Redis URL must start with redis://".to_string());
        }

        if self.max_connections == 0 {
            errors.push("Max connections must be > 0".to_string());
        }

        if self.timeout_ms == 0 {
            errors.push("Timeout must be > 0".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Apply environment variable overrides
    pub fn apply_env_overrides(&mut self, env: &HashMap<String, String>) {
        if let Some(val) = env.get("ASTRA_EXCHANGE") {
            self.exchange = val.clone();
        }
        if let Some(val) = env.get("ASTRA_REDIS_URL") {
            self.redis_url = val.clone();
        }
        if let Some(val) = env.get("ASTRA_LOG_LEVEL") {
            self.log_level = val.clone();
        }
        if let Some(val) = env.get("ASTRA_METRICS_PORT") {
            if let Ok(port) = val.parse() {
                self.metrics_port = port;
            }
        }
    }
}

// ============================================================================
// HEALTH CHECK HELPERS
// ============================================================================

/// Health status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Component health
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    pub name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
    pub last_check: Instant,
}

impl ComponentHealth {
    pub fn healthy(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Healthy,
            message: None,
            last_check: Instant::now(),
        }
    }

    pub fn degraded(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Degraded,
            message: Some(message.to_string()),
            last_check: Instant::now(),
        }
    }

    pub fn unhealthy(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Unhealthy,
            message: Some(message.to_string()),
            last_check: Instant::now(),
        }
    }
}

/// System health aggregator
pub struct HealthAggregator {
    pub components: Vec<ComponentHealth>,
}

impl HealthAggregator {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn add(&mut self, component: ComponentHealth) {
        self.components.push(component);
    }

    pub fn overall_status(&self) -> HealthStatus {
        if self
            .components
            .iter()
            .any(|c| c.status == HealthStatus::Unhealthy)
        {
            HealthStatus::Unhealthy
        } else if self
            .components
            .iter()
            .any(|c| c.status == HealthStatus::Degraded)
        {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.overall_status() == HealthStatus::Healthy
    }
}

impl Default for HealthAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// METRICS HELPERS
// ============================================================================

/// Mock counter metric
#[derive(Debug)]
pub struct MockCounter {
    pub name: String,
    pub value: AtomicU64,
    pub labels: HashMap<String, String>,
}

impl MockCounter {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            value: AtomicU64::new(0),
            labels: HashMap::new(),
        }
    }

    pub fn with_labels(name: &str, labels: HashMap<String, String>) -> Self {
        Self {
            name: name.to_string(),
            value: AtomicU64::new(0),
            labels,
        }
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add(&self, v: u64) {
        self.value.fetch_add(v, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Mock gauge metric
#[derive(Debug)]
pub struct MockGauge {
    pub name: String,
    pub value: AtomicU64,
}

impl MockGauge {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            value: AtomicU64::new(0),
        }
    }

    pub fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Mock histogram metric
#[derive(Debug)]
pub struct MockHistogram {
    pub name: String,
    pub values: std::sync::Mutex<Vec<f64>>,
    pub buckets: Vec<f64>,
}

impl MockHistogram {
    pub fn new(name: &str, buckets: Vec<f64>) -> Self {
        Self {
            name: name.to_string(),
            values: std::sync::Mutex::new(Vec::new()),
            buckets,
        }
    }

    pub fn observe(&self, v: f64) {
        self.values.lock().unwrap().push(v);
    }

    pub fn count(&self) -> usize {
        self.values.lock().unwrap().len()
    }

    pub fn sum(&self) -> f64 {
        self.values.lock().unwrap().iter().sum()
    }
}

// ============================================================================
// LOGGING HELPERS
// ============================================================================

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

/// Mock log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub context: HashMap<String, String>,
    pub timestamp: Instant,
}

impl LogEntry {
    pub fn new(level: LogLevel, message: &str) -> Self {
        Self {
            level,
            message: message.to_string(),
            context: HashMap::new(),
            timestamp: Instant::now(),
        }
    }

    pub fn with_context(mut self, key: &str, value: &str) -> Self {
        self.context.insert(key.to_string(), value.to_string());
        self
    }

    pub fn is_structured(&self) -> bool {
        !self.context.is_empty()
    }

    pub fn contains_sensitive(&self) -> bool {
        let sensitive_patterns = ["password", "secret", "token", "api_key", "credential"];
        let msg_lower = self.message.to_lowercase();

        for pattern in &sensitive_patterns {
            if msg_lower.contains(pattern) {
                // Check if it's masked
                if !msg_lower.contains("***") && !msg_lower.contains("[redacted]") {
                    return true;
                }
            }
        }
        false
    }
}

/// Mock logger
pub struct MockLogger {
    pub entries: std::sync::Mutex<Vec<LogEntry>>,
    pub min_level: LogLevel,
}

impl MockLogger {
    pub fn new(min_level: LogLevel) -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
            min_level,
        }
    }

    pub fn log(&self, entry: LogEntry) {
        if entry.level >= self.min_level {
            self.entries.lock().unwrap().push(entry);
        }
    }

    pub fn get_entries(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    pub fn count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

// ============================================================================
// SECURITY HELPERS
// ============================================================================

/// Mask sensitive data
pub fn mask_sensitive(input: &str) -> String {
    if input.len() <= 4 {
        "***".to_string()
    } else {
        format!("{}***", &input[..4])
    }
}

/// Check for injection patterns
pub fn contains_injection(input: &str) -> bool {
    let patterns = [
        "<script",
        "javascript:",
        "onclick",
        "onerror", // XSS
        "'; DROP",
        "\" OR ",
        "1=1",
        "--", // SQL injection
        "$(",
        "`",
        "&&",
        "||",
        ";", // Command injection
    ];

    let input_lower = input.to_lowercase();
    patterns
        .iter()
        .any(|p| input_lower.contains(&p.to_lowercase()))
}

/// Sanitize input
pub fn sanitize_input(input: &str) -> String {
    input
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

// ============================================================================
// DEPLOYMENT HELPERS
// ============================================================================

/// Startup phase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupPhase {
    Initializing,
    LoadingConfig,
    ConnectingDependencies,
    StartingServices,
    Ready,
}

/// Shutdown signal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    Sigterm,
    Sigint,
    Graceful,
}

/// Mock service for deployment testing
pub struct MockService {
    pub name: String,
    pub started: AtomicBool,
    pub phase: std::sync::Mutex<StartupPhase>,
    pub shutdown_complete: AtomicBool,
}

impl MockService {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            started: AtomicBool::new(false),
            phase: std::sync::Mutex::new(StartupPhase::Initializing),
            shutdown_complete: AtomicBool::new(false),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        *self.phase.lock().unwrap() = StartupPhase::LoadingConfig;
        *self.phase.lock().unwrap() = StartupPhase::ConnectingDependencies;
        *self.phase.lock().unwrap() = StartupPhase::StartingServices;
        *self.phase.lock().unwrap() = StartupPhase::Ready;
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn shutdown(&self, _signal: ShutdownSignal) -> Result<(), String> {
        self.started.store(false, Ordering::SeqCst);
        *self.phase.lock().unwrap() = StartupPhase::Initializing;
        self.shutdown_complete.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.started.load(Ordering::SeqCst) && *self.phase.lock().unwrap() == StartupPhase::Ready
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown_complete.load(Ordering::SeqCst)
    }
}

// ============================================================================
// RECOVERY HELPERS
// ============================================================================

/// State for recovery testing
#[derive(Debug, Clone)]
pub struct RecoverableState {
    pub data: HashMap<String, String>,
    pub checksum: String,
}

impl RecoverableState {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            checksum: String::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_string(), value.to_string());
        self.update_checksum();
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    fn update_checksum(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for (k, v) in &self.data {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        self.checksum = format!("{:016x}", hasher.finish());
    }

    pub fn verify_checksum(&self) -> bool {
        let mut test = self.clone();
        test.update_checksum();
        test.checksum == self.checksum
    }

    pub fn serialize(&self) -> String {
        serde_json::to_string(&self.data).unwrap_or_default()
    }

    pub fn deserialize(s: &str) -> Result<Self, String> {
        let data: HashMap<String, String> = serde_json::from_str(s).map_err(|e| e.to_string())?;
        let mut state = Self {
            data,
            checksum: String::new(),
        };
        state.update_checksum();
        Ok(state)
    }
}

impl Default for RecoverableState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VALIDATION HELPERS
// ============================================================================

/// Validate order book is consistent
pub fn validate_orderbook(book: &OrderBook) -> bool {
    // Check bid-ask ordering
    if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
        if bid.price >= ask.price {
            return false;
        }
    }
    true
}

// ============================================================================
// TIMING HELPERS
// ============================================================================

/// Measure operation duration
pub fn measure_duration<F, R>(f: F) -> (R, Duration)
where
    F: FnOnce() -> R,
{
    let start = Instant::now();
    let result = f();
    (result, start.elapsed())
}

/// Rate limiter for testing
pub struct RateLimiter {
    pub max_per_second: u64,
    pub current_count: AtomicU64,
    pub window_start: std::sync::Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(max_per_second: u64) -> Self {
        Self {
            max_per_second,
            current_count: AtomicU64::new(0),
            window_start: std::sync::Mutex::new(Instant::now()),
        }
    }

    pub fn try_acquire(&self) -> bool {
        let mut window_start = self.window_start.lock().unwrap();

        // Reset window if expired
        if window_start.elapsed() >= Duration::from_secs(1) {
            *window_start = Instant::now();
            self.current_count.store(0, Ordering::SeqCst);
        }

        // Check if under limit
        let current = self.current_count.fetch_add(1, Ordering::SeqCst);
        current < self.max_per_second
    }

    pub fn current_rate(&self) -> u64 {
        self.current_count.load(Ordering::SeqCst)
    }
}
