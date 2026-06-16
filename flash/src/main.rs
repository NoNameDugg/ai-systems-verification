//! Flash - Binary Entry Point
//!
//! This is the main entry point for the Flash binary.
//!
//! # Usage
//!
//! ```bash
//! # Run with default config
//! astra-flash
//!
//! # Run with custom config (environment overlay)
//! ASTRA_FLASH_ENV=prod astra-flash
//!
//! # Show version
//! astra-flash --version
//! ```

use astra_flash::book::{BookSnapshot, OrderBook};
use astra_flash::prelude::*;
use astra_flash::publisher::{DualPublisher, DualPublisherConfig, PoolConfig, RedisPool};
use futures_util::StreamExt;
use parking_lot::RwLock;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use astra_flash::core::metrics::{FLASH_BACKPRESSURE_STATUS, FLASH_MESSAGES_DROPPED_TOTAL};
use metrics::{counter, gauge};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

/// Default configuration file path.
const DEFAULT_CONFIG_PATH: &str = "config/flash.yaml";

/// Channel buffer size for the publishing pipeline.
const PUBLISH_CHANNEL_SIZE: usize = 10000;

/// Shared order books for all instruments.
type SharedOrderBooks = Arc<RwLock<HashMap<String, OrderBook>>>;

/// Main entry point for Flash.
///
/// Initializes logging, loads configuration, and starts the Flash engine.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    info!("==========================================================");
    info!("  Flash Starting...");
    info!("  Version: {}", astra_flash::VERSION);
    info!("  Target: <0.1ms P99 Latency");
    info!("==========================================================");
    info!("Mission: Be faster than the garbage collector. Never drop a tick.");

    // Load configuration
    let environment = std::env::var("ASTRA_FLASH_ENV").ok();
    let config_path =
        std::env::var("ASTRA_FLASH_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

    info!("Loading configuration from: {}", config_path);
    if let Some(ref env) = environment {
        info!("Environment overlay: {}", env);
    }

    let config = if Path::new(&config_path).exists() {
        match FlashConfig::load(&config_path, environment.as_deref()) {
            Ok(cfg) => {
                info!("Configuration loaded successfully");
                cfg
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                warn!("Using default configuration");
                FlashConfig::default()
            }
        }
    } else {
        warn!("Configuration file not found: {}", config_path);
        warn!("Using default configuration");
        FlashConfig::default()
    };

    // Log configuration summary
    log_config_summary(&config);

    // =========================================================================
    // PHASE 4: Initialize Redis Publisher
    // =========================================================================
    info!("Initializing Redis connection pool...");

    let pool_config = PoolConfig {
        url: config.redis.url.clone(),
        max_size: config.redis.pool_size as usize,
        database: config.redis.database,
        connect_timeout_ms: config.redis.connect_timeout_ms,
        ..PoolConfig::default()
    };

    let redis_pool = match RedisPool::new(pool_config).await {
        Ok(pool) => {
            info!("Redis pool created successfully");
            Arc::new(pool)
        }
        Err(e) => {
            error!("Failed to create Redis pool: {}", e);
            return Err(anyhow::anyhow!("Redis pool creation failed: {}", e));
        }
    };

    // Verify Redis connection
    match redis_pool.health_check().await {
        Ok(_) => info!("Redis health check: PASSED"),
        Err(e) => {
            error!("Redis health check failed: {}", e);
            return Err(anyhow::anyhow!("Redis not available: {}", e));
        }
    }

    // Create DualPublisher
    let publisher_config = DualPublisherConfig::default();
    let publisher = Arc::new(DualPublisher::new(Arc::clone(&redis_pool), publisher_config));
    info!("DualPublisher initialized");

    FlashMetrics::init_prometheus_exporter(9090)?;
    info!("Prometheus metrics endpoint initialized on port 9090");

    // =========================================================================
    // PHASE 3: Initialize Order Books
    // =========================================================================
    info!("Initializing order books...");

    let order_books: SharedOrderBooks = Arc::new(RwLock::new(HashMap::new()));

    // Pre-create order books for OANDA instruments
    if config.exchanges.oanda.enabled {
        let mut books = order_books.write();
        for instrument_name in &config.exchanges.oanda.instruments {
            let instrument = Instrument::new(
                instrument_name,
                "",
                Exchange::Oanda,
                instrument_name,
            );
            books.insert(instrument_name.clone(), OrderBook::with_instrument(instrument));
            info!("  Order book created: {}", instrument_name);
        }
    }

    // =========================================================================
    // PHASE 2: Initialize Exchange Connections
    // =========================================================================

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Create publishing channel
    let (publish_tx, publish_rx) = mpsc::channel::<BookSnapshot>(PUBLISH_CHANNEL_SIZE);

    // Spawn publisher task
    let publisher_clone = Arc::clone(&publisher);
    let publisher_handle = tokio::spawn(async move {
        run_publisher_task(publisher_clone, publish_rx).await;
    });

    // Spawn OANDA streaming task if enabled
    let oanda_handle = if config.exchanges.oanda.enabled {
        let oanda_config = config.exchanges.oanda.clone();
        let books = Arc::clone(&order_books);
        let tx = publish_tx.clone();

        Some(tokio::spawn(async move {
            run_oanda_stream(oanda_config, books, tx).await;
        }))
    } else {
        info!("OANDA: DISABLED");
        None
    };

    // =========================================================================
    // PHASE 5: Main Loop - Wait for Shutdown
    // =========================================================================

    info!("==========================================================");
    info!("  Flash ONLINE");
    info!("  Press Ctrl+C to shutdown...");
    info!("==========================================================");

    // Graceful shutdown on Ctrl+C
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutdown signal received, initiating graceful shutdown...");
        }
        _ = shutdown_rx.recv() => {
            info!("Internal shutdown triggered...");
        }
    }

    // Signal shutdown
    drop(shutdown_tx);
    drop(publish_tx);

    // Wait for tasks to complete with timeout
    info!("Waiting for tasks to complete...");

    let shutdown_timeout = Duration::from_secs(5);

    if let Some(handle) = oanda_handle {
        let _ = tokio::time::timeout(shutdown_timeout, handle).await;
    }

    let _ = tokio::time::timeout(shutdown_timeout, publisher_handle).await;

    info!("Flash shutdown complete.");

    Ok(())
}

