//! Health Check Tests
//!
//! Tests for production health monitoring including:
//! - Healthy state detection
//! - Degraded state detection
//! - Unhealthy state detection
//! - Dependency health checks
//! - Health metrics exposure
//! - Recovery after failure

use super::helpers::*;

// ============================================================================
// TEST 1: ALL COMPONENTS HEALTHY
// ============================================================================

#[test]
fn test_health_check_healthy() {
    // ARRANGE: All healthy components
    let mut aggregator = HealthAggregator::new();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::healthy("redis"));
    aggregator.add(ComponentHealth::healthy("orderbook"));

    // ACT: Check overall health
    let status = aggregator.overall_status();

    // ASSERT: System is healthy
    assert_eq!(status, HealthStatus::Healthy);
    assert!(aggregator.is_healthy());
}

// ============================================================================
// TEST 2: DEGRADED STATE DETECTION
// ============================================================================

#[test]
fn test_health_check_degraded() {
    // ARRANGE: One degraded component
    let mut aggregator = HealthAggregator::new();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::degraded("redis", "High latency"));
    aggregator.add(ComponentHealth::healthy("orderbook"));

    // ACT: Check overall health
    let status = aggregator.overall_status();

    // ASSERT: System is degraded
    assert_eq!(status, HealthStatus::Degraded);
    assert!(!aggregator.is_healthy());
}

// ============================================================================
// TEST 3: UNHEALTHY STATE DETECTION
// ============================================================================

#[test]
fn test_health_check_unhealthy() {
    // ARRANGE: One unhealthy component
    let mut aggregator = HealthAggregator::new();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::unhealthy("redis", "Connection failed"));
    aggregator.add(ComponentHealth::healthy("orderbook"));

    // ACT: Check overall health
    let status = aggregator.overall_status();

    // ASSERT: System is unhealthy
    assert_eq!(status, HealthStatus::Unhealthy);
    assert!(!aggregator.is_healthy());
}

// ============================================================================
// TEST 4: DEPENDENCY HEALTH CHECKS
// ============================================================================

#[test]
fn test_health_check_dependencies() {
    // ARRANGE: Check each dependency type
    let dependencies = vec![
        ("websocket", true), // Connected
        ("redis", true),     // Connected
        ("exchange", true),  // API accessible
    ];

    let mut aggregator = HealthAggregator::new();

    // ACT: Add health for each dependency
    for (name, healthy) in dependencies {
        if healthy {
            aggregator.add(ComponentHealth::healthy(name));
        } else {
            aggregator.add(ComponentHealth::unhealthy(name, "Not connected"));
        }
    }

    // ASSERT: All dependencies healthy
    assert!(aggregator.is_healthy());
    assert_eq!(aggregator.components.len(), 3);
}

// ============================================================================
// TEST 5: HEALTH METRICS EXPOSURE
// ============================================================================

#[test]
fn test_health_check_metrics() {
    // ARRANGE: Components with various states
    let mut aggregator = HealthAggregator::new();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::degraded("redis", "Slow"));
    aggregator.add(ComponentHealth::healthy("orderbook"));

    // ACT: Calculate metrics
    let total = aggregator.components.len();
    let healthy_count = aggregator
        .components
        .iter()
        .filter(|c| c.status == HealthStatus::Healthy)
        .count();
    let degraded_count = aggregator
        .components
        .iter()
        .filter(|c| c.status == HealthStatus::Degraded)
        .count();
    let unhealthy_count = aggregator
        .components
        .iter()
        .filter(|c| c.status == HealthStatus::Unhealthy)
        .count();

    // ASSERT: Metrics are correct
    assert_eq!(total, 3);
    assert_eq!(healthy_count, 2);
    assert_eq!(degraded_count, 1);
    assert_eq!(unhealthy_count, 0);
}

// ============================================================================
// TEST 6: HEALTH RECOVERY AFTER FAILURE
// ============================================================================

#[test]
fn test_health_check_recovery() {
    // ARRANGE: Initially unhealthy
    let mut aggregator = HealthAggregator::new();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::unhealthy("redis", "Down"));

    assert!(!aggregator.is_healthy());

    // ACT: Simulate recovery by replacing component health
    aggregator.components.clear();
    aggregator.add(ComponentHealth::healthy("websocket"));
    aggregator.add(ComponentHealth::healthy("redis"));

    // ASSERT: System recovered
    assert!(aggregator.is_healthy());
    assert_eq!(aggregator.overall_status(), HealthStatus::Healthy);
}
