"""Leaf `fundamental_factors` — CMA (investment / asset-growth) + RMW (profitability) PIT factors.

These are INTERPRETATION-residual factors — NON-GATING. They exist ONLY to LABEL the NSI verdict
NOVEL vs KNOWN-FACTOR (NSI ~ Fama-French CMA / Pontiff-Woodgate investment factor). They must NEVER
enter the GATED residual (factor_resid.pit_factor_returns's frozen 5-factor set): regressing NSI on the
near-collinear CMA would strip the signal -> a by-construction false-NULL, the inverse of the
never-subtract-alpha scar. A separate leaf makes that isolation structural, not a convention.

Construction (PINNED): CMA_t = log(assets_t / assets_{t-4q}) on SF1 ARQ (cfg.cma_field, cfg.cma_yoy_q), fiscal-quarter
aligned, PIT (datekey<=D-1 via the same shift(1) form-discipline as factor_resid). RMW_t = gp_t / assets_t (Novy-Marx
gross profitability; cfg.rmw_gp_field / cfg.rmw_assets_field). Daily factor return = tercile long-short spread
(form D-1, realize D), matching factor_resid._ls_spread: CMA = conservative-minus-aggressive (LOW asset-growth minus
HIGH); RMW = robust-minus-weak (HIGH profitability minus LOW).

Pure function library: depends only on _schema + factor_resid + numpy/pandas (no back-edge to harness/gauntlet).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot
import factor_resid


def _ret_panel(snap: Snapshot) -> pd.DataFrame:
    """Daily total returns per name from closeadj (total-return basis), date x ticker — as factor_resid."""
    sep = snap.sep[["ticker", "date", "closeadj"]].copy()
    sep["date"] = pd.to_datetime(sep["date"])
    sep = sep.sort_values(["ticker", "date"])
    sep["ret"] = sep.groupby("ticker", sort=False)["closeadj"].pct_change()
    return sep.pivot(index="date", columns="ticker", values="ret")


def _char_from_sf1(snap: Snapshot, cfg: ForkBConfig, kind: str) -> pd.DataFrame:
    """Per-(ticker, datekey) characteristic. kind in {'cma','rmw'}. Long form [ticker, datekey, val];
    drop-not-NaN; fiscal-quarter aligned for CMA's YoY base (nsi.py discipline)."""
    arq = snap.sf1[snap.sf1["dimension"] == cfg.sue_dim_primary]
    frames = []
    for ticker, g in arq.groupby("ticker", sort=False):
        g = g.sort_values(["reportperiod", "datekey"], kind="stable")
        dk = pd.to_datetime(g["datekey"].to_numpy())
        if kind == "cma":
            a = g[cfg.cma_field].to_numpy(dtype=float)
            q = cfg.cma_yoy_q
            if len(g) <= q:
                continue
            cur, base, dkk = a[q:], a[: len(g) - q], dk[q:]
            with np.errstate(divide="ignore", invalid="ignore"):
                valid = np.isfinite(cur) & np.isfinite(base) & (cur > 0.0) & (base > 0.0)
                val = np.log(cur[valid] / base[valid])
            dkk = dkk[valid]
        else:  # rmw = gp / assets (Novy-Marx gross profitability)
            gp = g[cfg.rmw_gp_field].to_numpy(dtype=float)
            assets = g[cfg.rmw_assets_field].to_numpy(dtype=float)
            with np.errstate(divide="ignore", invalid="ignore"):
                valid = np.isfinite(gp) & np.isfinite(assets) & (assets > 0.0)
                val = gp[valid] / assets[valid]
            dkk = dk[valid]
        if len(val) == 0:
            continue
        frames.append(pd.DataFrame({"ticker": ticker, "datekey": dkk, "val": val}))
    if not frames:
        return pd.DataFrame({"ticker": pd.Series(dtype=object), "datekey": pd.Series(dtype="datetime64[ns]"),
                             "val": pd.Series(dtype=float)})
    return pd.concat(frames, ignore_index=True).sort_values(["ticker", "datekey"], kind="stable")


