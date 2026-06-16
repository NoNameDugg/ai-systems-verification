"""TDI for cost.py (charter §5; DS-2/6/7/8) — asserts against KNOWN injected spreads + spec invariants.

Test map (>=4 meaningful):
  T1  corwin_schultz_bps recovers a KNOWN injected proportional spread (constant) within tolerance.
  T1b CS recovers a SECOND, larger injected spread (monotone — bigger spread -> bigger CS estimate).
  T2  conservative_band_bps >= max(CS*1.75, external-anchor[decile]) elementwise (anchor = mandatory floor).
  T2b the external anchor is a HARD floor: when CS=0 the band still >= the per-name decade anchor.
  T3  apply_costs: net < gross wherever turnover > 0 (and net == gross where turnover/short are 0).
  T3b apply_costs: borrow reduces net on short_gross > 0 (the borrow line-item bites).
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from synth_fixtures import small
from cost import corwin_schultz_bps, conservative_band_bps, apply_costs

CFG = ForkBConfig()


# ---------------------------------------------------------------------------------------------------
# Helper: build a tiny synthetic high/low SEP where a KNOWN proportional spread s is embedded into the
# observed high/low per the Corwin-Schultz model (observed High contains the ask, observed Low the bid):
#   observed_high = true_high * (1 + s/2),  observed_low = true_low * (1 - s/2)
# with a random-walk "true" mid so the high-low VOLATILITY component (beta vs gamma) lets CS separate
# the spread from variance — exactly the construction CS is designed to invert.
# ---------------------------------------------------------------------------------------------------
def test_external_anchor_decile_direction_least_liquid_most_expensive():
    """T2c (★ direction lock): with CS ≈ 0 so the anchor binds, the LEAST-liquid (smallest marketcap) name must get
    a HIGHER conservative band than the MOST-liquid name (decile 0 = most expensive). Guards the anchor-table order
    (a reversed table charges microcaps the cheapest anchor — masked by CS dominating on rich synthetic spreads)."""
    sep = _synthetic_hl_sep(spread=0.0001, n_days=300, n_names=10)        # near-zero spread -> CS≈0 -> anchor binds
    names = sorted(sep["ticker"].unique())
    dates = sep["date"].unique()
    daily = pd.concat([pd.DataFrame({"ticker": t, "date": dates, "marketcap": (i + 1) * 1e9,
                                     "pb": 2.0, "pe": 15.0, "ev": (i + 1) * 1e9})
                       for i, t in enumerate(names)], ignore_index=True)    # names[0] smallest .. names[-1] largest
    band = conservative_band_bps(sep, daily, CFG)
    assert band[names[0]] > band[names[-1]], (
        f"least-liquid {names[0]} band {band[names[0]]:.1f} must exceed most-liquid {names[-1]} band {band[names[-1]]:.1f}")
    assert band[names[0]] >= max(CFG.cost_external_anchor_bps) - 1e-9      # illiquid tail floored at the most-expensive anchor


def _synthetic_hl_sep(spread: float, n_days: int = 400, n_names: int = 3, vol: float = 0.015, seed: int = 7):
    rng = np.random.default_rng(seed)
    dates = pd.bdate_range("2015-01-02", periods=n_days)
    rows = []
    for i in range(n_names):
        tkr = f"HL{i:02d}"
        mid = 50.0 * np.cumprod(1.0 + rng.normal(0, vol, n_days))
        # intraday true range around the mid (the variance component CS must NOT mistake for spread)
        rng_pct = np.abs(rng.normal(0, vol, n_days))
        true_high = mid * (1.0 + rng_pct)
        true_low = mid * (1.0 - rng_pct)
        obs_high = true_high * (1.0 + spread / 2.0)   # ask embedded in the high
        obs_low = true_low * (1.0 - spread / 2.0)     # bid embedded in the low
        for k in range(n_days):
            rows.append({"ticker": tkr, "date": dates[k], "open": mid[k], "high": obs_high[k],
                         "low": obs_low[k], "close": mid[k], "closeadj": mid[k], "closeunadj": mid[k],
                         "dividends": 0.0, "volume": 1e5})
    return pd.DataFrame(rows)


# ---------------------------------------------------------------------------------------------------
# T1 — CS recovers a KNOWN injected spread.
# ---------------------------------------------------------------------------------------------------
def test_cs_recovers_known_injected_spread():
    s_true = 0.01                                   # 1% = 100 bps proportional spread
    sep = _synthetic_hl_sep(spread=s_true)
    cs = corwin_schultz_bps(sep, CFG)
    assert cs.notna().all(), "CS produced NaN on a clean synthetic series"
    est = cs.mean()                                  # bps
    true_bps = s_true * 1e4                           # 100 bps
    # CS is noisy per-pair but unbiased in the mean; allow a generous (but real) tolerance band.
    assert true_bps * 0.5 <= est <= true_bps * 1.6, f"CS {est:.1f}bps did not recover ~{true_bps:.0f}bps"


def test_cs_is_monotone_in_injected_spread():
    sep_small = _synthetic_hl_sep(spread=0.005, seed=11)
    sep_big = _synthetic_hl_sep(spread=0.02, seed=11)
    cs_small = corwin_schultz_bps(sep_small, CFG).mean()
    cs_big = corwin_schultz_bps(sep_big, CFG).mean()
    assert cs_big > cs_small, f"CS not monotone: 2% spread -> {cs_big:.1f} !> 0.5% spread -> {cs_small:.1f}"
    # and the bigger one should land near its ~200 bps truth
    assert cs_big >= 200.0 * 0.5


# ---------------------------------------------------------------------------------------------------
# T2 — conservative band >= max(CS*1.75, external-anchor) elementwise; anchor is a hard floor.
# ---------------------------------------------------------------------------------------------------
def test_band_dominates_cs_uplift_and_anchor():
    snap, _ = small()
    cs = corwin_schultz_bps(snap.sep, CFG)
    band = conservative_band_bps(snap.sep, snap.daily, CFG)
    # band must be defined for every name CS is defined for
    common = cs.dropna().index.intersection(band.index)
    assert len(common) > 0
    cs_uplift = cs.loc[common] * CFG.cost_uplift_floor_mult
    # band >= CS*1.75 everywhere (the uplift term is a lower bound)
    assert (band.loc[common] >= cs_uplift - 1e-9).all(), "band fell below CS*1.75 for some name"
    # band >= the minimum anchor (every name is anchored to SOME decade value, the mandatory floor)
    min_anchor = min(CFG.cost_external_anchor_bps)
    assert (band.loc[common] >= min_anchor - 1e-9).all(), "band fell below the external anchor floor"


def test_external_anchor_is_hard_floor_when_cs_zero():
    # a name with high==low (zero range -> CS ~ 0) must STILL be floored to its decade anchor
    dates = pd.bdate_range("2015-01-02", periods=120)
    rows = []
    for i in range(5):
        tkr = f"FLAT{i:02d}"
        px = 20.0 + i
        for d in dates:
            rows.append({"ticker": tkr, "date": d, "open": px, "high": px, "low": px, "close": px,
                         "closeadj": px, "closeunadj": px, "dividends": 0.0, "volume": 1e5})
    sep = pd.DataFrame(rows)
    daily = pd.DataFrame([{"ticker": f"FLAT{i:02d}", "date": dates[0],
                           "marketcap": (i + 1) * 1e8, "pb": 1.0, "pe": 10.0, "ev": (i + 1) * 1e8}
                          for i in range(5) for _ in [0]])
    band = conservative_band_bps(sep, daily, CFG)
    min_anchor = min(CFG.cost_external_anchor_bps)
    # CS ~ 0 here, so the band is driven ENTIRELY by the anchor floor -> >= the smallest anchor, and > 0
    assert (band > 0).all()
    assert (band >= min_anchor - 1e-9).all()


# ---------------------------------------------------------------------------------------------------
# T3 — apply_costs: turnover reduces net; borrow reduces net on shorts.
# ---------------------------------------------------------------------------------------------------
def test_apply_costs_turnover_reduces_net():
    idx = pd.bdate_range("2020-01-01", periods=10)
    gross = pd.Series(0.001, index=idx)
    turnover = pd.Series([0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0], index=idx)
    short_gross = pd.Series(0.0, index=idx)
    band_bps = 100.0
    net = apply_costs(gross, turnover, short_gross, band_bps, CFG)
    # where turnover > 0, net strictly < gross
    pos = turnover > 0
    assert (net[pos] < gross[pos] - 1e-12).all(), "net not reduced where turnover>0"
    # where turnover == 0 (and no shorts), net == gross
    zero = (turnover == 0) & (short_gross == 0)
    assert np.allclose(net[zero].to_numpy(), gross[zero].to_numpy()), "net changed with zero turnover/short"
    # exact arithmetic check on one bar: drag = turnover * band/1e4
    k = idx[4]  # turnover 1.0
    assert net[k] == pytest.approx(gross[k] - 1.0 * band_bps / 1e4)


def test_apply_costs_borrow_reduces_net_on_shorts():
    idx = pd.bdate_range("2020-01-01", periods=6)
    gross = pd.Series(0.001, index=idx)
    turnover = pd.Series(0.0, index=idx)                       # isolate the borrow term
    short_gross = pd.Series([0.0, 0.0, 1.0, 1.0, 0.0, 2.0], index=idx)
    net = apply_costs(gross, turnover, short_gross, band_bps=50.0, cfg=CFG)
    pos = short_gross > 0
    assert (net[pos] < gross[pos] - 1e-15).all(), "net not reduced where short_gross>0"
    zero = short_gross == 0
    assert np.allclose(net[zero].to_numpy(), gross[zero].to_numpy())
    # exact: borrow drag = short_gross * borrow_bps_floor/252/1e4
    k = idx[2]  # short_gross 1.0
    expected = gross[k] - 1.0 * CFG.borrow_bps_floor / 252.0 / 1e4
    assert net[k] == pytest.approx(expected)
    # heavier short notional -> heavier drag
    assert (gross[idx[5]] - net[idx[5]]) > (gross[idx[2]] - net[idx[2]])
