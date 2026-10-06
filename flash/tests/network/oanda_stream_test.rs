//! External review (2026-10) reproductions — OANDA stream loop (findings #15, #16)
//!
//! The loop and retry counter that the binary runs were private to `main.rs`
//! until v1.2; they are now `astra_flash::network::oanda_stream` so they can be
//! tested here. Tests marked `#[ignore]` reproduce the review's findings and
//! were committed failing on purpose; run them with `-- --ignored`.

use std::time::Duration;

use astra_flash::network::oanda_stream::{
    consume_ndjson_stream, oanda_instrument, OandaReconnectPolicy, StreamExit,
};
use futures_util::stream;

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
    let exit = consume_ndjson_stream(s, |l| lines.push(l.to_string())).await;

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
    let exit = consume_ndjson_stream(stream::iter(items), |_| n += 1).await;
    assert_eq!(n, 1);
    assert!(matches!(exit, StreamExit::Error(e) if e.contains("connection reset")));
}

#[test]
fn policy_gives_up_after_max_attempts() {
    let mut p = OandaReconnectPolicy::new(3);
    assert_eq!(p.begin_attempt(), Some(1));
    assert_eq!(p.begin_attempt(), Some(2));
    assert_eq!(p.begin_attempt(), Some(3));
    assert_eq!(p.begin_attempt(), None);
}

// =============================================================================
// #15 — a silent connection must be detected and the loop must give up on it
// =============================================================================

/// A peer that has gone silent (TCP half-open, stalled proxy) never yields and
/// never errors. OANDA heartbeats every ~5 s, and `WebSocketConfig.read_timeout_ms`
/// documents "force reconnect if no data for 5s" — but the loop had no timeout.
#[tokio::test(start_paused = true)]
#[ignore = "review #15: the stream loop has no idle timeout; a silent peer blocks forever"]
async fn silent_stream_terminates_within_the_idle_budget() {
    let silent = stream::pending::<Chunk>();
    let budget = Duration::from_secs(120); // far more than any sane idle timeout

    let outcome = tokio::time::timeout(budget, consume_ndjson_stream(silent, |_| ())).await;

    assert!(
        outcome.is_ok(),
        "a stream that delivers nothing for {:?} must be abandoned so the caller can reconnect",
        budget
    );
}

// =============================================================================
// #16 — reconnect delay must never collapse to zero, and must grow
// =============================================================================

/// The production failure mode: the server accepts the stream, then drops it.
/// The counter was reset to 0 on connect and then used as the delay multiplier.
#[test]
#[ignore = "review #16: connect-then-drop yields a zero delay (tight retry loop)"]
fn delay_after_connect_then_drop_is_at_least_the_initial_delay() {
    let mut p = OandaReconnectPolicy::new(100);
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
#[test]
#[ignore = "review #16: backoff is linear (1s, 2s, 3s) despite the 'exponential' comment"]
fn consecutive_connect_failures_back_off_exponentially() {
    let mut p = OandaReconnectPolicy::new(100);
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
#[ignore = "review (adjacent to #18): OANDA instrument built with an empty quote currency"]
fn oanda_instrument_splits_base_and_quote() {
    let i = oanda_instrument("EUR_USD");
    assert_eq!(i.base, "EUR");
    assert_eq!(i.quote, "USD");
    assert_eq!(i.raw_symbol, "EUR_USD");
    assert_eq!(i.symbol(), "EUR/USD");
}
