"""Centralized re-export of the shared statistical apparatus.

Leaf modules import the reusable, source-verified primitives from here. The five primitive modules
(carry_gate, event_study_car, selection_stats, stats, deflation) are vendored alongside this module
in the same package, so they resolve as flat local imports. Verified signatures:
  - psr_spell(spell_returns, sr_star=0.0) -> (psr, sr)     carry_gate.py — eff-N-unit PSR;
      returns a TUPLE (psr_prob, sharpe); the gate uses psr (>=0.95). nan,nan if n<3 or zero-var.
  - ols_market_model(y, factors) -> (alpha, betas)         event_study_car.py — k-factor OLS w/ intercept
  - deflated_sharpe_ratio(returns, all_trial_sharpes) -> (dsr_prob, sr_obs, sr0_benchmark)  selection_stats.py
  - probabilistic_sharpe_ratio(returns, sr_benchmark=0.0) -> float   stats.py (DAILY; NOT for the gate)
  - holm(pvalues, alpha=0.05) -> list[dict] / holm_reject / holm_adjusted_pvalues   deflation.py

RT-A: use `ols_market_model` for (alpha, beta); compute resid = raw - F.beta (alpha RETAINED).
  Do NOT use event_study_car.abnormal_returns (it subtracts alpha -> mean-zero -> false-NULL).
"""
from __future__ import annotations

from carry_gate import psr_spell
from event_study_car import ols_market_model
from selection_stats import deflated_sharpe_ratio
from stats import probabilistic_sharpe_ratio
from deflation import holm, holm_reject, holm_adjusted_pvalues

__all__ = ["psr_spell", "ols_market_model", "deflated_sharpe_ratio",
           "probabilistic_sharpe_ratio", "holm", "holm_reject", "holm_adjusted_pvalues"]
