"""TDI for p3.py — §4 P3 per-book crash-survival battery + stress injector + sign-check (DS-3).

Charter TDI points (charter §4 P3 / §6 net-new "per-book P3 injectors" + the "P3 sign-check" v3.8-I assert):
  (1) calmar / maxdd / sortino computed correctly on a hand-built series (exact arithmetic).
  (2) ★ sign-check: the stress injector REDUCES the stress-window net return -> stressed worst_episode >=
      unstressed worst_episode (it STRESSES, never pads); stressed net <= unstressed net inside the window.
  (3) a benign series PASSES; a crash-fragile (deep-DD) series FAILS.
  (4) the SHORT-leg sub-P&L is graded too (the single-name squeeze hits the short leg).

Faithfulness: thresholds + windows + gap come from ForkBConfig (frozen pins); the test does NOT invent any.
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from p3 import (p3_battery, inject_short_squeeze, _calmar, _max_drawdown,
                _sortino, _maxdd_recovery_months)


CFG = ForkBConfig()  # default frozen P3 pins


def _series(values, start="2007-07-02"):
    """Business-day-indexed Series of `values`, starting `start` (default lands a window of 2007 days)."""
    idx = pd.bdate_range(start, periods=len(values))
    return pd.Series(np.asarray(values, float), index=idx)


# ---------------------------------------------------------------------------------------------------
# (1) exact-arithmetic metric correctness on a hand-built series
# ---------------------------------------------------------------------------------------------------
def test_metrics_exact_on_handbuilt_series():
    d = np.array([0.01, -0.02, 0.01, -0.01])
    # equity (1.0 prepend): 1, 1.01, 0.9898, 0.999698, 0.98970102
    eq = np.concatenate([[1.0], (1.0 + d).cumprod()])
    exp_maxdd = float((eq / np.maximum.accumulate(eq) - 1.0).min())
    downside = np.minimum(0.0, d)
    dd = np.sqrt(np.mean(downside ** 2))
    exp_sortino = float(d.mean() / dd * np.sqrt(252))
    yrs = len(d) / 252
    exp_cagr = eq[-1] ** (1.0 / yrs) - 1.0
    exp_calmar = float(exp_cagr / abs(exp_maxdd))

    assert _max_drawdown(d) == pytest.approx(exp_maxdd, rel=1e-12)
    assert _sortino(d, ann=252) == pytest.approx(exp_sortino, rel=1e-12)
    assert _calmar(d, ann=252) == pytest.approx(exp_calmar, rel=1e-12)
    # the public dict reports the SAME unstressed numbers
    out = p3_battery(_series(d), _series(np.zeros(4)), CFG)
    assert out["maxdd"] == pytest.approx(exp_maxdd, rel=1e-12)
    assert out["sortino"] == pytest.approx(exp_sortino, rel=1e-12)
    assert out["calmar"] == pytest.approx(exp_calmar, rel=1e-12)
    assert out["worst_episode"] == pytest.approx(abs(exp_maxdd), rel=1e-12)
    assert out["ann"] == 252


def test_recovery_months_right_censored_to_inf():
    # a monotone-decline series never recovers -> RIGHT-CENSORED -> inf (FAILS the <=24mo bar)
    d = np.array([-0.01, -0.01, -0.01, -0.01, -0.01])
    months, censored, raw = _maxdd_recovery_months(d, ann=252)
    assert censored is True
    assert months == float("inf")
    # a series that fully recovers by the end is NOT censored -> finite months
    d2 = np.array([-0.05, 0.10, 0.0])  # dips then recovers above the prior peak
    months2, censored2, _ = _maxdd_recovery_months(d2, ann=252)
    assert censored2 is False
    assert np.isfinite(months2)


# ---------------------------------------------------------------------------------------------------
# (2) ★ sign-check: the injector STRESSES (reduces window net), it does not pad
# ---------------------------------------------------------------------------------------------------
def test_sign_check_stress_reduces_window_net():
    # build a long series spanning the Aug-2007 window with a benign baseline so the only DD is the injected one
    idx = pd.bdate_range("2007-07-02", "2007-09-28")  # straddles 2007-08-06..10
    net = pd.Series(np.full(len(idx), 0.0005), index=idx)        # mild positive drift
    short = pd.Series(np.full(len(idx), -0.0002), index=idx)     # short leg slight bleed (some negative days)

    stressed_net, stressed_short = inject_short_squeeze(net, short, CFG, 1.0)   # w=1.0 -> shock = gap midpoint (raw mechanic)
    win = np.asarray((idx >= pd.Timestamp("2007-08-06")) & (idx <= pd.Timestamp("2007-08-10")), dtype=bool)

    # inside the window the stressed net is STRICTLY below the unstressed net somewhere (a squeeze loss landed)
    assert (stressed_net.to_numpy()[win] < net.to_numpy()[win] - 1e-9).any()
    # OUTSIDE the window nothing is touched (identity)
    assert np.allclose(stressed_net.to_numpy()[~win], net.to_numpy()[~win])
    # the shock magnitude = the gap-band midpoint
    gap_lo, gap_hi = CFG.p3_single_name_gap
    shock = 0.5 * (gap_lo + gap_hi)
    delta = (net.to_numpy()[win] - stressed_net.to_numpy()[win]).max()
    assert delta == pytest.approx(shock, rel=1e-9)

    # ★ the battery: stressed worst_episode >= unstressed worst_episode (deeper DD, never shallower)
    out = p3_battery(net, short, CFG)
    assert out["stressed_worst_episode"] >= out["worst_episode"] - 1e-12
    assert out["stressed_maxdd"] <= out["maxdd"] + 1e-12      # maxdd is signed-negative -> more negative
    # ★ C-2 (S2): a REALISTIC single-name squeeze (gap × ~10% short-leg weight ≈ 7.5% book impact) is SURVIVABLE —
    # the book is NOT NULLed by construction (the prior bug subtracted the whole 75% gap -> ~-75% maxdd -> always FAIL).
    assert out["stressed_worst_episode"] > out["worst_episode"]      # the injector still bites (deeper than unstressed)
    assert out["stressed_maxdd"] > -0.30                             # sane book-scale stress, NOT a -75% whole-leg gap
    # but a fully-concentrated single name (weight 1.0) DOES blow the floor -> FAIL (the injector still has teeth)
    assert p3_battery(net, short, CFG, short_name_weight=1.0)["passed"] is False


def test_stress_is_noop_when_no_dates_in_windows():
    # a series entirely outside the pinned windows -> the injector is a pure identity
    idx = pd.bdate_range("2015-03-02", periods=30)  # no Aug-2007 / Jan-2021 day
    net = _series(np.full(30, 0.001))
    net.index = idx
    short = _series(np.full(30, -0.0003))
    short.index = idx
    stressed_net, stressed_short = inject_short_squeeze(net, short, CFG, 1.0)
    assert np.allclose(stressed_net.to_numpy(), net.to_numpy())
    assert np.allclose(stressed_short.to_numpy(), short.to_numpy())
    out = p3_battery(net, short, CFG)
    assert out["stressed_worst_episode"] == pytest.approx(out["worst_episode"], rel=1e-12)


# ---------------------------------------------------------------------------------------------------
# (3) a benign series passes; a crash-fragile one fails
# ---------------------------------------------------------------------------------------------------
def test_benign_passes_fragile_fails():
    rng = np.random.default_rng(7)
    # benign book: steady positive drift, tiny vol, no window dates -> no stress -> should PASS every floor.
    # strong-enough drift (relative to vol) that the curve ends at a new high (not right-censored underwater).
    idx = pd.bdate_range("2015-01-02", periods=600)  # well clear of both pinned windows
    benign = pd.Series(0.0010 + rng.normal(0, 0.0006, len(idx)), index=idx)
    short_b = pd.Series(0.0005 + rng.normal(0, 0.0006, len(idx)), index=idx)
    out_b = p3_battery(benign, short_b, CFG)
    assert out_b["recovery_censored"] is False      # ends at a new high -> recovery is finite
    assert out_b["passed"] is True
    assert out_b["calmar"] >= CFG.p3_calmar_floor
    assert out_b["maxdd"] > CFG.p3_maxdd_floor
    assert out_b["worst_episode"] < CFG.p3_worst_episode_floor
    assert out_b["sortino"] > CFG.p3_sortino_floor

    # crash-fragile book: a deep persistent drawdown that blows the maxdd / worst-episode / calmar floors
    fragile = np.array(benign.to_numpy(), dtype=float)   # writable copy
    fragile[200:230] = -0.02   # a sustained -40%-ish crash leg
    fragile = pd.Series(fragile, index=idx)
    out_f = p3_battery(fragile, short_b, CFG)
    assert out_f["passed"] is False
    # at least one of the deep-DD floors must be the cause
    assert (not out_f["maxdd_ok"]) or (not out_f["worst_episode_ok"]) or (not out_f["calmar_ok"])


# ---------------------------------------------------------------------------------------------------
# (4) the SHORT-leg sub-P&L is graded too (the squeeze hits the short)
# ---------------------------------------------------------------------------------------------------
def test_short_leg_graded_and_squeezed():
    # a benign short leg that would pass on its own, but the single-name squeeze inside the window wrecks it
    idx = pd.bdate_range("2021-01-04", "2021-03-01")  # straddles 2021-01-25..02-01
    net = pd.Series(np.full(len(idx), 0.0003), index=idx)
    short = pd.Series(np.full(len(idx), 0.0002), index=idx)   # benign positive short-leg P&L

    _, stressed_short = inject_short_squeeze(net, short, CFG, 1.0)
    win = np.asarray((idx >= pd.Timestamp("2021-01-25")) & (idx <= pd.Timestamp("2021-02-01")), dtype=bool)
    # ★ the squeeze lands on the SHORT leg inside the window (it is the short that gets hit)
    assert (stressed_short.to_numpy()[win] < short.to_numpy()[win] - 1e-9).any()
    assert np.allclose(stressed_short.to_numpy()[~win], short.to_numpy()[~win])

    out = p3_battery(net, short, CFG)
    # the short-leg battery is reported AND graded
    for key in ("short_calmar", "short_maxdd", "short_worst_episode", "short_sortino",
                "short_stressed_worst_episode", "short_passed"):
        assert key in out
    # the stressed short leg has a deeper episode than the unstressed short leg (the squeeze hit it)
    assert out["short_stressed_worst_episode"] > out["short_worst_episode"]
    # a 75% single-name squeeze on a benign short leg blows its worst-episode floor -> short fails
    assert out["short_passed"] is False


def test_short_leg_reindexed_to_net_index():
    # short leg given on a SUBSET of the net index -> reindexed (no crash, NaNs treated as 0 in the battery)
    idx = pd.bdate_range("2007-07-02", periods=60)
    net = pd.Series(np.full(60, 0.0005), index=idx)
    short = pd.Series(np.full(40, -0.0003), index=idx[:40])  # short missing the tail
    out = p3_battery(net, short, CFG)
    assert "short_passed" in out
    assert np.isfinite(out["short_maxdd"])
