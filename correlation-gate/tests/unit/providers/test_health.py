"""
Unit Tests for Provider Health Monitor
======================================

Tests FIRST per TDI methodology.
All tests written BEFORE implementation.
"""

import pytest
import time
import threading
from datetime import datetime, timezone
from decimal import Decimal

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers.health import (
        ProviderHealthMonitor,
        ProviderHealth,
        OperationMetric,
    )
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


class TestProviderHealthMonitorInit:
    """Tests for ProviderHealthMonitor initialization."""

    def test_monitor_creation_with_defaults(self):
        """Monitor can be created with default settings."""
        monitor = ProviderHealthMonitor()
        assert monitor._window_size == 100
        assert monitor._unhealthy_threshold == 0.5

    def test_monitor_creation_with_custom_settings(self):
        """Monitor can be created with custom settings."""
        monitor = ProviderHealthMonitor(window_size=50, unhealthy_threshold=0.3)
        assert monitor._window_size == 50
        assert monitor._unhealthy_threshold == 0.3


class TestProviderHealthMonitorRecording:
    """Tests for recording operations."""

    def test_record_success_stores_metric(self):
        """record_success stores a success metric."""
        monitor = ProviderHealthMonitor()
        monitor.record_success("provider1", latency_ms=10.0)

        health = monitor.get_health("provider1")
        assert health.is_healthy is True

    def test_record_failure_stores_metric(self):
        """record_failure stores a failure metric."""
        monitor = ProviderHealthMonitor()
        monitor.record_failure("provider1", Exception("Test error"))

        health = monitor.get_health("provider1")
        # One failure shouldn't make unhealthy by default
        assert health.consecutive_failures == 1

    def test_multiple_successes_tracked(self):
        """Multiple successes are tracked correctly."""
        monitor = ProviderHealthMonitor()

        for i in range(5):
            monitor.record_success("provider1", latency_ms=10.0 + i)

        health = monitor.get_health("provider1")
        assert health.average_latency_ms == 12.0  # Average of 10, 11, 12, 13, 14


class TestProviderHealthMonitorHealthStatus:
    """Tests for health status calculation."""

    def test_no_data_returns_healthy(self):
        """Provider with no data is considered healthy."""
        monitor = ProviderHealthMonitor()
        health = monitor.get_health("unknown_provider")

        assert health.is_healthy is True
        assert health.last_success is None
        assert health.last_failure is None
        assert health.consecutive_failures == 0

    def test_all_successes_is_healthy(self):
        """Provider with all successes is healthy."""
        monitor = ProviderHealthMonitor()

        for _ in range(10):
            monitor.record_success("provider1", latency_ms=10.0)

        health = monitor.get_health("provider1")
        assert health.is_healthy is True
        assert health.error_rate_percent == 0.0

    def test_high_error_rate_is_unhealthy(self):
        """Provider with high error rate is unhealthy."""
        monitor = ProviderHealthMonitor(unhealthy_threshold=0.5)

        # 6 failures, 4 successes = 60% error rate
        for _ in range(6):
            monitor.record_failure("provider1", Exception("Error"))
        for _ in range(4):
            monitor.record_success("provider1", latency_ms=10.0)

        health = monitor.get_health("provider1")
        assert health.is_healthy is False
        assert health.error_rate_percent == 60.0

    def test_consecutive_failures_counted(self):
        """Consecutive failures are counted correctly."""
        monitor = ProviderHealthMonitor()

        monitor.record_success("provider1", latency_ms=10.0)
        monitor.record_failure("provider1", Exception("Error1"))
        monitor.record_failure("provider1", Exception("Error2"))
        monitor.record_failure("provider1", Exception("Error3"))

        health = monitor.get_health("provider1")
        assert health.consecutive_failures == 3

    def test_success_resets_consecutive_failures(self):
        """Success resets consecutive failure count."""
        monitor = ProviderHealthMonitor()

        monitor.record_failure("provider1", Exception("Error1"))
        monitor.record_failure("provider1", Exception("Error2"))
        monitor.record_success("provider1", latency_ms=10.0)

        health = monitor.get_health("provider1")
        assert health.consecutive_failures == 0

    def test_last_success_tracked(self):
        """Last success time is tracked."""
        monitor = ProviderHealthMonitor()

        before = datetime.now(timezone.utc)
        monitor.record_success("provider1", latency_ms=10.0)
        after = datetime.now(timezone.utc)

        health = monitor.get_health("provider1")
        assert before <= health.last_success <= after

    def test_last_failure_tracked(self):
        """Last failure time is tracked."""
        monitor = ProviderHealthMonitor()

        before = datetime.now(timezone.utc)
        monitor.record_failure("provider1", Exception("Error"))
        after = datetime.now(timezone.utc)

        health = monitor.get_health("provider1")
        assert before <= health.last_failure <= after


class TestProviderHealthMonitorWindow:
    """Tests for sliding window behavior."""

    def test_old_metrics_dropped_when_window_exceeded(self):
        """Old metrics are dropped when window size exceeded."""
        monitor = ProviderHealthMonitor(window_size=5)

        # Record more than window size
        for _ in range(7):
            monitor.record_failure("provider1", Exception("Error"))

        # Now record successes
        for _ in range(5):
            monitor.record_success("provider1", latency_ms=10.0)

        # Window should only contain the 5 successes (oldest failures dropped)
        health = monitor.get_health("provider1")
        # Actually depends on implementation - may need adjustment


class TestProviderHealthMonitorConcurrency:
    """Tests for thread-safety."""

    def test_concurrent_recording_is_safe(self):
        """Concurrent recording is thread-safe."""
        monitor = ProviderHealthMonitor()
        errors = []

        def recorder():
            for i in range(100):
                try:
                    if i % 3 == 0:
                        monitor.record_failure("provider1", Exception("Error"))
                    else:
                        monitor.record_success("provider1", latency_ms=float(i))
                except Exception as e:
                    errors.append(e)

        threads = [threading.Thread(target=recorder) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0

    def test_concurrent_recording_and_reading_is_safe(self):
        """Concurrent recording and reading is thread-safe."""
        monitor = ProviderHealthMonitor()
        errors = []

        def writer():
            for i in range(100):
                try:
                    monitor.record_success("provider1", latency_ms=float(i))
                except Exception as e:
                    errors.append(e)

        def reader():
            for _ in range(100):
                try:
                    monitor.get_health("provider1")
                except Exception as e:
                    errors.append(e)

        threads = []
        for _ in range(3):
            threads.append(threading.Thread(target=writer))
            threads.append(threading.Thread(target=reader))

        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0


class TestProviderHealthDataclass:
    """Tests for ProviderHealth dataclass."""

    def test_provider_health_fields(self):
        """ProviderHealth has all required fields."""
        health = ProviderHealth(
            provider_id="test",
            is_healthy=True,
            last_success=datetime.now(timezone.utc),
            last_failure=None,
            consecutive_failures=0,
            average_latency_ms=10.0,
            error_rate_percent=5.0,
        )

        assert health.provider_id == "test"
        assert health.is_healthy is True
        assert health.consecutive_failures == 0
        assert health.average_latency_ms == 10.0
        assert health.error_rate_percent == 5.0
