"""Tail/crash gate + the randomized-SORT null / eff-N stats for a carry book.

Pure, testable gate primitives:
  - Crash-survival (NON-NEGOTIABLE) on the realized AND scenario-STRESSED daily equity curves:
    Calmar >= 0.5, maxDD-recovery <= 24mo (RIGHT-CENSORED -> inf if unrecovered), worst
    single-episode peak-to-trough < 50%, Sortino > 0. A frozen stress triple is applied to each
    pinned crash episode: (1) 30% adverse gap on the episode's worst day (one basket leg = the
    2015 CHF de-peg magnitude -> k-weight x 30%), (2) financing x5, (3) slippage 5 pips/leg/
    rebalance. Also the wipeout-leverage (per-side leverage at which the worst stressed episode
    = 100% loss). Skew is REPORTED, never a floor.
  - Randomized-SORT null (shuffle the carry sort; SAME spell timing/financing/costs) +
    eff-N-aware significance: the null + PSR resample on the SPELL unit (~24), NOT the thousands
    of overlapping daily returns. + leave-one-currency-out + USD-beta guard + carry/price/cost
    decomposition.

Offline; consumes a pre-built panel.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from selection_stats import _norm_cdf  # reuse the vetted normal-CDF

ANN = 252

# Pinned crash episodes (NIT-h): GFC / CHF de-peg / COVID / JPY-unwind
CRASH_WINDOWS = [("2008-09-01", "2009-06-30"), ("2015-01-01", "2015-02-28"),
                 ("2020-02-15", "2020-04-30"), ("2024-07-01", "2024-08-31")]
STRESS_GAP = 0.30          # (1) adverse gap magnitude (2015 CHF de-peg)
STRESS_FIN_MULT = 5.0      # (2) financing spike ×5
K_WEIGHT = 0.25            # equal-weight per leg (k=4)


# ---------- equity-curve / tail primitives ----------
def equity(daily):
    # prepend the 1.0 starting capital so a drawdown from t=0 is measured against the initial peak
    return np.concatenate([[1.0], (1.0 + np.asarray(daily, float)).cumprod()])


def max_drawdown(daily):
    eq = equity(daily)
    return float((eq / np.maximum.accumulate(eq) - 1.0).min())


def sortino(daily, target=0.0, ann=ANN):
    d = np.asarray(daily, float)
    downside = np.minimum(0.0, d - target)
    dd = np.sqrt(np.mean(downside ** 2))
    return float((d.mean() - target) / dd * np.sqrt(ann)) if dd > 0 else float("inf")


def cagr(daily, ann=ANN):
    eq = equity(daily)
    yrs = len(daily) / ann
    return float(eq[-1] ** (1.0 / yrs) - 1.0) if eq[-1] > 0 else -1.0


def calmar(daily, ann=ANN):
    mdd = abs(max_drawdown(daily))
    return float(cagr(daily, ann) / mdd) if mdd > 0 else float("inf")


def maxdd_recovery_months(daily, ann=ANN):
    """Longest peak->recovery duration in MONTHS. RIGHT-CENSORED: if the final drawdown never
    recovers by series end, return ∞ (FAILS the ≤24mo bar — N3)."""
    eq = equity(daily)
    peak = np.maximum.accumulate(eq)
    underwater = eq < peak * (1 - 1e-12)
    longest, cur, censored = 0, 0, False
    for i in range(len(eq)):
        if underwater[i]:
            cur += 1
            longest = max(longest, cur)
        else:
            cur = 0
    if underwater[-1]:
        censored = True
    months = (longest / ann) * 12.0
    return float("inf") if censored else months, censored, float((longest / ann) * 12.0)


def worst_episode_dd(daily):
    """Worst single peak-to-trough drawdown depth (= max drawdown for a single curve)."""
    return max_drawdown(daily)


def psr_spell(spell_returns, sr_star=0.0):
    """Probabilistic Sharpe Ratio on the SPELL-return series (eff-N unit, S2 NIT): P(true SR > 0).
    n = #spells; skew/kurt of the spell returns (independent unit, not autocorrelated daily)."""
    r = np.asarray(spell_returns, float)
    n = len(r)
    if n < 3 or r.std(ddof=1) == 0:
        return float("nan"), float("nan")
    sr = r.mean() / r.std(ddof=1)
    g3 = float(((r - r.mean()) ** 3).mean() / r.std() ** 3)
    g4 = float(((r - r.mean()) ** 4).mean() / r.std() ** 4)
    denom = np.sqrt(max(1e-12, 1 - g3 * sr + (g4 - 1) / 4.0 * sr ** 2))
    return float(_norm_cdf((sr - sr_star) * np.sqrt(n - 1) / denom)), float(sr)


# ---------- scenario stress (the frozen triple) ----------
def crash_day_mask(dates):
    m = pd.Series(False, index=dates)
    for lo, hi in CRASH_WINDOWS:
        m |= (dates >= pd.Timestamp(lo)) & (dates <= pd.Timestamp(hi))
    return m.values


