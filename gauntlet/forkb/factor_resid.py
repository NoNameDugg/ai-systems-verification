"""Leaf — PIT factor attribution, alpha RETAINED.

The deployable edge IS the alpha. The verdict reads off the factor-RESIDUAL net, and the
load-bearing scar is: NEVER subtract alpha. We strip only the factor exposure:

    resid = raw_net - factors @ betas        (betas from ols_market_model; alpha RETAINED)

so `mean(resid) == alpha` (~0 ONLY when the true alpha is 0). Using
`event_study_car.abnormal_returns` here would return `raw - (alpha + factors@betas)` = the OLS
residual epsilon, which is mean-ZERO by construction -> a guaranteed false-NULL. That is the
mistake this module exists to prevent (see the _apparatus.py docstring).

PUBLIC API (the gauntlet calls these — signatures are frozen):
  - pit_factor_returns(snap, cfg) -> pd.DataFrame[date x factor]
        PIT-formed size/value/mom/str/beta factor-return panel (charter §4: N6 — formed on
        as-of-date characteristics, never look-ahead). For the synthetic TDI the known factor
        returns live in truth['factors']; this builder reproduces that contract from the Snapshot
        (DAILY marketcap/pb/pe + SEP returns + DAILY market) so the gauntlet can run on real data.
  - residualize(raw_net, factor_returns, cfg) -> dict{resid, alpha, betas, alpha_t}
        regress raw_net ~ 1 + factors via ols_market_model -> (alpha, betas); then
        resid = raw_net - factors @ betas  (alpha RETAINED; mean(resid) == alpha).

Pure / numpy / pandas over the _schema/_apparatus/synth_fixtures contract only.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot
from _apparatus import ols_market_model


# ----------------------------------------------------------------------------------------------------
# PIT factor-return panel
# ----------------------------------------------------------------------------------------------------
def pit_factor_returns(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """Build the daily PIT factor-return panel: columns = cfg.factors, index = trading dates.

    N6 / charter §4: factors are formed on as-of-date (PIT) characteristics — the cross-sectional
    sort on day D uses marketcap/pb/pe/past-return KNOWN at D, never look-ahead. Each factor return
    is a daily long-short spread:
      size  = small-minus-big   (low marketcap minus high)
      value = value-minus-growth(low pb minus high pb)
      mom   = winners-minus-losers (high trailing 12-1 mom minus low)  [skip most-recent month]
      str   = short-term-reversal: losers-minus-winners over the prior ~21td
      beta  = the market factor = cap-weighted cross-sectional mean daily return (the systematic leg)

    The columns + index match the truth['factors'] contract (synth_fixtures), so the residualize()
    regression sees the same factor object on synthetic ground-truth as it will on real data.
    """
    factor_cols = list(cfg.factors)

    # --- daily total returns per name from SEP closeadj (total-return basis; charter §3 / Canary) ---
    sep = snap.sep[["ticker", "date", "closeadj"]].copy()
    sep["date"] = pd.to_datetime(sep["date"])
    sep = sep.sort_values(["ticker", "date"])
    sep["ret"] = sep.groupby("ticker", sort=False)["closeadj"].pct_change()
    ret_panel = sep.pivot(index="date", columns="ticker", values="ret")        # date x ticker
    dates = ret_panel.index

    # --- PIT characteristics from DAILY (marketcap / pb), as-of each date -------------------------
    daily = snap.daily[["ticker", "date", "marketcap", "pb"]].copy()
    daily["date"] = pd.to_datetime(daily["date"])
    mcap = daily.pivot(index="date", columns="ticker", values="marketcap").reindex(dates)
    pb = daily.pivot(index="date", columns="ticker", values="pb").reindex(dates)

    cols = ret_panel.columns
    mcap = mcap.reindex(columns=cols)
    pb = pb.reindex(columns=cols)

    # trailing-return characteristics (PIT — only uses past returns up to and including D-1) -------
    log1p = np.log1p(ret_panel.fillna(0.0))
    # 12-1 momentum: cumulative return over the prior ~252td excluding the most-recent ~21td
    mom_long = log1p.shift(1).rolling(231, min_periods=120).sum()
    str_char = log1p.shift(1).rolling(21, min_periods=10).sum()                 # short-term reversal char

    out = pd.DataFrame(index=dates, columns=factor_cols, dtype="float64")

    def _ls_spread(char: pd.DataFrame, low_minus_high: bool) -> pd.Series:
        """Daily long-short spread: terciles on `char` formed at D-1, return realized at D.

        low_minus_high=True  -> long the low-char tercile, short the high-char tercile.
        Characteristic is shifted by one day so formation is strictly PIT (no same-day peek)."""
        form = char.shift(1)                                                     # form on D-1, hold D
        res = pd.Series(np.nan, index=dates, dtype="float64")
        for d in dates:
            c = form.loc[d]
            r = ret_panel.loc[d]
            m = c.notna() & r.notna()
            if m.sum() < 6:
                continue
            cv = c[m]
            rv = r[m]
            lo_thr = cv.quantile(1.0 / 3.0)
            hi_thr = cv.quantile(2.0 / 3.0)
            low_leg = rv[cv <= lo_thr]
            high_leg = rv[cv >= hi_thr]
            if len(low_leg) == 0 or len(high_leg) == 0:
                continue
            spread = low_leg.mean() - high_leg.mean()
            res.loc[d] = spread if low_minus_high else -spread
        return res

    # size = small-minus-big = LOW marketcap minus HIGH marketcap
    out["size"] = _ls_spread(mcap, low_minus_high=True)
    # value = value-minus-growth = LOW pb minus HIGH pb
    out["value"] = _ls_spread(pb, low_minus_high=True)
    # mom = winners-minus-losers = HIGH mom minus LOW mom -> low_minus_high=False
    out["mom"] = _ls_spread(mom_long, low_minus_high=False)
    # str = short-term reversal = LOSERS-minus-winners over prior ~21td = LOW past-ret minus HIGH
    out["str"] = _ls_spread(str_char, low_minus_high=True)
    # beta = the market factor = cap-weighted cross-sectional mean daily return
    w = mcap.div(mcap.sum(axis=1), axis=0)
    out["beta"] = (ret_panel * w).sum(axis=1, min_count=1)

    return out[factor_cols].astype("float64")


# ----------------------------------------------------------------------------------------------------
# residualize — the RT-A core: strip factor exposure, RETAIN alpha
# ----------------------------------------------------------------------------------------------------
def _intercept_t(resid: np.ndarray, nw_lag: int = 0) -> float:
    """Plain (nw_lag=0) / Newey-West t of the mean of `resid` (== the regression intercept alpha).

    The charter §4 default for this leaf is a PLAIN t of the intercept; the conditional block-level
    1-lag NW correction (RT-B) lives in the M2/block module, not here. nw_lag>0 is supported for
    completeness (Bartlett kernel on the residual series itself) but defaults to 0.
    """
    x = np.asarray(resid, dtype="float64")
    x = x[np.isfinite(x)]
    n = x.size
    if n < 2:
        return float("nan")
    mu = x.mean()
    e = x - mu
    gamma0 = float(e @ e) / n                                  # population-style variance of resid
    var = gamma0
    for L in range(1, int(nw_lag) + 1):
        if L >= n:
            break
        wL = 1.0 - L / (nw_lag + 1.0)                          # Bartlett weight
        cov = float(e[L:] @ e[:-L]) / n
        var += 2.0 * wL * cov
    if var <= 0:
        return float("nan")
    se = np.sqrt(var / n)                                       # SE of the mean
    if se == 0:
        return float("nan")
    return float(mu / se)


def residualize(raw_net: pd.Series, factor_returns: pd.DataFrame, cfg: ForkBConfig) -> dict:
    """Regress raw_net ~ 1 + factors (ols_market_model) and strip ONLY the factor exposure.

    ★★ RT-A (load-bearing scar): resid = raw_net - factors @ betas  (alpha RETAINED).
       mean(resid) == alpha. Do NOT subtract alpha (that yields the mean-ZERO OLS epsilon ->
       a guaranteed false-NULL; the deployable edge IS alpha).

    Returns dict:
      resid   : pd.Series aligned to the clean (raw, factors) intersection — the factor-stripped,
                alpha-retaining series the verdict reads off.
      alpha   : float — the OLS intercept (the daily mean alpha).
      betas   : dict{factor -> beta} — the factor loadings.
      alpha_t : float — plain t of the intercept (== t of mean(resid)).
    """
    factor_cols = list(cfg.factors)
    F = factor_returns.reindex(columns=factor_cols)

    # align raw_net to the factor panel on the common, fully-finite dates (pairwise drop)
    rn = pd.to_numeric(raw_net, errors="coerce")
    aligned = pd.concat([rn.rename("y"), F], axis=1, join="inner")
    mask = aligned.notna().all(axis=1)
    aligned = aligned[mask]

    y = aligned["y"].to_numpy(dtype="float64")
    Fm = aligned[factor_cols].to_numpy(dtype="float64")

    alpha, beta_arr = ols_market_model(y, Fm)
    betas = {c: float(b) for c, b in zip(factor_cols, np.asarray(beta_arr, dtype="float64"))}

    # ★ RT-A: strip factor exposure ONLY — alpha is NOT subtracted.
    resid_vals = y - Fm @ np.asarray(beta_arr, dtype="float64")
    resid = pd.Series(resid_vals, index=aligned.index, name="resid")

    alpha_t = _intercept_t(resid_vals, nw_lag=0)

    return {"resid": resid, "alpha": float(alpha), "betas": betas, "alpha_t": alpha_t}
