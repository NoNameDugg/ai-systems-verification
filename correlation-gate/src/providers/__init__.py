"""
Position Providers Module
=========================

Phase 2: Data Integration components.

Provides position providers for:
- OANDA REST API (production)
- MarketSim simulator (testing/backtesting)
- Fallback with automatic failover

Example:
    >>> from src.providers import OandaPositionProvider, OandaConfig
    >>> config = OandaConfig(account_id="...", api_token="...")
    >>> provider = OandaPositionProvider(config)
    >>> positions = provider.fetch_positions()
"""

from src.providers.base import PositionProvider
from src.providers.cache import PositionCache
from src.providers.default import InMemoryPositionProvider
from src.providers.health import (
    ProviderHealthMonitor,
    ProviderHealth,
    OperationMetric,
)
from src.providers.oanda import (
    OandaPositionProvider,
    OandaConfig,
)
from src.providers.simulator import (
    SimulatorPositionProvider,
    PortfolioAccessor,
    InMemoryPortfolioAccessor,
    SimulatorPosition,
    PositionChangeEvent,
)
from src.providers.fallback import FallbackPositionProvider
from src.providers.factory import ProviderFactory


__all__ = [
    # Base
    "PositionProvider",
    # Default (in-memory)
    "InMemoryPositionProvider",
    # Cache
    "PositionCache",
    # Health
    "ProviderHealthMonitor",
    "ProviderHealth",
    "OperationMetric",
    # OANDA
    "OandaPositionProvider",
    "OandaConfig",
    # Simulator
    "SimulatorPositionProvider",
    "PortfolioAccessor",
    "InMemoryPortfolioAccessor",
    "SimulatorPosition",
    "PositionChangeEvent",
    # Fallback
    "FallbackPositionProvider",
    # Factory
    "ProviderFactory",
]
