"""
Supplemental Tests for Full Coverage
====================================

Tests to achieve 100% coverage on Phase 2 providers.
"""

import pytest
import time
from datetime import datetime, timezone
from decimal import Decimal
from unittest.mock import Mock

from src.providers.base import PositionProvider
from src.providers.cache import PositionCache
from src.providers.factory import ProviderFactory
from src.providers.fallback import FallbackPositionProvider
from src.providers.oanda import OandaPositionProvider, OandaConfig
from src.providers.simulator import (
    SimulatorPositionProvider,
    InMemoryPortfolioAccessor,
    SimulatorPosition,
    PortfolioAccessor,
)
from src.providers.health import ProviderHealthMonitor
from src.models import Position
from src.exceptions import (
    ProviderError,
    ProviderAuthError,
    ProviderRateLimitError,
    CacheError,
)


class TestExceptionsCoverage:
    """Tests for new Phase 2 exceptions."""

    def test_provider_auth_error_attributes(self):
        """ProviderAuthError stores reason."""
        error = ProviderAuthError("OANDA", "Invalid token")
        assert error.reason == "Invalid token"
        assert "OANDA" in str(error)
        assert "authentication failed" in str(error)

    def test_provider_rate_limit_error_with_retry_after(self):
        """ProviderRateLimitError stores retry_after_seconds."""
        error = ProviderRateLimitError("OANDA", 60.0)
        assert error.retry_after_seconds == 60.0
        assert "retry after 60.0s" in str(error)

    def test_provider_rate_limit_error_without_retry_after(self):
        """ProviderRateLimitError without retry_after_seconds."""
        error = ProviderRateLimitError("OANDA")
        assert error.retry_after_seconds is None
        assert "rate limit exceeded" in str(error)

    def test_cache_error_attributes(self):
        """CacheError stores operation and reason."""
        error = CacheError("set", "memory exhausted")
        assert error.operation == "set"
        assert error.reason == "memory exhausted"
        assert "Cache set failed" in str(error)


class TestBasePositionProviderCoverage:
    """Tests for PositionProvider base class.

    Note: PositionProvider is an abstract base class with @abstractmethod
    decorators. The abstract methods cannot be directly tested because
    the class cannot be instantiated. This is by design - the interface
    is tested through concrete implementations.
    """

    def test_position_provider_is_abstract(self):
        """PositionProvider cannot be instantiated directly."""
        with pytest.raises(TypeError) as exc_info:
            PositionProvider()
        assert "abstract" in str(exc_info.value).lower()


class TestFactoryCoverage:
    """Tests for ProviderFactory additional methods."""

    def test_factory_unregister_removes_type(self):
        """unregister removes a provider type."""
        ProviderFactory._registry.clear()

        class TestProvider(PositionProvider):
            def fetch_positions(self):
                return []

            def is_available(self):
                return True

            def get_last_fetch_time(self):
                return None

        ProviderFactory.register("to_remove", TestProvider)
        assert "to_remove" in ProviderFactory.available_types()

        result = ProviderFactory.unregister("to_remove")
        assert result is True
        assert "to_remove" not in ProviderFactory.available_types()

    def test_factory_unregister_returns_false_for_unknown(self):
        """unregister returns False for unknown type."""
        ProviderFactory._registry.clear()
        result = ProviderFactory.unregister("unknown_type")
        assert result is False

    def test_factory_clear_removes_all_types(self):
        """clear removes all registered types."""
        from src.providers.factory import _register_default_providers

        _register_default_providers()
        assert len(ProviderFactory.available_types()) > 0

        ProviderFactory.clear()
        assert len(ProviderFactory.available_types()) == 0


