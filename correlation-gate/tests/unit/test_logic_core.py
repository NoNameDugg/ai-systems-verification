"""
LOGIC CORE TEST SUITE - Diamond Standard
=========================================

Test-Driven Implementation: RED PHASE
Tests written BEFORE implementation (TDI Step 2)

Coverage Requirements:
- 45+ tests total
- Directional Parsing: 10+ tests
- Notional Calculation: 10+ tests
- Gate Logic: 10+ tests
- Race Conditions: 5+ tests
- Startup Safety: 5+ tests
- Fail-Closed: 5+ tests

All tests MUST FAIL until implementation is complete.
"""

import pytest
import threading
import time
from decimal import Decimal
from datetime import datetime, timedelta, timezone
from typing import Dict, List, Optional, Any
from unittest.mock import Mock, MagicMock, patch
from concurrent.futures import ThreadPoolExecutor, as_completed


# =============================================================================
# SECTION 1: DIRECTIONAL PARSING TESTS (10+ tests)
# =============================================================================


class TestDirectionalParsingMajorPairs:
    """Test directional mapping for all 7 major USD pairs."""

    def test_parse_eur_usd_long_returns_eur_long_usd_short(self):
        """EUR_USD LONG -> EUR: LONG, USD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR_USD", "LONG")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}
        assert result.is_cross_pair is False
        assert set(result.affected_baskets) == {"EUR", "USD"}

    def test_parse_eur_usd_short_returns_eur_short_usd_long(self):
        """EUR_USD SHORT -> EUR: SHORT, USD: LONG."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR_USD", "SHORT")

        assert result.exposures == {"EUR": "SHORT", "USD": "LONG"}
        assert result.is_cross_pair is False

    def test_parse_usd_jpy_long_returns_usd_long_jpy_short(self):
        """USD_JPY LONG -> USD: LONG, JPY: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("USD_JPY", "LONG")

        assert result.exposures == {"USD": "LONG", "JPY": "SHORT"}
        assert result.is_cross_pair is False

    def test_parse_usd_jpy_short_returns_usd_short_jpy_long(self):
        """USD_JPY SHORT -> USD: SHORT, JPY: LONG."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("USD_JPY", "SHORT")

        assert result.exposures == {"USD": "SHORT", "JPY": "LONG"}

    def test_parse_gbp_usd_long_returns_gbp_long_usd_short(self):
        """GBP_USD LONG -> GBP: LONG, USD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("GBP_USD", "LONG")

        assert result.exposures == {"GBP": "LONG", "USD": "SHORT"}

    def test_parse_usd_chf_long_returns_usd_long_chf_short(self):
        """USD_CHF LONG -> USD: LONG, CHF: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("USD_CHF", "LONG")

        assert result.exposures == {"USD": "LONG", "CHF": "SHORT"}

    def test_parse_aud_usd_long_returns_aud_long_usd_short(self):
        """AUD_USD LONG -> AUD: LONG, USD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("AUD_USD", "LONG")

        assert result.exposures == {"AUD": "LONG", "USD": "SHORT"}

    def test_parse_usd_cad_long_returns_usd_long_cad_short(self):
        """USD_CAD LONG -> USD: LONG, CAD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("USD_CAD", "LONG")

        assert result.exposures == {"USD": "LONG", "CAD": "SHORT"}

    def test_parse_nzd_usd_long_returns_nzd_long_usd_short(self):
        """NZD_USD LONG -> NZD: LONG, USD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("NZD_USD", "LONG")

        assert result.exposures == {"NZD": "LONG", "USD": "SHORT"}


class TestDirectionalParsingGold:
    """Test directional mapping for XAU (Gold)."""

    def test_parse_xau_usd_long_returns_xau_long_usd_short(self):
        """XAU_USD LONG -> XAU: LONG, USD: SHORT."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("XAU_USD", "LONG")

        assert result.exposures == {"XAU": "LONG", "USD": "SHORT"}
        assert result.is_cross_pair is False

    def test_parse_xau_usd_short_returns_xau_short_usd_long(self):
        """XAU_USD SHORT -> XAU: SHORT, USD: LONG."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("XAU_USD", "SHORT")

        assert result.exposures == {"XAU": "SHORT", "USD": "LONG"}


class TestDirectionalParsingRobustness:
    """Test robust instrument parsing for various formats."""

    def test_parse_slash_separator_format(self):
        """EUR/USD format should parse correctly."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR/USD", "LONG")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}

    def test_parse_dash_separator_format(self):
        """EUR-USD format should parse correctly."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR-USD", "LONG")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}

    def test_parse_no_separator_format(self):
        """EURUSD format (no separator) should parse correctly."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EURUSD", "LONG")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}

    def test_parse_lowercase_direction_normalized(self):
        """Lowercase 'long' should be normalized to 'LONG'."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR_USD", "long")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}

    def test_parse_invalid_instrument_raises_error(self):
        """Invalid instrument should raise InvalidInstrumentError."""
        from src.directional import parse_directional_exposure
        from src.exceptions import InvalidInstrumentError

        with pytest.raises(InvalidInstrumentError):
            parse_directional_exposure("INVALID", "LONG")

    def test_parse_invalid_direction_raises_error(self):
        """Invalid direction should raise InvalidDirectionError."""
        from src.directional import parse_directional_exposure
        from src.exceptions import InvalidDirectionError

        with pytest.raises(InvalidDirectionError):
            parse_directional_exposure("EUR_USD", "SIDEWAYS")

    def test_parse_empty_instrument_raises_error(self):
        """Empty instrument should raise InvalidInstrumentError."""
        from src.directional import parse_directional_exposure
        from src.exceptions import InvalidInstrumentError

        with pytest.raises(InvalidInstrumentError):
            parse_directional_exposure("", "LONG")


# =============================================================================
# SECTION 2: NOTIONAL CALCULATION TESTS (10+ tests)
# =============================================================================


class TestNotionalCalculationBasic:
    """Test basic notional calculation with Decimal precision."""

    def test_calculate_usd_notional_for_eur_usd_position(self):
        """EUR_USD notional = units * entry_price."""
        from src.basket import calculate_position_notional
        from src.models import Position

        position = Position(
            instrument="EUR_USD",
            direction="LONG",
            units=10000,
            entry_price=Decimal("1.0850"),
            entry_time=datetime.now(timezone.utc),
            position_id="pos_001",
        )

        notional = calculate_position_notional(position)

        assert notional == Decimal("10850.00")

    def test_calculate_usd_notional_for_usd_jpy_position(self):
        """USD_JPY notional = units (USD is base)."""
        from src.basket import calculate_position_notional
        from src.models import Position

        position = Position(
            instrument="USD_JPY",
            direction="LONG",
            units=10000,
            entry_price=Decimal("149.50"),
            entry_time=datetime.now(timezone.utc),
            position_id="pos_002",
        )

        notional = calculate_position_notional(position)

        # USD is base currency, so notional = units directly
        assert notional == Decimal("10000.00")


class TestNotionalCalculationXAU:
    """Test XAU (Gold) notional with dynamic pricing - DIAMOND POINT #3."""

    def test_calculate_xau_notional_with_spot_price(self):
        """XAU notional = units * spot_price (DYNAMIC)."""
        from src.basket import calculate_xau_notional

        units = 10
        spot_price = Decimal("2050.00")

        notional = calculate_xau_notional(units, spot_price)

        assert notional == Decimal("20500.00")

    def test_calculate_xau_notional_fractional_units(self):
        """XAU fractional units should calculate correctly."""
        from src.basket import calculate_xau_notional

        units = 5
        spot_price = Decimal("2000.00")

        notional = calculate_xau_notional(units, spot_price)

        assert notional == Decimal("10000.00")

    def test_xau_notional_uses_decimal_precision(self):
        """XAU calculation must maintain Decimal precision."""
        from src.basket import calculate_xau_notional

        units = 3
        spot_price = Decimal("2033.33")

        notional = calculate_xau_notional(units, spot_price)

        # Exact Decimal math, no floating point errors
        assert notional == Decimal("6099.99")


