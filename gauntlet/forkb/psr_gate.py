"""Leaf — PSR overfit gate + DSR report.

The overfit-control line of the deployability stack. Both functions operate on the
RESIDUAL BLOCK series — the eff-N-unit (non-overlapping 63-td block-summed residual returns),
NOT the daily series. Building the block series itself is upstream (block module); these
functions are pure consumers of an already-blocked array.

The PSR gate is `carry_gate.psr_spell(block_series, SR*=0)` >= 0.95 — re-pinned from the
  daily `stats` PSR (which left a tunable T) to the eff-N-unit spell PSR. The block series IS the
  spell series here.
DSR is REPORTED, not a gate (cfg.dsr_is_gate is False). dsr_report exposes the full
  (dsr, sr, sr0) triple deflated by the strategy-grid Sharpes; no pass/fail is emitted.
"""
from __future__ import annotations

import numpy as np

from _apparatus import psr_spell, deflated_sharpe_ratio


def psr_gate(resid_block_returns: np.ndarray, cfg) -> dict:
    """PSR overfit gate on the residual BLOCK series (the eff-N unit, NOT daily).

    psr, sr = psr_spell(resid_block_returns, cfg.psr_sr_star)   # SR* = 0.0 (charter pin)
    passed  = psr >= cfg.psr_threshold                          # >= 0.95 (charter pin)

    Returns dict{psr, sr, passed}. If psr is nan (n < 3 or zero-variance block series),
    passed is False (nan >= 0.95 is False, propagated explicitly).
    """
    block = np.asarray(resid_block_returns, dtype=float)
    psr, sr = psr_spell(block, cfg.psr_sr_star)
    passed = bool(np.isfinite(psr) and psr >= cfg.psr_threshold)
    return {"psr": float(psr), "sr": float(sr), "passed": passed}


def dsr_report(block_returns: np.ndarray, grid_sharpes: list[float]) -> dict:
    """Deflated Sharpe Ratio — REPORTED, not a gate (RT-E; cfg.dsr_is_gate is False).

    Deflates the block series' Sharpe by the expected-max Sharpe over the strategy-grid trial set
    (the grid Sharpes). Returns dict{dsr, sr, sr0} — the unpacked apparatus triple.
    """
    block = np.asarray(block_returns, dtype=float)
    dsr, sr, sr0 = deflated_sharpe_ratio(block, grid_sharpes)
    return {"dsr": float(dsr), "sr": float(sr), "sr0": float(sr0)}