/// Run the publisher task that receives snapshots and publishes to Redis.
async fn run_publisher_task(
    publisher: Arc<DualPublisher>,
    mut rx: mpsc::Receiver<BookSnapshot>,
) {
    info!("Publisher task started");

    let mut publish_count: u64 = 0;
    let mut error_count: u64 = 0;

    while let Some(snapshot) = rx.recv().await {
        match publisher.publish_dual(&snapshot).await {
            Ok(result) => {
                publish_count += 1;
                if publish_count % 1000 == 0 {
                    debug!(
                        "Published {} snapshots (latency: {}μs)",
                        publish_count,
                        result.latency_us
                    );
                }
            }
            Err(e) => {
                error_count += 1;
                if error_count <= 10 {
                    warn!("Publish error: {:?}", e);
                }
            }
        }
    }

    info!(
        "Publisher task stopped. Total published: {}, errors: {}",
        publish_count, error_count
    );
}

/// Run the OANDA HTTP streaming task.
///
/// OANDA uses HTTP streaming (Server-Sent Events), not WebSockets.
/// The streaming URL format: https://stream-fxpractice.oanda.com/v3/accounts/{accountID}/pricing/stream?instruments=EUR_USD,GBP_USD
async fn run_oanda_stream(
    config: astra_flash::core::config::ExchangeConfig,
    order_books: SharedOrderBooks,
    publish_tx: mpsc::Sender<BookSnapshot>,
) {
    info!("OANDA streaming task starting...");

    let account_id = match &config.account_id {
        Some(id) => id.clone(),
        None => {
            error!("OANDA account_id not configured");
            return;
        }
    };

    let api_key = match &config.api_key {
        Some(key) => key.clone(),
        None => {
            error!("OANDA api_key not configured");
            return;
        }
    };

    // Build streaming URL
    // Base URL should be HTTPS, not WSS
    let base_url = config
        .ws_url
        .replace("wss://", "https://")
        .replace("ws://", "http://");

    let instruments_param = config.instruments.join(",");
    let stream_url = format!(
        "{}/{}/pricing/stream?instruments={}",
        base_url, account_id, instruments_param
    );

    info!("OANDA Stream URL: {}", stream_url);
    info!("OANDA Instruments: {:?}", config.instruments);

    // Create HTTP client for streaming (no request timeout, but with connect timeout)
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    // Reconnection loop
    let mut reconnect_count = 0;
    let max_reconnects = 100;

    loop {
        reconnect_count += 1;
        if reconnect_count > max_reconnects {
            error!("Max reconnection attempts ({}) exceeded", max_reconnects);
            break;
        }

        info!(
            "Connecting to OANDA stream (attempt {})...",
            reconnect_count
        );

        let response = client
            .get(&stream_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await;

        match response {
            Ok(resp) => {
                if !resp.status().is_success() {
                    error!("OANDA connection failed: HTTP {}", resp.status());
                    let body = resp.text().await.unwrap_or_default();
                    error!("Response: {}", body);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                info!("OANDA stream connected!");
                reconnect_count = 0; // Reset on successful connection

                // Process the streaming response
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            // Append chunk to buffer
                            if let Ok(text) = std::str::from_utf8(&chunk) {
                                buffer.push_str(text);

                                // Process complete JSON lines
                                while let Some(newline_pos) = buffer.find('\n') {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 1..].to_string();

                                    if line.is_empty() {
                                        continue;
                                    }

                                    // Parse and process the message
                                    if let Err(e) = process_oanda_message(
                                        &line,
                                        &order_books,
                                        &publish_tx,
                                    )
                                    .await
                                    {
                                        debug!("Message processing error: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Stream error: {}", e);
                            break;
                        }
                    }
                }

                warn!("OANDA stream disconnected, reconnecting...");
            }
            Err(e) => {
                error!("OANDA connection error: {}", e);
            }
        }

        // Exponential backoff
        let delay = Duration::from_millis(1000 * reconnect_count.min(30) as u64);
        info!("Reconnecting in {:?}...", delay);
        tokio::time::sleep(delay).await;
    }

    info!("OANDA streaming task stopped");
}

/// Process a single OANDA message (PRICE or HEARTBEAT).
async fn process_oanda_message(
    json_line: &str,
    order_books: &SharedOrderBooks,
    publish_tx: &mpsc::Sender<BookSnapshot>,
) -> Result<()> {
    // Quick type detection
    if json_line.contains("\"type\":\"HEARTBEAT\"") || json_line.contains("\"type\": \"HEARTBEAT\"")
    {
        debug!("OANDA heartbeat received");
        return Ok(());
    }

    if !json_line.contains("\"type\":\"PRICE\"") && !json_line.contains("\"type\": \"PRICE\"") {
        return Ok(()); // Unknown message type
    }

    // Parse PRICE message
    let price: serde_json::Value = serde_json::from_str(json_line)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))?;

    let instrument_str = price
        .get("instrument")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing instrument"))?;

    let timestamp = now_micros();

    // Parse bids into PriceLevels
    let bids: Vec<PriceLevel> = price
        .get("bids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|level| {
                    let price_str = level.get("price")?.as_str()?;
                    let liquidity = level.get("liquidity")?.as_i64()?;
                    let price_f64 = price_str.parse::<f64>().ok()?;
                    let qty = Decimal::from_i64(liquidity).unwrap_or(Decimal::ZERO);
                    Some(PriceLevel::new(price_f64, qty, timestamp))
                })
                .collect()
        })
        .unwrap_or_default();

    // Parse asks into PriceLevels
    let asks: Vec<PriceLevel> = price
        .get("asks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|level| {
                    let price_str = level.get("price")?.as_str()?;
                    let liquidity = level.get("liquidity")?.as_i64()?;
                    let price_f64 = price_str.parse::<f64>().ok()?;
                    let qty = Decimal::from_i64(liquidity).unwrap_or(Decimal::ZERO);
                    Some(PriceLevel::new(price_f64, qty, timestamp))
                })
                .collect()
        })
        .unwrap_or_default();

    // Update order book
    {
        let mut books = order_books.write();
        if let Some(book) = books.get_mut(instrument_str) {
            // Apply as snapshot
            book.apply_snapshot(bids.clone(), asks.clone(), timestamp);
        }
    }

    // Create BookSnapshot for publishing
    let instrument = Instrument::new(
        instrument_str,
        "",
        Exchange::Oanda,
        instrument_str,
    );

    let snapshot = BookSnapshot {
        instrument,
        timestamp,
        bids,
        asks,
    };

    // Send to publisher (non-blocking)
    if publish_tx.try_send(snapshot).is_err() {
        gauge!(FLASH_BACKPRESSURE_STATUS).set(1.0);
        counter!(FLASH_MESSAGES_DROPPED_TOTAL).increment(1);
        debug!("Publisher backpressure - dropping message");
    } else {
        gauge!(FLASH_BACKPRESSURE_STATUS).set(0.0);
    }

    Ok(())
}

