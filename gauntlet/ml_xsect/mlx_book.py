"""Kill-test apparatus: build the L-S book, NET-turnover, crash-survival (max-DD / stressed Calmar), deflated
Sharpe. PURE numpy/pandas/scipy. The cheap P3 + net-turnover kill-order-first test.

The book is monthly-rebalanced quintile long-short, value-weighted within each leg (matching the construction-matched
factor returns). P3 uses the monthly book-return series (the crash MONTHS — COVID 2020-03, the Jan-2021 factor unwind,
the 2022 bear are all in the 2013-07+ window); stressed Calmar = annualized return / |max drawdown|.
"""
from __future__ import annotations

import numpy as np
import pandas as pd
from scipy import stats

PERIODS_PER_YEAR = 12        # monthly rebalance


def signed_book_weights(scores: dict, weights: dict, q: int = 5) -> dict:
    """{date: signed held weights} — long top-q-quantile (VW, sums +1), short bottom (VW, sums -1)."""
    held = {}
    for D, sc in scores.items():
        w = weights.get(D)
        df = pd.concat([sc.rename("s"), (w if w is not None else pd.Series(1.0, index=sc.index)).rename("w")],
                       axis=1).dropna()
        n = len(df)
        if n < 2 * q:
            continue
        ranks = df["s"].rank(method="first")
        long, short = ranks > n * (q - 1) / q, ranks <= n / q
        sw = pd.Series(0.0, index=df.index)
        wl, ws = df.loc[long, "w"], df.loc[short, "w"]
        if wl.sum() > 0:
            sw[long] = (wl / wl.sum()).values
        if ws.sum() > 0:
            sw[short] = (-ws / ws.sum()).values
        held[D] = sw
    return held


def net_turnover_series(held: dict) -> pd.Series:
    """Per-rebalance L1 change of the signed held book (Σ|w_t − w_{t-1}|); first period = full book on."""
    out, prev = {}, None
    for D in sorted(held):
        cur = held[D]
        if prev is None:
            out[D] = float(cur.abs().sum())
        else:
            idx = cur.index.union(prev.index)
            out[D] = float((cur.reindex(idx).fillna(0.0) - prev.reindex(idx).fillna(0.0)).abs().sum())
        prev = cur
    return pd.Series(out, dtype=float).sort_index()


def max_drawdown(ret: pd.Series) -> float:
    cum = (1.0 + ret.fillna(0.0)).cumprod()
    return float((cum / cum.cummax() - 1.0).min())


def calmar(ret: pd.Series, ppy: int = PERIODS_PER_YEAR) -> float:
    r = ret.dropna()
    if len(r) < 2:
        return float("nan")
    ann = (1.0 + r).prod() ** (ppy / len(r)) - 1.0
    mdd = abs(max_drawdown(r))
    return float(ann / mdd) if mdd > 0 else float("nan")


def ann_return(ret: pd.Series, ppy: int = PERIODS_PER_YEAR) -> float:
    r = ret.dropna()
    return float((1.0 + r).prod() ** (ppy / len(r)) - 1.0) if len(r) >= 2 else float("nan")


def probabilistic_sharpe(ret: pd.Series, sr_benchmark: float = 0.0) -> float:
    """PSR: P(true per-period SR > sr_benchmark), adjusting for skew/kurtosis (Bailey-López de Prado)."""
    r = ret.dropna().values
    T = len(r)
    sd = np.std(r, ddof=1)
    if T < 8 or sd == 0:
        return float("nan")
    sr = np.mean(r) / sd
    sk = float(stats.skew(r))
    ku = float(stats.kurtosis(r, fisher=False))
    denom = np.sqrt(1.0 - sk * sr + (ku - 1.0) / 4.0 * sr ** 2)
    if not np.isfinite(denom) or denom <= 0:
        return float("nan")
    return float(stats.norm.cdf((sr - sr_benchmark) * np.sqrt(T - 1) / denom))


def deflated_sharpe(ret: pd.Series, n_trials: int = 1, var_sr: float | None = None) -> float:
    """DSR = PSR at the deflated benchmark SR0 (the expected max SR over n_trials). n_trials=1 → SR0=0 → PSR(0)."""
    r = ret.dropna().values
    if len(r) < 8:
        return float("nan")
    if n_trials <= 1:
        sr0 = 0.0
    else:
        if var_sr is None:
            var_sr = 1.0 / max(len(r) - 1, 1)        # SR sampling variance proxy
        g = 0.5772156649
        e = np.e
        sr0 = np.sqrt(var_sr) * ((1 - g) * stats.norm.ppf(1 - 1.0 / n_trials) +
                                 g * stats.norm.ppf(1 - 1.0 / (n_trials * e)))
    return probabilistic_sharpe(ret, sr_benchmark=float(sr0))


def p3_killtest(book_ret_net: pd.Series, calmar_floor: float = 0.50,
                crash_months=("2020-03", "2021-01", "2022-09")) -> dict:
    """P3 deployability: REALIZED Calmar (ann/|maxDD|) vs the floor. ★ Note (S2): no stress is injected — this is the
    realized drawdown floor; a fail is a realized factor-bleed/DD, not a stress-scenario fail. Reports the crash-month
    returns (context: the book typically PASSES these — it fails on the realized DD floor) + the worst month."""
    r = book_ret_net.dropna()
    cm = {m: (float(r[r.index.to_period("M").astype(str) == m].iloc[0])
              if len(r[r.index.to_period("M").astype(str) == m]) else float("nan")) for m in crash_months}
    cal = calmar(r)
    return {"ann_return": ann_return(r), "max_drawdown": max_drawdown(r), "realized_calmar": cal,
            "calmar_floor": calmar_floor, "survives_p3": bool(np.isfinite(cal) and cal >= calmar_floor),
            "worst_month": float(r.min()), "crash_months": cm}
