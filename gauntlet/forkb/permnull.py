"""Leaf — formation-label permutation null.

The cross-sectional analogue of a time-series shuffle: instead of permuting *time*, we permute the
**formation-date cross-sectional label assignment** (which names are long / short / flat on each
formation date), then **propagate each name's shuffled label deterministically through its `hold_td`
hold** — so a permuted label persists across the cohort's whole hold, exactly matching how the real
book holds a label. This tests cross-sectional *predictability* while leaving the factor covariance
structure of `name_daily` untouched (the null is run on RAW net).

Faithfulness pins (from `ForkBConfig`, NOT invented here):
  - `hold_td`         (63)   : each formation date's labels are held this many trading days.
  - `perm_embargo_td` (63)   : `>= hold horizon`. The embargo discipline in the GATE null = each
                               cohort is shuffled INDEPENDENTLY and the shuffle propagated through its
                               whole hold, so NO shuffled label leaks across the overlapping cohorts
                               (the leakage a no-embargo shuffle would carry). A separate cohort-THINNED
                               embargo set (one non-overlapping cohort per embargo window) — fewer
                               independent draws — is the wider-null demonstration (`_embargoed_cohorts`
                               / `_null_block_sharpes(embargo=True)`); the gate itself does NOT thin
                               (it mirrors the real book's full overlapping cohort set, apples-to-apples).
  - `perm_n`          (1000) : permutation iterations (tests override small for speed).
  - `perm_p_threshold`(0.05) : one-sided pass line — real block-Sharpe must beat the 95th pctile of
                               the embargoed null  (==>  p == fraction(null >= real) <= 0.05).
  - block unit               : 63-td (`cfg.block_td`) non-overlapping blocks; block return =
                               compounded daily within block; block-Sharpe = mean/std over blocks
                               (the SAME time-series-independent unit M2-t / PSR / OOS consume).

Permutation invariants (asserted by tests):
  - WITHIN each formation date the permutation only relabels names → the per-formation cross-sectional
    composition (#long / #short / #flat) is preserved exactly.
  - the shuffled label is propagated through the full `hold_td` hold (it is not re-drawn daily) in the
    embargoed gate null.

Implementation note: the null is built on the SAME formation-date set as the real book (same #cohorts,
same overlap-averaging, same block grid) so the only thing that differs is WHICH names carry which
label — an apples-to-apples reference. The hot path is fully vectorised over numpy.
"""
from __future__ import annotations

import numpy as np
import pandas as pd


# ----------------------------------------------------------------------------------------------------
# Book construction — dollar-neutral long/short daily return from formation labels.
# ----------------------------------------------------------------------------------------------------
def _cohort_daily_weights(label: pd.Series) -> pd.Series:
    """Dollar-neutral weights for ONE formation cohort: long names sum to +1, shorts to -1.

    label : Series ticker -> {+1 long, -1 short, 0 flat}. Returns a Series ticker -> weight over the
    NON-flat names only. A degenerate one-sided cohort (only longs or only shorts) is normalised on the
    side present and left un-hedged on the empty side (weight 0).
    """
    lab = label[label != 0]
    if lab.empty:
        return pd.Series(dtype=float)
    w = pd.Series(0.0, index=lab.index)
    longs = lab[lab > 0].index
    shorts = lab[lab < 0].index
    if len(longs):
        w.loc[longs] = 1.0 / len(longs)
    if len(shorts):
        w.loc[shorts] = -1.0 / len(shorts)
    return w


def _cohort_arrays(formation_labels: dict, name_daily: pd.DataFrame, cfg):
    """Pre-compute the per-cohort (start, end, full-width weight vector) tuples + the returns matrix.

    Returns (R, cohorts) where:
      R       : np.ndarray [n_td x n_names] daily returns (NaN -> 0).
      cohorts : list of (start, end, w_full) with w_full an [n_names] dollar-neutral weight vector
                (zeros on flat names). Cohorts whose labels are all-flat / off-grid are dropped.
    """
    cols = list(name_daily.columns)
    col_pos = {c: i for i, c in enumerate(cols)}
    n_names = len(cols)
    R = np.nan_to_num(name_daily.to_numpy(dtype=float), nan=0.0)
    dates = name_daily.index
    n_td = len(dates)

    cohorts = []
    for f_date, label in formation_labels.items():
        pos = int(dates.searchsorted(pd.Timestamp(f_date)))
        if pos >= n_td:
            continue
        w = _cohort_daily_weights(label)
        if w.empty:
            continue
        w_full = np.zeros(n_names)
        any_in = False
        for tkr, wt in w.items():
            j = col_pos.get(tkr)
            if j is not None:
                w_full[j] = wt
                any_in = True
        if not any_in:
            continue
        cohorts.append((pos, min(pos + cfg.hold_td, n_td), w_full))
    return R, cohorts


