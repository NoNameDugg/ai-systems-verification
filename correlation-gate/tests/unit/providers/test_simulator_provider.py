"""
Unit Tests for Simulator Position Provider
==========================================

Tests FIRST per TDI methodology.
All tests written BEFORE implementation.
"""

import pytest
import time
import threading
from datetime import datetime, timezone
from decimal import Decimal
from typing import List, Optional, Callable
from unittest.mock import Mock, MagicMock

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers.simulator import (
        SimulatorPositionProvider,
        PortfolioAccessor,
        InMemoryPortfolioAccessor,
        SimulatorPosition,
        PositionChangeEvent,
    )
    from src.models import Position
    from src.exceptions import ProviderError
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


class TestSimulatorPositionProviderInit:
    """Tests for SimulatorPositionProvider initialization."""

    def test_provider_creation_with_portfolio_accessor(self):
        """Provider can be created with a portfolio accessor."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)
        assert provider is not None

    def test_provider_default_cache_ttl(self):
        """Provider has default 1 second cache TTL (faster for simulation)."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)
        assert provider._cache_ttl_seconds == 1.0

    def test_provider_custom_cache_ttl(self):
        """Provider accepts custom cache TTL."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=0.5)
        assert provider._cache_ttl_seconds == 0.5


class TestSimulatorPositionProviderFetch:
    """Tests for SimulatorPositionProvider.fetch_positions()."""

    def test_fetch_positions_returns_list(self):
        """fetch_positions returns a list of Position objects."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="sim_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        provider = SimulatorPositionProvider(accessor)

        positions = provider.fetch_positions()

        assert isinstance(positions, list)
        assert len(positions) == 1
        assert isinstance(positions[0], Position)

    def test_fetch_positions_maps_simulator_position_to_position(self):
        """fetch_positions correctly maps SimulatorPosition to Position."""
        accessor = InMemoryPortfolioAccessor()
        sim_pos = SimulatorPosition(
            position_id="sim_001",
            instrument="EUR_USD",
            direction="LONG",
            units=10000,
            entry_price=Decimal("1.0850"),
            entry_time=datetime.now(timezone.utc),
        )
        accessor.add_position(sim_pos)
        provider = SimulatorPositionProvider(accessor)

        positions = provider.fetch_positions()

        assert positions[0].instrument == "EUR_USD"
        assert positions[0].direction == "LONG"
        assert positions[0].units == 10000
        assert positions[0].entry_price == Decimal("1.0850")
        assert positions[0].position_id == "sim_001"

    def test_fetch_positions_returns_empty_list_for_empty_portfolio(self):
        """fetch_positions returns empty list when portfolio is empty."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        positions = provider.fetch_positions()

        assert positions == []

    def test_fetch_positions_handles_multiple_positions(self):
        """fetch_positions handles multiple positions."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="sim_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        accessor.add_position(
            SimulatorPosition(
                position_id="sim_002",
                instrument="GBP_USD",
                direction="SHORT",
                units=5000,
                entry_price=Decimal("1.2650"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        provider = SimulatorPositionProvider(accessor)

        positions = provider.fetch_positions()

        assert len(positions) == 2


class TestSimulatorPositionProviderInjection:
    """Tests for position injection feature."""

    def test_inject_positions_overrides_portfolio(self):
        """inject_positions overrides portfolio positions."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="orig",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        provider = SimulatorPositionProvider(accessor)

        # Inject different positions
        injected = [
            Position(
                instrument="GBP_USD",
                direction="SHORT",
                units=5000,
                entry_price=Decimal("1.2650"),
                entry_time=datetime.now(timezone.utc),
                position_id="injected_001",
            )
        ]
        provider.inject_positions(injected)

        positions = provider.fetch_positions()
        assert len(positions) == 1
        assert positions[0].instrument == "GBP_USD"

    def test_inject_positions_can_be_cleared(self):
        """Injected positions can be cleared to return to portfolio."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="orig",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        provider = SimulatorPositionProvider(accessor)

        # Inject and then clear
        provider.inject_positions([])
        provider.clear_injection()

        positions = provider.fetch_positions()
        assert len(positions) == 1
        assert positions[0].instrument == "EUR_USD"


class TestSimulatorPositionProviderChangeNotifications:
    """Tests for position change notifications."""

    def test_subscribe_changes_returns_subscription_id(self):
        """subscribe_changes returns a subscription ID."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        callback = Mock()
        sub_id = provider.subscribe_changes(callback)

        assert sub_id is not None
        assert isinstance(sub_id, str)

    def test_callback_invoked_on_position_change(self):
        """Callback is invoked when positions change."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        callback = Mock()
        provider.subscribe_changes(callback)

        # Add a position to trigger change
        accessor.add_position(
            SimulatorPosition(
                position_id="sim_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        # Allow time for notification
        time.sleep(0.1)

        callback.assert_called()

    def test_unsubscribe_stops_notifications(self):
        """unsubscribe_changes stops notifications."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        callback = Mock()
        sub_id = provider.subscribe_changes(callback)
        provider.unsubscribe_changes(sub_id)

        # Add a position - callback should not be invoked
        callback.reset_mock()
        accessor.add_position(
            SimulatorPosition(
                position_id="sim_001",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        time.sleep(0.1)
        callback.assert_not_called()

    def test_unsubscribe_returns_false_for_unknown_id(self):
        """unsubscribe_changes returns False for unknown ID."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        result = provider.unsubscribe_changes("unknown_id")
        assert result is False


class TestSimulatorPositionProviderAvailability:
    """Tests for availability methods."""

    def test_is_available_returns_true(self):
        """is_available returns True (simulator always available)."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        assert provider.is_available() is True

    def test_get_last_fetch_time_returns_none_before_fetch(self):
        """get_last_fetch_time returns None before first fetch."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        assert provider.get_last_fetch_time() is None

    def test_get_last_fetch_time_returns_datetime_after_fetch(self):
        """get_last_fetch_time returns datetime after fetch."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        before = datetime.now(timezone.utc)
        provider.fetch_positions()
        after = datetime.now(timezone.utc)

        last_fetch = provider.get_last_fetch_time()
        assert before <= last_fetch <= after


class TestInMemoryPortfolioAccessor:
    """Tests for InMemoryPortfolioAccessor."""

    def test_get_open_positions_returns_all_positions(self):
        """get_open_positions returns all added positions."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="pos1",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )
        accessor.add_position(
            SimulatorPosition(
                position_id="pos2",
                instrument="GBP_USD",
                direction="SHORT",
                units=5000,
                entry_price=Decimal("1.2650"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        positions = accessor.get_open_positions()
        assert len(positions) == 2

    def test_get_position_by_id_returns_position(self):
        """get_position_by_id returns the correct position."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="target",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        position = accessor.get_position_by_id("target")
        assert position is not None
        assert position.position_id == "target"

    def test_get_position_by_id_returns_none_for_unknown(self):
        """get_position_by_id returns None for unknown ID."""
        accessor = InMemoryPortfolioAccessor()

        position = accessor.get_position_by_id("unknown")
        assert position is None

    def test_remove_position_removes_position(self):
        """remove_position removes the position."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="to_remove",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        accessor.remove_position("to_remove")

        positions = accessor.get_open_positions()
        assert len(positions) == 0

    def test_change_listener_called_on_add(self):
        """Change listener is called when position is added."""
        accessor = InMemoryPortfolioAccessor()
        listener = Mock()
        accessor.add_change_listener(listener)

        accessor.add_position(
            SimulatorPosition(
                position_id="new",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        listener.assert_called_once()
        event = listener.call_args[0][0]
        assert event.event_type == "OPENED"

    def test_change_listener_called_on_remove(self):
        """Change listener is called when position is removed."""
        accessor = InMemoryPortfolioAccessor()
        accessor.add_position(
            SimulatorPosition(
                position_id="to_remove",
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        listener = Mock()
        accessor.add_change_listener(listener)
        accessor.remove_position("to_remove")

        listener.assert_called_once()
        event = listener.call_args[0][0]
        assert event.event_type == "CLOSED"
