//! Test fixtures for E2E testing.
//!
//! Provides generators for exchange-specific message formats and test instruments.

use astra_flash::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::str::FromStr;

// =============================================================================
// INSTRUMENT FIXTURES
// =============================================================================

/// Create a standard test instrument for the given exchange.
pub fn test_instrument(exchange: Exchange) -> Instrument {
    match exchange {
        Exchange::Deribit => Instrument::new("BTC", "USD", exchange, "BTC-PERPETUAL"),
        Exchange::Binance => Instrument::new("BTC", "USDT", exchange, "BTCUSDT"),
        Exchange::Oanda => Instrument::new("EUR", "USD", exchange, "EUR_USD"),
    }
}

/// Create a crypto test instrument.
pub fn crypto_instrument(base: &str, quote: &str, exchange: Exchange) -> Instrument {
    let raw_symbol = match exchange {
        Exchange::Deribit => format!("{}-{}", base, quote),
        Exchange::Binance => format!("{}{}", base, quote),
        Exchange::Oanda => format!("{}_{}", base, quote),
    };
    Instrument::new(base, quote, exchange, &raw_symbol)
}

// =============================================================================
// PRICE LEVEL FIXTURES
// =============================================================================

/// Create price levels from tuples.
pub fn price_levels(levels: &[(f64, f64)]) -> Vec<PriceLevel> {
    levels
        .iter()
        .map(|(price, qty)| PriceLevel {
            price: *price,
            quantity: Decimal::from_str(&qty.to_string()).unwrap_or(dec!(0)),
            order_count: None,
            timestamp: now_micros(),
        })
        .collect()
}

/// Create a standard bid ladder (descending prices).
pub fn standard_bids(best_bid: f64, levels: usize, step: f64) -> Vec<PriceLevel> {
    (0..levels)
        .map(|i| PriceLevel {
            price: best_bid - (i as f64 * step),
            quantity: Decimal::from(i as u32 + 1),
            order_count: Some(1),
            timestamp: now_micros(),
        })
        .collect()
}

/// Create a standard ask ladder (ascending prices).
pub fn standard_asks(best_ask: f64, levels: usize, step: f64) -> Vec<PriceLevel> {
    (0..levels)
        .map(|i| PriceLevel {
            price: best_ask + (i as f64 * step),
            quantity: Decimal::from(i as u32 + 1),
            order_count: Some(1),
            timestamp: now_micros(),
        })
        .collect()
}

// =============================================================================
// DERIBIT MESSAGE FIXTURES
// =============================================================================

/// Create a Deribit order book snapshot message.
pub fn deribit_book_snapshot(
    instrument: &str,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    change_id: u64,
) -> String {
    let timestamp = now_micros();
    let bids_json: Vec<[f64; 2]> = bids.iter().map(|(p, q)| [*p, *q]).collect();
    let asks_json: Vec<[f64; 2]> = asks.iter().map(|(p, q)| [*p, *q]).collect();

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": format!("book.{}.raw", instrument),
            "data": {
                "type": "snapshot",
                "timestamp": timestamp,
                "instrument_name": instrument,
                "change_id": change_id,
                "bids": bids_json,
                "asks": asks_json
            }
        }
    })
    .to_string()
}

/// Create a Deribit order book delta (change) message.
pub fn deribit_book_delta(
    instrument: &str,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    prev_change_id: u64,
    change_id: u64,
) -> String {
    let timestamp = now_micros();
    let bids_json: Vec<[f64; 3]> = bids
        .iter()
        .map(|(p, q)| {
            if *q > 0.0 {
                ["change".to_string(), p.to_string(), q.to_string()]
            } else {
                ["delete".to_string(), p.to_string(), "0".to_string()]
            }
        })
        .map(|[a, b, c]| [0.0, 0.0, 0.0]) // Placeholder - actual format differs
        .collect();

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": format!("book.{}.raw", instrument),
            "data": {
                "type": "change",
                "timestamp": timestamp,
                "instrument_name": instrument,
                "prev_change_id": prev_change_id,
                "change_id": change_id,
                "bids": bids.iter().map(|(p, q)| {
                    if *q > 0.0 {
                        vec!["change".to_string(), p.to_string(), q.to_string()]
                    } else {
                        vec!["delete".to_string(), p.to_string(), "0".to_string()]
                    }
                }).collect::<Vec<_>>(),
                "asks": asks.iter().map(|(p, q)| {
                    if *q > 0.0 {
                        vec!["change".to_string(), p.to_string(), q.to_string()]
                    } else {
                        vec!["delete".to_string(), p.to_string(), "0".to_string()]
                    }
                }).collect::<Vec<_>>()
            }
        }
    })
    .to_string()
}

