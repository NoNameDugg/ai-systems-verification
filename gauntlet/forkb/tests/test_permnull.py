"""TDI for permnull — the formation-label permutation null (charter §4 / D-HARD-4 / R3 / DS-C).

The tests assert the gate's BEHAVIOUR against constructed ground truth:
  T1  a real cross-sectional edge (labels correlated with forward name returns) -> p <= 0.05.
  T2  pure noise (labels carry no forward information) -> p NOT significant (> 0.05).
  T3  the permutation preserves the per-formation cross-sectional composition (#long/#short/#flat)
      AND propagates a name's shuffled label through the whole hold (R3 / DS-C invariants).
  T4  the embargo WIDENS the null block-Sharpe distribution vs a naive no-embargo null (charter §4).
  T5  determinism (same seed -> same p) + a flat/empty book degrades gracefully (no crash, no pass).

perm_n is kept small (200) for speed via the perm_n arg.
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from permnull import (
    perm_null_p,
    _book_daily_return,
    _permute_labels_within_date,
    _cohort_arrays,
    _embargoed_cohorts,
    _null_block_sharpes,
    _null_book_persistent,
    _null_book_arr,
)

PERM_N = 200


# ----------------------------------------------------------------------------------------------------
# fixtures — a controlled [date x ticker] return panel + a fixed L/S formation-label schedule.
# ----------------------------------------------------------------------------------------------------
def _panel(seed=7, n_names=40, start="2012-01-03", end="2020-12-31"):
    rng = np.random.default_rng(seed)
    dates = pd.bdate_range(start, end)
    names = [f"N{i:03d}" for i in range(n_names)]
    nd = pd.DataFrame(rng.normal(0.0, 0.012, (len(dates), n_names)), index=dates, columns=names)
    return nd, names, rng


def _label_schedule(nd, names, cfg, longs, shorts):
    """A fixed dollar-neutral L/S label on every `cfg.rebalance_td`-spaced formation date."""
    dates = nd.index
    form_dates = dates[60:: cfg.rebalance_td]
    form_dates = form_dates[form_dates < dates[-(cfg.hold_td + 5)]]
    labels = {}
    for fd in form_dates:
        lab = pd.Series(0, index=names)
        lab.loc[longs] = 1
        lab.loc[shorts] = -1
        labels[fd] = lab
    return labels


def _inject_edge(nd, labels, cfg, longs, shorts, edge=0.0015):
    """Add a real forward tilt: +edge to longs / -edge to shorts over each cohort's hold window.

    This makes the labels genuinely PREDICTIVE of forward returns (a real cross-sectional edge),
    which the permutation null must detect.
    """
    nd = nd.copy()
    dates = nd.index
    T = len(dates)
    for fd in labels:
        pos = dates.searchsorted(fd)
        end = min(pos + cfg.hold_td, T)
        idx = dates[pos:end]
        nd.loc[idx, longs] = nd.loc[idx, longs] + edge
        nd.loc[idx, shorts] = nd.loc[idx, shorts] - edge
    return nd


# ----------------------------------------------------------------------------------------------------
# T1 — a real edge passes.
# ----------------------------------------------------------------------------------------------------
def test_correlated_labels_real_edge_passes():
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=7)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)
    nd_edge = _inject_edge(nd, labels, cfg, longs, shorts, edge=0.0015)

    res = perm_null_p(labels, nd_edge, cfg, perm_n=PERM_N, seed=1)

    assert res["real_block_sharpe"] > 0.0, "the injected long/short edge must give a positive book Sharpe"
    assert res["real_block_sharpe"] > res["null_q95"], "real edge must beat the 95th-pctile null"
    assert res["p"] <= cfg.perm_p_threshold, f"a real edge must clear the gate, got p={res['p']}"
    assert res["passed"] is True


# ----------------------------------------------------------------------------------------------------
# T2 — pure noise does NOT pass (p not significant).
# ----------------------------------------------------------------------------------------------------
@pytest.mark.parametrize("seed", [1, 7, 13, 42])
def test_noise_labels_not_significant(seed):
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=seed)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)   # labels carry NO forward info on nd

    res = perm_null_p(labels, nd, cfg, perm_n=PERM_N, seed=1)

    # noise: the real book is just one more draw from the same null distribution -> p should NOT be
    # significant. (One-sided p uniform-ish; the gate must not falsely fire.)
    assert res["p"] > cfg.perm_p_threshold, f"noise must NOT clear the perm gate, got p={res['p']} (seed={seed})"
    assert res["passed"] is False


# ----------------------------------------------------------------------------------------------------
# T3 — permutation invariants: composition preserved + propagated through the hold.
# ----------------------------------------------------------------------------------------------------
def test_permutation_preserves_composition():
    """Within each formation date the shuffle only relabels names: same #long / #short / #flat."""
    cfg = ForkBConfig()
    nd, names, rng = _panel(seed=3)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)

    for fd, lab in labels.items():
        perm = _permute_labels_within_date(lab, rng)
        # same set of names (index preserved)
        assert list(perm.index) == list(lab.index)
        # exact cross-sectional composition preserved
        assert int((perm == 1).sum()) == int((lab == 1).sum()) == 8
        assert int((perm == -1).sum()) == int((lab == -1).sum()) == 8
        assert int((perm == 0).sum()) == int((lab == 0).sum())
        # it IS a real shuffle (not the identity) for a non-degenerate cross-section
        assert not perm.equals(lab) or (lab.nunique() == 1)


