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
//! This module is that logic, extracted so the tests in
//! `tests/network/oanda_stream_test.rs` can exercise it directly.
//!
//! [`Connector`]: crate::network::Connector
//! [`HeartbeatManager`]: crate::network::HeartbeatManager
//! [`ReconnectionManager`]: crate::network::ReconnectionManager

use std::time::Duration;

use futures_util::{Stream, StreamExt};

use crate::core::types::{Exchange, Instrument};

/// Why [`consume_ndjson_stream`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamExit {
    /// The peer closed the stream cleanly.
    Eof,
    /// The transport reported an error.
    Error(String),
}

/// Drain a newline-delimited JSON byte stream, calling `on_line` for each
/// complete, non-empty, trimmed line.
///
/// Chunks may split lines at arbitrary byte positions; partial lines are
/// buffered until their newline arrives.
pub async fn consume_ndjson_stream<S, B, E>(
    mut stream: S,
    mut on_line: impl FnMut(&str),
) -> StreamExit
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
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

    StreamExit::Eof
}

/// Retry policy for the OANDA stream task.
///
/// Call [`begin_attempt`](Self::begin_attempt) at the top of each connection
/// attempt, [`on_connected`](Self::on_connected) when the server accepts the
/// stream, and [`backoff_delay`](Self::backoff_delay) before retrying after
/// the stream ends or the connect fails.
#[derive(Debug, Clone)]
pub struct OandaReconnectPolicy {
    reconnect_count: u32,
    max_reconnects: u32,
}

impl OandaReconnectPolicy {
    /// Create a policy that gives up after `max_reconnects` attempts.
    #[must_use]
    pub const fn new(max_reconnects: u32) -> Self {
        Self {
            reconnect_count: 0,
            max_reconnects,
        }
    }

    /// Register the start of a connection attempt.
    ///
    /// Returns the 1-based attempt number, or `None` once the maximum has been
    /// exceeded (the caller should stop).
    pub fn begin_attempt(&mut self) -> Option<u32> {
        self.reconnect_count += 1;
        if self.reconnect_count > self.max_reconnects {
            None
        } else {
            Some(self.reconnect_count)
        }
    }

    /// Register that the server accepted the stream.
    pub fn on_connected(&mut self) {
        self.reconnect_count = 0;
    }

    /// Delay to wait before the next attempt after the stream ended or the
    /// connect failed.
    #[must_use]
    pub fn backoff_delay(&self) -> Duration {
        Duration::from_millis(1000 * u64::from(self.reconnect_count.min(30)))
    }

    /// Delay to wait after the server answered with a non-2xx status.
    #[must_use]
    pub const fn http_failure_delay(&self) -> Duration {
        Duration::from_secs(5)
    }

    /// Current attempt counter (for logging).
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.reconnect_count
    }
}

/// Build the [`Instrument`] the binary uses for an OANDA instrument name such
/// as `"EUR_USD"`.
#[must_use]
pub fn oanda_instrument(name: &str) -> Instrument {
    Instrument::new(name, "", Exchange::Oanda, name)
}
