//! Deployment Tests
//!
//! Tests for production deployment including:
//! - Startup sequence
//! - Graceful shutdown
//! - Signal handling
//! - Rolling restart compatibility

use std::time::Duration;

use super::helpers::*;

// ============================================================================
// TEST 1: STARTUP SEQUENCE VALIDATION
// ============================================================================

#[test]
fn test_startup_sequence() {
    // ARRANGE: Service
    let service = MockService::new("astra-flash");

    // Verify initial state
    assert!(!service.is_ready());
    assert_eq!(*service.phase.lock().unwrap(), StartupPhase::Initializing);

    // ACT: Start service
    let result = service.start();

    // ASSERT: Startup succeeded
    assert!(result.is_ok());
    assert!(service.is_ready());
    assert_eq!(*service.phase.lock().unwrap(), StartupPhase::Ready);
}

// ============================================================================
// TEST 2: GRACEFUL SHUTDOWN
// ============================================================================

#[test]
fn test_shutdown_graceful() {
    // ARRANGE: Running service
    let service = MockService::new("astra-flash");
    service.start().unwrap();
    assert!(service.is_ready());

    // ACT: Graceful shutdown
    let result = service.shutdown(ShutdownSignal::Graceful);

    // ASSERT: Shutdown succeeded
    assert!(result.is_ok());
    assert!(!service.is_ready());
    assert!(service.is_shutdown());
}

// ============================================================================
// TEST 3: SIGNAL HANDLING (SIGTERM, SIGINT)
// ============================================================================

#[test]
fn test_signal_handling() {
    // ARRANGE: Running services
    let service_term = MockService::new("service-term");
    let service_int = MockService::new("service-int");

    service_term.start().unwrap();
    service_int.start().unwrap();

    // ACT: Handle different signals
    let result_term = service_term.shutdown(ShutdownSignal::Sigterm);
    let result_int = service_int.shutdown(ShutdownSignal::Sigint);

    // ASSERT: Both signals handled gracefully
    assert!(result_term.is_ok());
    assert!(result_int.is_ok());
    assert!(service_term.is_shutdown());
    assert!(service_int.is_shutdown());
}

// ============================================================================
// TEST 4: ROLLING RESTART COMPATIBILITY
// ============================================================================

#[test]
fn test_rolling_restart() {
    // ARRANGE: Simulate rolling restart with instance replacement
    let instance_count = 3;
    let mut old_instances: Vec<MockService> = (0..instance_count)
        .map(|i| {
            let svc = MockService::new(&format!("instance-{}", i));
            svc.start().unwrap();
            svc
        })
        .collect();

    let mut new_instances: Vec<MockService> = Vec::new();

    // Verify all running
    assert!(old_instances.iter().all(|s| s.is_ready()));

    // ACT: Rolling restart (one at a time with replacement)
    for i in 0..instance_count {
        // Start new instance first (blue-green pattern)
        let new_svc = MockService::new(&format!("instance-{}-new", i));
        new_svc.start().unwrap();
        new_instances.push(new_svc);

        // Count total running instances (old + new)
        let running = old_instances.iter().filter(|s| s.is_ready()).count()
            + new_instances.iter().filter(|s| s.is_ready()).count();

        // Should have at least original count running
        assert!(
            running >= instance_count,
            "Should have at least {} instances running: got {}",
            instance_count,
            running
        );

        // Then shutdown old instance
        old_instances[i].shutdown(ShutdownSignal::Graceful).unwrap();
    }

    // ASSERT: All old instances stopped, all new running
    assert!(old_instances.iter().all(|s| s.is_shutdown()));
    assert!(new_instances.iter().all(|s| s.is_ready()));
}
