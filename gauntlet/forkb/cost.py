"""Single-name cost model.

ETF-scale cost assumptions (3 bps / 50 bps borrow) are 1-2 orders too cheap for single-name
micro-caps. This module is a per-name Corwin-Schultz spread (with the two-day negative-spread
correction, NOT floor-to-zero), graded up to a CONSERVATIVE band
(`max(CS*1.75, external-decade-anchor, data-derived-uplift)`), and the deployable-net composition
(turnover charged at the band, short notional charged a daily-accrued conservative borrow).

PURE function library over the contract — depends ONLY on _schema (+ numpy/pandas). No I/O, no data peek.

Design scars honored:
  - CS two-day negative-spread correction (NOT floor-to-zero) — `cfg.cost_neg_spread_two_day_correction`.
  - The external decade-anchor is a MANDATORY `max()` floor (caps any optimism the data-derived term
    could introduce) — anchor selected by the name's liquidity decile.
  - Opposite-side fills already in the band (buy ask / sell bid — bounce paid) — turnover is charged the
    full band once (the round-trip bounce is the band, not a separate add).
  - Borrow accrues DAILY on held short notional, at a conservative `max(borrow)` rate.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig

# Corwin-Schultz (2012) constant: 3 - 2*sqrt(2)
_CS_K = 3.0 - 2.0 * np.sqrt(2.0)


# ----------------------------------------------------------------------------------------------------
# Corwin-Schultz proportional spread, per name (bps).
# ----------------------------------------------------------------------------------------------------
def _cs_proportional_per_name(high: np.ndarray, low: np.ndarray, two_day_correction: bool) -> float:
    """Corwin-Schultz (2012) high-low proportional spread for one name's daily high/low arrays.

    For each pair of consecutive trading days (t, t+1):
      beta  = (ln(H_t/L_t))^2 + (ln(H_{t+1}/L_{t+1}))^2          [single-day squared log-ranges, summed]
      gamma = (ln(max(H_t,H_{t+1}) / min(L_t,L_{t+1})))^2        [two-day squared log-range]
      alpha = (sqrt(2*beta) - sqrt(beta)) / (3-2sqrt2) - sqrt(gamma/(3-2sqrt2))
      S     = 2*(e^alpha - 1) / (1 + e^alpha)                    [proportional spread for that pair]

    Returns the average proportional spread over all valid consecutive pairs (a fraction, not bps).
    `two_day_correction`: per Corwin-Schultz, a negative two-day spread estimate is set to 0 ON THAT PAIR
    (the "two-day" correction) rather than flooring the whole series or dropping it — NOT a floor-to-zero
    of the final per-name estimate (which would discard the conservative-uplift information downstream).
    """
    h = np.asarray(high, float)
    l = np.asarray(low, float)
    if h.size < 2:
        return np.nan
    # guard against non-positive / inverted bars
    valid_day = (h > 0) & (l > 0) & (h >= l)
    spreads = []
    for t in range(h.size - 1):
        if not (valid_day[t] and valid_day[t + 1]):
            continue
        hr_t = np.log(h[t] / l[t])
        hr_t1 = np.log(h[t + 1] / l[t + 1])
        beta = hr_t ** 2 + hr_t1 ** 2
        h2 = max(h[t], h[t + 1])
        l2 = min(l[t], l[t + 1])
        gamma = np.log(h2 / l2) ** 2
        alpha = (np.sqrt(2.0 * beta) - np.sqrt(beta)) / _CS_K - np.sqrt(gamma / _CS_K)
        s = 2.0 * (np.exp(alpha) - 1.0) / (1.0 + np.exp(alpha))
        if s < 0.0:
            if two_day_correction:
                # two-day correction: a negative pairwise estimate -> 0 for that pair (not dropped)
                s = 0.0
            else:
                continue
        spreads.append(s)
    if not spreads:
        return np.nan
    return float(np.mean(spreads))


def corwin_schultz_bps(sep: pd.DataFrame, cfg: ForkBConfig) -> pd.Series:
    """The Corwin-Schultz spread (bps) per name from high/low (SEP `high`/`low`), with the two-day
    negative-spread correction (NOT floor-to-zero) per `cfg.cost_neg_spread_two_day_correction`.

    Returns a pd.Series indexed by ticker (proportional spread * 1e4 = bps).
    """
    out = {}
    for tkr, g in sep.sort_values(["ticker", "date"]).groupby("ticker", sort=True):
        prop = _cs_proportional_per_name(
            g["high"].to_numpy(), g["low"].to_numpy(),
            two_day_correction=bool(cfg.cost_neg_spread_two_day_correction),
        )
        out[tkr] = prop * 1e4 if np.isfinite(prop) else np.nan
    return pd.Series(out, name="cs_bps").sort_index()


# ----------------------------------------------------------------------------------------------------
# Conservative band (bps), per name — the deployable grading spread.
# ----------------------------------------------------------------------------------------------------
def _liquidity_decile(sep: pd.DataFrame, daily: pd.DataFrame) -> pd.Series:
    """Per-name liquidity decile (0..9, 0 = least liquid) by median dollar-volume.

    Uses DAILY.marketcap when available (the size proxy the external anchor is decade-anchored on);
    falls back to SEP close*volume. Decile 9 = most liquid -> cheapest anchor; decile 0 = least liquid
    -> most-expensive anchor (the illiquid tail CS under-states most).
    """
    if daily is not None and "marketcap" in daily.columns and len(daily):
        liq = daily.groupby("ticker")["marketcap"].median()
    else:
        dv = sep.assign(_dv=sep["close"] * sep["volume"])
        liq = dv.groupby("ticker")["_dv"].median()
    # rank -> decile 0..9 (least->most liquid). qcut can fail on ties / tiny N -> rank fallback.
    n = len(liq)
    if n == 0:
        return pd.Series(dtype=int)
    ranks = liq.rank(method="first")
    dec = np.minimum((ranks.to_numpy() - 1) * 10 // n, 9).astype(int)
    return pd.Series(dec, index=liq.index, name="liq_decile")


def conservative_band_bps(sep: pd.DataFrame, daily: pd.DataFrame, cfg: ForkBConfig,
                          calibration_set=None, require_calibration: bool = False) -> pd.Series:
    """The conservative grading band (bps), per name:

        band = max( CS * cfg.cost_uplift_floor_mult [1.75],
                    external-decade-anchor[liquidity_decile]  (MANDATORY max() floor),
                    data-derived-uplift )

    `cfg.cost_external_anchor_bps` is a 10-tuple of decade-anchor bps by liquidity decile (index 0 = least
    liquid / most expensive ... index 9 = most liquid / cheapest); the anchor is selected by the name's
    liquidity decile and is a MANDATORY `max()` floor. The data-derived uplift (a decile-wise CS-vs-
    effective-spread multiplier, flat-extrapolated to the illiquid tail — R2) is supplied via the same CS
    column here as `CS * uplift_mult` (the binding-term form); absent a calibration set in this pure layer
    it coincides with the CS*1.75 term, so the `max()` of all three is the conservative result and the
    external anchor remains the hard floor. Returns a pd.Series indexed by ticker (bps).
    """
    # ★ E-1 (S2): deployable-grade REFUSES the CS×1.75 placeholder — it requires a fitted decile multiplier.
    if require_calibration and calibration_set is None:
        raise ValueError("E-1 (S2): deployable-grade cost band requires a fitted decile-multiplier "
                         "calibration_set; the CSx1.75 alias is a pre-data PLACEHOLDER and must not "
                         "produce a deployable verdict.")
    cs = corwin_schultz_bps(sep, cfg)
    dec = _liquidity_decile(sep, daily)
    anchors = np.asarray(cfg.cost_external_anchor_bps, float)
    out = {}
    for tkr in cs.index:
        cs_bps = cs.loc[tkr]
        cs_term = (cs_bps * cfg.cost_uplift_floor_mult) if np.isfinite(cs_bps) else 0.0
        # external decade anchor by liquidity decile (mandatory floor)
        d = int(dec.loc[tkr]) if tkr in dec.index else 0
        d = max(0, min(d, anchors.size - 1))
        anchor = float(anchors[d])
        # data-derived decile-uplift (R2): the FITTED CS-vs-effective multiplier when calibrated (flat-
        # extrapolated to the illiquid tail); else the CS*1.75 PLACEHOLDER (E-1 — not deployable-grade).
        if calibration_set is not None:
            mult = float(calibration_set[min(d, len(calibration_set) - 1)])
            data_derived = (cs_bps * mult) if np.isfinite(cs_bps) else 0.0
        else:
            data_derived = cs_term
        out[tkr] = float(max(cs_term, anchor, data_derived))
    return pd.Series(out, name="band_bps").sort_index()


# ----------------------------------------------------------------------------------------------------
# Net P&L composition — the deployable conservative-net series.
# ----------------------------------------------------------------------------------------------------
def apply_costs(gross_daily: pd.Series, turnover_daily: pd.Series, short_gross_daily: pd.Series,
                band_bps: float, cfg: ForkBConfig) -> pd.Series:
    """Compose the deployable net daily series:

        net = gross
              - turnover * band_bps/1e4                 (opposite-side fills already IN the band)
              - short_gross * max(borrow)/252/1e4       (borrow accrues DAILY on held short notional — DS-8)

    `band_bps` = the conservative-band spread in bps (a single deployable-grading scalar — the round-trip
    bid-ask bounce is already inside it, so turnover is charged the band once, not per leg). `max(borrow)`
    = the conservative max of the borrow ladder = `cfg.borrow_bps_floor` (>= 200 bps small-cap HTB).
    Returns the net pd.Series aligned to `gross_daily`'s index.
    """
    gross = pd.Series(gross_daily, dtype=float)
    turn = pd.Series(turnover_daily, dtype=float).reindex(gross.index).fillna(0.0)
    short_gross = pd.Series(short_gross_daily, dtype=float).reindex(gross.index).fillna(0.0)

    trade_drag = turn * (float(band_bps) / 1e4)
    borrow_rate = float(cfg.borrow_bps_floor)            # max() of the borrow ladder (conservative)
    borrow_drag = short_gross * (borrow_rate / 252.0 / 1e4)

    net = gross - trade_drag - borrow_drag
    return net.rename("net_daily")
