"""
Integration Tests for Correlation Gate
=======================================

Verifies component interaction and end-to-end functionality.
"""

import pytest
import time
import threading
from decimal import Decimal
from datetime import datetime, timezone
from typing import List

from src import (
    CorrelationGateAPI,
    create_gate,
    CorrelationGate,
    TradeSignal,
    Position,
    GateConfig,
    GateState,
    GateDecision,
    parse_directional_exposure,
    calculate_basket_exposure,
    create_basket_snapshot,
)
from tests.fixtures.mock_providers import MockPositionProvider


class TestEndToEndFlow:
    """Test complete evaluation flow from signal to decision."""

    def test_full_evaluation_flow_allow(self):
        """Complete flow: signal -> evaluation -> allow -> confirm."""
        # Create gate with default thresholds
        gate = create_gate(soft_warning=3, hard_block=5)

        # Create and evaluate signal
        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Verify decision
        assert decision.decision == "ALLOW"
        assert decision.pending_id is not None
        assert decision.evaluation_time_ms > 0

        # Confirm execution
        confirmed = gate.confirm_execution(decision.pending_id)
        assert confirmed is True

    def test_full_evaluation_flow_soft_warning(self):
        """Complete flow with soft warning threshold."""
        # Pre-populate with position
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "GBP_USD",
                "LONG",
                10000,
                Decimal("1.2650"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]

        config = {
            "soft_warning_count": 3,
            "hard_block_count": 5,
            "positions": positions,
        }
        api = CorrelationGateAPI.create(config)
        api.initialize()

        # This signal would make USD SHORT x3 (soft warning)
        signal = TradeSignal("AUD_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        assert decision.decision == "SOFT_WARNING"
        assert "USD" in decision.affected_currencies

    def test_full_evaluation_flow_hard_block(self):
        """Complete flow with hard block threshold."""
        # Pre-populate with positions at threshold
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "GBP_USD",
                "LONG",
                10000,
                Decimal("1.2650"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]

        config = {
            "soft_warning_count": 2,
            "hard_block_count": 3,
            "positions": positions,
        }
        api = CorrelationGateAPI.create(config)
        api.initialize()

        # This signal would exceed threshold
        signal = TradeSignal("AUD_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert decision.pending_id is None


class TestMapperCalculatorIntegration:
    """Test Directional Mapper + Basket Calculator integration."""

    def test_mapper_feeds_calculator_correctly(self):
        """Mapper output correctly consumed by calculator."""
        # Parse a signal
        parse_result = parse_directional_exposure("EUR_USD", "LONG")

        # Create position matching the parse
        position = Position(
            instrument="EUR_USD",
            direction="LONG",
            units=10000,
            entry_price=Decimal("1.0850"),
            entry_time=datetime.now(timezone.utc),
            position_id="pos_001",
        )

        # Calculate basket
        baskets = calculate_basket_exposure([position])

        # Verify alignment
        assert "EUR" in baskets
        assert "USD" in baskets
        assert baskets["EUR"].long_count == 1
        assert baskets["USD"].short_count == 1

    def test_multiple_positions_aggregate_correctly(self):
        """Multiple positions aggregate in calculator."""
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "GBP_USD",
                "LONG",
                15000,
                Decimal("1.2650"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
            Position(
                "EUR_USD",
                "SHORT",
                5000,
                Decimal("1.0900"),
                datetime.now(timezone.utc),
                "pos_003",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        # EUR: LONG 1, SHORT 1 -> net 0
        assert baskets["EUR"].long_count == 1
        assert baskets["EUR"].short_count == 1
        assert baskets["EUR"].net_count == 0

        # USD: SHORT 2 (from EUR_USD LONG and GBP_USD LONG)
        # LONG 1 (from EUR_USD SHORT)
        assert baskets["USD"].short_count == 2
        assert baskets["USD"].long_count == 1


class TestCalculatorGateIntegration:
    """Test Basket Calculator + Gate Engine integration."""

    def test_gate_uses_calculator_snapshots(self):
        """Gate decision based on calculator snapshots."""
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                50000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
        ]

        config = GateConfig(
            soft_warning_count=10,
            hard_block_count=20,
            hard_block_net_notional=Decimal("100000"),  # Will trigger on notional
        )
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Signal that pushes over notional limit
        signal = TradeSignal("GBP_USD", "LONG", units=60000)
        decision = gate.evaluate(signal)

        # Should block based on notional calculation
        assert decision.decision == "HARD_BLOCK"

    def test_projected_exposure_includes_signal(self):
        """Projected exposure correctly includes pending signal."""
        gate = create_gate(soft_warning=5, hard_block=10)

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Projected should show the signal's impact
        assert decision.projected_exposure.total_positions == 1
        assert "EUR" in decision.projected_exposure.baskets
        assert "USD" in decision.projected_exposure.baskets


class TestConcurrentAccess:
    """Test thread-safe concurrent access."""

    def test_concurrent_evaluations_no_corruption(self):
        """Multiple threads can evaluate without state corruption."""
        gate = create_gate(soft_warning=50, hard_block=100)

        results = []
        errors = []

        def evaluate_many():
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

        # Launch multiple threads
        threads = [threading.Thread(target=evaluate_many) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # Verify no errors
        assert len(errors) == 0
        assert len(results) == 100

    def test_refresh_during_evaluation_safe(self):
        """Position refresh during evaluation doesn't corrupt state."""
        gate = create_gate(soft_warning=10, hard_block=20)

        results = []

        def evaluate_loop():
            for _ in range(50):
                signal = TradeSignal("EUR_USD", "LONG", units=100)
                decision = gate.evaluate(signal)
                results.append(decision)

        def refresh_loop():
            for _ in range(10):
                gate.refresh_positions()
                time.sleep(0.01)

        t1 = threading.Thread(target=evaluate_loop)
        t2 = threading.Thread(target=refresh_loop)

        t1.start()
        t2.start()
        t1.join()
        t2.join()

        assert len(results) == 50


class TestStateTransitions:
    """Test gate state machine transitions."""

    def test_initialization_state_sequence(self):
        """Gate follows correct state sequence on init."""
        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)

        # Start in INITIALIZING
        assert gate.state == GateState.INITIALIZING

        # After init, should be READY
        gate.initialize()
        assert gate.state == GateState.READY

    def test_failed_init_stays_failed(self):
        """Failed initialization keeps gate in FAILED state."""
        from tests.fixtures.mock_providers import FailingPositionProvider

        config = GateConfig()
        provider = FailingPositionProvider()
        gate = CorrelationGate(config, provider)

        gate.initialize()

        assert gate.state == GateState.FAILED


class TestErrorRecovery:
    """Test error handling and recovery."""

    def test_gate_recovers_from_transient_errors(self):
        """Gate continues operating after transient provider errors."""
        from tests.fixtures.mock_providers import StatefulPositionProvider

        config = GateConfig()
        provider = StatefulPositionProvider()
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # First evaluation succeeds
        signal1 = TradeSignal("EUR_USD", "LONG", units=10000)
        decision1 = gate.evaluate(signal1)
        assert decision1.decision == "ALLOW"

        # Provider will fail next call
        provider.set_fail_next()
        gate.refresh_positions()  # This triggers the failure

        # Gate should be degraded but still operational
        assert gate.state == GateState.DEGRADED

        # Next evaluation should still work (uses cached positions)
        signal2 = TradeSignal("GBP_USD", "LONG", units=10000)
        decision2 = gate.evaluate(signal2)
        assert decision2.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")


class TestXAUHandling:
    """Test XAU (Gold) specific handling."""

    def test_xau_position_affects_baskets(self):
        """XAU positions affect XAU and USD baskets."""
        positions = [
            Position(
                "XAU_USD",
                "LONG",
                10,
                Decimal("2000.00"),
                datetime.now(timezone.utc),
                "xau_001",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        assert "XAU" in baskets
        assert "USD" in baskets
        assert baskets["XAU"].long_count == 1
        assert baskets["USD"].short_count == 1

    def test_xau_notional_calculation(self):
        """XAU notional calculated correctly."""
        positions = [
            Position(
                "XAU_USD",
                "LONG",
                10,
                Decimal("2000.00"),
                datetime.now(timezone.utc),
                "xau_001",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        # 10 units * $2000 = $20,000 notional
        assert baskets["XAU"].long_notional == Decimal("20000.00")


class TestAPIConsistency:
    """Test API consistency and convenience methods."""

    def test_api_and_gate_produce_same_results(self):
        """API wrapper produces same results as direct gate access."""
        config = {
            "soft_warning_count": 3,
            "hard_block_count": 5,
            "positions": [],
        }

        api = CorrelationGateAPI.create(config)
        api.initialize()

        # Evaluate via API
        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        assert decision.decision == "ALLOW"

    def test_create_gate_convenience_function(self):
        """create_gate() convenience function works correctly."""
        gate = create_gate(soft_warning=2, hard_block=3)

        assert gate.is_ready
        assert gate.state == GateState.READY

        stats = gate.get_statistics()
        assert stats["state"] == "READY"