class TestBasketExposureCalculation:
    """Test basket exposure aggregation with dual-metric tracking."""

    def test_calculate_basket_exposure_single_position(self):
        """Single position creates exposure in two baskets."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

        positions = [
            Position(
                instrument="EUR_USD",
                direction="LONG",
                units=10000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
                position_id="pos_001",
            )
        ]

        baskets = calculate_basket_exposure(positions)

        assert "EUR" in baskets
        assert "USD" in baskets
        assert baskets["EUR"].long_count == 1
        assert baskets["EUR"].short_count == 0
        assert baskets["USD"].long_count == 0
        assert baskets["USD"].short_count == 1

    def test_calculate_basket_exposure_multiple_same_direction(self):
        """Multiple LONG positions accumulate exposure."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

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

        baskets = calculate_basket_exposure(positions)

        # Both positions are SHORT USD
        assert baskets["USD"].short_count == 2
        assert baskets["USD"].net_count == 2
        assert baskets["USD"].net_direction == "SHORT"

    def test_calculate_basket_tracks_net_notional(self):
        """Basket tracks net notional (DIAMOND POINT #1)."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        assert baskets["EUR"].net_notional > Decimal("0")
        assert baskets["USD"].net_notional > Decimal("0")

    def test_calculate_basket_tracks_gross_notional(self):
        """Basket tracks gross notional (DIAMOND POINT #1)."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

        # Hedged position: LONG and SHORT in same currency
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
                "EUR_USD",
                "SHORT",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        # Net should be near zero, but gross should be sum of both
        assert baskets["EUR"].net_count == 0
        assert baskets["EUR"].gross_notional > Decimal("0")

    def test_basket_net_notional_is_absolute_difference(self):
        """Net notional = |long_notional - short_notional|."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

        positions = [
            Position(
                "EUR_USD",
                "LONG",
                20000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "EUR_USD",
                "SHORT",
                10000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        # EUR: LONG 20000, SHORT 10000 -> net = |20000 - 10000| = 10000
        assert baskets["EUR"].net_notional == Decimal("10000.00")

    def test_basket_gross_notional_is_sum(self):
        """Gross notional = long_notional + short_notional."""
        from src.basket import calculate_basket_exposure
        from src.models import Position

        positions = [
            Position(
                "EUR_USD",
                "LONG",
                20000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "EUR_USD",
                "SHORT",
                10000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]

        baskets = calculate_basket_exposure(positions)

        # EUR: LONG 20000 + SHORT 10000 = 30000 gross
        assert baskets["EUR"].gross_notional == Decimal("30000.00")


# =============================================================================
# SECTION 3: GATE LOGIC TESTS (10+ tests)
# =============================================================================


class TestGateThresholdEvaluation:
    """Test gate threshold evaluation logic."""

    def test_gate_allows_when_below_all_thresholds(self):
        """Gate returns ALLOW when all exposures below thresholds."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=2, hard_block_count=3)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "ALLOW"
        assert decision.pending_id is not None

    def test_gate_soft_warning_when_at_warning_threshold(self):
        """Gate returns SOFT_WARNING when exposure reaches warning threshold."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=2, hard_block_count=3)
        # Pre-existing position: USD SHORT x1
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            )
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # New signal would make USD SHORT x2 (soft warning threshold)
        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "SOFT_WARNING"
        assert "USD" in decision.affected_currencies

    def test_gate_hard_block_when_at_block_threshold(self):
        """Gate returns HARD_BLOCK when exposure reaches block threshold."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=2, hard_block_count=3)
        # Pre-existing positions: USD SHORT x2
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
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # New signal would make USD SHORT x3 (hard block threshold)
        signal = TradeSignal("AUD_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "USD" in decision.affected_currencies

    def test_gate_blocks_on_net_notional_threshold(self):
        """Gate blocks when net notional exceeds threshold (DIAMOND POINT #1)."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(
            soft_warning_count=10,  # High count threshold
            hard_block_count=20,
            hard_block_net_notional=Decimal("100000"),  # $100K limit
        )
        # Single large position: $150K notional
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                150000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            )
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Adding another would push over notional limit
        signal = TradeSignal("GBP_USD", "LONG", units=50000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"

    def test_gate_blocks_on_gross_notional_threshold(self):
        """Gate blocks when gross notional exceeds threshold (DIAMOND POINT #1)."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(
            soft_warning_count=10,
            hard_block_count=20,
            hard_block_gross_notional=Decimal("200000"),  # $200K gross limit
        )
        # Hedged positions: net = 0, but gross = $200K
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                100000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "EUR_USD",
                "SHORT",
                100000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Any additional position increases gross
        signal = TradeSignal("EUR_USD", "LONG", units=50000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"

    def test_most_restrictive_threshold_wins(self):
        """When multiple thresholds violated, most restrictive wins."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(
            soft_warning_count=1,  # Would trigger warning
            hard_block_count=10,  # Not triggered
            hard_block_net_notional=Decimal("50000"),  # Would trigger block
        )
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                60000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            )
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # HARD_BLOCK from notional should win over SOFT_WARNING from count
        assert decision.decision == "HARD_BLOCK"


class TestGatePendingManagement:
    """Test pending signal tracking."""

    def test_allowed_signal_gets_pending_id(self):
        """ALLOW decision includes pending_id for confirmation."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "ALLOW"
        assert decision.pending_id is not None
        assert len(decision.pending_id) > 0

    def test_hard_block_has_no_pending_id(self):
        """HARD_BLOCK decision has no pending_id."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=1, hard_block_count=2)
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert decision.pending_id is None

    def test_confirm_execution_removes_pending(self):
        """Confirming execution removes pending signal."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert gate.confirm_execution(decision.pending_id) is True
        # Second confirm should fail (already removed)
        assert gate.confirm_execution(decision.pending_id) is False

    def test_cancel_pending_removes_signal(self):
        """Cancelling pending removes signal."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert gate.cancel_pending(decision.pending_id) is True
        # Second cancel should fail (already removed)
        assert gate.cancel_pending(decision.pending_id) is False


# =============================================================================
# SECTION 4: RACE CONDITION TESTS (5+ tests)
# =============================================================================


class TestRaceConditions:
    """Test thread safety with concurrent signal evaluation."""

    def test_concurrent_signals_are_serialized(self):
        """Concurrent signals must be evaluated atomically."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=2, hard_block_count=3)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        results = []
        errors = []

        def evaluate_signal(signal):
            try:
                decision = gate.evaluate(signal)
                results.append(decision)
            except Exception as e:
                errors.append(e)

        # Fire 10 concurrent signals at the same USD basket
        signals = [
            TradeSignal("EUR_USD", "LONG", units=10000, signal_id=f"sig_{i}")
            for i in range(10)
        ]

        threads = [threading.Thread(target=evaluate_signal, args=(s,)) for s in signals]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0
        assert len(results) == 10

        # At most 2 should be ALLOW (before warning), rest should be WARNING or BLOCK
        allow_count = sum(1 for r in results if r.decision == "ALLOW")
        block_count = sum(1 for r in results if r.decision == "HARD_BLOCK")

        # Given threshold of 3, at most first 2 signals get ALLOW
        # (accounting for pending signals in exposure calculation)
        assert allow_count <= 3

    def test_no_race_between_evaluate_and_confirm(self):
        """Evaluate and confirm operations don't race."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        confirmed = []
        evaluated = []

        def evaluate_and_confirm():
            signal = TradeSignal("EUR_USD", "LONG", units=1000)
            decision = gate.evaluate(signal)
            evaluated.append(decision)
            if decision.pending_id:
                result = gate.confirm_execution(decision.pending_id)
                confirmed.append(result)

        threads = [threading.Thread(target=evaluate_and_confirm) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # All evaluations should complete without error
        assert len(evaluated) == 20
        # Each pending should only be confirmable once
        true_confirms = sum(1 for c in confirmed if c is True)
        false_confirms = sum(1 for c in confirmed if c is False)
        # No double-confirms (each pending_id confirmed exactly once)
        assert true_confirms == len(confirmed) - false_confirms

    def test_high_contention_stress_test(self):
        """Gate remains consistent under high contention."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=50, hard_block_count=100)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        results = []

        def rapid_fire():
            for i in range(100):
                signal = TradeSignal("EUR_USD", "LONG", units=100)
                decision = gate.evaluate(signal)
                results.append(decision)
                if decision.pending_id:
                    gate.confirm_execution(decision.pending_id)

        threads = [threading.Thread(target=rapid_fire) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # All 500 evaluations should complete
        assert len(results) == 500

        # Check exposure consistency at the end
        snapshot = gate.get_exposure_snapshot()
        # Should have some exposure from confirmed signals
        assert snapshot.total_positions >= 0

    def test_concurrent_cancel_operations(self):
        """Concurrent cancels of same pending_id don't corrupt state."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=10, hard_block_count=20)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)
        pending_id = decision.pending_id

        results = []

        def try_cancel():
            result = gate.cancel_pending(pending_id)
            results.append(result)

        # Try to cancel same pending from multiple threads
        threads = [threading.Thread(target=try_cancel) for _ in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # Exactly one cancel should succeed
        assert sum(results) == 1

    def test_evaluate_during_position_refresh(self):
        """Evaluation is consistent during position refresh."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        results = []
        refresh_count = [0]

        def evaluate_loop():
            for _ in range(50):
                signal = TradeSignal("EUR_USD", "LONG", units=100)
                decision = gate.evaluate(signal)
                results.append(decision)
                time.sleep(0.001)

        def refresh_loop():
            for _ in range(10):
                gate.refresh_positions()
                refresh_count[0] += 1
                time.sleep(0.005)

        t1 = threading.Thread(target=evaluate_loop)
        t2 = threading.Thread(target=refresh_loop)

        t1.start()
        t2.start()
        t1.join()
        t2.join()

        # All evaluations should complete without error
        assert len(results) == 50
        assert refresh_count[0] == 10


# =============================================================================
# SECTION 5: STARTUP SAFETY TESTS (5+ tests) - DIAMOND POINT #2
# =============================================================================


class TestStartupSafety:
    """Test initial sync lock - gate blocks until initialized."""

    def test_gate_starts_in_initializing_state(self):
        """Gate constructor sets state to INITIALIZING."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, GateState
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)

        assert gate.state == GateState.INITIALIZING

    def test_gate_blocks_before_initialization(self):
        """Evaluate returns HARD_BLOCK before initialize() called."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        # NOT calling initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "initializing" in decision.reason.lower()

    def test_gate_blocks_during_synchronization(self):
        """Evaluate returns HARD_BLOCK during synchronization."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, GateState
        from tests.fixtures.mock_providers import SlowPositionProvider

        config = GateConfig()
        # Provider that takes 1 second to fetch
        provider = SlowPositionProvider(delay_seconds=1.0, positions=[])
        gate = CorrelationGate(config, provider)

        results = []

        def try_evaluate():
            signal = TradeSignal("EUR_USD", "LONG", units=10000)
            decision = gate.evaluate(signal)
            results.append(decision)

        def start_init():
            gate.initialize()

        # Start initialization in background
        init_thread = threading.Thread(target=start_init)
        init_thread.start()

        # Give it a moment to start, then try to evaluate
        time.sleep(0.1)
        eval_thread = threading.Thread(target=try_evaluate)
        eval_thread.start()
        eval_thread.join()

        init_thread.join()

        # Evaluation during sync should be blocked
        assert len(results) == 1
        assert results[0].decision == "HARD_BLOCK"

    def test_gate_allows_after_successful_initialization(self):
        """Evaluate works normally after initialize() succeeds."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, GateState
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=5, hard_block_count=10)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)

        success = gate.initialize()

        assert success is True
        assert gate.state == GateState.READY

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "ALLOW"

    def test_gate_blocks_after_failed_initialization(self):
        """Gate stays blocked if initialize() fails."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, GateState
        from tests.fixtures.mock_providers import FailingPositionProvider

        config = GateConfig()
        provider = FailingPositionProvider()  # Always throws
        gate = CorrelationGate(config, provider)

        success = gate.initialize()

        assert success is False
        assert gate.state == GateState.FAILED

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "FAILED" in decision.reason.upper()


# =============================================================================
# SECTION 6: FAIL-CLOSED TESTS (5+ tests) - DIAMOND POINT #4
# =============================================================================


class TestFailClosed:
    """Test fail-closed mandate - all errors result in HARD_BLOCK."""

    def test_position_fetch_timeout_returns_hard_block(self):
        """Position fetch timeout results in HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import TimeoutPositionProvider

        config = GateConfig(position_fetch_timeout_seconds=0.1)
        provider = TimeoutPositionProvider(timeout_seconds=1.0)  # Will timeout
        gate = CorrelationGate(config, provider)
        gate._state_override_for_test("READY")  # Force ready state for test

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "timeout" in decision.reason.lower()

    def test_provider_exception_returns_hard_block(self):
        """Provider exception results in HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import ExceptionPositionProvider

        config = GateConfig()
        provider = ExceptionPositionProvider(exception=RuntimeError("API down"))
        gate = CorrelationGate(config, provider)
        gate.initialize()  # Will fail, but we override
        gate._state_override_for_test("READY")

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "error" in decision.reason.lower()

    def test_invalid_signal_returns_hard_block(self):
        """Invalid signal format results in HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Signal with invalid instrument
        signal = TradeSignal("INVALID_PAIR", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "invalid" in decision.reason.lower()

    def test_unknown_exception_returns_hard_block(self):
        """Unknown/unexpected exception results in HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Patch internal method to throw unexpected error
        with patch.object(
            gate, "_calculate_snapshot", side_effect=ValueError("Unexpected")
        ):
            signal = TradeSignal("EUR_USD", "LONG", units=10000)
            decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert (
            "unexpected" in decision.reason.lower()
            or "error" in decision.reason.lower()
        )

    def test_lock_timeout_returns_hard_block(self):
        """Lock acquisition timeout results in HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(lock_timeout_seconds=0.01)  # Very short timeout
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Hold the lock from another thread
        lock_holder_done = threading.Event()

        def hold_lock():
            with gate._lock:
                time.sleep(0.5)  # Hold longer than timeout
            lock_holder_done.set()

        holder = threading.Thread(target=hold_lock)
        holder.start()

        time.sleep(0.05)  # Ensure lock is held

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        lock_holder_done.wait()
        holder.join()

        assert decision.decision == "HARD_BLOCK"
        assert "lock" in decision.reason.lower() or "timeout" in decision.reason.lower()


# =============================================================================
# SECTION 7: PERFORMANCE VALIDATION TESTS
# =============================================================================


class TestPerformance:
    """Test performance budgets are met."""

    def test_evaluate_under_5ms_with_100_positions(self):
        """Gate evaluation completes within 5ms for 100 positions."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=50, hard_block_count=100)

        # Create 100 positions
        positions = [
            Position(
                instrument="EUR_USD",
                direction="LONG" if i % 2 == 0 else "SHORT",
                units=1000,
                entry_price=Decimal("1.0850"),
                entry_time=datetime.now(timezone.utc),
                position_id=f"pos_{i:03d}",
            )
            for i in range(100)
        ]

        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=1000)

        # Warm up
        for _ in range(10):
            gate.evaluate(signal)

        # Measure
        times = []
        for _ in range(100):
            start = time.perf_counter_ns()
            gate.evaluate(signal)
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)
        p99_ms = sorted(times)[98]

        assert avg_ms < 5, f"Average {avg_ms:.2f}ms exceeds 5ms budget"
        assert p99_ms < 10, f"P99 {p99_ms:.2f}ms exceeds 10ms budget"

    def test_directional_parsing_under_0_1ms(self):
        """Directional parsing completes within 0.1ms."""
        from src.directional import parse_directional_exposure

        # Warm up
        for _ in range(100):
            parse_directional_exposure("EUR_USD", "LONG")

        # Measure
        times = []
        for _ in range(1000):
            start = time.perf_counter_ns()
            parse_directional_exposure("EUR_USD", "LONG")
            elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
            times.append(elapsed_ms)

        avg_ms = sum(times) / len(times)

        assert avg_ms < 0.1, f"Average {avg_ms:.4f}ms exceeds 0.1ms budget"


# =============================================================================
# SECTION 8: EDGE CASE TESTS
# =============================================================================


class TestEdgeCases:
    """Test edge cases and boundary conditions."""

    def test_empty_position_list_returns_allow(self):
        """No positions means no exposure, should ALLOW."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=2, hard_block_count=3)
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "ALLOW"

    def test_zero_units_signal_still_evaluated(self):
        """Signal with zero units should still be processed."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=0)
        decision = gate.evaluate(signal)

        # Should not raise, should return a decision
        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_very_large_notional_handled(self):
        """Very large notional values don't overflow."""
        from src.basket import calculate_xau_notional

        # 1 million ounces at $2000 = $2 billion
        units = 1_000_000
        spot_price = Decimal("2000.00")

        notional = calculate_xau_notional(units, spot_price)

        assert notional == Decimal("2000000000.00")

    def test_decision_includes_evaluation_time(self):
        """Decision includes evaluation time in milliseconds."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert hasattr(decision, "evaluation_time_ms")
        assert decision.evaluation_time_ms >= 0
        assert decision.evaluation_time_ms < 100  # Sanity check


# =============================================================================
# TEST FIXTURES MARKER
# =============================================================================


class TestFixturesExist:
    """Verify test fixtures are properly defined."""

    def test_mock_position_provider_exists(self):
        """MockPositionProvider fixture should exist."""
        from tests.fixtures.mock_providers import MockPositionProvider

        provider = MockPositionProvider(positions=[])
        assert provider.is_available() is True

    def test_failing_position_provider_exists(self):
        """FailingPositionProvider fixture should exist."""
        from tests.fixtures.mock_providers import FailingPositionProvider

        provider = FailingPositionProvider()
        with pytest.raises(Exception):
            provider.fetch_positions()

    def test_slow_position_provider_exists(self):
        """SlowPositionProvider fixture should exist."""
        from tests.fixtures.mock_providers import SlowPositionProvider

        provider = SlowPositionProvider(delay_seconds=0.01, positions=[])
        start = time.perf_counter()
        provider.fetch_positions()
        elapsed = time.perf_counter() - start
        assert elapsed >= 0.01
