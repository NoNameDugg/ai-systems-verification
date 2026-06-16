"""
Provider Health Monitor
=======================

Tracks provider health metrics for monitoring and alerting.
"""

import threading
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Dict, List, Optional


@dataclass
class OperationMetric:
    """Single operation metric."""

    timestamp: datetime
    success: bool
    latency_ms: float
    error: Optional[str]


@dataclass
class ProviderHealth:
    """Health status for a provider."""

    provider_id: str
    is_healthy: bool
    last_success: Optional[datetime]
    last_failure: Optional[datetime]
    consecutive_failures: int
    average_latency_ms: float
    error_rate_percent: float


class ProviderHealthMonitor:
    """
    Monitors health of position providers.

    Features:
    - Tracks success/failure rates
    - Measures latency
    - Provides health scores
    - Thread-safe operations

    Example:
        >>> monitor = ProviderHealthMonitor()
        >>> monitor.record_success("oanda", latency_ms=50.0)
        >>> health = monitor.get_health("oanda")
        >>> print(f"Healthy: {health.is_healthy}")
    """

    def __init__(
        self, window_size: int = 100, unhealthy_threshold: float = 0.5
    ) -> None:
        """
        Initialize health monitor.

        Args:
            window_size: Number of operations to track per provider
            unhealthy_threshold: Error rate threshold for unhealthy status
        """
        self._metrics: Dict[str, deque] = {}
        self._window_size = window_size
        self._unhealthy_threshold = unhealthy_threshold
        self._lock = threading.RLock()

    def record_success(self, provider_id: str, latency_ms: float) -> None:
        """
        Record successful operation.

        Args:
            provider_id: Provider identifier
            latency_ms: Operation latency in milliseconds
        """
        metric = OperationMetric(
            timestamp=datetime.now(timezone.utc),
            success=True,
            latency_ms=latency_ms,
            error=None,
        )
        self._record(provider_id, metric)

    def record_failure(self, provider_id: str, error: Exception) -> None:
        """
        Record failed operation.

        Args:
            provider_id: Provider identifier
            error: Exception that occurred
        """
        metric = OperationMetric(
            timestamp=datetime.now(timezone.utc),
            success=False,
            latency_ms=0.0,
            error=str(error),
        )
        self._record(provider_id, metric)

    def get_health(self, provider_id: str) -> ProviderHealth:
        """
        Get current health status for provider.

        Args:
            provider_id: Provider identifier

        Returns:
            ProviderHealth with current metrics
        """
        with self._lock:
            metrics = list(self._metrics.get(provider_id, []))

        if not metrics:
            return ProviderHealth(
                provider_id=provider_id,
                is_healthy=True,
                last_success=None,
                last_failure=None,
                consecutive_failures=0,
                average_latency_ms=0.0,
                error_rate_percent=0.0,
            )

        successes = [m for m in metrics if m.success]
        failures = [m for m in metrics if not m.success]

        error_rate = len(failures) / len(metrics)
        avg_latency = (
            sum(m.latency_ms for m in successes) / len(successes) if successes else 0.0
        )

        # Count consecutive failures from end
        consecutive = 0
        for m in reversed(metrics):
            if not m.success:
                consecutive += 1
            else:
                break

        return ProviderHealth(
            provider_id=provider_id,
            is_healthy=error_rate < self._unhealthy_threshold,
            last_success=successes[-1].timestamp if successes else None,
            last_failure=failures[-1].timestamp if failures else None,
            consecutive_failures=consecutive,
            average_latency_ms=avg_latency,
            error_rate_percent=error_rate * 100,
        )

    def _record(self, provider_id: str, metric: OperationMetric) -> None:
        """
        Record a metric for a provider.

        Args:
            provider_id: Provider identifier
            metric: Metric to record
        """
        with self._lock:
            if provider_id not in self._metrics:
                self._metrics[provider_id] = deque(maxlen=self._window_size)
            self._metrics[provider_id].append(metric)
