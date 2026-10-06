//! External review (2026-10) reproductions — OANDA stream loop (findings #15, #16)
//!
//! The loop and retry counter that the binary runs were private to `main.rs`
//! until v1.2; they are now `astra_flash::network::oanda_stream` so they can be
//! tested here. The review reproductions were committed `#[ignore]`d-and-failing
//! in 1078a65 and flipped to plain tests by the fix commit.

use std::time::Duration;

use astra_flash::core::config::WebSocketConfig;
use astra_flash::network::oanda_stream::{
    consume_ndjson_stream, idle_timeout_for, oanda_instrument, OandaReconnectPolicy, StreamExit,
    OANDA_HEARTBEAT_INTERVAL,
};
use futures_util::stream;

const IDLE: Duration = Duration::from_secs(15);

/// Deterministic policy config: no jitter, 1 s initial, 30 s cap.
fn ws(max_attempts: u32) -> WebSocketConfig {
    WebSocketConfig {
        reconnect_delay_ms: 1000,
        max_reconnect_delay_ms: 30_000,
        reconnect_jitter: 0.0,
        max_reconnect_attempts: max_attempts,
        ..WebSocketConfig::default()
    }
}

type Chunk = Result<Vec<u8>, std::io::Error>;

fn chunks(parts: &[&str]) -> Vec<Chunk> {
    parts.iter().map(|p| Ok(p.as_bytes().to_vec())).collect()
}

// =============================================================================
// Sanity: the extraction behaves like the loop it replaced
// =============================================================================

#[tokio::test]
async fn ndjson_lines_are_delivered_across_chunk_boundaries() {
    let s = stream::iter(chunks(&[
        "{\"type\":\"HEART",
        "BEAT\"}\n{\"type\":\"PRICE\",\"instrument\":\"EUR_USD\"}\n\n   \n{\"type\":\"PRI",
        "CE\",\"instrument\":\"GBP_USD\"}\n",
    ]));
    let mut lines = Vec::new();
    let exit = consume_ndjson_stream(s, IDLE, |l| lines.push(l.to_string())).await;

    assert_eq!(exit, StreamExit::Eof);
    assert_eq!(
        lines,
        vec![
            "{\"type\":\"HEARTBEAT\"}",
            "{\"type\":\"PRICE\",\"instrument\":\"EUR_USD\"}",
            "{\"type\":\"PRICE\",\"instrument\":\"GBP_USD\"}",
        ]
    );
}

#[tokio::test]
async fn transport_error_ends_the_stream_with_error() {
    let items: Vec<Chunk> = vec![
        Ok(b"{\"type\":\"HEARTBEAT\"}\n".to_vec()),
        Err(std::io::Error::other("connection reset")),
    ];
    let mut n = 0;
    let exit = consume_ndjson_stream(stream::iter(items), IDLE, |_| n += 1).await;
    assert_eq!(n, 1);
    assert!(matches!(exit, StreamExit::Error(e) if e.contains("connection reset")));
}

#[test]
fn policy_gives_up_after_max_attempts() {
    let mut p = OandaReconnectPolicy::new(&ws(3));
    assert_eq!(p.begin_attempt(), Some(1));
    assert_eq!(p.begin_attempt(), Some(2));
    assert_eq!(p.begin_attempt(), Some(3));
    assert_eq!(p.begin_attempt(), None);
}

#[test]
fn policy_zero_max_attempts_means_unlimited() {
    let mut p = OandaReconnectPolicy::new(&ws(0));
    for _ in 0..1000 {
        assert!(p.begin_attempt().is_some());
    }
}

#[test]
fn idle_timeout_is_floored_at_three_heartbeats() {
    // the config default (5 s) equals one heartbeat period and would reconnect
    // on any single late heartbeat
    assert_eq!(
        idle_timeout_for(Duration::from_secs(5)),
        OANDA_HEARTBEAT_INTERVAL * 3
    );
    assert_eq!(
        idle_timeout_for(Duration::from_secs(30)),
        Duration::from_secs(30)
    );
}

// =============================================================================
// #15 — a silent connection must be detected and the loop must give up on it
// =============================================================================

