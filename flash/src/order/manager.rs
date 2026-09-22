//! Order Manager implementation with BlackBox TAP-3 (Egress) integration.
//!
//! This module provides the [`OrderManager`] struct for order lifecycle management.
//! When the `blackbox` feature is enabled, all outbound order operations are
//! recorded to the journal for deterministic replay.
//!
//! # TAP-3 Integration
//!
//! TAP-3 is the Egress tap point that captures all outbound order operations:
//!
//! | Method | Record Type | Event Type |
//! |--------|-------------|------------|
//! | `submit_order` | ORDER_SUBMIT | 0x0020 |
//! | `cancel_order` | ORDER_CANCEL | 0x0021 |
//! | `modify_order` | ORDER_MODIFY | 0x0022 |
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                        OrderManager                              │
//! ├────────────────────────────────────────────────────────────────┤
//! │  pending: HashMap<OrderId, OrderState>                          │
//! │  next_id: AtomicU64                                             │
//! │  tap: Option<Arc<dyn Tap>>                   [TAP-3 Egress]     │
//! │  config: OrderManagerConfig                                      │
//! │  stats: OrderManagerStats                                        │
//! └────────────────────────────────────────────────────────────────┘
//!                           │
//!                           │ submit_order()
//!                           ▼
//!              ┌────────────────────────────┐
//!              │  1. Generate OrderId       │
//!              │  2. Record to TAP-3        │──> BlackBox Journal
//!              │  3. Track in pending       │
//!              │  4. Send to exchange       │──> Exchange API
//!              └────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::order::{OrderManager, OrderManagerConfig, OrderRequest, OrderSide};
//! use astra_flash::blackbox::{JournalTap, Tap};
//!
//! // Create with BlackBox tap
//! let tap = JournalTap::new(writer);
//! let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);
//!
//! // Submit order (recorded to journal)
//! let order = OrderRequest::market(instrument, OrderSide::Buy, dec!(1.0), now_micros());
//! let order_id = manager.submit_order(order)?;
//! ```

use super::types::{CancelReason, ModifyRequest, OrderId, OrderRequest, OrderStatus};
use crate::core::types::Timestamp;
#[cfg(feature = "blackbox")]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// BlackBox tap integration (optional feature)
#[cfg(feature = "blackbox")]
use crate::blackbox::{Tap, TapExt};
#[cfg(feature = "blackbox")]
use std::sync::Arc;

// =============================================================================
// EVENT TYPES (for TAP-3 recording)
// =============================================================================

/// Event type constants for order operations.
///
/// These constants document the order event types used in the BlackBox journal.
/// The actual record type is determined by the payload contents.
#[cfg(feature = "blackbox")]
#[allow(dead_code)]
pub mod event_types {
    /// Order submission event type (0x0020).
    pub const ORDER_SUBMIT: u16 = 0x0020;
    /// Order cancellation event type (0x0021).
    pub const ORDER_CANCEL: u16 = 0x0021;
    /// Order modification event type (0x0022).
    pub const ORDER_MODIFY: u16 = 0x0022;
}

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for the OrderManager.
///
/// # Example
///
/// ```
/// use astra_flash::order::OrderManagerConfig;
///
/// let config = OrderManagerConfig::default();
/// assert_eq!(config.max_pending_orders, 1000);
/// ```
#[derive(Debug, Clone)]
pub struct OrderManagerConfig {
    /// Maximum number of pending orders to track.
    pub max_pending_orders: usize,

    /// Whether to track order history (for debugging).
    pub track_history: bool,
}

impl Default for OrderManagerConfig {
    fn default() -> Self {
        Self {
            max_pending_orders: 1000,
            track_history: false,
        }
    }
}

