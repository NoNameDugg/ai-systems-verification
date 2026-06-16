"""P3 per-book crash-survival battery + stress injector + sign-check.

Pure-function leaf over the contract (_schema.ForkBConfig). NO real data; depends only on numpy/pandas
+ the frozen config. This is the per-book P3 injectors build, using single-name loci/magnitudes (not the
directional/crypto constants of a momentum book).

P3 (PINNED), the exact spec this implements:
  - Thresholds re-derived for a dollar-neutral ~5-10%-vol book (the inherited directional/crypto
    {Calmar>=0.5, maxDD>-25%, worst-ep<50%} are "false comfort"); the recalibrated set is the freeze-time
    pin carried in ForkBConfig (p3_calmar_floor / p3_maxdd_floor / p3_worst_episode_floor /
    p3_recovery_max_months / p3_sortino_floor). Override at Step-0c only.
  - Adverse locus = H1 (L/S) drift-reversal + bottom-quintile SHORT-SQUEEZE/borrow-recall over the PINNED
    windows: Aug-2007 (~2007-08-06->10) + Jan-2021 (~2021-01-25->02-01); single-name GAP 50-100%
    (cfg.p3_single_name_gap; NOT the ETF 0.20 / crypto 0.30).
  - Series = RAW net daily (§4 per-gate series: "P3 = RAW daily") AND the SHORT-leg sub-P&L.
  - The injector applies a single-name short-squeeze gap on the SHORT leg DURING each window: the shorted
    (bottom-quintile) names squeeze UP against the short -> a loss to the short leg -> reduces window net.
    Sign-check TDI proves the injector REDUCES the stress-window return (stresses, never pads).
  - Graded on the STRESSED net. ann = cfg.p3_ann (252).

Metric definitions are source-faithful ports of the apparatus's vetted carry_gate primitives
(carry_gate.equity / max_drawdown / sortino / calmar / maxdd_recovery_months / worst_episode_dd), kept local
so this leaf depends only on the contract (no sibling-leaf / cross-package import).
"""
from __future__ import annotations

import numpy as np
import pandas as pd


# ---------------------------------------------------------------------------------------------------
# equity-curve / tail primitives — source-faithful ports of carry_gate.py (V-CARRY, S2-vetted)
# ---------------------------------------------------------------------------------------------------
def _equity(daily) -> np.ndarray:
    """Equity curve with the 1.0 starting-capital prepend (so a t=0 drawdown is measured vs the initial peak)."""
    return np.concatenate([[1.0], (1.0 + np.asarray(daily, float)).cumprod()])


def _max_drawdown(daily) -> float:
    eq = _equity(daily)
    return float((eq / np.maximum.accumulate(eq) - 1.0).min())


def _sortino(daily, target: float = 0.0, ann: int = 252) -> float:
    d = np.asarray(daily, float)
    downside = np.minimum(0.0, d - target)
    dd = np.sqrt(np.mean(downside ** 2))
    return float((d.mean() - target) / dd * np.sqrt(ann)) if dd > 0 else float("inf")


def _cagr(daily, ann: int = 252) -> float:
    eq = _equity(daily)
    yrs = len(np.asarray(daily, float)) / ann
    return float(eq[-1] ** (1.0 / yrs) - 1.0) if eq[-1] > 0 and yrs > 0 else -1.0


def _calmar(daily, ann: int = 252) -> float:
    mdd = abs(_max_drawdown(daily))
    return float(_cagr(daily, ann) / mdd) if mdd > 0 else float("inf")


def _maxdd_recovery_months(daily, ann: int = 252):
    """Longest peak->recovery duration in MONTHS. RIGHT-CENSORED: an unrecovered final drawdown -> inf
    (which FAILS the <=24mo bar). Returns (months_or_inf, censored, raw_uncensored_months)."""
    eq = _equity(daily)
    peak = np.maximum.accumulate(eq)
    underwater = eq < peak * (1 - 1e-12)
    longest, cur = 0, 0
    for i in range(len(eq)):
        if underwater[i]:
            cur += 1
            longest = max(longest, cur)
        else:
            cur = 0
    censored = bool(underwater[-1])
    raw_months = float((longest / ann) * 12.0)
    return (float("inf") if censored else raw_months), censored, raw_months


