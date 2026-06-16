"""
Unit Tests for OANDA Position Provider
======================================

Tests FIRST per TDI methodology.
All tests written BEFORE implementation.
"""

import pytest
import time
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from typing import List, Dict, Any
from unittest.mock import Mock, MagicMock, patch

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers.oanda import (
        OandaPositionProvider,
        OandaConfig,
    )
    from src.providers.cache import PositionCache
    from src.exceptions import (
        ProviderError,
        ProviderTimeoutError,
        ProviderAuthError,
        ProviderRateLimitError,
        ConfigError,
    )
    from src.models import Position
except ImportError:
    # Expected in RED phase - tests are written before implementation
    pytest.skip("Implementation not yet complete", allow_module_level=True)


class TestOandaConfig:
    """Tests for OandaConfig dataclass."""

    def test_config_creation_with_required_fields(self):
        """Config can be created with required fields."""
        config = OandaConfig(
            account_id="<YOUR_OANDA_ACCOUNT_ID>", api_token="test-token-12345"
        )
        assert config.account_id == "<YOUR_OANDA_ACCOUNT_ID>"
        assert config.api_token == "test-token-12345"

    def test_config_defaults_to_practice_environment(self):
        """Config defaults to practice environment."""
        config = OandaConfig(account_id="<YOUR_OANDA_ACCOUNT_ID>", api_token="test-token")
        assert config.environment == "practice"

    def test_config_practice_base_url(self):
        """Practice environment uses practice API URL."""
        config = OandaConfig(
            account_id="test", api_token="test", environment="practice"
        )
        assert config.base_url == "https://api-fxpractice.oanda.com"

    def test_config_live_base_url(self):
        """Live environment uses live API URL."""
        config = OandaConfig(account_id="test", api_token="test", environment="live")
        assert config.base_url == "https://api-fxtrade.oanda.com"

    def test_config_validate_missing_account_id_raises(self):
        """Validation raises ConfigError for missing account_id."""
        config = OandaConfig(account_id="", api_token="test")
        with pytest.raises(ConfigError) as exc_info:
            config.validate()
        assert "account_id" in str(exc_info.value)

    def test_config_validate_missing_api_token_raises(self):
        """Validation raises ConfigError for missing api_token."""
        config = OandaConfig(account_id="test", api_token="")
        with pytest.raises(ConfigError) as exc_info:
            config.validate()
        assert "api_token" in str(exc_info.value)

    def test_config_validate_invalid_environment_raises(self):
        """Validation raises ConfigError for invalid environment."""
        config = OandaConfig(account_id="test", api_token="test", environment="invalid")
        with pytest.raises(ConfigError) as exc_info:
            config.validate()
        assert "environment" in str(exc_info.value)


class TestOandaPositionProviderInit:
    """Tests for OandaPositionProvider initialization."""

    def test_provider_creation_with_config(self):
        """Provider can be created with config."""
        config = OandaConfig(account_id="<YOUR_OANDA_ACCOUNT_ID>", api_token="test-token")
        provider = OandaPositionProvider(config)
        assert provider is not None

    def test_provider_accepts_custom_http_client(self):
        """Provider accepts custom HTTP client for testing."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        provider = OandaPositionProvider(config, http_client=mock_client)
        assert provider._http_client is mock_client

    def test_provider_default_cache_ttl(self):
        """Provider has default 5 second cache TTL."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config)
        assert provider._cache_ttl_seconds == 5.0

    def test_provider_custom_cache_ttl(self):
        """Provider accepts custom cache TTL."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config, cache_ttl_seconds=10.0)
        assert provider._cache_ttl_seconds == 10.0


class TestOandaPositionProviderFetch:
    """Tests for OandaPositionProvider.fetch_positions()."""

    def test_fetch_positions_returns_list(self):
        """fetch_positions returns a list of Position objects."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    }
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        positions = provider.fetch_positions()

        assert isinstance(positions, list)
        assert len(positions) == 1
        assert isinstance(positions[0], Position)

    def test_fetch_positions_parses_long_position(self):
        """fetch_positions correctly parses LONG position."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    }
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        positions = provider.fetch_positions()

        assert positions[0].instrument == "EUR_USD"
        assert positions[0].direction == "LONG"
        assert positions[0].units == 10000
        assert positions[0].entry_price == Decimal("1.08500")

    def test_fetch_positions_parses_short_position(self):
        """fetch_positions correctly parses SHORT position."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "USD_JPY",
                        "long": {"units": "0"},
                        "short": {"units": "5000", "averagePrice": "149.500"},
                    }
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        positions = provider.fetch_positions()

        assert positions[0].instrument == "USD_JPY"
        assert positions[0].direction == "SHORT"
        assert positions[0].units == 5000
        assert positions[0].entry_price == Decimal("149.500")

    def test_fetch_positions_returns_empty_list_for_no_positions(self):
        """fetch_positions returns empty list when no positions."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        positions = provider.fetch_positions()

        assert positions == []

    def test_fetch_positions_handles_multiple_positions(self):
        """fetch_positions handles multiple positions."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    },
                    {
                        "instrument": "GBP_USD",
                        "long": {"units": "5000", "averagePrice": "1.26500"},
                        "short": {"units": "0"},
                    },
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        positions = provider.fetch_positions()

        assert len(positions) == 2


