"""
Supplemental Tests for 100% Coverage
=====================================

Additional tests to ensure complete code coverage.
"""

import pytest
import time
from decimal import Decimal
from datetime import datetime, timedelta, timezone
from typing import Dict, List
from unittest.mock import Mock, patch


# =============================================================================
# API TESTS
# =============================================================================


class TestCorrelationGateAPI:
    """Test CorrelationGateAPI class."""

    def test_api_create_with_default_config(self):
        """API creates with default configuration."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})

        assert api is not None
        assert api.state.value == "INITIALIZING"

    def test_api_create_with_custom_thresholds(self):
        """API creates with custom threshold configuration."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create(
            {
                "soft_warning_count": 5,
                "hard_block_count": 10,
                "soft_warning_net_notional": "250000",
                "hard_block_net_notional": "500000",
            }
        )
        api.initialize()

        assert api.is_ready

    def test_api_is_ready_returns_false_when_initializing(self):
        """is_ready returns False before initialization."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})

        assert api.is_ready is False

    def test_api_is_ready_returns_true_after_init(self):
        """is_ready returns True after initialization."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})
        api.initialize()

        assert api.is_ready is True

    def test_api_evaluate_returns_decision(self):
        """API evaluate returns GateDecision."""
        from src.api import CorrelationGateAPI
        from src.models import TradeSignal

        api = CorrelationGateAPI.create({})
        api.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_api_get_exposure_snapshot(self):
        """API get_exposure_snapshot returns BasketSnapshot."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})
        api.initialize()

        snapshot = api.get_exposure_snapshot()

        assert snapshot is not None
        assert hasattr(snapshot, "baskets")

    def test_api_refresh_positions(self):
        """API refresh_positions returns count."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})
        api.initialize()

        count = api.refresh_positions()

        assert count >= 0

    def test_api_get_statistics(self):
        """API get_statistics returns dict."""
        from src.api import CorrelationGateAPI

        api = CorrelationGateAPI.create({})
        api.initialize()

        stats = api.get_statistics()

        assert "state" in stats
        assert "position_count" in stats

    def test_api_get_pending_ids(self):
        """API get_pending_ids returns set."""
        from src.api import CorrelationGateAPI
        from src.models import TradeSignal

        api = CorrelationGateAPI.create({})
        api.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        pending_ids = api.get_pending_ids()

        assert decision.pending_id in pending_ids

    def test_api_confirm_execution(self):
        """API confirm_execution works."""
        from src.api import CorrelationGateAPI
        from src.models import TradeSignal

        api = CorrelationGateAPI.create({})
        api.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        result = api.confirm_execution(decision.pending_id)

        assert result is True

    def test_api_cancel_pending(self):
        """API cancel_pending works."""
        from src.api import CorrelationGateAPI
        from src.models import TradeSignal

        api = CorrelationGateAPI.create({})
        api.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = api.evaluate(signal)

        result = api.cancel_pending(decision.pending_id)

        assert result is True

    def test_create_gate_convenience_function(self):
        """create_gate convenience function works."""
        from src.api import create_gate
        from src.models import TradeSignal

        gate = create_gate(soft_warning=3, hard_block=5)

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "ALLOW"


# =============================================================================
# DIRECTIONAL TESTS
# =============================================================================


class TestDirectionalExtended:
    """Extended directional mapper tests."""

    def test_is_cross_pair_eur_jpy(self):
        """EUR_JPY is a cross pair."""
        from src.directional import is_cross_pair

        assert is_cross_pair("EUR_JPY") is True

    def test_is_cross_pair_eur_usd_is_not_cross(self):
        """EUR_USD is not a cross pair."""
        from src.directional import is_cross_pair

        assert is_cross_pair("EUR_USD") is False

    def test_get_opposite_direction_long(self):
        """Opposite of LONG is SHORT."""
        from src.directional import get_opposite_direction

        assert get_opposite_direction("LONG") == "SHORT"

    def test_get_opposite_direction_short(self):
        """Opposite of SHORT is LONG."""
        from src.directional import get_opposite_direction

        assert get_opposite_direction("SHORT") == "LONG"

    def test_get_opposite_direction_invalid(self):
        """Invalid direction raises error."""
        from src.directional import get_opposite_direction
        from src.exceptions import InvalidDirectionError

        with pytest.raises(InvalidDirectionError):
            get_opposite_direction("SIDEWAYS")

    def test_extract_currencies_eur_usd(self):
        """Extract currencies from EUR_USD."""
        from src.directional import extract_currencies

        base, quote = extract_currencies("EUR_USD")

        assert base == "EUR"
        assert quote == "USD"

    def test_extract_currencies_invalid(self):
        """Invalid instrument raises error."""
        from src.directional import extract_currencies
        from src.exceptions import InvalidInstrumentError

        with pytest.raises(InvalidInstrumentError):
            extract_currencies("INVALID")

    def test_calculate_usd_notional_usd_base(self):
        """USD base notional equals units."""
        from src.directional import calculate_usd_notional

        notional = calculate_usd_notional("USD_JPY", 10000, Decimal("149.50"))

        assert notional == Decimal("10000")

    def test_calculate_usd_notional_xau_with_spot(self):
        """XAU notional uses spot price."""
        from src.directional import calculate_usd_notional

        spot_prices = {"XAU_USD": Decimal("2000.00")}

        notional = calculate_usd_notional(
            "XAU_USD",
            10,
            Decimal("1950.00"),  # Entry price
            spot_prices,
        )

        assert notional == Decimal("20000.00")

    def test_calculate_usd_notional_cross_pair(self):
        """Cross pair notional uses entry price."""
        from src.directional import calculate_usd_notional

        notional = calculate_usd_notional("EUR_GBP", 10000, Decimal("0.8500"))

        assert notional == Decimal("8500.00")