def _book_from_cohorts(R: np.ndarray, cohorts: list, n_td: int) -> np.ndarray:
    """Daily dollar-neutral L/S book: each cohort held its window, OVERLAPPING cohorts averaged
    (equal-weight across the cohorts live on each day). Returns an [n_td] daily-return array."""
    ret_sum = np.zeros(n_td)
    live_cnt = np.zeros(n_td)
    for start, end, w_full in cohorts:
        ret_sum[start:end] += R[start:end] @ w_full
        live_cnt[start:end] += 1.0
    live = live_cnt > 0
    book = np.zeros(n_td)
    book[live] = ret_sum[live] / live_cnt[live]
    return book


def _book_daily_return(formation_labels: dict, name_daily: pd.DataFrame, cfg) -> pd.Series:
    """Daily dollar-neutral L/S book return (Series, indexed by name_daily.index). Thin wrapper over
    the vectorised core — kept for readability + direct TDI of the book construction."""
    R, cohorts = _cohort_arrays(formation_labels, name_daily, cfg)
    book = _book_from_cohorts(R, cohorts, len(name_daily.index))
    return pd.Series(book, index=name_daily.index)


def _block_sharpe_arr(r: np.ndarray, cfg) -> tuple[float, int]:
    """Aggregate a daily return ARRAY into non-overlapping `cfg.block_td` blocks (block return =
    compounded daily within block; trailing partial block dropped — RT-C) and return
    (block_sharpe = mean/std, n_blocks). nan if <2 blocks or zero-variance."""
    r = np.nan_to_num(np.asarray(r, dtype=float), nan=0.0)
    bt = int(cfg.block_td)
    n_full = len(r) // bt
    if n_full < 2:
        return float("nan"), n_full
    blocks = r[: n_full * bt].reshape(n_full, bt)
    block_ret = np.prod(1.0 + blocks, axis=1) - 1.0
    sd = block_ret.std(ddof=1)
    if not np.isfinite(sd) or sd == 0.0:
        return float("nan"), n_full
    return float(block_ret.mean() / sd), n_full


def _block_sharpe(daily: pd.Series, cfg) -> tuple[float, int]:
    """Series-facing wrapper for _block_sharpe_arr (used by the book-construction TDI)."""
    return _block_sharpe_arr(daily.to_numpy(dtype=float), cfg)


# ----------------------------------------------------------------------------------------------------
# Permutation — within-formation-date cross-sectional relabel, with embargoed propagation.
# ----------------------------------------------------------------------------------------------------
def _permute_labels_within_date(label: pd.Series, rng: np.random.Generator) -> pd.Series:
    """Shuffle the label VALUES across the names of one formation date (preserves the per-date
    cross-sectional composition exactly: same #long / #short / #flat) — R3 / DS-C invariant."""
    vals = label.to_numpy().copy()
    rng.shuffle(vals)
    return pd.Series(vals, index=label.index)


def _embargo_window_id(formation_labels: dict, name_daily: pd.DataFrame, cfg) -> dict:
    """Map each formation date -> an embargo-window id: dates are grouped into consecutive windows of
    width `cfg.perm_embargo_td` trading days (the propagation unit). Diagnostic / TDI helper."""
    dates = name_daily.index
    ordered = sorted(formation_labels.keys(), key=pd.Timestamp)
    win = {}
    anchor_pos = None
    wid = -1
    for f_date in ordered:
        pos = int(dates.searchsorted(pd.Timestamp(f_date)))
        if anchor_pos is None or (pos - anchor_pos) >= cfg.perm_embargo_td:
            wid += 1
            anchor_pos = pos
        win[f_date] = wid
    return win


def _embargoed_cohorts(cohorts: list, cfg) -> list:
    """Thin a (time-ordered) cohort list to a non-overlapping, ≥`cfg.perm_embargo_td`-spaced subset —
    the embargo set. Walk in start-position order; keep a cohort only if its start is ≥ embargo trading
    days past the last kept start. Used to demonstrate the embargo's effect (fewer independent draws →
    a WIDER null) vs the naive all-cohort null. The GATE itself does NOT thin (the null must mirror the
    real book's full overlapping cohort set — an apples-to-apples reference); the embargo enters the
    gate as the INDEPENDENT-per-cohort shuffle (no shuffle is shared across the overlap → no leakage)."""
    ordered = sorted(cohorts, key=lambda c: c[0])
    kept = []
    last_start = None
    for start, end, w_full in ordered:
        if last_start is None or (start - last_start) >= cfg.perm_embargo_td:
            kept.append((start, end, w_full))
            last_start = start
    return kept


