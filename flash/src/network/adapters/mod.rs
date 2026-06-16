//! Exchange adapters for Flash.
//!
//! This module provides exchange-specific adapters that implement the
//! [`ExchangeAdapter`] trait. Each adapter handles:
//!
//! - Parsing exchange-specific message formats into normalized [`MarketEvent`]s
//! - Building subscription/unsubscription messages
//! - Handling exchange-specific ping/pong protocols
//! - Parsing error responses
//!
//! # Supported Exchanges
//!
//! | Exchange | Adapter | Description |
//! |----------|---------|-------------|
//! | Deribit | [`DeribitAdapter`] | Crypto derivatives |
//! | Binance | [`BinanceAdapter`] | Crypto spot/futures |
//! | OANDA | [`OandaAdapter`] | Forex |
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        ADAPTER LAYER                             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                  │
//! │   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
//! │   │ DeribitAdapter  │  │ BinanceAdapter  │  │  OandaAdapter   │ │
//! │   └────────┬────────┘  └────────┬────────┘  └────────┬────────┘ │
//! │            │                    │                    │          │
//! │            └────────────────────┼────────────────────┘          │
//! │                                 │                               │
//! │                                 ▼                               │
//! │                    ┌───────────────────────┐                    │
//! │                    │    ExchangeAdapter    │                    │
//! │                    │        (trait)        │                    │
//! │                    └───────────────────────┘                    │
//! │                                 │                               │
//! │                                 ▼                               │
//! │                    ┌───────────────────────┐                    │
//! │                    │     MarketEvent       │                    │
//! │                    │    (normalized)       │                    │
//! │                    └───────────────────────┘                    │
//! │                                                                  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```
//! use astra_flash::core::types::Exchange;
//! use astra_flash::network::adapters::{create_adapter, ExchangeAdapter};
//!
//! // Create an adapter using the factory function
//! let adapter = create_adapter(Exchange::Deribit);
//!
//! // Or create directly
//! use astra_flash::network::adapters::DeribitAdapter;
//! let deribit = DeribitAdapter::default();
//! ```
//!
//! # Performance
//!
//! All adapters are designed for minimal allocation in the hot path:
//!
//! | Operation | Target Latency |
//! |-----------|----------------|
//! | Parse message | < 1 μs |
//! | Build subscribe | < 1 μs |
//!
//! # Thread Safety
//!
//! All adapters implement `Send + Sync` and can be safely shared
//! across async tasks.

// Submodules
mod binance;
mod common;
mod deribit;
mod oanda;
mod traits;

// Re-exports
pub use binance::{BinanceAdapter, UpdateInterval};
pub use deribit::{BookInterval, DeribitAdapter};
pub use oanda::{OandaAdapter, OandaHeartbeat, OandaLevel, OandaPrice};
pub use traits::{ExchangeAdapter, RateLimit};

use crate::core::types::Exchange;

// =============================================================================
// ADAPTER FACTORY
// =============================================================================

/// Create an exchange adapter for the given exchange.
///
/// This factory function creates the appropriate adapter based on the
/// exchange enum value. Use this when you need to create adapters
/// dynamically at runtime.
///
/// # Arguments
///
/// * `exchange` - The exchange to create an adapter for
///
/// # Returns
///
/// A boxed trait object implementing [`ExchangeAdapter`].
///
/// # Example
///
/// ```
/// use astra_flash::core::types::Exchange;
/// use astra_flash::network::adapters::{create_adapter, ExchangeAdapter};
///
/// let adapter = create_adapter(Exchange::Deribit);
/// assert_eq!(adapter.exchange(), Exchange::Deribit);
///
/// let adapter = create_adapter(Exchange::Binance);
/// assert_eq!(adapter.exchange(), Exchange::Binance);
/// ```
#[must_use]
pub fn create_adapter(exchange: Exchange) -> Box<dyn ExchangeAdapter> {
    match exchange {
        Exchange::Deribit => Box::new(DeribitAdapter::default()),
        Exchange::Binance => Box::new(BinanceAdapter::default()),
        Exchange::Oanda => Box::new(OandaAdapter::new("default")),
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_adapter_deribit() {
        let adapter = create_adapter(Exchange::Deribit);
        assert_eq!(adapter.exchange(), Exchange::Deribit);
    }

    #[test]
    fn test_create_adapter_binance() {
        let adapter = create_adapter(Exchange::Binance);
        assert_eq!(adapter.exchange(), Exchange::Binance);
    }

    #[test]
    fn test_create_adapter_oanda() {
        let adapter = create_adapter(Exchange::Oanda);
        assert_eq!(adapter.exchange(), Exchange::Oanda);
    }

    #[test]
    fn test_adapters_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<DeribitAdapter>();
        assert_send_sync::<BinanceAdapter>();
        assert_send_sync::<OandaAdapter>();
    }

    #[test]
    fn test_boxed_adapter_is_send_sync() {
        let adapter: Box<dyn ExchangeAdapter> = create_adapter(Exchange::Deribit);
        // The following line would fail to compile if the trait object wasn't Send + Sync
        fn takes_send_sync(_: impl Send + Sync) {}
        takes_send_sync(adapter);
    }
}
