"""
Fallback Position Provider
==========================

Provider with automatic failover between primary and fallback.
"""

import logging
import threading
from datetime import datetime, timedelta, timezone
from typing import List, Optional

from src.models import Position
from src.providers.base import PositionProvider


logger = logging.getLogger(__name__)


class FallbackPositionProvider(PositionProvider):
    """
    Provider with automatic fallback on failure.

    Tries primary provider first, falls back to secondary
    if primary fails. Useful for high availability.

    Features:
    - Automatic failover on primary failure
    - Configurable failure threshold
    - Automatic recovery when primary returns
    - Thread-safe operations

    Example:
        >>> primary = OandaPositionProvider(config)
        >>> fallback = SimulatorPositionProvider(portfolio)
        >>> provider = FallbackPositionProvider(primary, fallback)
        >>> positions = provider.fetch_positions()
    """

    def __init__(
        self,
        primary: PositionProvider,
        fallback: PositionProvider,
        failure_threshold: int = 3,
        recovery_interval_seconds: float = 60.0,
    ) -> None:
        """
        Initialize fallback provider.

        Args:
            primary: Primary provider (preferred)
            fallback: Fallback provider (used on primary failure)
            failure_threshold: Failures before switching to fallback
            recovery_interval_seconds: How often to retry primary
        """
        self._primary = primary
        self._fallback = fallback
        self._failure_threshold = failure_threshold
        self._recovery_interval = timedelta(seconds=recovery_interval_seconds)

        self._primary_failures = 0
        self._using_fallback = False
        self._last_primary_attempt: Optional[datetime] = None
        self._last_fetch_time: Optional[datetime] = None
        self._lock = threading.RLock()

    def fetch_positions(self) -> List[Position]:
        """
        Fetch positions with automatic fallback.

        Returns:
            List of Position objects from active provider
        """
        with self._lock:
            # Check for primary recovery
            if self._using_fallback and self._should_try_recovery():
                if self._try_primary_recovery():
                    self._using_fallback = False
                    self._primary_failures = 0
                    logger.info("Primary provider recovered")

            # Try primary if not in fallback mode
            if not self._using_fallback:
                try:
                    positions = self._primary.fetch_positions()
                    self._primary_failures = 0
                    self._last_fetch_time = datetime.now(timezone.utc)
                    return positions
                except Exception as e:
                    self._primary_failures += 1
                    logger.warning(
                        f"Primary provider failed ({self._primary_failures}/"
                        f"{self._failure_threshold}): {e}"
                    )
                    if self._primary_failures >= self._failure_threshold:
                        self._using_fallback = True
                        self._last_primary_attempt = datetime.now(timezone.utc)
                        logger.warning("Switching to fallback provider")

            # Use fallback
            positions = self._fallback.fetch_positions()
            self._last_fetch_time = datetime.now(timezone.utc)
            return positions

    def is_available(self) -> bool:
        """
        Check if any provider is available.

        Returns:
            True if either primary or fallback is available
        """
        return self._primary.is_available() or self._fallback.is_available()

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Get time of last successful fetch."""
        return self._last_fetch_time

    @property
    def active_provider(self) -> str:
        """Get name of currently active provider."""
        return "fallback" if self._using_fallback else "primary"

    def force_fallback(self) -> None:
        """
        Force switch to fallback provider.

        Useful for testing or manual intervention.
        """
        with self._lock:
            self._using_fallback = True
            self._last_primary_attempt = datetime.now(timezone.utc)

    def force_primary(self) -> None:
        """
        Force switch back to primary provider.

        Useful for testing or manual recovery.
        """
        with self._lock:
            self._using_fallback = False
            self._primary_failures = 0

    def _should_try_recovery(self) -> bool:
        """
        Check if recovery interval has elapsed.

        Returns:
            True if should try primary again
        """
        if self._last_primary_attempt is None:
            return True
        elapsed = datetime.now(timezone.utc) - self._last_primary_attempt
        return elapsed >= self._recovery_interval

    def _try_primary_recovery(self) -> bool:
        """
        Attempt to recover primary provider.

        Returns:
            True if primary is working again
        """
        self._last_primary_attempt = datetime.now(timezone.utc)
        try:
            self._primary.fetch_positions()
            return True
        except Exception as e:
            logger.debug(f"Primary recovery failed: {e}")
            return False
