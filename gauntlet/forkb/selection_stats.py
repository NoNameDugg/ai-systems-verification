"""Selection-bias statistics for a many-variant sweep.

When a sweep is N >> 1 (e.g. 48 variants x 2 populations), selection-bias machinery is REQUIRED,
not optional. This module provides:
  - CPCV: combinatorial purged cross-validation paths (N=8 groups, k=2 test → C(8,2)=28 splits),
    with TRAIN-side purging of any trade whose [entry, t1] overlaps a test block ± embargo
    (H-7 leakage guard).
  - PBO: probability of backtest overfitting (Bailey-LdP) — the SOLE primary overfit gate;
    logit of the IS-best variant's OOS rank degradation across the 28 splits.
  - DSR: deflated Sharpe (SECONDARY, reported-not-gating; per-trade SR convention, NIT-5).

The metric is excess-over-random Δ; CPCV/PBO are metric-agnostic (operate on per-group Δ).
"""
from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import combinations

import numpy as np


# ---------- group assignment (shared bar-grid; strat + random map to the same N groups) ----------
def assign_groups(entry_bars, n_bars, n_groups=8):
    """Map each trade to one of N contiguous, equal-WIDTH bar-index groups over [0, n_bars).
    Shared grid ⇒ strategy + random trades use identical group spans (so Δ pools correctly)."""
    edges = np.linspace(0, n_bars, n_groups + 1)
    gid = np.clip(np.searchsorted(edges, np.asarray(entry_bars), side="right") - 1, 0, n_groups - 1)
    return gid.astype(int), edges


def cpcv_combinations(n_groups=8, k_test=2):
    """All C(N,k) test-group selections (28 for N=8,k=2)."""
    return list(combinations(range(n_groups), k_test))


# ---------- excess-over-random Δ over an arbitrary group subset, with optional train purge ----------
def _mean(x):
    return float(np.mean(x)) if len(x) else float("nan")


def delta_over_groups(strat_net, strat_gid, rand_net, rand_gid, groups,
                      *, purge_mask_strat=None, purge_mask_rand=None):
    """Δ = mean(strat net | trades in `groups`) − mean(rand net | trades in `groups`).
    Optional boolean keep-masks (purge) applied first (used for the TRAIN side)."""
    gset = np.asarray(sorted(groups))
    s_in = np.isin(strat_gid, gset)
    r_in = np.isin(rand_gid, gset)
    if purge_mask_strat is not None:
        s_in &= purge_mask_strat
    if purge_mask_rand is not None:
        r_in &= purge_mask_rand
    return _mean(strat_net[s_in]) - _mean(rand_net[r_in])


def _purge_keep(entry_bars, t1s, edges, test_groups, embargo_bars):
    """Keep-mask for TRAIN trades: drop any trade whose [entry, t1] overlaps a test group's
    bar-span ± embargo (H-7 leakage guard)."""
    entry = np.asarray(entry_bars); t1 = np.asarray(t1s)
    keep = np.ones(len(entry), dtype=bool)
    for g in test_groups:
        lo, hi = edges[g] - embargo_bars, edges[g + 1] + embargo_bars
        overlap = (entry <= hi) & (t1 >= lo)        # interval [entry,t1] intersects [lo,hi]
        keep &= ~overlap
    return keep


@dataclass
class CPCVResult:
    cpcv_mean_delta: dict        # variant_label -> mean OOS test-fold Δ over the 28 splits
    train_delta: np.ndarray      # (n_splits, n_variants)
    test_delta: np.ndarray       # (n_splits, n_variants)
    pbo: float
    pbo_lambdas: list


def run_cpcv(variants, n_bars, *, n_groups=8, k_test=2, embargo_bars=200):
    """variants: list of dicts, each with label + per-trade arrays:
       {label, strat_net, strat_entry, strat_t1, rand_net, rand_entry, rand_t1}.
    Returns CPCVResult (per-variant CPCV-mean Δ over the test-folds + the PBO over the splits)."""
    labels = [v["label"] for v in variants]
    combos = cpcv_combinations(n_groups, k_test)
    # precompute group ids + edges per variant (entries are FIXED across variants, but t1 varies
    # with h, so purge masks are per-variant per-split)
    sgid, edges = {}, None
    rgid = {}
    for v in variants:
        sg, edges = assign_groups(v["strat_entry"], n_bars, n_groups)
        rg, _ = assign_groups(v["rand_entry"], n_bars, n_groups)
        sgid[v["label"]] = sg; rgid[v["label"]] = rg

    nC, nV = len(combos), len(variants)
    train_d = np.full((nC, nV), np.nan); test_d = np.full((nC, nV), np.nan)
    for ci, test_groups in enumerate(combos):
        train_groups = [g for g in range(n_groups) if g not in test_groups]
        for vi, v in enumerate(variants):
            lab = v["label"]
            keep_s = _purge_keep(v["strat_entry"], v["strat_t1"], edges, test_groups, embargo_bars)
            keep_r = _purge_keep(v["rand_entry"], v["rand_t1"], edges, test_groups, embargo_bars)
            train_d[ci, vi] = delta_over_groups(v["strat_net"], sgid[lab], v["rand_net"], rgid[lab],
                                                train_groups, purge_mask_strat=keep_s, purge_mask_rand=keep_r)
            test_d[ci, vi] = delta_over_groups(v["strat_net"], sgid[lab], v["rand_net"], rgid[lab], test_groups)
    cpcv_mean = {labels[vi]: float(np.nanmean(test_d[:, vi])) for vi in range(nV)}
    pbo, lams = probability_of_backtest_overfitting(train_d, test_d)
    return CPCVResult(cpcv_mean, train_d, test_d, pbo, lams)


