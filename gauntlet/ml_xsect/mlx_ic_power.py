"""IC-IR power + inference. PURE numpy/pandas — no I/O, deterministic.

The headline statistic is the TIME SERIES of per-rebalance cross-sectional Spearman ICs {IC_t}. This module:
  - cross_sectional_ic : one Spearman rank-IC over names at a rebalance (collapses the cross-section -> handles the
                         same-date cross-sectional return correlation).
  - ic_series          : {date -> IC_t} over rebalances.
  - t_eff              : autocorrelation-discounted effective T of {IC_t} (mean-of-AR(1) correction) — the SAME T_eff
                         feeds BOTH the MDE and the block-t inference (no split-inconsistency).
  - mde_ic             : MDE_meanIC = Z_SUM * std(IC_t)/sqrt(T_eff)  (NOT a single-correlation 1/sqrt(N_eff)).
  - block_t            : analytic t on mean({IC_t}) with SE = std/sqrt(T_eff) (NEVER the per-date Fisher CI).
  - power_verdict      : POWERED iff MDE_meanIC <= ceiling (= the 0.02 decision bar).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from mlx_config import MLXConfig


def cross_sectional_ic(scores: pd.Series, fwd_ret: pd.Series, min_n: int = 50) -> float:
    """Spearman rank-IC between `scores` and `fwd_ret` across names at ONE rebalance (NaN if < min_n overlap)."""
    df = pd.concat([scores.rename("s"), fwd_ret.rename("r")], axis=1).dropna()
    if len(df) < min_n:
        return float("nan")
    if df["s"].nunique() < 2 or df["r"].nunique() < 2:
        return float("nan")
    return float(df["s"].corr(df["r"], method="spearman"))


def ic_series(scores_by_date: dict, fwd_by_date: dict, min_n: int | None = None, cfg: MLXConfig | None = None) -> pd.Series:
    """{date → IC_t} over the rebalances present in BOTH dicts. `min_n` defaults to `cfg.min_xsection`."""
    cfg = cfg if cfg is not None else MLXConfig()
    min_n = cfg.min_xsection if min_n is None else min_n
    out = {}
    for d, s in scores_by_date.items():
        r = fwd_by_date.get(d)
        if r is not None:
            ic = cross_sectional_ic(s, r, min_n=min_n)
            if np.isfinite(ic):
                out[d] = ic
    return pd.Series(out, dtype=float).sort_index()


def ar1(x) -> float:
    """Lag-1 autocorrelation of a 1-D series (0.0 if undefined)."""
    a = np.asarray(x, dtype=float)
    a = a[np.isfinite(a)]
    if a.size < 3:
        return 0.0
    x1, x0 = a[1:], a[:-1]
    if np.std(x1) == 0.0 or np.std(x0) == 0.0:
        return 0.0
    return float(np.corrcoef(x1, x0)[0, 1])


def t_eff(ic: pd.Series) -> float:
    """Autocorrelation-discounted effective T for the MEAN of an AR(1)-ish {IC_t} series: T·(1−ρ)/(1+ρ), capped at T."""
    vals = np.asarray(ic.dropna().values, dtype=float)
    T = vals.size
    if T <= 1:
        return float(max(T, 1))
    rho = float(np.clip(ar1(vals), -0.999, 0.999))
    te = T * (1.0 - rho) / (1.0 + rho)
    return float(min(max(te, 1.0), float(T)))     # cap at T (cannot gain power beyond the realized sample)


def _std(ic: pd.Series) -> float:
    vals = np.asarray(ic.dropna().values, dtype=float)
    return float(np.std(vals, ddof=1)) if vals.size >= 2 else float("nan")


def mde_ic(ic: pd.Series, z_sum: float | None = None, cfg: MLXConfig | None = None) -> float:
    """Minimum detectable mean rank-IC = z_sum · std(IC_t)/√T_eff. `z_sum` defaults to `cfg.z_sum`."""
    cfg = cfg if cfg is not None else MLXConfig()
    z_sum = cfg.z_sum if z_sum is None else z_sum
    s, te = _std(ic), t_eff(ic)
    if not np.isfinite(s) or te <= 0:
        return float("nan")
    return float(z_sum * s / np.sqrt(te))


def block_t(ic: pd.Series, cfg: MLXConfig | None = None) -> dict:
    """Analytic inference on mean({IC_t}). Returns mean, se (= std/√T_eff), t, ci_lo/ci_hi (at `cfg.block_t_crit`,
    1.96 = 95%), T, T_eff, and `significant` (t >= block_t_crit)."""
    cfg = cfg if cfg is not None else MLXConfig()
    vals = np.asarray(ic.dropna().values, dtype=float)
    T = vals.size
    m = float(np.mean(vals)) if T else float("nan")
    s, te = _std(ic), t_eff(ic)
    se = s / np.sqrt(te) if (np.isfinite(s) and te > 0) else float("nan")
    t = m / se if (np.isfinite(se) and se > 0) else float("nan")
    half = cfg.block_t_crit * se if np.isfinite(se) else float("nan")
    return {"mean": m, "se": se, "t": t, "ci_lo": m - half, "ci_hi": m + half, "T": int(T), "T_eff": te,
            "significant": bool(np.isfinite(t) and t >= cfg.block_t_crit)}


def power_verdict(ic: pd.Series, z_sum: float | None = None, ceiling: float | None = None,
                  cfg: MLXConfig | None = None) -> dict:
    """POWERED iff MDE_meanIC ≤ ceiling (`cfg.ic_power_ceiling`, the decision bar). A CLOSE-FRONTIER is honest only
    when POWERED. Also labels the realized mean IC against the frozen bars: GLIMMER (mean ≥ `cfg.ic_glimmer_bar` with
    ci_lo > 0) and STRONG (mean ≥ `cfg.ic_strong_bar` with ci_lo > 0)."""
    cfg = cfg if cfg is not None else MLXConfig()
    z_sum = cfg.z_sum if z_sum is None else z_sum
    ceiling = cfg.ic_power_ceiling if ceiling is None else ceiling
    mde = mde_ic(ic, z_sum=z_sum, cfg=cfg)
    powered = bool(np.isfinite(mde) and mde <= ceiling)
    bt = block_t(ic, cfg=cfg)
    positive = bool(np.isfinite(bt["ci_lo"]) and bt["ci_lo"] > 0)
    return {"mde_ic": mde, "ceiling": ceiling, "powered": powered, "mean_ic": bt["mean"], "ci_lo": bt["ci_lo"],
            "glimmer": bool(positive and bt["mean"] >= cfg.ic_glimmer_bar),
            "strong": bool(positive and bt["mean"] >= cfg.ic_strong_bar)}