/// Log a summary of the loaded configuration.
fn log_config_summary(config: &FlashConfig) {
    info!("----------------------------------------------------------");
    info!("  Configuration Summary");
    info!("----------------------------------------------------------");

    // OANDA Configuration
    if config.exchanges.oanda.enabled {
        info!("  OANDA: ENABLED");
        info!("    - Stream URL: {}", config.exchanges.oanda.ws_url);
        info!(
            "    - Account ID: {}",
            config
                .exchanges
                .oanda
                .account_id
                .as_deref()
                .unwrap_or("Not Set")
        );
        info!(
            "    - Instruments: {:?}",
            config.exchanges.oanda.instruments
        );
        info!(
            "    - Rate Limit: {}/sec",
            config.exchanges.oanda.rate_limit_per_second
        );
    } else {
        info!("  OANDA: DISABLED");
    }

    // Redis Configuration
    info!("  Redis:");
    info!("    - URL: {}", config.redis.url);
    info!("    - Pool Size: {}", config.redis.pool_size);
    info!("    - Database: {}", config.redis.database);

    // Performance Configuration
    info!("  Performance:");
    info!(
        "    - Channel Buffer: {}",
        config.publisher.backpressure.capacity
    );
    info!(
        "    - Backpressure Policy: {:?}",
        config.publisher.backpressure.action
    );
    info!("    - Serialization Format: {:?}", config.publisher.format);

    // WebSocket Configuration
    info!("  WebSocket:");
    info!(
        "    - Connect Timeout: {}ms",
        config.websocket.connect_timeout_ms
    );
    info!(
        "    - Max Reconnect Attempts: {}",
        config.websocket.max_reconnect_attempts
    );

    // Publisher Configuration
    info!("  Publisher:");
    info!("    - Topic Prefix: {}", config.publisher.topic_prefix);

    info!("----------------------------------------------------------");
}