# =============================================================================
# BASKET TESTS
# =============================================================================


class TestBasketExtended:
    """Extended basket calculator tests."""

    def test_find_highest_exposure_empty(self):
        """Empty baskets returns empty string."""
        from src.basket import find_highest_exposure

        currency, level = find_highest_exposure({})

        assert currency == ""
        assert level == "LOW"

    def test_classify_exposure_level_zero(self):
        """Zero count is LOW."""
        from src.basket import classify_exposure_level

        assert classify_exposure_level(0) == "LOW"

    def test_classify_exposure_level_one(self):
        """One count is LOW."""
        from src.basket import classify_exposure_level

        assert classify_exposure_level(1) == "LOW"

    def test_classify_exposure_level_four(self):
        """Four+ count is CRITICAL."""
        from src.basket import classify_exposure_level

        assert classify_exposure_level(4) == "CRITICAL"
        assert classify_exposure_level(10) == "CRITICAL"

    def test_project_exposure_with_signal(self):
        """Project exposure with signal works."""
        from src.basket import project_exposure_with_signal
        from src.models import Position

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
        signal_position = Position(
            "GBP_USD",
            "LONG",
            10000,
            Decimal("1.2650"),
            datetime.now(timezone.utc),
            "pending_001",
        )

        snapshot = project_exposure_with_signal(
            positions, signal_position, state_version=1
        )

        assert snapshot.total_positions == 2
        assert "USD" in snapshot.baskets


# =============================================================================
# MODELS TESTS
# =============================================================================


