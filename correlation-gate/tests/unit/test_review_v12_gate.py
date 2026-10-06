"""
External review (2026-10) reproductions — correlation gate
==========================================================

Each test here reproduces one finding from an independent code review of
v1.1.0. They were committed FAILING (``xfail(strict=True)``) before any fix so
the defect is in the tree as a test, not as prose. When the fix lands the
``xfail`` marker is removed and the test must pass; if a later change
re-introduces the defect the test fails again.

Findings covered:
  #5  pending-approval off-by-one: the new trade's own slot is dropped from the
      projection whenever at least one approval is pending.
  #7  the OANDA provider never sends ``Authorization: Bearer <token>``.
  #8  a proposed XAU trade is projected at a placeholder price of 1.0, so its
      notional is never size-checked.
"""

from datetime import datetime, timezone
from decimal import Decimal
from typing import List
from unittest.mock import Mock

import pytest

from src.gate import CorrelationGate
from src.models import GateConfig, Position, TradeSignal
from src.providers.oanda import OandaConfig, OandaPositionProvider


class _StaticProvider:
    """Position provider that returns a fixed list."""

    def __init__(self, positions: List[Position] = None):
        self._positions = positions or []

    def fetch_positions(self) -> List[Position]:
        return self._positions.copy()

    def is_available(self) -> bool:
        return True

    def get_last_fetch_time(self):
        return datetime.now(timezone.utc)


def _gate(config: GateConfig, positions: List[Position] = None) -> CorrelationGate:
    gate = CorrelationGate(config, _StaticProvider(positions))
    assert gate.initialize()
    return gate


# =============================================================================
# #5 — the new trade must be counted alongside pending approvals
# =============================================================================


@pytest.mark.xfail(strict=True, reason="review #5: new trade dropped from projection when any approval is pending")
class TestPendingProjectionCountsNewTrade:
    """With N open + M pending, a new same-basket trade is slot N+M+1."""

    def test_third_unconfirmed_same_direction_signal_is_hard_blocked(self):
        gate = _gate(GateConfig(soft_warning_count=2, hard_block_count=3))

        d1 = gate.evaluate(TradeSignal("EUR_USD", "LONG", 10000, signal_id="s1"))
        d2 = gate.evaluate(TradeSignal("GBP_USD", "LONG", 10000, signal_id="s2"))
        d3 = gate.evaluate(TradeSignal("AUD_USD", "LONG", 10000, signal_id="s3"))

        assert d1.decision == "ALLOW"
        # second signal: 1 pending + itself = 2 USD shorts -> soft warning
        assert d2.projected_exposure.baskets["USD"].net_count == 2
        assert d2.decision == "SOFT_WARNING"
        # third signal: 2 pending + itself = 3 -> hard block, no pending id issued
        assert d3.projected_exposure.baskets["USD"].net_count == 3
        assert d3.decision == "HARD_BLOCK"
        assert d3.pending_id is None
        assert len(gate.get_pending_ids()) == 2

    def test_one_open_plus_one_pending_plus_new_is_hard_blocked(self):
        existing = Position(
            instrument="EUR_USD",
            direction="LONG",
            units=10000,
            entry_price=Decimal("1.0850"),
            entry_time=datetime.now(timezone.utc),
            position_id="existing_001",
        )
        gate = _gate(GateConfig(soft_warning_count=2, hard_block_count=3), [existing])

        d_pending = gate.evaluate(TradeSignal("GBP_USD", "LONG", 10000, signal_id="p1"))
        assert d_pending.decision == "SOFT_WARNING"  # 1 open + itself = 2
        assert d_pending.pending_id is not None

        d_new = gate.evaluate(TradeSignal("AUD_USD", "LONG", 10000, signal_id="n1"))
        # 1 open + 1 pending + itself = 3 -> hard block
        assert d_new.projected_exposure.baskets["USD"].net_count == 3
        assert d_new.decision == "HARD_BLOCK"
        assert d_new.pending_id is None


# =============================================================================
# #7 — OANDA requests must carry the bearer token
# =============================================================================


@pytest.mark.xfail(strict=True, reason="review #7: OANDA provider sends no Authorization header")
class TestOandaAuthorizationHeader:
    """Every HTTP call to OANDA must authenticate."""

    @staticmethod
    def _provider_and_client():
        config = OandaConfig(account_id="acct-1", api_token="tok-abc")
        client = Mock()
        client.get.return_value = Mock(status_code=200, json=lambda: {"positions": []})
        return OandaPositionProvider(config, http_client=client), client

    def test_fetch_positions_sends_bearer_token(self):
        provider, client = self._provider_and_client()
        provider.fetch_positions()

        client.get.assert_called_once()
        headers = client.get.call_args.kwargs.get("headers") or {}
        assert headers.get("Authorization") == "Bearer tok-abc"

    def test_is_available_sends_bearer_token(self):
        provider, client = self._provider_and_client()
        assert provider.is_available() is True

        client.get.assert_called_once()
        headers = client.get.call_args.kwargs.get("headers") or {}
        assert headers.get("Authorization") == "Bearer tok-abc"


# =============================================================================
# #8 — XAU notional comes from a real price or the gate fails closed
# =============================================================================


@pytest.mark.xfail(strict=True, reason="review #8: XAU projected at placeholder price 1.0; no spot-price input exists")
class TestXauNotionalIsNeverAPlaceholder:
    """A proposed gold trade is sized at spot, or refused — never at $1/oz."""

    # Count thresholds set high so only the notional gates can fire.
    _CFG = dict(soft_warning_count=50, hard_block_count=100)

    def test_xau_signal_without_spot_price_fails_closed(self):
        gate = _gate(GateConfig(**self._CFG))

        decision = gate.evaluate(TradeSignal("XAU_USD", "LONG", 100, signal_id="x1"))

        assert decision.decision == "HARD_BLOCK"
        assert "XAU" in decision.reason
        assert decision.pending_id is None

    def test_xau_signal_with_spot_price_is_sized_at_spot(self):
        existing = Position(
            instrument="XAU_USD",
            direction="LONG",
            units=50,
            entry_price=Decimal("2350.00"),
            entry_time=datetime.now(timezone.utc),
            position_id="xau_open",
        )
        gate = _gate(GateConfig(**self._CFG), [existing])
        gate.update_spot_prices({"XAU_USD": Decimal("2350.00")})

        # 50 oz open + 50 oz proposed = 100 oz * 2350 = $235,000 gross
        # >= soft_warning_gross_notional (150,000) -> SOFT_WARNING
        decision = gate.evaluate(TradeSignal("XAU_USD", "LONG", 50, signal_id="x2"))

        xau = decision.projected_exposure.baskets["XAU"]
        assert xau.gross_notional == Decimal("235000.00")
        assert decision.decision == "SOFT_WARNING"