def _one_null_book(formation_labels: dict, name_daily: pd.DataFrame, cfg,
                   rng: np.random.Generator, embargo: bool = True, window_id: dict | None = None):
    """Build ONE permuted-label null book daily Series (readability / TDI wrapper).

    embargo=True  : the embargo cohort set (non-overlapping; one independent shuffle per cohort).
    embargo=False : ALL (overlapping) cohorts — the naive no-embargo null.
    """
    R, cohorts = _cohort_arrays(formation_labels, name_daily, cfg)
    if embargo:
        cohorts = _embargoed_cohorts(cohorts, cfg)
    n_td = len(name_daily.index)
    n_names = R.shape[1]
    book = _null_book_arr(R, cohorts, n_td, n_names, rng)
    return pd.Series(book, index=name_daily.index)


def _null_book_arr(R, cohorts, n_td, n_names, rng):
    """Vectorised one-permutation null book. A cohort's weight vector is re-indexed by a random,
    INDEPENDENT name permutation (preserves composition: the multiset of weights is conserved, only
    WHICH names carry them changes; the shuffle is propagated across the cohort's whole hold — R3).
    Overlapping cohorts are averaged exactly as the real book is."""
    ret_sum = np.zeros(n_td)
    live_cnt = np.zeros(n_td)
    for start, end, w_full in cohorts:
        perm = rng.permutation(n_names)            # independent per cohort (the embargo discipline)
        ret_sum[start:end] += R[start:end] @ w_full[perm]   # propagated across the whole hold
        live_cnt[start:end] += 1.0
    live = live_cnt > 0
    book = np.zeros(n_td)
    book[live] = ret_sum[live] / live_cnt[live]
    return book