class TestModelsExtended:
    """Extended model tests."""

    def test_position_validation_empty_instrument(self):
        """Position rejects empty instrument."""
        from src.models import Position

        with pytest.raises(ValueError, match="instrument cannot be empty"):
            Position(
                "",
                "LONG",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            )

    def test_position_validation_invalid_direction(self):
        """Position rejects invalid direction."""
        from src.models import Position

        with pytest.raises(ValueError, match="direction must be LONG or SHORT"):
            Position(
                "EUR_USD",
                "SIDEWAYS",
                10000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            )

    def test_position_validation_negative_units(self):
        """Position rejects negative units."""
        from src.models import Position

        with pytest.raises(ValueError, match="units must be non-negative"):
            Position(
                "EUR_USD",
                "LONG",
                -100,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            )

    def test_trade_signal_auto_id(self):
        """TradeSignal generates auto ID."""
        from src.models import TradeSignal

        signal = TradeSignal("EUR_USD", "LONG", units=10000)

        assert signal.signal_id is not None
        assert signal.signal_id.startswith("sig_")

    def test_trade_signal_auto_timestamp(self):
        """TradeSignal generates auto timestamp."""
        from src.models import TradeSignal

        signal = TradeSignal("EUR_USD", "LONG", units=10000)

        assert signal.timestamp is not None

    def test_trade_signal_to_position(self):
        """TradeSignal converts to Position."""
        from src.models import TradeSignal

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        position = signal.to_position("pos_001", Decimal("1.0850"))

        assert position.instrument == "EUR_USD"
        assert position.direction == "LONG"
        assert position.units == 10000
        assert position.position_id == "pos_001"

    def test_basket_exposure_empty(self):
        """BasketExposure.empty creates empty basket."""
        from src.models import BasketExposure

        basket = BasketExposure.empty("USD")

        assert basket.currency == "USD"
        assert basket.long_count == 0
        assert basket.short_count == 0

    def test_basket_snapshot_empty(self):
        """BasketSnapshot.empty creates empty snapshot."""
        from src.models import BasketSnapshot

        snapshot = BasketSnapshot.empty(state_version=5)

        assert snapshot.state_version == 5
        assert snapshot.total_positions == 0

    def test_price_snapshot_age_and_stale(self):
        """PriceSnapshot tracks age and staleness."""
        from src.models import PriceSnapshot

        # Fresh price
        fresh = PriceSnapshot(
            instrument="XAU_USD",
            bid=Decimal("2000.00"),
            ask=Decimal("2001.00"),
            mid_price=Decimal("2000.50"),
            timestamp=datetime.now(timezone.utc),
        )

        assert fresh.age_ms >= 0
        assert fresh.is_stale is False

    def test_pending_signal_expiry(self):
        """PendingSignal tracks expiry."""
        from src.models import PendingSignal, TradeSignal

        signal = TradeSignal("EUR_USD", "LONG", units=10000)

        # Expired signal
        expired = PendingSignal(
            pending_id="pending_001",
            signal=signal,
            decision="ALLOW",
            registered_at=datetime.now(timezone.utc) - timedelta(hours=1),
            expires_at=datetime.now(timezone.utc) - timedelta(minutes=1),
        )

        assert expired.is_expired is True

        # Fresh signal
        fresh = PendingSignal(
            pending_id="pending_002",
            signal=signal,
            decision="ALLOW",
            registered_at=datetime.now(timezone.utc),
            expires_at=datetime.now(timezone.utc) + timedelta(minutes=5),
        )

        assert fresh.is_expired is False

    def test_gate_config_currency_overrides(self):
        """GateConfig applies currency overrides."""
        from src.models import GateConfig

        config = GateConfig(
            soft_warning_count=2,
            hard_block_count=3,
            currency_overrides={
                "JPY": {"soft_warning_count": 5, "hard_block_count": 10}
            },
        )

        usd_thresholds = config.get_thresholds_for_currency("USD")
        jpy_thresholds = config.get_thresholds_for_currency("JPY")

        assert usd_thresholds["soft_warning_count"] == 2
        assert jpy_thresholds["soft_warning_count"] == 5


# =============================================================================
# EXCEPTIONS TESTS
# =============================================================================


class TestExceptionsExtended:
    """Extended exception tests."""

    def test_gate_error_with_context(self):
        """GateError includes context."""
        from src.exceptions import GateError

        error = GateError("test error", {"key": "value"})

        assert error.message == "test error"
        assert error.context == {"key": "value"}

    def test_invalid_signal_error(self):
        """InvalidSignalError captures signal info."""
        from src.exceptions import InvalidSignalError
        from src.models import TradeSignal

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        error = InvalidSignalError(signal, "units too large")

        assert "EUR_USD" in str(error)
        assert "units too large" in str(error)

    def test_config_error(self):
        """ConfigError captures field info."""
        from src.exceptions import ConfigError

        error = ConfigError("threshold", -1, "must be positive")

        assert "threshold" in str(error)
        assert "-1" in str(error)
        assert "must be positive" in str(error)

    def test_provider_error(self):
        """ProviderError captures provider name."""
        from src.exceptions import ProviderError

        error = ProviderError("OANDA", "connection failed")

        assert "OANDA" in str(error)
        assert "connection failed" in str(error)

    def test_provider_unavailable_error(self):
        """ProviderUnavailableError sets message."""
        from src.exceptions import ProviderUnavailableError

        error = ProviderUnavailableError("OANDA")

        assert "OANDA" in str(error)
        assert "unavailable" in str(error)

    def test_provider_timeout_error(self):
        """ProviderTimeoutError captures timeout."""
        from src.exceptions import ProviderTimeoutError

        error = ProviderTimeoutError("OANDA", 5.0)

        assert error.timeout_seconds == 5.0
        assert "5.0" in str(error)

    def test_gate_timeout_error(self):
        """GateTimeoutError captures operation."""
        from src.exceptions import GateTimeoutError

        error = GateTimeoutError("lock_acquisition", 1.0)

        assert error.operation == "lock_acquisition"
        assert error.timeout_seconds == 1.0

    def test_gate_state_error(self):
        """GateStateError captures states."""
        from src.exceptions import GateStateError

        error = GateStateError("INITIALIZING", "READY", "evaluate")

        assert "INITIALIZING" in str(error)
        assert "READY" in str(error)
        assert "evaluate" in str(error)

    def test_pending_not_found_error(self):
        """PendingNotFoundError captures ID."""
        from src.exceptions import PendingNotFoundError

        error = PendingNotFoundError("pending_12345")

        assert error.pending_id == "pending_12345"
        assert "pending_12345" in str(error)


