//! Publisher module tests.
//!
//! Tests for Redis publishing functionality:
//! - Connection pool management
//! - Stream publishing
//! - Message batching
//! - Topic routing
//! - Dual output (orderbook + signal)
//! - Backpressure handling (try_send, dropped_frames)
//! - Shadow mode (Batch 5.1)
//! - Verification (Batch 5.2)
//! - Cutover (Batch 6.1)

mod backpressure_test;
mod batch_test;
mod cutover_test;
mod dual_test;
mod pool_test;
mod shadow_mode_test;
mod stream_test;
mod topics_test;
mod verification_test;