def _worst_episode_dd(daily) -> float:
    """Worst single peak-to-trough drawdown depth (= max drawdown for a single curve)."""
    return _max_drawdown(daily)


# ---------------------------------------------------------------------------------------------------
# stress injector — single-name short-squeeze gap on the short leg, during the pinned windows
# ---------------------------------------------------------------------------------------------------
def _window_mask(dates: pd.DatetimeIndex, windows) -> np.ndarray:
    """Boolean mask over `dates` of days falling inside ANY pinned stress window (inclusive)."""
    dates = pd.DatetimeIndex(dates)
    m = np.zeros(len(dates), dtype=bool)
    for lo, hi in windows:
        m |= np.asarray((dates >= pd.Timestamp(lo)) & (dates <= pd.Timestamp(hi)), dtype=bool)
    return m


def inject_short_squeeze(net_daily: pd.Series, short_leg_daily: pd.Series, cfg, short_name_weight: float):
    """Inject the single-name short-squeeze on the SHORT leg during each pinned window.

    A short-squeeze means a bottom-quintile name the book is SHORT gaps UP by cfg.p3_single_name_gap
    (50-100%); a rally against a short is a LOSS to the short-leg P&L, which flows into net. We apply the
    adverse shock on the worst (most-negative) short-leg day inside each window (the borrow-recall/squeeze
    locus), one shock per window.

    ★ C-2 (S2 ratification): the shock is the single-name gap × the SQUEEZED NAME'S SHORT-LEG WEIGHT
    (`short_name_weight`) — the BOOK-level impact of ONE name gapping, NOT the whole short leg gapping. A
    75% gap on a name held at weight w costs the book ~gap·w (~1-4%), landing in the charter's ~10-20%
    stress region; subtracting the whole 75% gap (the prior bug) NULLed P3 by construction.

    Returns (stressed_net: pd.Series, stressed_short: pd.Series) aligned to net_daily's index.
    """
    net = pd.Series(net_daily, dtype=float).copy()
    short = pd.Series(short_leg_daily, dtype=float).reindex(net.index).copy()
    dates = pd.DatetimeIndex(net.index)
    gap_lo, gap_hi = cfg.p3_single_name_gap
    gap_mid = 0.5 * (float(gap_lo) + float(gap_hi))           # midpoint of the 50-100% single-name gap
    shock = gap_mid * float(short_name_weight)                # BOOK-level single-name impact = gap × weight

    s_net = net.to_numpy().copy()
    s_short = short.to_numpy(na_value=0.0).copy()
    for lo, hi in cfg.p3_stress_windows:
        win = np.asarray((dates >= pd.Timestamp(lo)) & (dates <= pd.Timestamp(hi)), dtype=bool)
        wpos = np.where(win)[0]
        if wpos.size == 0:
            continue
        # squeeze locus = the worst (most-negative) short-leg day in-window (where the short is already hurt)
        i = wpos[int(np.argmin(s_short[wpos]))]
        s_short[i] -= shock      # the shorted names rally -> the short leg loses `shock`
        s_net[i] -= shock        # that loss flows straight into net (the short leg is part of net)
    return (pd.Series(s_net, index=net.index), pd.Series(s_short, index=net.index))


# ---------------------------------------------------------------------------------------------------
# the battery
# ---------------------------------------------------------------------------------------------------
def _battery_metrics(daily, *, ann: int) -> dict:
    """Compute the 5 P3 metrics on one return series. worst_episode is reported as a POSITIVE depth."""
    d = np.asarray(daily, float)
    calmar = _calmar(d, ann=ann)
    maxdd = _max_drawdown(d)
    worst = abs(_worst_episode_dd(d))
    months, censored, _raw = _maxdd_recovery_months(d, ann=ann)
    sortino = _sortino(d, ann=ann)
    return {"calmar": float(calmar), "maxdd": float(maxdd), "worst_episode": float(worst),
            "recovery_months": float(months), "recovery_censored": bool(censored),
            "sortino": float(sortino)}