def _broadcast_asof(char_df: pd.DataFrame, dates: pd.DatetimeIndex, cols, max_stale_days: int | None = None) -> pd.DataFrame:
    """date x ticker panel: most-recent char with datekey <= D (the shift(1) in _ls_spread then enforces <= D-1).
    ★ BD S2-AMEND: if max_stale_days is set, mask any cell whose most-recent datekey is staler than N CALENDAR days
    (matches the wave-2 screen's 400d cap; default None = unbounded ffill so BC/interp are bit-identical)."""
    if len(char_df) == 0:
        return pd.DataFrame(index=dates, columns=cols, dtype="float64")
    piv = char_df.pivot_table(index="datekey", values="val", columns="ticker", aggfunc="last")
    full = piv.index.union(dates)
    panel = piv.reindex(full).sort_index().ffill().reindex(dates).reindex(columns=cols).astype("float64")
    if max_stale_days is not None:
        # last-known datekey per (date, ticker): ffill the OBSERVED datekey itself, then age = D - last_datekey.
        # ★ force datetime64[ns] on BOTH sides before astype(int64) — the snapshot is µs-resolution, so a raw
        #   .astype("int64") would yield µs and the ns-per-day divisor would make every age 1000x too small (no mask).
        dk_ns = piv.index.values.astype("datetime64[ns]").astype("int64")
        dk_obs = pd.DataFrame(np.where(piv.notna().to_numpy(), dk_ns[:, None], np.nan),
                              index=piv.index, columns=piv.columns)
        last_dk = dk_obs.reindex(full).sort_index().ffill().reindex(dates).reindex(columns=cols)
        date_ns = dates.values.astype("datetime64[ns]").astype("int64")
        age_days = (date_ns[:, None] - last_dk.to_numpy()) / 8.64e13                          # ns -> calendar days
        panel = panel.mask(age_days > float(max_stale_days))
    return panel


def pit_fundamental_chars(snap: Snapshot, cfg: ForkBConfig, max_stale_days: int | None = None) -> dict:
    """{'cma': date x ticker, 'rmw': date x ticker} — PIT as-of-date characteristic panels.
    max_stale_days (BD S2-AMEND) is forwarded to _broadcast_asof; callers that pass None (BC interp factors) are
    bit-identical to the pre-AMEND behavior."""
    ret = _ret_panel(snap)
    return {"cma": _broadcast_asof(_char_from_sf1(snap, cfg, "cma"), ret.index, ret.columns, max_stale_days),
            "rmw": _broadcast_asof(_char_from_sf1(snap, cfg, "rmw"), ret.index, ret.columns, max_stale_days)}


def _ls_spread(char: pd.DataFrame, ret_panel: pd.DataFrame, dates, low_minus_high: bool) -> pd.Series:
    """Daily tercile long-short spread; form on D-1, realize at D — identical mechanics to factor_resid._ls_spread."""
    form = char.shift(1)
    res = pd.Series(np.nan, index=dates, dtype="float64")
    for d in dates:
        c = form.loc[d]
        r = ret_panel.loc[d]
        m = c.notna() & r.notna()
        if m.sum() < 6:
            continue
        cv, rv = c[m], r[m]
        lo_thr, hi_thr = cv.quantile(1.0 / 3.0), cv.quantile(2.0 / 3.0)
        low_leg, high_leg = rv[cv <= lo_thr], rv[cv >= hi_thr]
        if len(low_leg) == 0 or len(high_leg) == 0:
            continue
        spread = low_leg.mean() - high_leg.mean()
        res.loc[d] = spread if low_minus_high else -spread
    return res


def cma_rmw_factor_returns(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """date x {cma, rmw} daily long-short factor returns. CMA = low-growth minus high; RMW = high-prof minus low."""
    ret = _ret_panel(snap)
    chars = pit_fundamental_chars(snap, cfg)
    out = pd.DataFrame(index=ret.index, dtype="float64")
    out["cma"] = _ls_spread(chars["cma"], ret, ret.index, low_minus_high=True)
    out["rmw"] = _ls_spread(chars["rmw"], ret, ret.index, low_minus_high=False)
    return out


def interp_factor_returns(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """The 7-factor INTERPRETATION panel: the frozen 5 gating factors + CMA + RMW, ordered to cfg.interp_factors.
    The first 5 columns are bit-identical to factor_resid.pit_factor_returns (the gating set)."""
    base = factor_resid.pit_factor_returns(snap, cfg)         # the 5 gating factors (unchanged)
    fr = cma_rmw_factor_returns(snap, cfg)
    both = base.join(fr, how="left")
    return both.reindex(columns=list(cfg.interp_factors)).astype("float64")
