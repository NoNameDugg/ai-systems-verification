"""
Provider Factory
================

Factory for creating position providers by type.
"""

from typing import Any, Dict, List, Type

from src.providers.base import PositionProvider
from src.exceptions import ProviderError


class ProviderFactory:
    """
    Factory for creating position providers.

    Supports:
    - Type-based creation ("oanda", "simulator", "mock")
    - Configuration injection
    - Extensibility through registration

    Example:
        >>> factory = ProviderFactory()
        >>> provider = ProviderFactory.create("oanda", config=oanda_config)
    """

    _registry: Dict[str, Type[PositionProvider]] = {}

    @classmethod
    def register(
        cls, provider_type: str, provider_class: Type[PositionProvider]
    ) -> None:
        """
        Register a provider type.

        Args:
            provider_type: String identifier (e.g., "oanda")
            provider_class: Provider class to instantiate
        """
        cls._registry[provider_type] = provider_class

    @classmethod
    def create(cls, provider_type: str, **kwargs: Any) -> PositionProvider:
        """
        Create a provider instance.

        Args:
            provider_type: Type of provider to create
            **kwargs: Arguments passed to provider constructor

        Returns:
            Configured PositionProvider instance

        Raises:
            ProviderError: If type not registered
        """
        if provider_type not in cls._registry:
            available = list(cls._registry.keys())
            raise ProviderError(
                provider_type,
                f"Unknown provider type: '{provider_type}'. Available: {available}",
            )
        return cls._registry[provider_type](**kwargs)

    @classmethod
    def available_types(cls) -> List[str]:
        """
        Get list of registered provider types.

        Returns:
            List of provider type strings
        """
        return list(cls._registry.keys())

    @classmethod
    def unregister(cls, provider_type: str) -> bool:
        """
        Unregister a provider type.

        Args:
            provider_type: Type to unregister

        Returns:
            True if unregistered, False if not found
        """
        if provider_type in cls._registry:
            del cls._registry[provider_type]
            return True
        return False

    @classmethod
    def clear(cls) -> None:
        """Clear all registered providers."""
        cls._registry.clear()


def _register_default_providers() -> None:
    """Register default providers."""
    from src.providers.oanda import OandaPositionProvider
    from src.providers.simulator import SimulatorPositionProvider

    ProviderFactory.register("oanda", OandaPositionProvider)
    ProviderFactory.register("simulator", SimulatorPositionProvider)


# Register defaults on module load
_register_default_providers()