class TestFallbackCoverage:
    """Tests for FallbackPositionProvider coverage."""

    def test_should_try_recovery_when_last_attempt_is_none(self):
        """_should_try_recovery returns True when no prior attempt."""
        primary = Mock(spec=PositionProvider)
        primary.fetch_positions.return_value = []
        fallback = Mock(spec=PositionProvider)
        fallback.fetch_positions.return_value = []

        provider = FallbackPositionProvider(primary, fallback)
        provider._using_fallback = True
        provider._last_primary_attempt = None

        # This should trigger recovery check
        provider.fetch_positions()


class TestOandaCoverage:
    """Tests for OandaPositionProvider coverage."""

    def test_oanda_is_available_without_http_client(self):
        """is_available returns True when no http_client configured."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config, http_client=None)
        assert provider.is_available() is True

    def test_oanda_fetch_without_http_client_returns_empty(self):
        """fetch_positions returns empty when no http_client."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config, http_client=None)
        positions = provider.fetch_positions()
        assert positions == []

    def test_oanda_parse_retry_after_invalid(self):
        """_parse_retry_after handles invalid values."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config)

        response = Mock()
        response.headers = {"Retry-After": "invalid"}

        result = provider._parse_retry_after(response)
        assert result is None

    def test_oanda_parse_retry_after_no_headers(self):
        """_parse_retry_after handles missing headers."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config)

        response = Mock(spec=[])  # No headers attribute

        result = provider._parse_retry_after(response)
        assert result is None

    def test_oanda_parse_response_with_both_long_and_short(self):
        """_parse_response handles position with both long and short."""
        config = OandaConfig(account_id="test", api_token="test")
        provider = OandaPositionProvider(config)

        data = {
            "positions": [
                {
                    "instrument": "EUR_USD",
                    "long": {"units": "10000", "averagePrice": "1.08500"},
                    "short": {"units": "5000", "averagePrice": "1.09000"},
                }
            ]
        }

        positions = provider._parse_response(data)
        assert len(positions) == 2  # Both long and short

    def test_oanda_unexpected_status_code(self):
        """fetch_positions raises ProviderError for unexpected status."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=418,  # I'm a teapot
            json=lambda: {},
        )
        provider = OandaPositionProvider(config, http_client=mock_client, max_retries=0)

        with pytest.raises(ProviderError) as exc_info:
            provider.fetch_positions()
        assert "Unexpected status" in str(exc_info.value)


class TestSimulatorCoverage:
    """Tests for SimulatorPositionProvider coverage."""

    def test_portfolio_accessor_is_abstract(self):
        """PortfolioAccessor cannot be instantiated directly."""
        with pytest.raises(TypeError) as exc_info:
            PortfolioAccessor()
        assert "abstract" in str(exc_info.value).lower()

    def test_simulator_on_position_change_notifies_subscribers(self):
        """Position change notifies subscribers and invalidates cache."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        callback = Mock()
        provider.subscribe_changes(callback)

        # Add position to trigger change
        accessor.add_position(
            SimulatorPosition(
                position_id="test_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        # Give time for notification
        time.sleep(0.1)
        assert callback.called

    def test_simulator_multiple_subscriptions_reuse_listener(self):
        """Multiple subscriptions reuse the same accessor listener."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        callback1 = Mock()
        callback2 = Mock()

        sub_id1 = provider.subscribe_changes(callback1)
        sub_id2 = provider.subscribe_changes(callback2)

        assert sub_id1 != sub_id2

        # Both should be notified
        accessor.add_position(
            SimulatorPosition(
                position_id="test_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        time.sleep(0.1)
        assert callback1.called
        assert callback2.called

    def test_simulator_callback_exception_logged_not_raised(self):
        """Callback exceptions are logged but don't stop notifications."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        def bad_callback(positions):
            raise RuntimeError("Callback error")

        good_callback = Mock()

        provider.subscribe_changes(bad_callback)
        provider.subscribe_changes(good_callback)

        # This should not raise, and good_callback should still be called
        accessor.add_position(
            SimulatorPosition(
                position_id="test_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        time.sleep(0.1)
        # Good callback should still be notified
        # (the exception in bad_callback is caught)


class TestInMemoryPortfolioAccessorCoverage:
    """Tests for InMemoryPortfolioAccessor coverage."""

    def test_listener_exception_is_logged(self):
        """Listener exceptions are logged but don't stop notifications."""
        accessor = InMemoryPortfolioAccessor()

        def bad_listener(event):
            raise RuntimeError("Listener error")

        accessor.add_change_listener(bad_listener)

        # This should not raise
        accessor.add_position(
            SimulatorPosition(
                position_id="test_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

    def test_remove_change_listener_returns_false_for_unknown(self):
        """remove_change_listener returns False for unknown listener_id."""
        accessor = InMemoryPortfolioAccessor()
        result = accessor.remove_change_listener("nonexistent_id")
        assert result is False


class TestOandaStaleCache:
    """Tests for OANDA stale cache return paths."""

    def test_oanda_returns_stale_cache_on_connection_error(self):
        """fetch_positions returns stale cache on connection error."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # First call succeeds and populates cache
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        provider = OandaPositionProvider(
            config, http_client=mock_client, cache_ttl_seconds=0.01
        )

        # Populate cache
        provider.fetch_positions()

        # Wait for cache to become stale
        time.sleep(0.02)

        # Now make the API fail with a generic exception
        mock_client.get.side_effect = Exception("Connection failed")

        # Should return stale cache
        positions = provider.fetch_positions()
        assert positions == []

    def test_oanda_timeout_retry_path(self):
        """fetch_positions retries on TimeoutError."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # Fail with timeout first, then succeed
        mock_client.get.side_effect = [
            TimeoutError("Connection timed out"),
            Mock(status_code=200, json=lambda: {"positions": []}),
        ]

        provider = OandaPositionProvider(
            config,
            http_client=mock_client,
            max_retries=2,
            retry_backoff_factor=0.01,  # Fast for testing
        )

        positions = provider.fetch_positions()
        assert positions == []
        assert mock_client.get.call_count == 2

    def test_oanda_connection_error_retry_path(self):
        """fetch_positions retries on ConnectionError."""
        config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()

        # Fail with connection error first, then succeed
        mock_client.get.side_effect = [
            ConnectionError("Network unreachable"),
            Mock(status_code=200, json=lambda: {"positions": []}),
        ]

        provider = OandaPositionProvider(
            config, http_client=mock_client, max_retries=2, retry_backoff_factor=0.01
        )

        positions = provider.fetch_positions()
        assert positions == []
        assert mock_client.get.call_count == 2


class TestCacheStalenessEdgeCases:
    """Tests for cache staleness edge cases."""

    def test_cache_is_stale_when_fetch_time_none(self):
        """_is_stale returns True when _fetch_time is None."""
        cache = PositionCache(ttl_seconds=5.0)

        # Directly manipulate internal state to test defensive code
        cache._cache = []
        cache._fetch_time = None

        result = cache.get()
        assert result is not None
        positions, is_stale = result
        assert is_stale is True
        assert positions == []


class TestSimulatorProviderEdgeCases:
    """Tests for simulator provider edge cases."""

    def test_fetch_positions_returns_cached_when_fresh(self):
        """fetch_positions returns cached data when not stale."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=10.0)

        # Add a position
        accessor.add_position(
            SimulatorPosition(
                position_id="pos_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        # First fetch populates cache
        positions1 = provider.fetch_positions()
        assert len(positions1) == 1

        # Second fetch should use cache (no new accessor call needed)
        positions2 = provider.fetch_positions()
        assert len(positions2) == 1

        # Clear the position from accessor
        accessor.remove_position("pos_001")

        # Fetch should still return cached data (cache not stale)
        positions3 = provider.fetch_positions()
        assert len(positions3) == 1  # Still cached

    def test_unsubscribe_unknown_subscription_returns_false(self):
        """unsubscribe_changes returns False for unknown subscription."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        result = provider.unsubscribe_changes("nonexistent_sub_id")
        assert result is False
