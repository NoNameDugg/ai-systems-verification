"""
Integration Tests for Position Providers
========================================

Tests component interaction and end-to-end provider functionality.
"""

import pytest
import time
import threading
from datetime import datetime, timezone
from decimal import Decimal
from typing import List
from unittest.mock import Mock, patch

# These imports will fail until implementation exists (RED phase)
try:
    from src.providers import (
        OandaPositionProvider,
        OandaConfig,
        SimulatorPositionProvider,
        InMemoryPortfolioAccessor,
        SimulatorPosition,
        FallbackPositionProvider,
        ProviderFactory,
        ProviderHealthMonitor,
    )
    from src.gate import CorrelationGate
    from src.models import Position, GateConfig, TradeSignal
    from src.api import CorrelationGateAPI
except ImportError:
    pytest.skip("Implementation not yet complete", allow_module_level=True)


class TestProviderGateIntegration:
    """Tests for provider integration with CorrelationGate."""

    def test_gate_uses_oanda_provider(self):
        """CorrelationGate can use OandaPositionProvider."""
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
        gate_config = GateConfig(soft_warning_count=3, hard_block_count=5)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision is not None
        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_gate_uses_simulator_provider(self):
        """CorrelationGate can use SimulatorPositionProvider."""
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
        gate_config = GateConfig(soft_warning_count=3, hard_block_count=5)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision is not None

    def test_gate_uses_fallback_provider(self):
        """CorrelationGate can use FallbackPositionProvider."""
        primary_config = OandaConfig(account_id="test", api_token="test")
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )
        primary = OandaPositionProvider(primary_config, http_client=mock_client)

        accessor = InMemoryPortfolioAccessor()
        fallback = SimulatorPositionProvider(accessor)

        provider = FallbackPositionProvider(primary, fallback)
        gate_config = GateConfig(soft_warning_count=3, hard_block_count=5)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision is not None


class TestProviderFactoryIntegration:
    """Tests for ProviderFactory with real providers."""

    def test_factory_creates_gate_with_oanda(self):
        """Factory-created OANDA provider works with gate."""
        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )

        config = OandaConfig(account_id="test", api_token="test")
        provider = ProviderFactory.create(
            "oanda", config=config, http_client=mock_client
        )

        gate_config = GateConfig()
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        assert gate.state.name == "READY"

    def test_factory_creates_gate_with_simulator(self):
        """Factory-created simulator provider works with gate."""
        accessor = InMemoryPortfolioAccessor()
        provider = ProviderFactory.create("simulator", portfolio_accessor=accessor)

        gate_config = GateConfig()
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        assert gate.state.name == "READY"


class TestHealthMonitorIntegration:
    """Tests for health monitor with providers."""

    def test_health_monitor_tracks_oanda_operations(self):
        """Health monitor tracks OANDA provider operations."""
        monitor = ProviderHealthMonitor()
        config = OandaConfig(account_id="test", api_token="test")

        mock_client = Mock()
        mock_client.get.return_value = Mock(
            status_code=200, json=lambda: {"positions": []}
        )

        provider = OandaPositionProvider(config, http_client=mock_client)

        # Simulate monitored fetch
        start = time.perf_counter_ns()
        provider.fetch_positions()
        latency_ms = (time.perf_counter_ns() - start) / 1_000_000
        monitor.record_success("oanda", latency_ms)

        health = monitor.get_health("oanda")
        assert health.is_healthy is True
        assert health.average_latency_ms > 0

    def test_health_monitor_detects_failing_provider(self):
        """Health monitor detects consistently failing provider."""
        monitor = ProviderHealthMonitor(unhealthy_threshold=0.5)

        # Simulate multiple failures
        for _ in range(10):
            monitor.record_failure("failing_provider", Exception("Error"))

        health = monitor.get_health("failing_provider")
        assert health.is_healthy is False
        assert health.error_rate_percent == 100.0


class TestFallbackIntegration:
    """Tests for fallback provider integration."""

    def test_fallback_failover_during_gate_operation(self):
        """Fallback provider handles failover during gate evaluation."""
        # Primary that will fail
        primary_mock = Mock()
        primary_mock.get.side_effect = ConnectionError("Network error")
        primary_config = OandaConfig(account_id="test", api_token="test")
        primary = OandaPositionProvider(primary_config, http_client=primary_mock)

        # Working fallback
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
        fallback = SimulatorPositionProvider(accessor)

        provider = FallbackPositionProvider(primary, fallback, failure_threshold=1)

        gate_config = GateConfig(soft_warning_count=3, hard_block_count=5)
        gate = CorrelationGate(gate_config, provider)

        # Gate should initialize using fallback after primary fails
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision is not None


class TestAPIProviderIntegration:
    """Tests for CorrelationGateAPI with providers."""

    def test_api_with_custom_provider(self):
        """CorrelationGateAPI works with custom provider."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        config = {
            "soft_warning_count": 3,
            "hard_block_count": 5,
        }

        api = CorrelationGateAPI.create(config, provider=provider)
        api.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        assert decision is not None


class TestConcurrentProviderAccess:
    """Tests for concurrent provider access through gate."""

    def test_concurrent_gate_evaluations_with_provider(self):
        """Concurrent gate evaluations work with provider."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor)

        gate_config = GateConfig(soft_warning_count=50, hard_block_count=100)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        results = []
        errors = []

        def evaluate_loop():
            for i in range(20):
                try:
                    signal = TradeSignal(
                        "EUR_USD", "LONG" if i % 2 == 0 else "SHORT", units=1000
                    )
                    decision = gate.evaluate(signal)
                    results.append(decision)
                    if decision.pending_id:
                        gate.confirm_execution(decision.pending_id)
                except Exception as e:
                    errors.append(e)

        threads = [threading.Thread(target=evaluate_loop) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0
        assert len(results) == 100


class TestProviderStateSync:
    """Tests for provider state synchronization."""

    def test_position_changes_reflected_in_gate(self):
        """Position changes in provider are reflected in gate evaluation."""
        accessor = InMemoryPortfolioAccessor()
        provider = SimulatorPositionProvider(accessor, cache_ttl_seconds=0.1)

        gate_config = GateConfig(soft_warning_count=2, hard_block_count=3)
        gate = CorrelationGate(gate_config, provider)
        gate.initialize()

        # Initial evaluation - no positions
        signal1 = TradeSignal("EUR_USD", "LONG", units=10000)
        decision1 = gate.evaluate(signal1)
        assert decision1.decision == "ALLOW"

        # Add positions to accessor
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
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.2650"),
                entry_time=datetime.now(timezone.utc),
            )
        )

        # Wait for cache to expire
        time.sleep(0.15)
        gate.refresh_positions()

        # Evaluation should now reflect the positions
        signal2 = TradeSignal("AUD_USD", "LONG", units=10000)
        decision2 = gate.evaluate(signal2)

        # Should hit soft warning or hard block due to USD exposure
        assert decision2.decision in ("SOFT_WARNING", "HARD_BLOCK")
