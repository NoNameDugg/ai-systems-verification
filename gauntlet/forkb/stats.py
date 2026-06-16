"""Performance statistics (float-native, on net-R per-trade series).

PSR (Probabilistic Sharpe Ratio, Bailey-López de Prado) is the headline statistic — the honest,
non-degenerate measure for a SINGLE fixed strategy (PBO/CPCV need a config matrix, deferred
to the selection phase). SR-hat + T are at the SAME per-trade frequency (NOT annualized;
annualizing SR-hat with a trade-count T silently inflates PSR).

PF / expectancy / Sharpe / Sortino / MaxDD are textbook one-liners on a float series.
"""
from __future__ import annotations

import math
from typing import Sequence

import numpy as np


def _arr(returns: Sequence[float]) -> np.ndarray:
    return np.asarray(list(returns), dtype=float)


def profit_factor(returns: Sequence[float]) -> float:
    r = _arr(returns)
    gains = r[r > 0].sum()
    losses = -r[r < 0].sum()
    if losses == 0:
        return float("inf") if gains > 0 else 0.0
    return float(gains / losses)


def expectancy_r(returns: Sequence[float]) -> float:
    r = _arr(returns)
    return float(r.mean()) if len(r) else 0.0


def win_rate(returns: Sequence[float]) -> float:
    r = _arr(returns)
    return float((r > 0).mean()) if len(r) else 0.0


def sharpe(returns: Sequence[float]) -> float:
    """Per-trade Sharpe = mean / sample-std (ddof=1). NOT annualized."""
    r = _arr(returns)
    if len(r) < 2:
        return 0.0
    sd = r.std(ddof=1)
    return float(r.mean() / sd) if sd > 0 else 0.0


def sortino(returns: Sequence[float]) -> float:
    r = _arr(returns)
    if len(r) < 2:
        return 0.0
    downside = r[r < 0]
    dd = downside.std(ddof=1) if len(downside) > 1 else 0.0
    return float(r.mean() / dd) if dd > 0 else 0.0


def max_drawdown_r(returns: Sequence[float]) -> float:
    """Max peak-to-trough drop of the cumulative (additive R) equity curve. Returns ≤ 0."""
    r = _arr(returns)
    if len(r) == 0:
        return 0.0
    eq = np.cumsum(r)
    peak = np.maximum.accumulate(eq)
    return float((eq - peak).min())


def _norm_cdf(x: float) -> float:
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def probabilistic_sharpe_ratio(returns: Sequence[float], sr_benchmark: float = 0.0) -> float:
    """PSR(SR*) = Φ( (ŜR − SR*)·√(T−1) / √(1 − γ₃·ŜR + ((γ₄−1)/4)·ŜR²) ).
    ŜR per-trade; γ₃ skewness; γ₄ non-excess kurtosis; T = n. v1 SR*=0."""
    r = _arr(returns)
    T = len(r)
    if T < 3:
        return float("nan")
    mu, sd = r.mean(), r.std(ddof=1)
    if sd == 0:
        return float("nan")
    sr = mu / sd
    z = (r - mu) / r.std(ddof=0)               # standardized (population σ for moments)
    g3 = float((z ** 3).mean())                # skewness
    g4 = float((z ** 4).mean())                # kurtosis (non-excess; normal → 3)
    denom = 1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr ** 2
    if denom <= 0:
        return float("nan")
    return _norm_cdf((sr - sr_benchmark) * math.sqrt(T - 1) / math.sqrt(denom))


def summary(returns: Sequence[float]) -> dict:
    return {
        "n": len(_arr(returns)),
        "profit_factor": profit_factor(returns),
        "expectancy_r": expectancy_r(returns),
        "win_rate": win_rate(returns),
        "sharpe": sharpe(returns),
        "sortino": sortino(returns),
        "max_drawdown_r": max_drawdown_r(returns),
        "psr": probabilistic_sharpe_ratio(returns),
    }
