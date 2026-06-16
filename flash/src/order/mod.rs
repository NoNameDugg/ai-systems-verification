//! Order Management module for Flash.
//!
//! This module provides order lifecycle management with:
//!
//! - **Order Types**: Submit, Cancel, Modify operations
//! - **TAP-3 (Egress)**: BlackBox integration for order recording
//! - **Type Definitions**: OrderRequest, OrderId, OrderType, TimeInForce
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      OrderManager                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │  pending_orders: HashMap<OrderId, OrderRequest>             │
//! │  tap: Option<Arc<dyn Tap>>                [Egress recording] │
//! │  metrics: FlashMetrics                                       │
//! └─────────────────────────────────────────────────────────────┘
//!                           │
//!                           ▼ TAP-3 (Egress)
//!              ┌────────────────────────┐
//!              │  BlackBox Journal      │
//!              │  - ORDER_SUBMIT (0x20) │
//!              │  - ORDER_CANCEL        │
//!              │  - ORDER_MODIFY        │
//!              └────────────────────────┘
//! ```
//!
//! # TAP-3 Integration
//!
//! When `blackbox` feature is enabled, all order operations are recorded
//! to the journal for deterministic replay:
//!
//! | Operation | Record Type | Payload |
//! |-----------|-------------|---------|
//! | Submit | ORDER_SUBMIT | Serialized OrderRequest |
//! | Cancel | ORDER_CANCEL | OrderId + reason |
//! | Modify | ORDER_MODIFY | OrderId + changes |
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::order::{OrderManager, OrderRequest, OrderType, TimeInForce};
//! use astra_flash::blackbox::{JournalTap, Tap};
//!
//! // Create order manager with tap
//! let tap = JournalTap::new(writer);
//! let manager = OrderManager::with_tap(tap);
//!
//! // Submit order (recorded to BlackBox journal)
//! let order = OrderRequest::limit(
//!     instrument,
//!     Side::Bid,
//!     50000.0,
//!     dec!(1.0),
//!     TimeInForce::GTC,
//! );
//! manager.submit(order).await?;
//! ```

// =============================================================================
// SUBMODULES
// =============================================================================

mod manager;
mod types;

// =============================================================================
// RE-EXPORTS
// =============================================================================

pub use manager::{OrderManager, OrderManagerConfig, OrderManagerStats};
pub use types::{
    CancelReason, ModifyRequest, OrderId, OrderRequest, OrderSide, OrderStatus, OrderType,
    TimeInForce,
};