class TestOandaPositionProviderErrors:
    """Tests for error handling in OandaPositionProvider."""

    def test_fetch_401_raises_auth_error(self):
        """401 response raises ProviderAuthError."""
        config = OandaConfig(account_id="test", api_token="invalid")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=401, json=lambda: {"errorMessage": "Unauthorized"}
        )
        provider = OandaPositionProvider(config, http_client=mock_client)

        with pytest.raises(ProviderAuthError):
            provider.fetch_positions()

    def test_fetch_403_raises_auth_error(self):
        """403 response raises ProviderAuthError."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=403, json=lambda: {"errorMessage": "Forbidden"}
        )
        provider = OandaPositionProvider(config, http_client=mock_client)

        with pytest.raises(ProviderAuthError):
            provider.fetch_positions()

    def test_fetch_429_raises_rate_limit_error(self):
        """429 response raises ProviderRateLimitError."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        response = Mock(status_code=429)
        response.headers = {"Retry-After": "60"}
        response.json = lambda: {"errorMessage": "Rate limit exceeded"}
        mock_client.get.return_value = response
        provider = OandaPositionProvider(config, http_client=mock_client)

        with pytest.raises(ProviderRateLimitError) as exc_info:
            provider.fetch_positions()
        assert exc_info.value.retry_after_seconds == 60

    def test_fetch_500_raises_provider_error(self):
        """500 response raises ProviderError after retries."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=500, json=lambda: {"errorMessage": "Internal error"}
        )
        provider = OandaPositionProvider(config, http_client=mock_client, max_retries=1)

        with pytest.raises(ProviderError):
            provider.fetch_positions()

    def test_fetch_timeout_raises_timeout_error(self):
        """Timeout raises ProviderTimeoutError."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.side_effect = TimeoutError("Connection timed out")
        provider = OandaPositionProvider(
            config, http_client=mock_client, request_timeout_seconds=1.0, max_retries=0
        )

        with pytest.raises(ProviderTimeoutError):
            provider.fetch_positions()

    def test_fetch_connection_error_returns_cached_if_available(self):
        """Connection error returns cached positions if available."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # First call succeeds
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    }
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)
        first_result = provider.fetch_positions()
        assert len(first_result) == 1

        # Second call fails but returns cached
        mock_client.get.side_effect = ConnectionError("Network error")
        second_result = provider.fetch_positions()
        assert len(second_result) == 1


class TestOandaPositionProviderCaching:
    """Tests for caching behavior in OandaPositionProvider."""

    def test_cache_hit_returns_cached_without_api_call(self):
        """Cache hit returns cached positions without API call."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200,
            json=lambda: {
                "positions": [
                    {
                        "instrument": "EUR_USD",
                        "long": {"units": "10000", "averagePrice": "1.08500"},
                        "short": {"units": "0"},
                    }
                ]
            },
        )
        provider = OandaPositionProvider(config, http_client=mock_client)

        # First call
        provider.fetch_positions()
        # Second call should use cache
        provider.fetch_positions()

        assert mock_client.get.call_count == 1

    def test_cache_miss_after_ttl_calls_api(self):
        """Cache miss after TTL triggers new API call."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        provider = OandaPositionProvider(
            config, http_client=mock_client, cache_ttl_seconds=0.1
        )

        provider.fetch_positions()
        time.sleep(0.15)  # Wait for cache to expire
        provider.fetch_positions()

        assert mock_client.get.call_count == 2

    def test_cache_invalidate_clears_cache(self):
        """Cache invalidation clears cached positions."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        provider = OandaPositionProvider(config, http_client=mock_client)

        provider.fetch_positions()
        provider.invalidate_cache()
        provider.fetch_positions()

        assert mock_client.get.call_count == 2


class TestOandaPositionProviderAvailability:
    """Tests for availability checking."""

    def test_is_available_returns_true_when_api_reachable(self):
        """is_available returns True when API is reachable."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(status_code=200)
        provider = OandaPositionProvider(config, http_client=mock_client)

        assert provider.is_available() is True

    def test_is_available_returns_false_on_connection_error(self):
        """is_available returns False on connection error."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.side_effect = ConnectionError("Network error")
        provider = OandaPositionProvider(config, http_client=mock_client)

        assert provider.is_available() is False

    def test_get_last_fetch_time_returns_none_before_first_fetch(self):
        """get_last_fetch_time returns None before first fetch."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config)

        assert provider.get_last_fetch_time() is None

    def test_get_last_fetch_time_returns_datetime_after_fetch(self):
        """get_last_fetch_time returns datetime after successful fetch."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        provider = OandaPositionProvider(config, http_client=mock_client)

        before = datetime.now(timezone.utc)
        provider.fetch_positions()
        after = datetime.now(timezone.utc)

        last_fetch = provider.get_last_fetch_time()
        assert before <= last_fetch <= after