def _null_book_persistent(R, cohorts, n_td, n_names, rng, embargo_td):
    """★ A (S2 ratification): the PERSISTENT-OVERLAPPING null book — the GATE null.

    ONE name-bijection π per >=embargo_td formation window; every cohort whose formation falls in that window
    applies the SAME π to its weight vector → a name's shuffled label PERSISTS across the ~3 overlapping
    cohorts it is simultaneously live in, reproducing the real book's sticky-SUE cross-cohort persistence
    (hence its lag-1 block-autocorrelation, RT-B). Composition is preserved per cohort (π is a bijection of
    the weight vector). Windows are >=embargo_td apart → independent π across windows: the embargo is the
    per-name label-LOCK duration (>= the hold), NOT cohort-thinning. (The as-built per-cohort-independent
    shuffle was rejected as anti-conservative — it destroys the cross-cohort ρ the real book carries.)"""
    ret_sum = np.zeros(n_td); live_cnt = np.zeros(n_td)
    bins: dict = {}
    for (start, end, w_full) in cohorts:
        bins.setdefault(start // max(1, int(embargo_td)), []).append((start, end, w_full))
    for _b, clist in bins.items():
        perm = rng.permutation(n_names)                       # ONE π per window → persists across its cohorts
        for (start, end, w_full) in clist:
            ret_sum[start:end] += R[start:end] @ w_full[perm]
            live_cnt[start:end] += 1.0
    live = live_cnt > 0
    book = np.zeros(n_td)
    book[live] = ret_sum[live] / live_cnt[live]
    return book


def _null_block_sharpes(formation_labels: dict, name_daily: pd.DataFrame, cfg,
                        n_perm: int, rng: np.random.Generator, mode: str = "persistent") -> np.ndarray:
    """The `n_perm` null block-Sharpes under one of three shuffle schemes (charter §4 / R3 / S2 A):
      mode='persistent' (THE GATE): full overlapping cohort set, ONE π per >=embargo window so a name's label
          persists across its overlapping cohorts → reproduces the real lag-1 block-ρ → correctly sized.
      mode='independent' (reported, anti-conservative LOWER bound; NOT the gate): full set, independent
          per-cohort shuffles → null ρ≈0 → understates the null width.
      mode='thinned' (reported, over-conservative widening demo): the thinned non-overlapping cohort set."""
    R, cohorts = _cohort_arrays(formation_labels, name_daily, cfg)
    n_td = len(name_daily.index); n_names = R.shape[1]
    emb = int(getattr(cfg, "perm_embargo_td", cfg.hold_td))
    if mode == "thinned":
        cohorts = _embargoed_cohorts(cohorts, cfg)
    out = np.empty(n_perm)
    for i in range(n_perm):
        if mode == "persistent":
            book = _null_book_persistent(R, cohorts, n_td, n_names, rng, emb)
        else:                                                 # 'independent' or 'thinned'
            book = _null_book_arr(R, cohorts, n_td, n_names, rng)
        s, _nb = _block_sharpe_arr(book, cfg)
        out[i] = s
    return out


def perm_null_p(formation_labels: dict, name_daily: pd.DataFrame, cfg,
                perm_n: int | None = None, seed: int = 0, report_diagnostics: bool = False) -> dict:
    """Formation-label permutation null (charter §4 / D-HARD-4 / R3 / DS-C).

    The GATE: build the real dollar-neutral L/S book block-Sharpe, then a null distribution where the
    cross-sectional labels are permuted WITHIN each formation date (composition preserved), the shuffle
    propagated through the cohort's hold, and — the embargo discipline — drawn INDEPENDENTLY per cohort
    so no shuffled label leaks across the overlapping cohorts (`cfg.perm_embargo_td ≥ hold`). The null
    is built on the SAME (full, overlapping) cohort set as the real book → an apples-to-apples
    reference. p = fraction(null block-Sharpe ≥ real); pass iff p ≤ `cfg.perm_p_threshold`.

    Parameters
    ----------
    formation_labels : dict[Timestamp -> pd.Series(ticker -> {+1 long, -1 short, 0 flat})]
        The pre-registered H1 cross-sectional label assignment on each formation date.
    name_daily : pd.DataFrame  [date x ticker]
        Daily TOTAL returns per name (the RAW series — N4; factor covariance preserved).
    cfg : ForkBConfig
        Frozen params: `hold_td`, `block_td`, `perm_embargo_td`, `perm_n`, `perm_p_threshold`.
    perm_n : int, optional
        Override `cfg.perm_n` (tests pass a small value, e.g. 200, for speed). Default = `cfg.perm_n`.
    seed : int
        RNG seed (deterministic null).

    Returns
    -------
    dict with keys:
        p                : one-sided p = fraction of null block-Sharpes >= the real block-Sharpe.
        real_block_sharpe: the real dollar-neutral L/S book's 63-td block-Sharpe (mean/std).
        null_q95         : 95th percentile of the null block-Sharpe distribution.
        n                : number of null block-Sharpes that were finite/usable.
        passed           : bool, p <= cfg.perm_p_threshold.
        n_blocks         : number of non-overlapping blocks in the real series (diagnostic).
        null_std         : std of the (finite) null block-Sharpes (dispersion diagnostic).
        n_formations     : number of formation dates in the book.
    """
    n_perm = int(cfg.perm_n if perm_n is None else perm_n)
    rng = np.random.default_rng(seed)

    R, cohorts = _cohort_arrays(formation_labels, name_daily, cfg)
    n_td = len(name_daily.index)

    # --- real book -------------------------------------------------------------------------------
    real_daily = _book_from_cohorts(R, cohorts, n_td)
    real_block_sharpe, n_blocks = _block_sharpe_arr(real_daily, cfg)

    # --- GATE null: the PERSISTENT-OVERLAPPING null (S2 A) — full overlapping set; label persists across cohorts -
    null_arr = _null_block_sharpes(formation_labels, name_daily, cfg, n_perm, rng, mode="persistent")
    finite = null_arr[np.isfinite(null_arr)]
    n_finite = int(finite.size)

    if not np.isfinite(real_block_sharpe) or n_finite == 0:
        p = float("nan")
        null_q95 = float("nan")
        null_std = float("nan")
    else:
        # one-sided: fraction of null >= real (>= so an exact tie counts toward the null — conservative).
        p = float(np.mean(finite >= real_block_sharpe))
        null_q95 = float(np.percentile(finite, 95))
        null_std = float(finite.std(ddof=1)) if n_finite > 1 else float("nan")

    passed = bool(np.isfinite(p) and p <= cfg.perm_p_threshold)

    result = {
        "p": p,
        "real_block_sharpe": real_block_sharpe,
        "null_q95": null_q95,                                 # the persistent-overlapping (gate) null
        "n": n_finite,
        "passed": passed,
        "n_blocks": int(n_blocks),
        "null_std": null_std,
        "n_formations": len(formation_labels),
        "null_mode": "persistent-overlapping",
    }
    if report_diagnostics:                                    # reported bounds (NOT the gate): indep + thinned q95
        nd = max(50, n_perm // 4)
        for m, sd in (("independent", seed + 1), ("thinned", seed + 2)):
            arr = _null_block_sharpes(formation_labels, name_daily, cfg, nd, np.random.default_rng(sd), mode=m)
            fa = arr[np.isfinite(arr)]
            result[f"diag_{m}_q95"] = float(np.percentile(fa, 95)) if fa.size else float("nan")
        result["diag_persistent_q95"] = null_q95
    return result
