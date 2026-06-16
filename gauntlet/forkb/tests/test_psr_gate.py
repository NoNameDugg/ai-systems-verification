"""TDI — psr_gate (charter §4; N1/DH-A; RT-E).

Asserts the gate semantics against constructed ground-truth block series + the apparatus contract:
  (1) strong positive block series (mean>0, modest vol, n>=30) -> psr >= 0.95, passed True
  (2) zero-mean block series                                   -> psr ~= 0.5, passed False
  (3) dsr_report returns the apparatus 3-tuple unpacked into {dsr, sr, sr0}
  (4) psr_spell is fed the BLOCK series (called with the block-length array, NOT a daily array)

The block series is the eff-N unit (non-overlapping 63-td block-summed residual returns, DH-A);
these are pure consumers of an already-blocked array, so the fixtures construct blocks directly.
"""
from __future__ import annotations

import numpy as np
import pytest

import psr_gate
from psr_gate import psr_gate as run_psr_gate, dsr_report
from _schema import ForkBConfig
from _apparatus import psr_spell, deflated_sharpe_ratio


CFG = ForkBConfig()


# ---------------------------------------------------------------------------------------------------
# (1) a strong positive block series clears the 0.95 PSR gate
# ---------------------------------------------------------------------------------------------------
def test_strong_positive_block_passes():
    rng = np.random.default_rng(7)
    n = 100                                            # >= 30 blocks (well past the gate's n>=3 floor)
    # strong, persistent positive edge: high mean relative to modest vol -> high block-Sharpe -> PSR->1
    block = rng.normal(0.02, 0.01, n)
    out = run_psr_gate(block, CFG)

    # cross-check against the apparatus directly (verdict-numbers-from-source discipline)
    psr_ref, sr_ref = psr_spell(block, CFG.psr_sr_star)
    assert out["psr"] == pytest.approx(psr_ref)
    assert out["sr"] == pytest.approx(sr_ref)
    assert sr_ref > 0                                  # genuinely positive Sharpe by construction
    assert out["psr"] >= CFG.psr_threshold            # clears 0.95
    assert out["passed"] is True


# ---------------------------------------------------------------------------------------------------
# (2) a zero-mean block series -> PSR ~= 0.5, gate fails
# ---------------------------------------------------------------------------------------------------
def test_zero_mean_block_fails():
    # exactly mean-zero, symmetric block series: Sharpe = 0 -> PSR = Phi(0) = 0.5
    block = np.array([0.01, -0.01] * 25, dtype=float)  # n=50, mean exactly 0
    out = run_psr_gate(block, CFG)

    assert out["sr"] == pytest.approx(0.0, abs=1e-9)
    assert out["psr"] == pytest.approx(0.5, abs=1e-6)  # Phi(0)
    assert out["passed"] is False                      # 0.5 < 0.95


# ---------------------------------------------------------------------------------------------------
# (3) dsr_report unpacks the apparatus (dsr, sr, sr0) triple into the dict — REPORTED, not a gate
# ---------------------------------------------------------------------------------------------------
def test_dsr_report_unpacks_triple():
    rng = np.random.default_rng(11)
    block = rng.normal(0.01, 0.012, 80)
    grid = [0.4, 0.55, 0.3, 0.62, 0.48, 0.51, 0.39, 0.58]   # the strategy-grid trial Sharpes

    out = dsr_report(block, grid)
    assert set(out) == {"dsr", "sr", "sr0"}

    dsr_ref, sr_ref, sr0_ref = deflated_sharpe_ratio(block, grid)
    assert out["dsr"] == pytest.approx(dsr_ref)
    assert out["sr"] == pytest.approx(sr_ref)
    assert out["sr0"] == pytest.approx(sr0_ref)
    # RT-E: dsr_report emits NO pass/fail key (it is reported, never gating)
    assert "passed" not in out
    assert CFG.dsr_is_gate is False


# ---------------------------------------------------------------------------------------------------
# (4) psr_gate feeds psr_spell the BLOCK series (block-length array), NOT a daily array
# ---------------------------------------------------------------------------------------------------
def test_psr_spell_fed_the_block_series(monkeypatch):
    captured = {}

    def spy(arr, sr_star=0.0):
        captured["arr"] = np.asarray(arr, float)
        captured["sr_star"] = sr_star
        return 0.99, 0.5

    monkeypatch.setattr(psr_gate, "psr_spell", spy)

    block = np.linspace(0.001, 0.02, 104)              # ~104-pt residual block series (the eff-N unit)
    out = run_psr_gate(block, CFG)

    # the array handed to psr_spell is EXACTLY the block series (block-length), not a longer daily one
    assert captured["arr"].shape == block.shape
    assert captured["arr"].shape[0] == 104             # block length, NOT ~63*104 daily length
    np.testing.assert_allclose(captured["arr"], block)
    # and the charter SR* pin is passed through
    assert captured["sr_star"] == CFG.psr_sr_star == 0.0
    assert out["passed"] is True                       # 0.99 >= 0.95 from the spy
