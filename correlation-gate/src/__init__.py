"""
ASTRA Correlation Gate
======================

A production-grade correlation gate for forex trading risk management.

Diamond Standard Implementation:
- Dual-Metric Gating (count + notional)
- Initial Sync Lock (HARD_BLOCK during initialization)
- Dynamic XAU Scaling (real-time gold pricing)
- Fail-Closed Mandate (all errors -> HARD_BLOCK)

Quick Start:
    >>> from src import CorrelationGateAPI, TradeSignal, create_gate
    >>> gate = create_gate(soft_warning=2, hard_block=3)
    >>> signal = TradeSignal("EUR_USD", "LONG", units=10000)
    >>> decision = gate.evaluate(signal)
    >>> if decision.decision == "ALLOW":
    ...     execute_trade(signal)
    ...     gate.confirm_execution(decision.pending_id)

Full Documentation:
    See docs/api/API_REFERENCE.md
"""

__version__ = "1.0.0"
__author__ = "ASTRA Team"

# =============================================================================
# PUBLIC API
# =============================================================================

from src.api import (
    CorrelationGateAPI,
    create_gate,
)

from src.gate import (
    CorrelationGate,
    PositionProvider,
)

from src.models import (
    # Core models
    Position,
    TradeSignal,
    GateDecision,
    GateConfig,
    GateState,
    # Exposure models
    BasketExposure,
    BasketSnapshot,
    PositionContribution,
    # Price models
    PriceSnapshot,
    # Parsing models
    ParseResult,
    ParsedInstrument,
    # Internal models
    PendingSignal,
)

from src.directional import (
    parse_directional_exposure,
    extract_currencies,
    get_opposite_direction,
    is_cross_pair,
    calculate_xau_notional,
    calculate_usd_notional,
    MAJOR_CURRENCIES,
    SUPPORTED_PAIRS,
    VALID_DIRECTIONS,
)

from src.basket import (
    calculate_basket_exposure,
    calculate_position_notional,
    classify_exposure_level,
    find_highest_exposure,
    create_basket_snapshot,
    project_exposure_with_signal,
)

from src.exceptions import (
    # Base
    GateError,
    # Signal errors
    InvalidInstrumentError,
    InvalidDirectionError,
    InvalidSignalError,
    # Config errors
    ConfigError,
    # Provider errors
    ProviderError,
    ProviderUnavailableError,
    ProviderTimeoutError,
    ProviderAuthError,
    ProviderRateLimitError,
    CacheError,
    # Gate errors
    GateTimeoutError,
    GateStateError,
    PendingNotFoundError,
)

# Phase 2: Position Providers
from src.providers import (
    # Base
    PositionProvider as PositionProviderBase,
    # Cache
    PositionCache,
    # Health
    ProviderHealthMonitor,
    ProviderHealth,
    # OANDA
    OandaPositionProvider,
    OandaConfig,
    # Simulator
    SimulatorPositionProvider,
    PortfolioAccessor,
    InMemoryPortfolioAccessor,
    SimulatorPosition,
    PositionChangeEvent,
    # Fallback
    FallbackPositionProvider,
    # Factory
    ProviderFactory,
)

# =============================================================================
# PUBLIC SYMBOLS
# =============================================================================

__all__ = [
    # Version
    "__version__",
    # API
    "CorrelationGateAPI",
    "create_gate",
    # Gate
    "CorrelationGate",
    "PositionProvider",
    # Models
    "Position",
    "TradeSignal",
    "GateDecision",
    "GateConfig",
    "GateState",
    "BasketExposure",
    "BasketSnapshot",
    "PositionContribution",
    "PriceSnapshot",
    "ParseResult",
    "ParsedInstrument",
    "PendingSignal",
    # Directional
    "parse_directional_exposure",
    "extract_currencies",
    "get_opposite_direction",
    "is_cross_pair",
    "calculate_xau_notional",
    "calculate_usd_notional",
    "MAJOR_CURRENCIES",
    "SUPPORTED_PAIRS",
    "VALID_DIRECTIONS",
    # Basket
    "calculate_basket_exposure",
    "calculate_position_notional",
    "classify_exposure_level",
    "find_highest_exposure",
    "create_basket_snapshot",
    "project_exposure_with_signal",
    # Exceptions
    "GateError",
    "InvalidInstrumentError",
    "InvalidDirectionError",
    "InvalidSignalError",
    "ConfigError",
    "ProviderError",
    "ProviderUnavailableError",
    "ProviderTimeoutError",
    "ProviderAuthError",
    "ProviderRateLimitError",
    "CacheError",
    "GateTimeoutError",
    "GateStateError",
    "PendingNotFoundError",
    # Phase 2: Position Providers
    "PositionProviderBase",
    "PositionCache",
    "ProviderHealthMonitor",
    "ProviderHealth",
    "OandaPositionProvider",
    "OandaConfig",
    "SimulatorPositionProvider",
    "PortfolioAccessor",
    "InMemoryPortfolioAccessor",
    "SimulatorPosition",
    "PositionChangeEvent",
    "FallbackPositionProvider",
    "ProviderFactory",
]
