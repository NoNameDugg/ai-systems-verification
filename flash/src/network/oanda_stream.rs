//! OANDA HTTP-streaming consumption and reconnect policy.
//!
//! The binary (`src/main.rs`) talks to OANDA over HTTP chunked streaming
//! (newline-delimited JSON), not WebSocket, so it cannot use [`Connector`],
//! [`HeartbeatManager`] or [`ReconnectionManager`]. Until v1.2 the stream loop
//! and the retry counter lived as private code inside the binary, where no test
//! could reach them. An external review (2026-10) found two defects there:
//!
//! * a silent connection was never detected (no idle timeout), and
//! * the retry counter was reset to zero on every successful connect and then
//!   used as the delay multiplier, so a connection that was accepted and then
//!   dropped was retried with **zero** delay, forever.
//!
//! This module is that logic, fixed and tested in
//! `tests/network/oanda_stream_test.rs`:
//!
//! * [`consume_ndjson_stream`] abandons a stream that delivers nothing for
//!   `idle_timeout` and reports [`StreamExit::IdleTimeout`]. OANDA sends a
//!   HEARTBEAT line roughly every 5 s, so any live stream resets the timer;
//!   [`idle_timeout_for`] floors the configured `read_timeout` at three
//!   heartbeat intervals so a single late heartbeat cannot cause a reconnect.
//! * [`OandaReconnectPolicy`] delegates delays to the crate's existing
//!   [`ReconnectionConfig::calculate_delay`] (exponential, capped, jittered)
//!   and resets the attempt counter only once the connection has delivered
//!   data, so accept-then-drop still backs off.
//!
//! [`Connector`]: crate::network::Connector
//! [`HeartbeatManager`]: crate::network::HeartbeatManager
//! [`ReconnectionManager`]: crate::network::ReconnectionManager
//! [`ReconnectionConfig::calculate_delay`]: crate::network::ReconnectionConfig::calculate_delay

use std::time::Duration;

use futures_util::{Stream, StreamExt};

use crate::core::config::WebSocketConfig;
use crate::core::types::{Exchange, Instrument};
use crate::network::reconnect::ReconnectionConfig;

/// Interval at which OANDA's pricing stream emits `HEARTBEAT` lines.
pub const OANDA_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Idle timeout to apply to the OANDA stream for a configured `read_timeout`.
///
/// Floors the value at three heartbeat intervals: with heartbeats every ~5 s a
/// 5 s timeout (the config default) would reconnect on any single late
/// heartbeat.
#[must_use]
pub fn idle_timeout_for(read_timeout: Duration) -> Duration {
    read_timeout.max(OANDA_HEARTBEAT_INTERVAL * 3)
}

/// Why [`consume_ndjson_stream`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamExit {
    /// The peer closed the stream cleanly.
    Eof,
    /// The transport reported an error.
    Error(String),
    /// Nothing arrived for the idle timeout; the connection is presumed dead.
    IdleTimeout,
}

/// Drain a newline-delimited JSON byte stream, calling `on_line` for each
/// complete, non-empty, trimmed line.
///
/// Chunks may split lines at arbitrary byte positions; partial lines are
/// buffered until their newline arrives. Any chunk, including a heartbeat,
/// resets the idle timer; if `idle_timeout` passes with no chunk the stream is
/// abandoned with [`StreamExit::IdleTimeout`].
pub async fn consume_ndjson_stream<S, B, E>(
    mut stream: S,
    idle_timeout: Duration,
    mut on_line: impl FnMut(&str),
) -> StreamExit
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut buffer = String::new();

    loop {
        let next = match tokio::time::timeout(idle_timeout, stream.next()).await {
            Err(_elapsed) => return StreamExit::IdleTimeout,
            Ok(None) => return StreamExit::Eof,
            Ok(Some(item)) => item,
        };

        match next {
            Ok(chunk) => {
                if let Ok(text) = std::str::from_utf8(chunk.as_ref()) {
                    buffer.push_str(text);

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }
                        on_line(&line);
                    }
                }
            }
            Err(e) => return StreamExit::Error(e.to_string()),
        }
    }
}

/// Retry policy for the OANDA stream task.
///
/// Call [`begin_attempt`](Self::begin_attempt) at the top of each connection
/// attempt, [`on_data`](Self::on_data) when the stream delivers a line (this
/// is what proves the connection good and resets the attempt counter), and
/// [`backoff_delay`](Self::backoff_delay) before retrying after the stream
/// ends, the connect fails, or the server answers with a non-2xx status.
#[derive(Debug, Clone)]
pub struct OandaReconnectPolicy {
    attempt: u32,
    max_attempts: u32,
    backoff: ReconnectionConfig,
}

impl OandaReconnectPolicy {
    /// Build the policy from the `websocket` section of the config:
    /// `reconnect_delay_ms` (initial), `max_reconnect_delay_ms` (cap),
    /// `reconnect_jitter`, and `max_reconnect_attempts` (0 = unlimited).
    #[must_use]
    pub fn new(ws: &WebSocketConfig) -> Self {
        Self {
            attempt: 0,
            max_attempts: ws.max_reconnect_attempts,
            backoff: ReconnectionConfig {
                initial_delay_ms: ws.reconnect_delay_ms,
                max_delay_ms: ws.max_reconnect_delay_ms,
                backoff_multiplier: 2.0,
                jitter_percent: ws.reconnect_jitter.clamp(0.0, 0.5),
                max_retries: ws.max_reconnect_attempts,
                ..ReconnectionConfig::default()
            },
        }
    }

    /// Register the start of a connection attempt.
    ///
    /// Returns the 1-based attempt number, or `None` once the maximum has been
    /// exceeded (the caller should stop). A maximum of 0 means unlimited.
    pub fn begin_attempt(&mut self) -> Option<u32> {
        self.attempt = self.attempt.saturating_add(1);
        if self.max_attempts != 0 && self.attempt > self.max_attempts {
            None
        } else {
            Some(self.attempt)
        }
    }

    /// Register that the server accepted the stream.
    ///
    /// Deliberately does **not** reset the attempt counter: a server that
    /// accepts and immediately drops the connection must still back off.
    pub fn on_connected(&mut self) {}

    /// Register that the stream delivered data. The connection is good, so
    /// the attempt counter restarts from zero.
    pub fn on_data(&mut self) {
        self.attempt = 0;
    }

    /// Delay before the next attempt. Never zero: at least the configured
    /// initial delay, doubling per consecutive failed attempt up to the cap.
    #[must_use]
    pub fn backoff_delay(&self) -> Duration {
        self.backoff.calculate_delay(self.attempt.max(1))
    }

    /// Current attempt counter (for logging).
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// Build the [`Instrument`] for an OANDA instrument name such as `"EUR_USD"`.
///
/// OANDA names are `BASE_QUOTE`; the raw symbol is kept verbatim. (Before v1.2
/// the binary used the whole name as the base and an empty quote, so the
/// published symbol and Redis key read `EUR_USD_`.)
#[must_use]
pub fn oanda_instrument(name: &str) -> Instrument {
    match name.split_once('_') {
        Some((base, quote)) if !base.is_empty() && !quote.is_empty() => {
            Instrument::new(base, quote, Exchange::Oanda, name)
        }
        _ => Instrument::new(name, "", Exchange::Oanda, name),
    }
}
