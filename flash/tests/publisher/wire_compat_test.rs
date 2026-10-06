//! External review (2026-10) reproduction — finding #18
//!
//! The binary publishes `gateway::OrderBookSnapshot` as JSON on
//! `market:orderbook:{SYMBOL}`. The Python binding decoded that stream as the
//! *internal* `BookSnapshot` / `MarketEvent` shapes, which have different field
//! names, so it could not decode a single message the program publishes.
//!
//! These tests pin the published wire format to a committed fixture and assert
//! that the decode path the binding uses can read it. Tests marked `#[ignore]`
//! reproduce the finding and were committed failing on purpose.

use astra_flash::book::BookSnapshot;
use astra_flash::core::types::{Exchange, Instrument, PriceLevel};
use astra_flash::gateway::OrderBookSnapshot;
use rust_decimal_macros::dec;

const TS: i64 = 1_704_067_200_000_000;
const FIXTURE: &str = include_str!("../fixtures/published/orderbook_EUR_USD.json");

/// The snapshot the binary builds from one OANDA PRICE message for EUR_USD.
fn sample_book() -> BookSnapshot {
    BookSnapshot {
        instrument: Instrument::new("EUR", "USD", Exchange::Oanda, "EUR_USD"),
        timestamp: TS,
        bids: vec![
            PriceLevel::new(1.105, dec!(10000000), TS),
            PriceLevel::new(1.1049, dec!(5000000), TS),
        ],
        asks: vec![PriceLevel::new(1.1052, dec!(10000000), TS)],
    }
}

fn published_json() -> String {
    OrderBookSnapshot::from_book_snapshot(&sample_book())
        .to_json()
        .expect("serialises")
}

/// Freeze the wire format: anything that changes what consumers receive must
/// change this fixture on purpose.
#[test]
fn published_orderbook_wire_format_matches_committed_fixture() {
    assert_eq!(published_json(), FIXTURE.trim());
}

/// What the program publishes must decode through the path the binding uses.
#[test]
#[ignore = "review #18: published JSON (symbol/exchange/f64 levels) is decoded as internal BookSnapshot (instrument/Decimal levels)"]
fn published_orderbook_json_decodes_back_to_a_book_snapshot() {
    let wire = published_json();
    let decoded = OrderBookSnapshot::decode_published_orderbook(wire.as_bytes())
        .expect("the binding must be able to decode what the binary publishes");

    let book = sample_book();
    assert_eq!(decoded.instrument.base, "EUR");
    assert_eq!(decoded.instrument.quote, "USD");
    assert_eq!(decoded.instrument.exchange, Exchange::Oanda);
    assert_eq!(decoded.timestamp, TS);
    assert_eq!(decoded.bids.len(), 2);
    assert_eq!(decoded.asks.len(), 1);
    assert_eq!(decoded.bids[0].price, book.bids[0].price);
    assert_eq!(decoded.bids[0].quantity, book.bids[0].quantity);
    assert_eq!(decoded.bids[1].quantity, book.bids[1].quantity);
    assert_eq!(decoded.asks[0].price, book.asks[0].price);
    assert_eq!(decoded.asks[0].quantity, book.asks[0].quantity);
}

/// The committed fixture is exactly what a consumer sees on the wire.
#[test]
#[ignore = "review #18: the committed published message cannot be decoded"]
fn committed_published_fixture_decodes() {
    let decoded = OrderBookSnapshot::decode_published_orderbook(FIXTURE.trim().as_bytes())
        .expect("committed published fixture must decode");
    assert_eq!(decoded.instrument.raw_symbol, "EUR_USD");
}

/// The Python binding's own entry point (only built with `--features python`).
#[cfg(feature = "python")]
#[test]
#[ignore = "review #18: PyFlashClient's deserialize_book cannot decode a published orderbook"]
fn python_binding_decodes_a_published_orderbook() {
    use astra_flash::bindings::stream::deserialize_book;
    let wire = published_json();
    let snapshot = deserialize_book(wire.as_bytes(), "json")
        .expect("the Python binding must decode what the binary publishes");
    let _ = snapshot;
}