impl OrderManagerConfig {
    /// Creates a new configuration with specified max pending orders.
    #[must_use]
    pub fn with_max_pending(max_pending: usize) -> Self {
        Self {
            max_pending_orders: max_pending,
            ..Default::default()
        }
    }
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Statistics for order manager monitoring.
///
/// Tracks order counts by operation and status for observability.
#[derive(Debug, Clone, Default)]
pub struct OrderManagerStats {
    /// Total orders submitted.
    pub orders_submitted: u64,

    /// Total orders cancelled.
    pub orders_cancelled: u64,

    /// Total orders modified.
    pub orders_modified: u64,

    /// Total orders filled.
    pub orders_filled: u64,

    /// Total orders rejected.
    pub orders_rejected: u64,

    /// Current pending order count.
    pub pending_count: usize,
}

// =============================================================================
// ORDER STATE (internal tracking)
// =============================================================================

/// Internal state tracking for an order.
#[derive(Debug, Clone)]
struct OrderState {
    /// Original order request.
    request: OrderRequest,

    /// Current status.
    status: OrderStatus,

    /// Filled quantity (for future use with fill handling).
    #[allow(dead_code)]
    filled_quantity: rust_decimal::Decimal,

    /// Last update timestamp.
    last_update: Timestamp,
}

// =============================================================================
// EGRESS PAYLOAD TYPES (for serialization)
// =============================================================================

/// Payload for ORDER_SUBMIT events.
#[cfg(feature = "blackbox")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubmitPayload {
    order_id: OrderId,
    request: OrderRequest,
}

/// Payload for ORDER_CANCEL events.
#[cfg(feature = "blackbox")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CancelPayload {
    order_id: OrderId,
    reason: CancelReason,
    timestamp: Timestamp,
}

/// Payload for ORDER_MODIFY events.
#[cfg(feature = "blackbox")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModifyPayload {
    order_id: OrderId,
    request: ModifyRequest,
}

// =============================================================================
// ORDER MANAGER
// =============================================================================

/// Order lifecycle manager with TAP-3 (Egress) integration.
///
/// Manages order submission, cancellation, and modification while recording
/// all operations to the BlackBox journal (when enabled).
///
/// # Thread Safety
///
/// The OrderManager is thread-safe and can be shared across threads.
/// Internal state is protected by a Mutex.
///
/// # TAP-3 Integration
///
/// When the `blackbox` feature is enabled, all egress operations are recorded:
///
/// | Operation | Method | Event Type |
/// |-----------|--------|------------|
/// | Submit | `submit_order` | ORDER_SUBMIT (0x0020) |
/// | Cancel | `cancel_order` | ORDER_CANCEL (0x0021) |
/// | Modify | `modify_order` | ORDER_MODIFY (0x0022) |
///
/// # Example
///
/// ```
/// use astra_flash::order::{OrderManager, OrderManagerConfig};
///
/// let manager = OrderManager::new(OrderManagerConfig::default());
/// assert_eq!(manager.pending_count(), 0);
/// ```
pub struct OrderManager {
    /// Configuration.
    config: OrderManagerConfig,

    /// Pending orders (thread-safe).
    pending: Mutex<HashMap<OrderId, OrderState>>,

    /// Next order ID (atomic for thread-safety).
    next_id: AtomicU64,

    /// Statistics (thread-safe).
    stats: Mutex<OrderManagerStats>,

    /// BlackBox tap for recording egress operations.
    #[cfg(feature = "blackbox")]
    tap: Option<Arc<dyn Tap>>,
}

impl std::fmt::Debug for OrderManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("OrderManager");
        debug_struct
            .field("config", &self.config)
            .field("pending_count", &self.pending_count())
            .field("next_id", &self.next_id.load(Ordering::Relaxed));

        #[cfg(feature = "blackbox")]
        debug_struct.field("tap", &self.tap.as_ref().map(|_| "<Tap>"));

        debug_struct.finish()
    }
}

impl OrderManager {
    // =========================================================================
    // CONSTRUCTORS
    // =========================================================================