def _grade(m: dict, cfg) -> dict:
    """Apply the §4 P3 thresholds (graded on the stressed-net metric dict `m`)."""
    calmar_ok = m["calmar"] >= cfg.p3_calmar_floor
    maxdd_ok = m["maxdd"] > cfg.p3_maxdd_floor
    worst_ok = m["worst_episode"] < cfg.p3_worst_episode_floor
    sortino_ok = m["sortino"] > cfg.p3_sortino_floor
    recovery_ok = m["recovery_months"] <= cfg.p3_recovery_max_months
    passed = bool(calmar_ok and maxdd_ok and worst_ok and sortino_ok and recovery_ok)
    return {"calmar_ok": bool(calmar_ok), "maxdd_ok": bool(maxdd_ok),
            "worst_episode_ok": bool(worst_ok), "sortino_ok": bool(sortino_ok),
            "recovery_ok": bool(recovery_ok), "passed": passed}


def p3_battery(net_daily: pd.Series, short_leg_daily: pd.Series, cfg, short_name_weight: float | None = None) -> dict:
    """§4 P3 per-book crash-survival battery + stress injector + sign-check.

    Args:
      net_daily       : pd.Series indexed by date -> RAW net daily return (the deployable raw-exposure book).
      short_leg_daily : pd.Series indexed by date -> the short-leg sub-P&L daily return (the squeeze hits here).
      cfg             : ForkBConfig (frozen P3 pins: p3_calmar_floor, p3_maxdd_floor, p3_worst_episode_floor,
                        p3_recovery_max_months, p3_sortino_floor, p3_ann, p3_stress_windows, p3_single_name_gap).

    Returns a dict with the UNSTRESSED metrics (calmar/maxdd/worst_episode/sortino/recovery_months),
    their STRESSED variants (stressed_*), the short-leg battery (short_*), the grade booleans, and
    `passed` (ALL thresholds, graded on the STRESSED net). ann = cfg.p3_ann.
    """
    ann = int(cfg.p3_ann)
    swt = cfg.p3_short_name_weight_default if short_name_weight is None else float(short_name_weight)
    net = pd.Series(net_daily, dtype=float)
    short = pd.Series(short_leg_daily, dtype=float).reindex(net.index)

    # ---- unstressed (RAW net) ----
    un = _battery_metrics(net, ann=ann)

    # ---- stressed (single-name short-squeeze injected on the short leg during the pinned windows) ----
    stressed_net, stressed_short = inject_short_squeeze(net, short, cfg, swt)
    st = _battery_metrics(stressed_net, ann=ann)

    # ---- short-leg sub-P&L battery (the squeeze hits the short directly) — graded too ----
    short_un = _battery_metrics(short.fillna(0.0), ann=ann)
    short_st = _battery_metrics(stressed_short, ann=ann)

    # the verdict is graded on the STRESSED net
    grade = _grade(st, cfg)
    short_grade = _grade(short_st, cfg)

    out = {
        # unstressed net battery
        "calmar": un["calmar"], "maxdd": un["maxdd"], "worst_episode": un["worst_episode"],
        "sortino": un["sortino"], "recovery_months": un["recovery_months"],
        "recovery_censored": un["recovery_censored"],
        # stressed net battery (the graded basis)
        "stressed_calmar": st["calmar"], "stressed_maxdd": st["maxdd"],
        "stressed_worst_episode": st["worst_episode"], "stressed_sortino": st["sortino"],
        "stressed_recovery_months": st["recovery_months"],
        "stressed_recovery_censored": st["recovery_censored"],
        # short-leg sub-P&L (unstressed + stressed) + its own grade
        "short_calmar": short_un["calmar"], "short_maxdd": short_un["maxdd"],
        "short_worst_episode": short_un["worst_episode"], "short_sortino": short_un["sortino"],
        "short_recovery_months": short_un["recovery_months"],
        "short_stressed_calmar": short_st["calmar"], "short_stressed_maxdd": short_st["maxdd"],
        "short_stressed_worst_episode": short_st["worst_episode"],
        "short_stressed_sortino": short_st["sortino"],
        "short_stressed_recovery_months": short_st["recovery_months"],
        "short_passed": short_grade["passed"],
        # grade booleans (on stressed net) + the verdict
        "calmar_ok": grade["calmar_ok"], "maxdd_ok": grade["maxdd_ok"],
        "worst_episode_ok": grade["worst_episode_ok"], "sortino_ok": grade["sortino_ok"],
        "recovery_ok": grade["recovery_ok"],
        "passed": grade["passed"],
        "ann": ann,
    }
    return out