def apply_stress(dates, ret_total, ret_swap, ret_price, rebalance_mask, slippage_on_rebal):
    """Return the STRESSED daily series. (2) financing ×5 on crash days; (3) slippage on
    rebalance days; (1) a k-weight×30% adverse gap on each crash episode's WORST realized day."""
    dates = pd.DatetimeIndex(dates)
    rt = np.asarray(ret_total, float).copy()
    sw = np.asarray(ret_swap, float)
    cmask = crash_day_mask(dates)
    rt[cmask] += (STRESS_FIN_MULT - 1.0) * sw[cmask]            # (2) ×5 financing
    rt[np.asarray(rebalance_mask, bool)] -= np.asarray(slippage_on_rebal, float)[np.asarray(rebalance_mask, bool)]  # (3)
    for lo, hi in CRASH_WINDOWS:                                # (1) worst-day adverse gap, per episode
        win = (dates >= pd.Timestamp(lo)) & (dates <= pd.Timestamp(hi))
        if win.any():
            idx = np.where(win)[0]
            worst = idx[np.argmin(np.asarray(ret_total, float)[idx])]
            rt[worst] -= K_WEIGHT * STRESS_GAP                 # one leg gaps 30%
    return rt


def wipeout_leverage(daily_stressed):
    """Per-side leverage L at which the worst stressed peak-to-trough episode = 100% loss.
    L ≈ 1 / |worst-episode drawdown depth|."""
    wdd = abs(worst_episode_dd(daily_stressed))
    return float(1.0 / wdd) if wdd > 0 else float("inf")


# ---------- randomized-SORT null on the spell unit ----------
class SpellSums:
    """Per-spell × per-pair sums of price-return and the long/short net financing (already /365
    -> daily fractions). Lets any long/short partition's spell return be evaluated by dot product
    — so the null resamples the SORT at the SPELL unit (eff-N), cheaply."""
    def __init__(self, panel, sets_daily, pairs, ccys, usd_pair, k):
        self.pairs, self.ccys, self.usd_pair, self.k = pairs, ccys, usd_pair, k
        spell_ids = sets_daily["spell_id"].values
        self.spells = np.unique(spell_ids)
        self.total_days = len(sets_daily)
        ret = panel.pivot(index="date", columns="pair", values="ret").reindex(columns=pairs)
        ln = panel.pivot(index="date", columns="pair", values="long_net").reindex(columns=pairs)
        sn = panel.pivot(index="date", columns="pair", values="short_net").reindex(columns=pairs)
        ret = ret.reindex(sets_daily.index).fillna(0.0)
        ln = ln.reindex(sets_daily.index).fillna(0.0); sn = sn.reindex(sets_daily.index).fillna(0.0)
        self.sum_ret, self.sum_ln, self.sum_sn, self.ndays = {}, {}, {}, {}
        for sid in self.spells:
            d = (spell_ids == sid)
            self.sum_ret[sid] = ret.values[d].sum(0)
            self.sum_ln[sid] = ln.values[d].sum(0) / 100.0 / 365.0
            self.sum_sn[sid] = sn.values[d].sum(0) / 100.0 / 365.0
            self.ndays[sid] = int(d.sum())

    def _pos_vec(self, longset, shortset):
        target = {c: (1.0 / self.k if c in longset else (-1.0 / self.k if c in shortset else 0.0)) for c in self.ccys}
        pos = np.zeros(len(self.pairs))
        for c in self.ccys:
            if c == "USD":
                continue
            pair, sgn = self.usd_pair[c]
            pos[self.pairs.index(pair)] = sgn * target[c]
        return pos

    def spell_return(self, sid, longset, shortset):
        pos = self._pos_vec(longset, shortset)
        price = float(pos @ self.sum_ret[sid])
        netrate = np.where(pos > 0, self.sum_ln[sid], self.sum_sn[sid])
        swap = float(np.abs(pos) @ netrate)
        return price + swap

    def mean_daily(self, partitions):
        """partitions: dict sid -> (longset, shortset). Mean daily net return over the corpus."""
        tot = sum(self.spell_return(sid, *partitions[sid]) for sid in self.spells)
        return tot / self.total_days


def randomized_sort_null(spell_sums, real_partitions, n_draws=2000, seed=7, drop=None):
    """Null = random 4/4 (k=4) partitions per spell (same timing/financing/costs); excess =
    real − mean(null) annualized; p = P(null ≥ real). `drop` excludes a currency (LOCO)."""
    rng = np.random.default_rng(seed)
    ccys = [c for c in spell_sums.ccys if c != drop]
    kk = len(ccys) // 2
    real = spell_sums.mean_daily(real_partitions) * ANN
    nulls = np.empty(n_draws)
    for i in range(n_draws):
        parts = {}
        for sid in spell_sums.spells:
            perm = list(rng.permutation(ccys))
            parts[sid] = (frozenset(perm[:kk]), frozenset(perm[-kk:]))
        nulls[i] = spell_sums.mean_daily(parts) * ANN
    excess = real - float(nulls.mean())
    p = float((nulls >= real).mean())
    return {"real_ann": real, "null_mean_ann": float(nulls.mean()), "excess_ann": excess,
            "p_value": p, "n_draws": n_draws}


def usd_beta(basket_daily, usd_daily):
    """β of the basket return to the trade-weighted USD return (D-SOFT-3; |β| ≤ 0.20)."""
    a = np.asarray(basket_daily, float); b = np.asarray(usd_daily, float)
    m = np.isfinite(a) & np.isfinite(b)
    if m.sum() < 10 or np.var(b[m], ddof=1) == 0:
        return float("nan")
    return float(np.cov(a[m], b[m])[0, 1] / np.var(b[m], ddof=1))  # consistent ddof=1