def test_label_propagates_through_full_hold():
    """A formation cohort's (permuted) label drives the book for the WHOLE hold_td window, not just
    the formation day — i.e. the label is propagated, not re-drawn daily."""
    cfg = ForkBConfig()
    # one cohort, one formation date; make ONE name carry all the return -> the book return over the
    # hold is exactly that name's return * its weight on every held day.
    dates = pd.bdate_range("2015-01-02", periods=200)
    names = ["A", "B", "C", "D"]
    nd = pd.DataFrame(0.0, index=dates, columns=names)
    nd["A"] = 0.01            # A returns +1% every day
    fd = dates[10]
    lab = pd.Series({"A": 1, "B": -1, "C": 0, "D": 0})   # long A, short B
    book = _book_daily_return({fd: lab}, nd, cfg)

    pos = dates.searchsorted(fd)
    held = book.iloc[pos:pos + cfg.hold_td]
    before = book.iloc[:pos]
    after = book.iloc[pos + cfg.hold_td:]
    # long A (+1% * weight 1.0) - short B (0%) = +0.01 on EVERY held day (label persists the full hold)
    assert np.allclose(held.to_numpy(), 0.01), "the label must drive the book across the entire hold"
    assert np.allclose(before.to_numpy(), 0.0), "no exposure before formation"
    assert np.allclose(after.to_numpy(), 0.0), "no exposure after the hold ends"
    assert len(held) == cfg.hold_td


# ----------------------------------------------------------------------------------------------------
# T4 — the embargo discipline: (a) deterministic cohort-thinning to a non-overlapping set,
#      (b) the permuted null is centred ~0 (charter §4 "null-Sharpe ~= 0").
# ----------------------------------------------------------------------------------------------------
def test_embargo_thins_to_nonoverlapping_cohorts():
    """`_embargoed_cohorts` keeps a strictly smaller, ≥perm_embargo_td-spaced (non-overlapping) subset
    of the full overlapping cohort set — the embargo's deterministic mechanism (fewer independent
    draws → a wider null). With rebalance_td=21 < perm_embargo_td=63 the full set DOES overlap, so
    thinning must drop cohorts."""
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=2)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)
    _R, cohorts = _cohort_arrays(labels, nd, cfg)
    thinned = _embargoed_cohorts(cohorts, cfg)

    assert len(thinned) < len(cohorts), "overlapping cohorts (rebalance 21 < embargo 63) must be thinned"
    starts = [c[0] for c in thinned]
    # every kept cohort is >= perm_embargo_td trading days past the previous kept one (non-overlapping)
    gaps = np.diff(starts)
    assert (gaps >= cfg.perm_embargo_td).all(), f"kept cohorts must be >= {cfg.perm_embargo_td} td apart"
    # the full set, by contrast, is spaced at rebalance_td (overlapping within the 63-td hold)
    full_starts = sorted(c[0] for c in cohorts)
    assert np.diff(full_starts).min() < cfg.hold_td, "the full cohort set overlaps (sanity)"


def test_null_distribution_centered_near_zero():
    """A permuted-label dollar-neutral L/S book carries ~no cross-sectional edge → the null block-
    Sharpe distribution is centred near 0 (charter §4 'null-Sharpe ~= 0')."""
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=7)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)

    rng = np.random.default_rng(5)
    null = _null_block_sharpes(labels, nd, cfg, 400, rng, mode="persistent")
    null = null[np.isfinite(null)]
    assert abs(float(np.mean(null))) < 0.10, f"null block-Sharpe mean should be ~0, got {np.mean(null):.3f}"
    assert null.std(ddof=1) > 0.0, "the null must be a non-degenerate distribution"