/// Create a Deribit trade message.
pub fn deribit_trade(
    instrument: &str,
    price: f64,
    amount: f64,
    direction: &str,
    trade_id: &str,
) -> String {
    let timestamp = now_micros();

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": format!("trades.{}.raw", instrument),
            "data": [{
                "trade_seq": 1,
                "trade_id": trade_id,
                "timestamp": timestamp,
                "tick_direction": 0,
                "price": price,
                "mark_price": price,
                "instrument_name": instrument,
                "index_price": price,
                "direction": direction,
                "amount": amount
            }]
        }
    })
    .to_string()
}

/// Create a Deribit heartbeat message.
pub fn deribit_heartbeat() -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "heartbeat",
        "params": {
            "type": "heartbeat"
        }
    })
    .to_string()
}

// =============================================================================
// BINANCE MESSAGE FIXTURES
// =============================================================================

/// Create a Binance order book depth snapshot.
pub fn binance_book_snapshot(
    symbol: &str,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    first_update_id: u64,
    last_update_id: u64,
) -> String {
    let timestamp = now_micros() / 1000; // Binance uses milliseconds
    let bids_json: Vec<[String; 2]> = bids
        .iter()
        .map(|(p, q)| [p.to_string(), q.to_string()])
        .collect();
    let asks_json: Vec<[String; 2]> = asks
        .iter()
        .map(|(p, q)| [p.to_string(), q.to_string()])
        .collect();

    serde_json::json!({
        "stream": format!("{}@depth@100ms", symbol.to_lowercase()),
        "data": {
            "e": "depthUpdate",
            "E": timestamp,
            "s": symbol.to_uppercase(),
            "U": first_update_id,
            "u": last_update_id,
            "b": bids_json,
            "a": asks_json
        }
    })
    .to_string()
}

/// Create a Binance depth delta update.
pub fn binance_book_delta(
    symbol: &str,
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    first_update_id: u64,
    last_update_id: u64,
) -> String {
    // Binance deltas have same format as snapshots
    binance_book_snapshot(symbol, bids, asks, first_update_id, last_update_id)
}

/// Create a Binance trade message.
pub fn binance_trade(symbol: &str, price: f64, quantity: f64, is_buyer_maker: bool) -> String {
    let timestamp = now_micros() / 1000;

    serde_json::json!({
        "stream": format!("{}@trade", symbol.to_lowercase()),
        "data": {
            "e": "trade",
            "E": timestamp,
            "s": symbol.to_uppercase(),
            "t": 12345,
            "p": price.to_string(),
            "q": quantity.to_string(),
            "b": 88,
            "a": 50,
            "T": timestamp,
            "m": is_buyer_maker,
            "M": true
        }
    })
    .to_string()
}

// =============================================================================
// OANDA MESSAGE FIXTURES
// =============================================================================

/// Create an OANDA price message.
pub fn oanda_price(instrument: &str, bid: f64, ask: f64, tradeable: bool) -> String {
    let time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    serde_json::json!({
        "type": "PRICE",
        "time": time,
        "bids": [{"price": bid.to_string(), "liquidity": 1000000}],
        "asks": [{"price": ask.to_string(), "liquidity": 1000000}],
        "closeoutBid": (bid - 0.0001).to_string(),
        "closeoutAsk": (ask + 0.0001).to_string(),
        "status": "tradeable",
        "tradeable": tradeable,
        "instrument": instrument
    })
    .to_string()
}

/// Create an OANDA heartbeat message.
pub fn oanda_heartbeat() -> String {
    let time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    serde_json::json!({
        "type": "HEARTBEAT",
        "time": time
    })
    .to_string()
}

// =============================================================================
// MALFORMED MESSAGE FIXTURES
// =============================================================================

/// Create various malformed messages for failure testing.
pub mod malformed {
    /// Empty message.
    pub fn empty() -> String {
        String::new()
    }

    /// Invalid JSON.
    pub fn invalid_json() -> String {
        "{invalid json".to_string()
    }

    /// Valid JSON but missing required fields.
    pub fn missing_fields() -> String {
        serde_json::json!({"type": "unknown"}).to_string()
    }

    /// Message with NaN price.
    pub fn nan_price() -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-PERPETUAL.raw",
                "data": {
                    "type": "snapshot",
                    "timestamp": 0,
                    "bids": [[std::f64::NAN, 1.0]],
                    "asks": [[50010.0, 1.0]]
                }
            }
        })
        .to_string()
    }

    /// Message with Infinity price.
    pub fn infinity_price() -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-PERPETUAL.raw",
                "data": {
                    "type": "snapshot",
                    "timestamp": 0,
                    "bids": [[std::f64::INFINITY, 1.0]],
                    "asks": [[50010.0, 1.0]]
                }
            }
        })
        .to_string()
    }

    /// Message with negative price.
    pub fn negative_price() -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-PERPETUAL.raw",
                "data": {
                    "type": "snapshot",
                    "timestamp": 0,
                    "bids": [[-50000.0, 1.0]],
                    "asks": [[50010.0, 1.0]]
                }
            }
        })
        .to_string()
    }

    /// Message with crossed book (best bid >= best ask).
    pub fn crossed_book() -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-PERPETUAL.raw",
                "data": {
                    "type": "snapshot",
                    "timestamp": 0,
                    "bids": [[50100.0, 1.0]],
                    "asks": [[50000.0, 1.0]]
                }
            }
        })
        .to_string()
    }
}

