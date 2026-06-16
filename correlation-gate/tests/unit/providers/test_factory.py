"""
Unit Tests for Provider Factory
===============================

Tests FIRST per TDI methodology.
All tests written BEFORE implementation.
"""

import pytest
from unittest.mock import Mock, MagicMock

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers.factory import ProviderFactory
    from src.providers.base import PositionProvider
    from src.providers.oanda import OandaPositionProvider, OandaConfig
    from src.providers.simulator import SimulatorPositionProvider
    from src.exceptions import ProviderError
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


class TestProviderFactoryRegistration:
    """Tests for provider registration."""

    def test_register_adds_provider_type(self):
        """register adds a provider type to the registry."""
        # Reset registry for test
        ProviderFactory._registry.clear()

        class CustomProvider(PositionProvider):
            def fetch_positions(self):
                return []

            def is_available(self):
                return True

            def get_last_fetch_time(self):
                return None

        ProviderFactory.register("custom", CustomProvider)

        assert "custom" in ProviderFactory._registry

    def test_register_overwrites_existing_type(self):
        """register overwrites existing provider type."""
        ProviderFactory._registry.clear()

        class Provider1(PositionProvider):
            pass

        class Provider2(PositionProvider):
            pass

        ProviderFactory.register("test", Provider1)
        ProviderFactory.register("test", Provider2)

        assert ProviderFactory._registry["test"] is Provider2

    def test_available_types_returns_registered_types(self):
        """available_types returns list of registered types."""
        ProviderFactory._registry.clear()

        class P1(PositionProvider):
            pass

        class P2(PositionProvider):
            pass

        ProviderFactory.register("type1", P1)
        ProviderFactory.register("type2", P2)

        types = ProviderFactory.available_types()

        assert "type1" in types
        assert "type2" in types
        assert len(types) == 2


class TestProviderFactoryCreation:
    """Tests for provider creation."""

    def test_create_returns_provider_instance(self):
        """create returns instance of registered provider."""
        ProviderFactory._registry.clear()

        class TestProvider(PositionProvider):
            def __init__(self, **kwargs):
                self.kwargs = kwargs

            def fetch_positions(self):
                return []

            def is_available(self):
                return True

            def get_last_fetch_time(self):
                return None

        ProviderFactory.register("test", TestProvider)

        provider = ProviderFactory.create("test")

        assert isinstance(provider, TestProvider)

    def test_create_passes_kwargs_to_provider(self):
        """create passes keyword arguments to provider constructor."""
        ProviderFactory._registry.clear()

        class TestProvider(PositionProvider):
            def __init__(self, arg1=None, arg2=None):
                self.arg1 = arg1
                self.arg2 = arg2

            def fetch_positions(self):
                return []

            def is_available(self):
                return True

            def get_last_fetch_time(self):
                return None

        ProviderFactory.register("test", TestProvider)

        provider = ProviderFactory.create("test", arg1="value1", arg2="value2")

        assert provider.arg1 == "value1"
        assert provider.arg2 == "value2"

    def test_create_unknown_type_raises_provider_error(self):
        """create raises ProviderError for unknown type."""
        ProviderFactory._registry.clear()

        with pytest.raises(ProviderError) as exc_info:
            ProviderFactory.create("unknown_type")

        assert "Unknown provider type" in str(exc_info.value)
        assert "unknown_type" in str(exc_info.value)

    def test_create_error_includes_available_types(self):
        """ProviderError for unknown type includes available types."""
        ProviderFactory._registry.clear()

        class P1(PositionProvider):
            pass

        ProviderFactory.register("known_type", P1)

        with pytest.raises(ProviderError) as exc_info:
            ProviderFactory.create("unknown_type")

        assert "known_type" in str(exc_info.value)


class TestProviderFactoryIntegration:
    """Tests for factory integration with real providers."""

    def test_factory_creates_oanda_provider(self):
        """Factory can create OandaPositionProvider."""
        ProviderFactory._registry.clear()
        ProviderFactory.register("oanda", OandaPositionProvider)

        config = OandaConfig(account_id="test", api_token="test")
        provider = ProviderFactory.create("oanda", config=config)

        assert isinstance(provider, OandaPositionProvider)

    def test_factory_creates_simulator_provider(self):
        """Factory can create SimulatorPositionProvider."""
        from src.providers.simulator import InMemoryPortfolioAccessor

        ProviderFactory._registry.clear()
        ProviderFactory.register("simulator", SimulatorPositionProvider)

        accessor = InMemoryPortfolioAccessor()
        provider = ProviderFactory.create("simulator", portfolio_accessor=accessor)

        assert isinstance(provider, SimulatorPositionProvider)


class TestProviderFactoryDefaults:
    """Tests for default provider registrations."""

    def test_default_providers_registered(self):
        """Default providers (oanda, simulator) are pre-registered."""
        # Re-register defaults since other tests may have cleared registry
        from src.providers.factory import _register_default_providers

        _register_default_providers()

        types = ProviderFactory.available_types()

        assert "oanda" in types
        assert "simulator" in types