/// A peer that has gone silent (TCP half-open, stalled proxy) never yields and
/// never errors. OANDA heartbeats every ~5 s, and `WebSocketConfig.read_timeout_ms`
/// documents "force reconnect if no data for 5s" — but the loop had no timeout.
#[tokio::test(start_paused = true)]
async fn silent_stream_terminates_within_the_idle_budget() {
    let silent = stream::pending::<Chunk>();
    let budget = Duration::from_secs(120); // far more than any sane idle timeout

    let outcome = tokio::time::timeout(budget, consume_ndjson_stream(silent, IDLE, |_| ())).await;

    assert_eq!(
        outcome.ok(),
        Some(StreamExit::IdleTimeout),
        "a stream that delivers nothing for {:?} must be abandoned so the caller can reconnect",
        budget
    );
}

/// Heartbeats arriving well inside the idle timeout keep the stream alive.
#[tokio::test(start_paused = true)]
async fn heartbeats_reset_the_idle_timer() {
    let heartbeats = stream::unfold(0u32, |i| async move {
        if i == 6 {
            return None;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        let chunk: Chunk = Ok(b"{\"type\":\"HEARTBEAT\"}\n".to_vec());
        Some((chunk, i + 1))
    });
    let mut n = 0;
    let exit = consume_ndjson_stream(Box::pin(heartbeats), IDLE, |_| n += 1).await;
    assert_eq!(
        exit,
        StreamExit::Eof,
        "30 s of 5 s heartbeats must not trip a 15 s idle timeout"
    );
    assert_eq!(n, 6);
}

// =============================================================================
// #16 — reconnect delay must never collapse to zero, and must grow
// =============================================================================

/// The production failure mode: the server accepts the stream, then drops it.
/// The counter was reset to 0 on connect and then used as the delay multiplier.
#[test]
fn delay_after_connect_then_drop_is_at_least_the_initial_delay() {
    let mut p = OandaReconnectPolicy::new(&ws(100));
    assert!(p.begin_attempt().is_some());
    p.on_connected();
    // ... stream drops here ...
    let d = p.backoff_delay();
    assert!(
        d >= Duration::from_millis(1000),
        "delay after an accepted-then-dropped stream was {:?}",
        d
    );
}

/// Three connect failures in a row: the delays must back off geometrically,
/// not linearly, and attempt 3 must wait longer than attempt 1.
/// Only delivered data proves a connection good; accept-then-drop keeps
/// backing off, and the counter restarts once a line arrives.
#[test]
fn accept_then_drop_keeps_backing_off_until_data_arrives() {
    let mut p = OandaReconnectPolicy::new(&ws(100));
    let mut delays = Vec::new();
    for _ in 0..3 {
        p.begin_attempt();
        p.on_connected(); // server accepted ...
        delays.push(p.backoff_delay()); // ... then dropped before any line
    }
    assert!(
        delays[0] < delays[1] && delays[1] < delays[2],
        "{:?}",
        delays
    );

    p.begin_attempt();
    p.on_connected();
    p.on_data(); // a PRICE/HEARTBEAT line arrived
    assert_eq!(p.attempt(), 0);
    assert_eq!(p.backoff_delay(), Duration::from_millis(1000));
}

#[test]
fn consecutive_connect_failures_back_off_exponentially() {
    let mut p = OandaReconnectPolicy::new(&ws(100));
    let mut delays = Vec::new();
    for _ in 0..3 {
        assert!(p.begin_attempt().is_some());
        delays.push(p.backoff_delay());
    }
    let (d1, d2, d3) = (delays[0], delays[1], delays[2]);
    assert!(d1 >= Duration::from_millis(1000), "d1 = {:?}", d1);
    assert!(
        d3 > d1,
        "attempt 3 ({:?}) must wait longer than attempt 1 ({:?})",
        d3,
        d1
    );
    assert!(
        d3.as_millis() >= 2 * d2.as_millis() && d2.as_millis() >= 2 * d1.as_millis(),
        "delays must at least double each attempt: {:?}",
        delays
    );
}

// =============================================================================
// Adjacent to #18 — the instrument the binary builds for "EUR_USD"
// =============================================================================

/// `Instrument::new("EUR_USD", "", Oanda, "EUR_USD")` produced symbol
/// "EUR_USD_" and Redis key `market:orderbook:EUR_USD_`.
#[test]
fn oanda_instrument_splits_base_and_quote() {
    let i = oanda_instrument("EUR_USD");
    assert_eq!(i.base, "EUR");
    assert_eq!(i.quote, "USD");
    assert_eq!(i.raw_symbol, "EUR_USD");
    assert_eq!(i.symbol(), "EUR/USD");
}
