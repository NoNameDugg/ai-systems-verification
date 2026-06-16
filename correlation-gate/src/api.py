"""
Public API for Correlation Gate
================================

Unified interface for a live trading engine and a market simulator.
"""

import logging
from decimal import Decimal, InvalidOperation
from typing import Any, Dict, List, Optional, Set

from src.models import (
    Position,
    TradeSignal,
    BasketSnapshot,
    GateDecision,
    GateConfig,
    GateState,
)
from src.gate import CorrelationGate, PositionProvider
from src.exceptions import ConfigError, GateError


logger = logging.getLogger(__name__)


class CorrelationGateAPI:
    """
    Public API for Correlation Gate.

    Unified interface for a live trading engine and a market simulator.
    Provides simplified access to gate functionality with
    configuration validation and error handling.

    Example:
        >>> api = CorrelationGateAPI.create({
        ...     "soft_warning_count": 2,
        ...     "hard_block_count": 3,
        ...     "provider": "mock",
        ...     "positions": []
        ... })
        >>> decision = api.evaluate(signal)
    """

    def __init__(self, gate: CorrelationGate) -> None:
        """
        Initialize API with gate instance.

        Use create() factory method for easier construction.

        Args:
            gate: Configured CorrelationGate instance
        """
        self._gate = gate

    @classmethod
    def create(
        cls, config: Dict[str, Any], provider: Optional[PositionProvider] = None
    ) -> "CorrelationGateAPI":
        """
        Factory method to create gate instance.

        Args:
            config: Configuration dictionary with keys:
                - enabled: bool (default True)
                - soft_warning_count: int (default 2)
                - hard_block_count: int (default 3)
                - soft_warning_net_notional: Decimal (default 100000)
                - hard_block_net_notional: Decimal (default 200000)
                - soft_warning_gross_notional: Decimal (default 150000)
                - hard_block_gross_notional: Decimal (default 300000)
                - currency_overrides: Dict (optional)
                - position_fetch_timeout_seconds: float (default 5.0)
                - lock_timeout_seconds: float (default 1.0)
                - pending_timeout_seconds: float (default 30.0)
            provider: Optional position provider

        Returns:
            Configured CorrelationGateAPI

        Raises:
            ConfigError: If configuration invalid
        """
        # Validate and parse config
        gate_config = cls._parse_config(config)

        # Get or create provider
        if provider is None:
            provider = cls._create_default_provider(config)

        # Create gate
        gate = CorrelationGate(gate_config, provider)

        return cls(gate)

    @staticmethod
    def _parse_config(config: Dict[str, Any]) -> GateConfig:
        """Parse and validate configuration dictionary."""
        try:
            return GateConfig(
                enabled=config.get("enabled", True),
                soft_warning_count=config.get("soft_warning_count", 2),
                hard_block_count=config.get("hard_block_count", 3),
                soft_warning_net_notional=Decimal(
                    str(config.get("soft_warning_net_notional", "100000"))
                ),
                hard_block_net_notional=Decimal(
                    str(config.get("hard_block_net_notional", "200000"))
                ),
                soft_warning_gross_notional=Decimal(
                    str(config.get("soft_warning_gross_notional", "150000"))
                ),
                hard_block_gross_notional=Decimal(
                    str(config.get("hard_block_gross_notional", "300000"))
                ),
                currency_overrides=config.get("currency_overrides"),
                position_fetch_timeout_seconds=config.get(
                    "position_fetch_timeout_seconds", 5.0
                ),
                lock_timeout_seconds=config.get("lock_timeout_seconds", 1.0),
                pending_timeout_seconds=config.get("pending_timeout_seconds", 30.0),
            )
        except (ValueError, TypeError, InvalidOperation) as e:
            raise ConfigError("config", config, str(e))

    @staticmethod
    def _create_default_provider(config: Dict[str, Any]) -> PositionProvider:
        """
        Create the default in-memory provider based on config.

        Uses the self-contained :class:`InMemoryPositionProvider` shipped in
        ``src`` so the package works on a clean install with no test fixtures
        on the path. For production, pass an explicit provider to ``create()``.
        """
        from src.providers import InMemoryPositionProvider

        positions = config.get("positions", [])
        return InMemoryPositionProvider(positions)

    # =========================================================================
    # PUBLIC METHODS
    # =========================================================================

    def initialize(self) -> bool:
        """
        Initialize the gate.

        Must be called before evaluate().

        Returns:
            True if initialization successful
        """
        return self._gate.initialize()

    @property
    def state(self) -> GateState:
        """Get current gate state."""
        return self._gate.state

    @property
    def is_ready(self) -> bool:
        """Check if gate is ready for evaluation."""
        return self._gate.state in (GateState.READY, GateState.DEGRADED)

    def evaluate(self, signal: TradeSignal) -> GateDecision:
        """
        Evaluate trade signal.

        Args:
            signal: Trade signal to evaluate

        Returns:
            GateDecision with ALLOW/SOFT_WARNING/HARD_BLOCK
        """
        return self._gate.evaluate(signal)

    def confirm_execution(self, pending_id: str) -> bool:
        """
        Confirm pending signal was executed.

        Args:
            pending_id: ID from GateDecision

        Returns:
            True if confirmed, False if not found
        """
        return self._gate.confirm_execution(pending_id)

    def cancel_pending(self, pending_id: str) -> bool:
        """
        Cancel pending signal.

        Args:
            pending_id: ID from GateDecision

        Returns:
            True if cancelled, False if not found
        """
        return self._gate.cancel_pending(pending_id)

    def get_exposure_snapshot(self) -> BasketSnapshot:
        """
        Get current exposure snapshot.

        Returns:
            BasketSnapshot with current exposures
        """
        return self._gate.get_exposure_snapshot()

    def refresh_positions(self) -> int:
        """
        Force position cache refresh.

        Returns:
            Number of positions after refresh
        """
        return self._gate.refresh_positions()

    def get_statistics(self) -> Dict[str, Any]:
        """
        Get gate statistics.

        Returns:
            Dict with statistics
        """
        return self._gate.get_statistics()

    def get_pending_ids(self) -> Set[str]:
        """
        Get all current pending IDs.

        Returns:
            Set of pending IDs
        """
        return self._gate.get_pending_ids()


# =============================================================================
# CONVENIENCE FUNCTIONS
# =============================================================================


def create_gate(
    soft_warning: int = 2,
    hard_block: int = 3,
    positions: Optional[List[Position]] = None,
) -> CorrelationGateAPI:
    """
    Convenience function to create a gate with common settings.

    Args:
        soft_warning: Soft warning threshold (default 2)
        hard_block: Hard block threshold (default 3)
        positions: Initial positions (default empty)

    Returns:
        Configured and initialized CorrelationGateAPI

    Example:
        >>> gate = create_gate(soft_warning=2, hard_block=3)
        >>> decision = gate.evaluate(signal)
    """
    config = {
        "soft_warning_count": soft_warning,
        "hard_block_count": hard_block,
        "positions": positions or [],
    }

    api = CorrelationGateAPI.create(config)
    api.initialize()

    return api