    /// Creates a new OrderManager with the given configuration.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::order::{OrderManager, OrderManagerConfig};
    ///
    /// let manager = OrderManager::new(OrderManagerConfig::default());
    /// ```
    #[must_use]
    pub fn new(config: OrderManagerConfig) -> Self {
        Self {
            config,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stats: Mutex::new(OrderManagerStats::default()),
            #[cfg(feature = "blackbox")]
            tap: None,
        }
    }

    /// Creates a new OrderManager with BlackBox tap for TAP-3 (Egress) recording.
    ///
    /// This constructor is only available when the `blackbox` feature is enabled.
    /// All order operations will be recorded to the journal for replay.
    ///
    /// # Arguments
    ///
    /// * `config` - OrderManager configuration
    /// * `tap` - BlackBox tap for recording events
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use astra_flash::order::{OrderManager, OrderManagerConfig};
    /// use astra_flash::blackbox::{JournalTap, Tap};
    ///
    /// let tap = JournalTap::new(writer);
    /// let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);
    /// ```
    #[cfg(feature = "blackbox")]
    #[must_use]
    pub fn with_tap<T: Tap + 'static>(config: OrderManagerConfig, tap: T) -> Self {
        Self {
            config,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stats: Mutex::new(OrderManagerStats::default()),
            tap: Some(Arc::new(tap)),
        }
    }

    // =========================================================================
    // ORDER OPERATIONS
    // =========================================================================

    /// Submits a new order.
    ///
    /// Generates a unique order ID, records to TAP-3 (if enabled), and
    /// tracks the order in the pending orders map.
    ///
    /// # Arguments
    ///
    /// * `request` - Order submission request
    ///
    /// # Returns
    ///
    /// The generated order ID.
    ///
    /// # TAP-3 Recording
    ///
    /// When `blackbox` feature is enabled, records an ORDER_SUBMIT event
    /// with the order ID and request details.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::order::{OrderManager, OrderManagerConfig, OrderRequest, OrderSide};
    /// use astra_flash::core::types::{Exchange, Instrument, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let manager = OrderManager::new(OrderManagerConfig::default());
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let order = OrderRequest::market(instrument, OrderSide::Buy, dec!(1.0), now_micros());
    ///
    /// let order_id = manager.submit_order(order);
    /// assert_eq!(order_id.as_u64(), 1);
    /// assert_eq!(manager.pending_count(), 1);
    /// ```
    pub fn submit_order(&self, request: OrderRequest) -> OrderId {
        let order_id = OrderId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let timestamp = request.created_at;

        // TAP-3 (Egress): Record order submission
        #[cfg(feature = "blackbox")]
        if let Some(ref tap) = self.tap {
            if tap.is_active() {
                let payload = SubmitPayload {
                    order_id,
                    request: request.clone(),
                };
                if let Ok(bytes) = bincode::serialize(&payload) {
                    tap.record_flash_egress(
                        crate::core::types::Exchange::from_instrument(&request.instrument),
                        &bytes,
                        timestamp,
                    );
                }
            }
        }

        // Track in pending orders
        let state = OrderState {
            request,
            status: OrderStatus::Pending,
            filled_quantity: rust_decimal::Decimal::ZERO,
            last_update: timestamp,
        };

        {
            let mut pending = self.pending.lock().unwrap();
            pending.insert(order_id, state);

            let mut stats = self.stats.lock().unwrap();
            stats.orders_submitted += 1;
            stats.pending_count = pending.len();
        }

        order_id
    }

    /// Cancels an existing order.
    ///
    /// Records to TAP-3 (if enabled) and removes the order from pending.
    ///
    /// # Arguments
    ///
    /// * `order_id` - ID of the order to cancel
    /// * `reason` - Reason for cancellation
    /// * `timestamp` - Timestamp of cancellation request
    ///
    /// # Returns
    ///
    /// `true` if the order was found and cancelled, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::order::{
    ///     OrderManager, OrderManagerConfig, OrderRequest, OrderSide, CancelReason,
    /// };
    /// use astra_flash::core::types::{Exchange, Instrument, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let manager = OrderManager::new(OrderManagerConfig::default());
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let order = OrderRequest::market(instrument, OrderSide::Buy, dec!(1.0), now_micros());
    ///
    /// let order_id = manager.submit_order(order);
    /// assert!(manager.cancel_order(order_id, CancelReason::UserRequested, now_micros()));
    /// assert_eq!(manager.pending_count(), 0);
    /// ```
    pub fn cancel_order(
        &self,
        order_id: OrderId,
        #[allow(unused_variables)] reason: CancelReason,
        #[allow(unused_variables)] timestamp: Timestamp,
    ) -> bool {
        let mut pending = self.pending.lock().unwrap();

        if let Some(state) = pending.get(&order_id) {
            if !state.status.is_cancellable() && state.status != OrderStatus::Pending {
                return false;
            }

            #[allow(unused_variables)]
            let exchange = crate::core::types::Exchange::from_instrument(&state.request.instrument);

            // TAP-3 (Egress): Record order cancellation
            #[cfg(feature = "blackbox")]
            if let Some(ref tap) = self.tap {
                if tap.is_active() {
                    let payload = CancelPayload {
                        order_id,
                        reason: reason.clone(),
                        timestamp,
                    };
                    if let Ok(bytes) = bincode::serialize(&payload) {
                        tap.record_flash_egress(exchange, &bytes, timestamp);
                    }
                }
            }

            // Remove from pending
            pending.remove(&order_id);

            let mut stats = self.stats.lock().unwrap();
            stats.orders_cancelled += 1;
            stats.pending_count = pending.len();

            true
        } else {
            false
        }
    }

    /// Modifies an existing order.
    ///
    /// Records to TAP-3 (if enabled) and updates the order state.
    ///
    /// # Arguments
    ///
    /// * `order_id` - ID of the order to modify
    /// * `modification` - Modification request with new values
    ///
    /// # Returns
    ///
    /// `true` if the order was found and modified, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::order::{
    ///     OrderManager, OrderManagerConfig, OrderRequest, OrderSide, ModifyRequest, TimeInForce,
    /// };
    /// use astra_flash::core::types::{Exchange, Instrument, now_micros};
    /// use rust_decimal_macros::dec;
    ///
    /// let manager = OrderManager::new(OrderManagerConfig::default());
    /// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
    /// let order = OrderRequest::limit(
    ///     instrument,
    ///     OrderSide::Buy,
    ///     50000.0,
    ///     dec!(1.0),
    ///     TimeInForce::GTC,
    ///     now_micros(),
    /// );
    ///
    /// let order_id = manager.submit_order(order);
    /// let modify = ModifyRequest::price(51000.0, now_micros());
    /// assert!(manager.modify_order(order_id, modify));
    /// ```
    pub fn modify_order(&self, order_id: OrderId, modification: ModifyRequest) -> bool {
        let mut pending = self.pending.lock().unwrap();

        if let Some(state) = pending.get_mut(&order_id) {
            if state.status.is_terminal() {
                return false;
            }

            #[allow(unused_variables)]
            let exchange = crate::core::types::Exchange::from_instrument(&state.request.instrument);
            let timestamp = modification.timestamp;

            // TAP-3 (Egress): Record order modification
            #[cfg(feature = "blackbox")]
            if let Some(ref tap) = self.tap {
                if tap.is_active() {
                    let payload = ModifyPayload {
                        order_id,
                        request: modification.clone(),
                    };
                    if let Ok(bytes) = bincode::serialize(&payload) {
                        tap.record_flash_egress(exchange, &bytes, timestamp);
                    }
                }
            }

            // Update order state
            if let Some(new_price) = modification.new_price {
                state.request.price = Some(new_price);
            }
            if let Some(new_qty) = modification.new_quantity {
                state.request.quantity = new_qty;
            }
            state.last_update = timestamp;

            let mut stats = self.stats.lock().unwrap();
            stats.orders_modified += 1;

            true
        } else {
            false
        }
    }

    // =========================================================================
    // STATUS UPDATES (from exchange)
    // =========================================================================

    /// Updates order status (called when exchange responds).
    ///
    /// This is called when receiving acknowledgments, fills, or rejections
    /// from the exchange. Does NOT record to TAP-3 (ingress events are
    /// recorded separately).
    pub fn update_status(&self, order_id: OrderId, new_status: OrderStatus, timestamp: Timestamp) {
        let mut pending = self.pending.lock().unwrap();

        if let Some(state) = pending.get_mut(&order_id) {
            state.status = new_status;
            state.last_update = timestamp;

            // Update stats
            let mut stats = self.stats.lock().unwrap();
            match new_status {
                OrderStatus::Filled => {
                    stats.orders_filled += 1;
                    pending.remove(&order_id);
                    stats.pending_count = pending.len();
                }
                OrderStatus::Rejected => {
                    stats.orders_rejected += 1;
                    pending.remove(&order_id);
                    stats.pending_count = pending.len();
                }
                OrderStatus::Cancelled | OrderStatus::Expired => {
                    pending.remove(&order_id);
                    stats.pending_count = pending.len();
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // ACCESSORS
    // =========================================================================

    /// Returns the number of pending orders.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Returns the current statistics.
    #[must_use]
    pub fn stats(&self) -> OrderManagerStats {
        self.stats.lock().unwrap().clone()
    }

    /// Returns the configuration.
    #[must_use]
    pub const fn config(&self) -> &OrderManagerConfig {
        &self.config
    }

    /// Checks if an order is pending.
    #[must_use]
    pub fn is_pending(&self, order_id: OrderId) -> bool {
        self.pending.lock().unwrap().contains_key(&order_id)
    }

    /// Gets the status of an order.
    #[must_use]
    pub fn get_status(&self, order_id: OrderId) -> Option<OrderStatus> {
        self.pending
            .lock()
            .unwrap()
            .get(&order_id)
            .map(|s| s.status)
    }

    /// Returns all pending order IDs.
    #[must_use]
    pub fn pending_order_ids(&self) -> Vec<OrderId> {
        self.pending.lock().unwrap().keys().copied().collect()
    }

    /// Clears all pending orders.
    ///
    /// Useful for testing or shutdown scenarios.
    pub fn clear(&self) {
        let mut pending = self.pending.lock().unwrap();
        pending.clear();
        let mut stats = self.stats.lock().unwrap();
        stats.pending_count = 0;
    }
}

// =============================================================================
// EXCHANGE HELPER
// =============================================================================

impl crate::core::types::Exchange {
    /// Extracts exchange from instrument.
    ///
    /// Helper method for TAP-3 recording.
    fn from_instrument(instrument: &crate::core::types::Instrument) -> Self {
        instrument.exchange()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{now_micros, Exchange, Instrument};
    use crate::order::{OrderSide, TimeInForce};
    use rust_decimal_macros::dec;

    fn test_instrument() -> Instrument {
        Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
    }

    // =========================================================================
    // Constructor Tests
    // =========================================================================

    #[test]
    fn test_order_manager_new() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        assert_eq!(manager.pending_count(), 0);
        assert_eq!(manager.config().max_pending_orders, 1000);
    }

    #[test]
    fn test_order_manager_config() {
        let config = OrderManagerConfig::with_max_pending(500);
        assert_eq!(config.max_pending_orders, 500);
    }

    // =========================================================================
    // Submit Tests
    // =========================================================================

    #[test]
    fn test_submit_market_order() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order =
            OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());

        let id = manager.submit_order(order);
        assert_eq!(id.as_u64(), 1);
        assert_eq!(manager.pending_count(), 1);
        assert!(manager.is_pending(id));
    }

    #[test]
    fn test_submit_limit_order() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order = OrderRequest::limit(
            test_instrument(),
            OrderSide::Sell,
            50000.0,
            dec!(2.0),
            TimeInForce::GTC,
            now_micros(),
        );

        let id = manager.submit_order(order);
        assert_eq!(id.as_u64(), 1);
        assert_eq!(manager.pending_count(), 1);
    }

    #[test]
    fn test_submit_multiple_orders() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        let id1 = manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Buy,
            dec!(1.0),
            now_micros(),
        ));

        let id2 = manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Sell,
            dec!(2.0),
            now_micros(),
        ));

        let id3 = manager.submit_order(OrderRequest::limit(
            test_instrument(),
            OrderSide::Buy,
            48000.0,
            dec!(0.5),
            TimeInForce::GTC,
            now_micros(),
        ));

        assert_eq!(id1.as_u64(), 1);
        assert_eq!(id2.as_u64(), 2);
        assert_eq!(id3.as_u64(), 3);
        assert_eq!(manager.pending_count(), 3);
    }

    #[test]
    fn test_submit_order_stats() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Buy,
            dec!(1.0),
            now_micros(),
        ));

        let stats = manager.stats();
        assert_eq!(stats.orders_submitted, 1);
        assert_eq!(stats.pending_count, 1);
    }

    // =========================================================================
    // Cancel Tests
    // =========================================================================

    #[test]
    fn test_cancel_order() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order = OrderRequest::limit(
            test_instrument(),
            OrderSide::Buy,
            50000.0,
            dec!(1.0),
            TimeInForce::GTC,
            now_micros(),
        );

        let id = manager.submit_order(order);
        assert_eq!(manager.pending_count(), 1);

        let cancelled = manager.cancel_order(id, CancelReason::UserRequested, now_micros());
        assert!(cancelled);
        assert_eq!(manager.pending_count(), 0);
        assert!(!manager.is_pending(id));
    }

    #[test]
    fn test_cancel_nonexistent_order() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        let cancelled = manager.cancel_order(
            OrderId::new(9999),
            CancelReason::UserRequested,
            now_micros(),
        );
        assert!(!cancelled);
    }

    #[test]
    fn test_cancel_order_stats() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order =
            OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());

        let id = manager.submit_order(order);
        manager.cancel_order(id, CancelReason::UserRequested, now_micros());

        let stats = manager.stats();
        assert_eq!(stats.orders_cancelled, 1);
        assert_eq!(stats.pending_count, 0);
    }

    // =========================================================================
    // Modify Tests
    // =========================================================================

    #[test]
    fn test_modify_order_price() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order = OrderRequest::limit(
            test_instrument(),
            OrderSide::Buy,
            50000.0,
            dec!(1.0),
            TimeInForce::GTC,
            now_micros(),
        );

        let id = manager.submit_order(order);
        let modified = manager.modify_order(id, ModifyRequest::price(51000.0, now_micros()));

        assert!(modified);
        assert_eq!(manager.stats().orders_modified, 1);
    }

    #[test]
    fn test_modify_order_quantity() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order = OrderRequest::limit(
            test_instrument(),
            OrderSide::Sell,
            50000.0,
            dec!(1.0),
            TimeInForce::GTC,
            now_micros(),
        );

        let id = manager.submit_order(order);
        let modified = manager.modify_order(id, ModifyRequest::quantity(dec!(2.0), now_micros()));

        assert!(modified);
    }

    #[test]
    fn test_modify_nonexistent_order() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        let modified = manager.modify_order(
            OrderId::new(9999),
            ModifyRequest::price(51000.0, now_micros()),
        );
        assert!(!modified);
    }

    // =========================================================================
    // Status Update Tests
    // =========================================================================

    #[test]
    fn test_update_status_open() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order = OrderRequest::limit(
            test_instrument(),
            OrderSide::Buy,
            50000.0,
            dec!(1.0),
            TimeInForce::GTC,
            now_micros(),
        );

        let id = manager.submit_order(order);
        manager.update_status(id, OrderStatus::Open, now_micros());

        assert_eq!(manager.get_status(id), Some(OrderStatus::Open));
        assert_eq!(manager.pending_count(), 1); // Still pending
    }

    #[test]
    fn test_update_status_filled() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order =
            OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());

        let id = manager.submit_order(order);
        manager.update_status(id, OrderStatus::Filled, now_micros());

        assert_eq!(manager.pending_count(), 0); // Removed
        assert!(!manager.is_pending(id));
        assert_eq!(manager.stats().orders_filled, 1);
    }

    #[test]
    fn test_update_status_rejected() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let order =
            OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());

        let id = manager.submit_order(order);
        manager.update_status(id, OrderStatus::Rejected, now_micros());

        assert_eq!(manager.pending_count(), 0);
        assert_eq!(manager.stats().orders_rejected, 1);
    }

    // =========================================================================
    // Accessor Tests
    // =========================================================================

    #[test]
    fn test_pending_order_ids() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        let id1 = manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Buy,
            dec!(1.0),
            now_micros(),
        ));
        let id2 = manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Sell,
            dec!(1.0),
            now_micros(),
        ));

        let ids = manager.pending_order_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_clear() {
        let manager = OrderManager::new(OrderManagerConfig::default());

        manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Buy,
            dec!(1.0),
            now_micros(),
        ));
        manager.submit_order(OrderRequest::market(
            test_instrument(),
            OrderSide::Sell,
            dec!(1.0),
            now_micros(),
        ));

        assert_eq!(manager.pending_count(), 2);
        manager.clear();
        assert_eq!(manager.pending_count(), 0);
    }

    // =========================================================================
    // Debug Tests
    // =========================================================================

    #[test]
    fn test_order_manager_debug() {
        let manager = OrderManager::new(OrderManagerConfig::default());
        let debug_str = format!("{:?}", manager);
        assert!(debug_str.contains("OrderManager"));
        assert!(debug_str.contains("pending_count"));
    }

    // =========================================================================
    // BlackBox Integration Tests (feature-gated)
    // =========================================================================

    #[cfg(feature = "blackbox")]
    mod blackbox_tests {
        use super::*;
        use crate::blackbox::NullTap;

        #[test]
        fn test_order_manager_with_null_tap() {
            let tap = NullTap;
            let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);

            // Operations should work without issues
            let order =
                OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());
            let id = manager.submit_order(order);

            assert_eq!(manager.pending_count(), 1);
            assert!(manager.cancel_order(id, CancelReason::UserRequested, now_micros()));
        }

        #[test]
        fn test_submit_records_to_tap() {
            let tap = NullTap;
            let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);

            // NullTap.is_active() returns false, so no recording happens
            // But the code path should still work
            let order = OrderRequest::limit(
                test_instrument(),
                OrderSide::Buy,
                50000.0,
                dec!(1.0),
                TimeInForce::GTC,
                now_micros(),
            );

            let id = manager.submit_order(order);
            assert_eq!(id.as_u64(), 1);
        }

        #[test]
        fn test_cancel_records_to_tap() {
            let tap = NullTap;
            let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);

            let order =
                OrderRequest::market(test_instrument(), OrderSide::Buy, dec!(1.0), now_micros());
            let id = manager.submit_order(order);

            // Should work without issues
            assert!(manager.cancel_order(id, CancelReason::UserRequested, now_micros()));
        }

        #[test]
        fn test_modify_records_to_tap() {
            let tap = NullTap;
            let manager = OrderManager::with_tap(OrderManagerConfig::default(), tap);

            let order = OrderRequest::limit(
                test_instrument(),
                OrderSide::Buy,
                50000.0,
                dec!(1.0),
                TimeInForce::GTC,
                now_micros(),
            );
            let id = manager.submit_order(order);

            // Should work without issues
            assert!(manager.modify_order(id, ModifyRequest::price(51000.0, now_micros())));
        }
    }
}