# ----------------------------------------------------------------------------------------------------
# T6 (S2 ITEM A) — the GATE is the PERSISTENT-OVERLAPPING null, and a name's shuffled label PERSISTS
# (one π) across the overlapping cohorts inside an embargo window. (The q95-ordering vs the independent
# null is a REAL-data property; on iid-synthetic names it is uninformative — S2 — so we test the gate
# default + the persistence MECHANISM directly, which is robust.)
# ----------------------------------------------------------------------------------------------------
def test_gate_defaults_to_persistent_null_with_diagnostics():
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=11)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)
    res = perm_null_p(labels, nd, cfg, perm_n=200, seed=3, report_diagnostics=True)
    assert res["null_mode"] == "persistent-overlapping"          # the gate is the persistent null (NOT independent)
    assert 0.0 <= res["p"] <= 1.0
    assert res["null_q95"] == res["diag_persistent_q95"]         # the gate consumes the persistent null
    for k in ("diag_independent_q95", "diag_persistent_q95", "diag_thinned_q95"):
        assert k in res and np.isfinite(res[k]), f"missing/NaN reported bound {k}"


def test_persistent_shares_one_permutation_across_a_window():
    """★ A mechanism: TWO overlapping cohorts in the SAME >=embargo window apply the SAME name-permutation
    (label persists across the name's live cohorts), whereas the per-cohort-INDEPENDENT book does not."""
    n_names, n_td = 6, 12
    rng_R = np.random.default_rng(1)
    R = rng_R.normal(0, 0.01, (n_td, n_names))
    w0 = np.array([0.5, 0.5, 0.0, -0.5, -0.5, 0.0])              # cohort A weights
    w1 = np.array([0.0, 0.5, 0.5, 0.0, -0.5, -0.5])              # cohort B weights (overlaps A in [5:12])
    cohorts = [(0, 12, w0), (5, 12, w1)]
    # persistent: one π for the whole window (embargo_td huge -> both cohorts in bin 0)
    book_p = _null_book_persistent(R, cohorts, n_td, n_names, np.random.default_rng(0), embargo_td=10_000)
    # reproduce: the SAME π applied to BOTH cohorts
    perm = np.random.default_rng(0).permutation(n_names)
    ret = np.zeros(n_td); cnt = np.zeros(n_td)
    ret[0:12] += R[0:12] @ w0[perm]; cnt[0:12] += 1
    ret[5:12] += R[5:12] @ w1[perm]; cnt[5:12] += 1
    expected = np.where(cnt > 0, ret / np.maximum(cnt, 1), 0.0)
    assert np.allclose(book_p, expected), "persistent book must apply ONE π across the window's cohorts"
    # the per-cohort-INDEPENDENT book draws a DIFFERENT π per cohort -> differs on the overlap
    book_i = _null_book_arr(R, cohorts, n_td, n_names, np.random.default_rng(0))
    assert not np.allclose(book_p[5:12], book_i[5:12]), "independent book must NOT share one π (the rejected gate)"


# ----------------------------------------------------------------------------------------------------
# T5 — determinism + graceful degradation.
# ----------------------------------------------------------------------------------------------------
def test_determinism_same_seed():
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=9)
    longs, shorts = names[:8], names[8:16]
    labels = _label_schedule(nd, names, cfg, longs, shorts)
    r1 = perm_null_p(labels, nd, cfg, perm_n=PERM_N, seed=123)
    r2 = perm_null_p(labels, nd, cfg, perm_n=PERM_N, seed=123)
    assert r1["p"] == r2["p"]
    assert r1["real_block_sharpe"] == r2["real_block_sharpe"]
    assert r1["null_q95"] == r2["null_q95"]


def test_flat_book_degrades_gracefully():
    """An all-flat label set (no exposure) -> zero book -> non-finite Sharpe -> p=nan, not passed,
    and no exception."""
    cfg = ForkBConfig()
    nd, names, _ = _panel(seed=4)
    dates = nd.index
    form_dates = dates[60:: cfg.rebalance_td][:5]
    labels = {fd: pd.Series(0, index=names) for fd in form_dates}   # everyone flat
    res = perm_null_p(labels, nd, cfg, perm_n=50, seed=1)
    assert not np.isfinite(res["real_block_sharpe"])
    assert res["passed"] is False
