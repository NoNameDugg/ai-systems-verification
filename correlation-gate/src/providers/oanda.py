"""
OANDA Position Provider
=======================

Fetches positions from OANDA REST API v20.
"""

import logging
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal
from typing import Any, Dict, List, Optional

from src.models import Position
from src.providers.base import PositionProvider
from src.providers.cache import PositionCache
from src.exceptions import (
    ProviderError,
    ProviderTimeoutError,
    ProviderAuthError,
    ProviderRateLimitError,
    ConfigError,
)


logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class OandaConfig:
    """
    OANDA API configuration.

    Attributes:
        account_id: OANDA account identifier
        api_token: API authentication token
        environment: 'practice' or 'live'
    """

    account_id: str
    api_token: str
    environment: str = "practice"

    @property
    def base_url(self) -> str:
        """Get API base URL for environment."""
        if self.environment == "live":
            return "https://api-fxtrade.oanda.com"
        return "https://api-fxpractice.oanda.com"

    def validate(self) -> None:
        """
        Validate configuration.

        Raises:
            ConfigError: If configuration is invalid
        """
        if not self.account_id:
            raise ConfigError("account_id", None, "account_id is required")
        if not self.api_token:
            raise ConfigError("api_token", None, "api_token is required")
        if self.environment not in ("practice", "live"):
            raise ConfigError(
                "environment", self.environment, "must be 'practice' or 'live'"
            )


class OandaPositionProvider(PositionProvider):
    """
    Position provider for OANDA REST API v20.

    Features:
    - Response caching with TTL
    - Automatic retry with exponential backoff
    - Rate limit handling
    - Timeout enforcement

    Example:
        >>> config = OandaConfig(
        ...     account_id="<YOUR_OANDA_ACCOUNT_ID>",
        ...     api_token="your-api-token",
        ...     environment="practice"
        ... )
        >>> provider = OandaPositionProvider(config)
        >>> positions = provider.fetch_positions()
    """

    def __init__(
        self,
        config: OandaConfig,
        http_client: Optional[Any] = None,
        cache_ttl_seconds: float = 5.0,
        request_timeout_seconds: float = 10.0,
        max_retries: int = 3,
        retry_backoff_factor: float = 0.5,
    ) -> None:
        """
        Initialize OANDA position provider.

        Args:
            config: OANDA API configuration
            http_client: Optional HTTP client (for testing)
            cache_ttl_seconds: Cache time-to-live
            request_timeout_seconds: HTTP request timeout
            max_retries: Maximum retry attempts
            retry_backoff_factor: Exponential backoff multiplier
        """
        self._config = config
        self._http_client = http_client
        self._cache_ttl_seconds = cache_ttl_seconds
        self._request_timeout_seconds = request_timeout_seconds
        self._max_retries = max_retries
        self._retry_backoff_factor = retry_backoff_factor
        self._cache = PositionCache(ttl_seconds=cache_ttl_seconds)
        self._last_fetch_time: Optional[datetime] = None

    def fetch_positions(self) -> List[Position]:
        """
        Fetch current open positions from OANDA.

        Returns:
            List of Position objects

        Raises:
            ProviderError: On non-recoverable API errors
            ProviderTimeoutError: On timeout
            ProviderAuthError: On authentication failure
            ProviderRateLimitError: On rate limit exceeded
        """
        # Check cache first
        cached = self._cache.get()
        if cached is not None:
            positions, is_stale = cached
            if not is_stale:
                return positions

        # Fetch from API
        try:
            positions = self._fetch_from_api()
            self._cache.set(positions)
            self._last_fetch_time = datetime.now(timezone.utc)
            return positions
        except (ProviderAuthError, ProviderRateLimitError):
            # Don't use cache for auth/rate limit errors
            raise
        except Exception as e:
            # On other errors, try to return cached data
            if cached is not None:
                logger.warning(f"OANDA API error, returning stale cache: {e}")
                return cached[0]
            raise

    def is_available(self) -> bool:
        """Check if OANDA API is reachable."""
        if self._http_client is None:
            return True  # Assume available without client

        try:
            response = self._http_client.get(
                f"{self._config.base_url}/v3/accounts/{self._config.account_id}",
                timeout=5.0,
            )
            return response.status_code == 200
        except Exception:
            return False

    def get_last_fetch_time(self) -> Optional[datetime]:
        """Get time of last successful fetch."""
        return self._last_fetch_time

    def invalidate_cache(self) -> None:
        """Invalidate the position cache."""
        self._cache.invalidate()

    def _fetch_from_api(self) -> List[Position]:
        """
        Fetch positions from OANDA API.

        Returns:
            List of Position objects

        Raises:
            Various provider errors based on HTTP response
        """
        if self._http_client is None:
            return []  # No client configured

        url = (
            f"{self._config.base_url}/v3/accounts/"
            f"{self._config.account_id}/openPositions"
        )

        last_error: Optional[Exception] = None

        for attempt in range(self._max_retries + 1):
            try:
                response = self._http_client.get(
                    url, timeout=self._request_timeout_seconds
                )

                if response.status_code == 200:
                    return self._parse_response(response.json())
                elif response.status_code == 401:
                    raise ProviderAuthError("OANDA", "Invalid API token")
                elif response.status_code == 403:
                    raise ProviderAuthError("OANDA", "Access forbidden")
                elif response.status_code == 429:
                    retry_after = self._parse_retry_after(response)
                    raise ProviderRateLimitError("OANDA", retry_after)
                elif 500 <= response.status_code < 600:
                    last_error = ProviderError(
                        "OANDA", f"Server error: {response.status_code}"
                    )
                    # Retry server errors
                    if attempt < self._max_retries:
                        time.sleep(self._retry_backoff_factor * (2**attempt))
                        continue
                    raise last_error
                else:
                    raise ProviderError(
                        "OANDA", f"Unexpected status: {response.status_code}"
                    )

            except (TimeoutError, ConnectionError) as e:
                last_error = ProviderTimeoutError(
                    "OANDA", self._request_timeout_seconds
                )
                if attempt < self._max_retries:
                    time.sleep(self._retry_backoff_factor * (2**attempt))
                    continue
                raise last_error from e

    def _parse_response(self, data: Dict[str, Any]) -> List[Position]:
        """
        Parse OANDA API response into Position objects.

        Args:
            data: Raw API response data

        Returns:
            List of Position objects
        """
        positions = []
        for pos in data.get("positions", []):
            instrument = pos.get("instrument", "")

            # Check long position
            long_data = pos.get("long", {})
            long_units = int(long_data.get("units", "0"))
            if long_units > 0:
                positions.append(
                    Position(
                        instrument=instrument,
                        direction="LONG",
                        units=long_units,
                        entry_price=Decimal(long_data.get("averagePrice", "0")),
                        entry_time=datetime.now(timezone.utc),
                        position_id=f"{instrument}_LONG",
                    )
                )

            # Check short position
            short_data = pos.get("short", {})
            short_units = abs(int(short_data.get("units", "0")))
            if short_units > 0:
                positions.append(
                    Position(
                        instrument=instrument,
                        direction="SHORT",
                        units=short_units,
                        entry_price=Decimal(short_data.get("averagePrice", "0")),
                        entry_time=datetime.now(timezone.utc),
                        position_id=f"{instrument}_SHORT",
                    )
                )

        return positions

    def _parse_retry_after(self, response: Any) -> Optional[float]:
        """Parse Retry-After header from response."""
        try:
            headers = getattr(response, "headers", {})
            retry_after = headers.get("Retry-After")
            if retry_after:
                return float(retry_after)
        except (ValueError, TypeError):
            pass
        return None