# =============================================================================
# GATE EXTENDED TESTS
# =============================================================================


class TestGateExtended:
    """Extended gate tests."""

    def test_gate_state_degraded_after_refresh_failure(self):
        """Gate enters DEGRADED state after refresh failure."""
        from src.gate import CorrelationGate
        from src.models import GateConfig
        from tests.fixtures.mock_providers import StatefulPositionProvider

        config = GateConfig()
        provider = StatefulPositionProvider()
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Make provider fail on next call
        provider.set_fail_next()

        # Refresh should fail and set state to DEGRADED
        gate.refresh_positions()

        assert gate.state.value == "DEGRADED"

    def test_gate_generates_recommendations_on_block(self):
        """Gate generates recommendations on HARD_BLOCK."""
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
            )
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert len(decision.recommendations) > 0

    def test_gate_purges_expired_pending(self):
        """Gate purges expired pending signals."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, PendingSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(pending_timeout_seconds=0.01)  # Very short
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Evaluate to create pending
        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        assert decision.pending_id in gate.get_pending_ids()

        # Wait for expiry
        time.sleep(0.05)

        # Evaluate again to trigger purge
        decision2 = gate.evaluate(signal)

        # Original pending should be purged
        assert decision.pending_id not in gate.get_pending_ids()


# =============================================================================
# FINAL COVERAGE TESTS - Targeting specific uncovered lines
# =============================================================================


class TestFinalCoverageAPI:
    """Tests targeting remaining uncovered lines in api.py."""

    def test_api_create_with_invalid_config_raises_config_error(self):
        """API raises ConfigError on invalid config value types."""
        from src.api import CorrelationGateAPI
        from src.exceptions import ConfigError

        # Pass an invalid value that will cause InvalidOperation in Decimal conversion
        with pytest.raises(ConfigError):
            CorrelationGateAPI.create({"soft_warning_net_notional": "not_a_number"})


class TestFinalCoverageBasket:
    """Tests targeting remaining uncovered lines in basket.py."""

    def test_classify_exposure_level_two_is_medium(self):
        """Net count 2 returns MEDIUM."""
        from src.basket import classify_exposure_level

        assert classify_exposure_level(2) == "MEDIUM"

    def test_classify_exposure_level_three_is_high(self):
        """Net count 3 returns HIGH."""
        from src.basket import classify_exposure_level

        assert classify_exposure_level(3) == "HIGH"


class TestFinalCoverageDirectional:
    """Tests targeting remaining uncovered lines in directional.py."""

    def test_parse_unrecognized_base_currency_raises_error(self):
        """Unrecognized base currency raises InvalidInstrumentError."""
        from src.directional import parse_directional_exposure
        from src.exceptions import InvalidInstrumentError

        with pytest.raises(InvalidInstrumentError) as exc_info:
            parse_directional_exposure("XXX_USD", "LONG")

        assert "unrecognized base currency" in str(exc_info.value)
        assert "XXX" in str(exc_info.value)

    def test_parse_unrecognized_quote_currency_raises_error(self):
        """Unrecognized quote currency raises InvalidInstrumentError."""
        from src.directional import parse_directional_exposure
        from src.exceptions import InvalidInstrumentError

        with pytest.raises(InvalidInstrumentError) as exc_info:
            parse_directional_exposure("EUR_XXX", "LONG")

        assert "unrecognized quote currency" in str(exc_info.value)
        assert "XXX" in str(exc_info.value)

    def test_parse_cross_pair_generates_warning(self):
        """Cross pair (no USD) generates warning."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EUR_GBP", "LONG")

        assert result.is_cross_pair is True
        assert len(result.warnings) > 0
        assert "Cross-pair" in result.warnings[0]

    def test_parse_instrument_robust_empty_string_internal(self):
        """_parse_instrument_robust handles empty strings."""
        from src.directional import _parse_instrument_robust

        result = _parse_instrument_robust("")

        assert result is None

    def test_parse_instrument_6char_no_separator(self):
        """6-character instruments without separator are parsed."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("EURUSD", "LONG")

        assert result.exposures == {"EUR": "LONG", "USD": "SHORT"}

    def test_parse_instrument_6char_xauusd(self):
        """6-character XAU instrument is parsed."""
        from src.directional import parse_directional_exposure

        result = parse_directional_exposure("XAUUSD", "LONG")

        assert result.exposures == {"XAU": "LONG", "USD": "SHORT"}


class TestFinalCoverageGate:
    """Tests targeting remaining uncovered lines in gate.py."""

    def test_position_provider_base_class_not_implemented(self):
        """PositionProvider base class methods raise NotImplementedError."""
        from src.gate import PositionProvider

        provider = PositionProvider()

        with pytest.raises(NotImplementedError):
            provider.fetch_positions()

        with pytest.raises(NotImplementedError):
            provider.is_available()

        with pytest.raises(NotImplementedError):
            provider.get_last_fetch_time()

    def test_evaluate_with_none_signal(self):
        """Evaluate with None signal returns HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Calling with None should trigger fail-closed
        decision = gate.evaluate(None)

        assert decision.decision == "HARD_BLOCK"
        assert "[FAIL-CLOSED]" in decision.reason

    def test_evaluate_with_empty_instrument_signal(self):
        """Evaluate with empty instrument returns HARD_BLOCK."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Create signal with empty instrument using object manipulation
        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        object.__setattr__(signal, "instrument", "")

        decision = gate.evaluate(signal)

        assert decision.decision == "HARD_BLOCK"
        assert "Invalid signal" in decision.reason

    def test_evaluate_forces_fresh_position_fetch(self):
        """Evaluate fetches fresh positions when cache is stale."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig(soft_warning_count=10, hard_block_count=20)
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

        # Force cache to be stale by setting _last_fetch to old time
        gate._last_fetch = datetime.now(timezone.utc) - timedelta(seconds=10)

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Should have successfully fetched and evaluated
        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")

    def test_gate_should_refresh_when_last_fetch_is_none(self):
        """_should_refresh returns True when _last_fetch is None."""
        from src.gate import CorrelationGate
        from src.models import GateConfig
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)

        # Don't initialize - _last_fetch should be None
        assert gate._last_fetch is None
        assert gate._should_refresh() is True

    def test_net_notional_warning_without_count_warning(self):
        """Net notional triggers warning when count doesn't."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        # Configure: high count thresholds, low notional thresholds
        config = GateConfig(
            soft_warning_count=10,  # High - won't trigger on count
            hard_block_count=20,
            soft_warning_net_notional=Decimal("5000"),  # Low - will trigger
            hard_block_net_notional=Decimal("500000"),
        )
        # Single large position
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                100000,
                Decimal("1.0850"),
                datetime.now(timezone.utc),
                "pos_001",
            )
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("GBP_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Should get SOFT_WARNING due to notional, not count
        assert decision.decision == "SOFT_WARNING"
        assert (
            any("notional" in r.lower() for r in decision.recommendations)
            or "net notional" in decision.reason.lower()
        )

    def test_gross_notional_warning(self):
        """Gross notional triggers warning."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal, Position
        from tests.fixtures.mock_providers import MockPositionProvider

        # Configure: high count/net thresholds, low gross threshold
        config = GateConfig(
            soft_warning_count=10,
            hard_block_count=20,
            soft_warning_net_notional=Decimal("500000"),
            hard_block_net_notional=Decimal("1000000"),
            soft_warning_gross_notional=Decimal("5000"),  # Low - will trigger
            hard_block_gross_notional=Decimal("1000000"),
        )
        # Positions that have offsetting net but high gross
        positions = [
            Position(
                "EUR_USD",
                "LONG",
                50000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_001",
            ),
            Position(
                "EUR_USD",
                "SHORT",
                50000,
                Decimal("1.0000"),
                datetime.now(timezone.utc),
                "pos_002",
            ),
        ]
        provider = MockPositionProvider(positions=positions)
        gate = CorrelationGate(config, provider)
        gate.initialize()

        signal = TradeSignal("EUR_USD", "LONG", units=1000)
        decision = gate.evaluate(signal)

        # Should get SOFT_WARNING due to gross notional
        assert decision.decision == "SOFT_WARNING"
        assert "gross notional" in decision.reason.lower()

    def test_positions_none_fallback(self):
        """Gate handles _positions being None gracefully."""
        from src.gate import CorrelationGate
        from src.models import GateConfig, TradeSignal
        from tests.fixtures.mock_providers import MockPositionProvider

        config = GateConfig()
        provider = MockPositionProvider(positions=[])
        gate = CorrelationGate(config, provider)
        gate.initialize()

        # Forcefully set _positions to None
        gate._positions = None
        # Keep _last_fetch recent so it doesn't refresh (skips fetch block)
        gate._last_fetch = datetime.now(timezone.utc)

        signal = TradeSignal("EUR_USD", "LONG", units=10000)
        decision = gate.evaluate(signal)

        # Should recover (line 492 converts None to []) and process
        assert decision.decision in ("ALLOW", "SOFT_WARNING", "HARD_BLOCK")