def probability_of_backtest_overfitting(train_delta, test_delta):
    """Bailey-LdP PBO: for each split, the IS(train)-best variant's OOS(test) relative rank →
    logit λ; PBO = fraction of splits with λ ≤ 0 (IS-best lands in the bottom half OOS)."""
    nC, nV = train_delta.shape
    lambdas = []
    for c in range(nC):
        tr, te = train_delta[c], test_delta[c]
        if np.all(np.isnan(tr)):
            continue
        best = int(np.nanargmax(tr))
        # ★ NaN-best guard (DEC-320): if the IS-best variant has no OOS value, the split is non-evaluable —
        # skip it (else `te[valid] < NaN` is all-False -> rank 0 -> the split is falsely counted as overfit ->
        # PBO inflated toward 1.0 -> a manufactured false NULL on sparse/empty test folds).
        valid = ~np.isnan(te)
        if np.isnan(te[best]) or valid.sum() == 0:
            continue
        # relative OOS rank of the IS-best (fraction of variants it beats), in (0,1)
        rank = (np.sum(te[valid] < te[best]) + 0.5 * np.sum(te[valid] == te[best])) / valid.sum()
        w = min(max(rank, 1.0 / (nV + 1)), nV / (nV + 1.0))
        lambdas.append(math.log(w / (1.0 - w)))
    pbo = float(np.mean([l <= 0 for l in lambdas])) if lambdas else float("nan")
    return pbo, lambdas


# ---------- DSR (secondary, reported-not-gating; per-trade SR convention NIT-5) ----------
def _norm_cdf(x):
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def _norm_ppf(p):
    # Acklam rational approximation (sufficient for a reported secondary stat)
    a = [-3.969683028665376e+01, 2.209460984245205e+02, -2.759285104469687e+02,
         1.383577518672690e+02, -3.066479806614716e+01, 2.506628277459239e+00]
    b = [-5.447609879822406e+01, 1.615858368580409e+02, -1.556989798598866e+02,
         6.680131188771972e+01, -1.328068155288572e+01]
    c = [-7.784894002430293e-03, -3.223964580411365e-01, -2.400758277161838e+00,
         -2.549732539343734e+00, 4.374664141464968e+00, 2.938163982698783e+00]
    d = [7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00, 3.754408661907416e+00]
    pl, ph = 0.02425, 1 - 0.02425
    if p < pl:
        q = math.sqrt(-2 * math.log(p))
        return (((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)
    if p <= ph:
        q = p - 0.5; r = q*q
        return (((((a[0]*r+a[1])*r+a[2])*r+a[3])*r+a[4])*r+a[5])*q / (((((b[0]*r+b[1])*r+b[2])*r+b[3])*r+b[4])*r+1)
    q = math.sqrt(-2 * math.log(1 - p))
    return -(((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)


def deflated_sharpe_ratio(returns, all_trial_sharpes):
    """DSR (Bailey-LdP) on the per-trade Sharpe of `returns`, deflated by the expected-max Sharpe
    over the trial set (using the OBSERVED cross-trial variance -> small effective-N under
    correlation). Per-trade frequency (NOT annualized). Returns
    (dsr_prob, sr_obs, effective_benchmark_sr). Secondary/reported."""
    r = np.asarray(returns, dtype=float)
    n = len(r)
    if n < 3 or r.std(ddof=1) == 0:
        return float("nan"), float("nan"), float("nan")
    sr = r.mean() / r.std(ddof=1)                                   # per-trade Sharpe
    trials = np.asarray([s for s in all_trial_sharpes if np.isfinite(s)], dtype=float)
    N = max(len(trials), 2)
    var_sr = np.var(trials, ddof=1) if len(trials) > 1 else 1.0
    emc = 0.5772156649
    z = (1 - emc) * _norm_ppf(1 - 1.0 / N) + emc * _norm_ppf(1 - 1.0 / (N * math.e))
    sr0 = math.sqrt(var_sr) * z                                     # expected-max benchmark
    g3 = float(((r - r.mean())**3).mean() / r.std()**3)             # skew
    g4 = float(((r - r.mean())**4).mean() / r.std()**4)             # kurtosis
    denom = math.sqrt(max(1e-12, 1 - g3 * sr + (g4 - 1) / 4.0 * sr**2))
    dsr = _norm_cdf((sr - sr0) * math.sqrt(n - 1) / denom)
    return float(dsr), float(sr), float(sr0)