// =============================================================================
// SEQUENCE FIXTURES
// =============================================================================

/// Generate a sequence of order book messages simulating a live feed.
pub fn generate_message_sequence(
    exchange: Exchange,
    message_count: usize,
    include_gaps: bool,
) -> Vec<String> {
    let mut messages = Vec::with_capacity(message_count);
    let mut sequence = 1u64;

    // Start with snapshot
    let snapshot = match exchange {
        Exchange::Deribit => deribit_book_snapshot(
            "BTC-PERPETUAL",
            &[(50000.0, 1.0), (49990.0, 2.0)],
            &[(50010.0, 1.0), (50020.0, 2.0)],
            sequence,
        ),
        Exchange::Binance => binance_book_snapshot(
            "BTCUSDT",
            &[(50000.0, 1.0), (49990.0, 2.0)],
            &[(50010.0, 1.0), (50020.0, 2.0)],
            1,
            sequence,
        ),
        Exchange::Oanda => oanda_price("EUR_USD", 1.10000, 1.10020, true),
    };
    messages.push(snapshot);
    sequence += 1;

    // Generate deltas
    for i in 1..message_count {
        // Optionally inject sequence gaps
        if include_gaps && i == message_count / 2 {
            sequence += 10; // Skip 10 sequence numbers
        }

        let delta = match exchange {
            Exchange::Deribit => deribit_book_delta(
                "BTC-PERPETUAL",
                &[(50000.0 + i as f64, 1.0)],
                &[(50010.0 + i as f64, 1.0)],
                sequence - 1,
                sequence,
            ),
            Exchange::Binance => binance_book_delta(
                "BTCUSDT",
                &[(50000.0 + i as f64, 1.0)],
                &[(50010.0 + i as f64, 1.0)],
                sequence - 1,
                sequence,
            ),
            Exchange::Oanda => oanda_price(
                "EUR_USD",
                1.10000 + i as f64 * 0.0001,
                1.10020 + i as f64 * 0.0001,
                true,
            ),
        };
        messages.push(delta);
        sequence += 1;
    }

    messages
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_deribit() {
        let inst = test_instrument(Exchange::Deribit);
        assert_eq!(inst.base, "BTC");
        assert_eq!(inst.quote, "USD");
        assert_eq!(inst.raw_symbol, "BTC-PERPETUAL");
    }

    #[test]
    fn test_instrument_binance() {
        let inst = test_instrument(Exchange::Binance);
        assert_eq!(inst.base, "BTC");
        assert_eq!(inst.quote, "USDT");
        assert_eq!(inst.raw_symbol, "BTCUSDT");
    }

    #[test]
    fn test_instrument_oanda() {
        let inst = test_instrument(Exchange::Oanda);
        assert_eq!(inst.base, "EUR");
        assert_eq!(inst.quote, "USD");
        assert_eq!(inst.raw_symbol, "EUR_USD");
    }

    #[test]
    fn test_deribit_snapshot_valid_json() {
        let msg = deribit_book_snapshot("BTC-PERPETUAL", &[(50000.0, 1.0)], &[(50010.0, 1.0)], 1);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "subscription");
    }

    #[test]
    fn test_binance_snapshot_valid_json() {
        let msg = binance_book_snapshot("BTCUSDT", &[(50000.0, 1.0)], &[(50010.0, 1.0)], 1, 100);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["data"]["e"], "depthUpdate");
    }

    #[test]
    fn test_oanda_price_valid_json() {
        let msg = oanda_price("EUR_USD", 1.10000, 1.10020, true);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["type"], "PRICE");
        assert_eq!(parsed["tradeable"], true);
    }

    #[test]
    fn test_generate_message_sequence() {
        let messages = generate_message_sequence(Exchange::Deribit, 10, false);
        assert_eq!(messages.len(), 10);

        // First message should be snapshot
        let first: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(first["params"]["data"]["type"], "snapshot");
    }

    #[test]
    fn test_malformed_messages() {
        assert!(malformed::empty().is_empty());
        assert!(serde_json::from_str::<serde_json::Value>(&malformed::invalid_json()).is_err());
        assert!(serde_json::from_str::<serde_json::Value>(&malformed::missing_fields()).is_ok());
    }
}
