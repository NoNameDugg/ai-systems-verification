"""TDI for blocks.py — the non-overlapping 63-td block unit + the conditional 1-lag block-NW (RT-B).

Asserts against KNOWN ground truth (constructed series), per charter §4-M2 / RT-B / RT-C:
  T1 n_blocks  : non-overlapping cfg.block_td blocks, trailing partial DROPPED → n ≈ len(daily)/63.
  T2 compound  : block return = prod(1+r)-1 (geometric within-block) — checked on one exact block.
  T3 anchor    : the grid is anchored at the FIRST day (RT-C) — block 0 = days[0:63].
  T4 partial   : drop_trailing_partial_block flips whether the ragged tail block is kept.
  T5 plain-t   : near-iid block series → nw_applied False, t = mean/(std/sqrt(n)) exactly.
  T6 NW (RT-B) : a POSITIVELY-autocorrelated AR(1) (ρ≈0.4) → ρ measured > 0.2 → nw_applied True
                 AND |t_NW| < |t_plain| (the anti-conservative inflation is corrected).
  T7 NW-neg    : a NEGATIVELY-autocorrelated series also trips the |ρ|>0.2 gate (NW fires).
  T8 degenerate: n<2 / zero-variance → NaN t, nw_applied False (no crash).
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from blocks import to_blocks, block_t, _lag1_autocorr

CFG = ForkBConfig()
BT = CFG.block_td   # 63


def _daily_series(vals):
    idx = pd.bdate_range("2010-01-04", periods=len(vals))
    return pd.Series(np.asarray(vals, dtype=float), index=idx)


# --------------------------------------------------------------------------------------------------
# T1 — n_blocks ≈ len(daily)/63 with the trailing partial dropped (RT-C).
# --------------------------------------------------------------------------------------------------
def test_n_blocks_drops_trailing_partial():
    n_days = 5 * BT + 17                       # 5 full blocks + a 17-day ragged tail
    rng = np.random.default_rng(1)
    s = _daily_series(rng.normal(0, 0.01, n_days))
    blk = to_blocks(s, CFG)
    assert blk.shape == (5,)                   # exactly floor(n/63); the 17-day tail is dropped
    assert blk.ndim == 1
    # sanity: n_blocks ≈ len(daily)/63 (floor)
    assert blk.size == n_days // BT


# --------------------------------------------------------------------------------------------------
# T2 — block return = compounded daily within-block (one exact block).
# --------------------------------------------------------------------------------------------------
def test_block_return_is_compounded():
    rng = np.random.default_rng(2)
    daily = rng.normal(0.0, 0.01, 2 * BT)      # exactly 2 full blocks
    s = _daily_series(daily)
    blk = to_blocks(s, CFG)
    assert blk.shape == (2,)
    # block 0 is the geometric compound of days [0:63]
    expect0 = np.prod(1.0 + daily[0:BT]) - 1.0
    expect1 = np.prod(1.0 + daily[BT:2 * BT]) - 1.0
    assert blk[0] == pytest.approx(expect0, rel=1e-12, abs=1e-12)
    assert blk[1] == pytest.approx(expect1, rel=1e-12, abs=1e-12)
    # compounded != summed for nonzero returns
    assert not np.isclose(blk[0], daily[0:BT].sum())


# --------------------------------------------------------------------------------------------------
# T3 — anchored at the first day (RT-C): block 0 uses days[0:63], not a tail-anchored grid.
# --------------------------------------------------------------------------------------------------
def test_anchored_at_first_day():
    # ramp so block-0 vs a hypothetical tail-anchored block-0 differ.
    daily = np.full(3 * BT + 10, 0.0)
    daily[0:BT] = 0.001                         # only the FIRST 63 days carry return
    s = _daily_series(daily)
    blk = to_blocks(s, CFG)                     # 3 full blocks (tail of 10 dropped)
    assert blk.size == 3
    assert blk[0] == pytest.approx(np.prod(1.0 + np.full(BT, 0.001)) - 1.0, rel=1e-12)
    assert blk[1] == pytest.approx(0.0, abs=1e-15)   # days [63:126] are all zero → first-day anchor


# --------------------------------------------------------------------------------------------------
# T4 — drop_trailing_partial_block flag actually controls the ragged-tail block.
# --------------------------------------------------------------------------------------------------
def test_drop_trailing_partial_flag():
    import dataclasses
    n_days = 3 * BT + 20
    rng = np.random.default_rng(4)
    s = _daily_series(rng.normal(0, 0.01, n_days))

    cfg_drop = CFG                               # default True
    cfg_keep = dataclasses.replace(CFG, drop_trailing_partial_block=False)

    assert to_blocks(s, cfg_drop).size == 3      # tail dropped
    blk_keep = to_blocks(s, cfg_keep)
    assert blk_keep.size == 4                     # ragged 20-day tail kept as a partial block
    # the kept partial block == compound of the last 20 days
    expect_tail = np.prod(1.0 + s.values[3 * BT:]) - 1.0
    assert blk_keep[-1] == pytest.approx(expect_tail, rel=1e-12)


# --------------------------------------------------------------------------------------------------
# T5 — near-iid block series → plain t, nw_applied False, exact plain-t formula.
# --------------------------------------------------------------------------------------------------
def test_block_t_plain_iid():
    rng = np.random.default_rng(5)
    blocks = rng.normal(0.02, 0.05, 104)        # ~104 blocks, iid → |ρ| small
    out = block_t(blocks, CFG)
    assert out["nw_applied"] is False
    assert abs(out["rho"]) <= CFG.block_rho_threshold
    assert out["n"] == 104
    # plain t = mean / (std_ddof1 / sqrt(n))
    n = blocks.size
    expect_t = blocks.mean() / (blocks.std(ddof=1) / np.sqrt(n))
    assert out["t"] == pytest.approx(expect_t, rel=1e-12)


# --------------------------------------------------------------------------------------------------
# T6 — POSITIVELY-autocorrelated AR(1) (ρ≈0.4): ρ>0.2 → NW fires AND |t_NW| < |t_plain| (RT-B).
# --------------------------------------------------------------------------------------------------
def _ar1(n, phi, mu, sigma, seed):
    rng = np.random.default_rng(seed)
    e = rng.normal(0, sigma, n)
    x = np.empty(n)
    x[0] = mu + e[0]
    for i in range(1, n):
        x[i] = mu + phi * (x[i - 1] - mu) + e[i]
    return x


def test_block_t_nw_fires_on_positive_ar1():
    x = _ar1(n=400, phi=0.4, mu=0.02, sigma=0.04, seed=6)   # long → ρ_hat ≈ 0.4
    out = block_t(x, CFG)
    # ρ measured > the 0.2 threshold
    assert out["rho"] > 0.2
    assert out["nw_applied"] is True
    # the NW |t| is SMALLER than the naive plain |t| (positive autocorr was inflating it)
    n = x.size
    t_plain = x.mean() / (x.std(ddof=1) / np.sqrt(n))
    assert abs(out["t"]) < abs(t_plain)
    # cross-check the exact NW arithmetic (Bartlett L=1: Var_NW(mean) = (γ0 + γ1)/n)
    dx = x - x.mean()
    g0 = np.dot(dx, dx) / n
    g1 = np.dot(dx[1:], dx[:-1]) / n
    t_nw_expect = x.mean() / np.sqrt((g0 + g1) / n)
    assert out["t"] == pytest.approx(t_nw_expect, rel=1e-12)


# --------------------------------------------------------------------------------------------------
# T7 — strongly NEGATIVE autocorrelation also trips |ρ|>0.2 (NW fires; symmetry of the gate).
# --------------------------------------------------------------------------------------------------
def test_block_t_nw_fires_on_negative_ar1():
    x = _ar1(n=400, phi=-0.45, mu=0.02, sigma=0.04, seed=7)
    out = block_t(x, CFG)
    assert out["rho"] < -0.2
    assert out["nw_applied"] is True
    # γ1 < 0 → NW variance < plain variance → |t_NW| > |t_plain| (negative autocorr deflates the var)
    n = x.size
    t_plain = x.mean() / (x.std(ddof=1) / np.sqrt(n))
    assert abs(out["t"]) > abs(t_plain)


# --------------------------------------------------------------------------------------------------
# T8 — degenerate inputs: n<2 and zero-variance → NaN t, nw_applied False, no crash.
# --------------------------------------------------------------------------------------------------
def test_degenerate_series():
    one = block_t(np.array([0.03]), CFG)
    assert np.isnan(one["t"]) and one["nw_applied"] is False and one["n"] == 1

    flat = block_t(np.full(50, 0.01), CFG)      # zero variance
    assert np.isnan(flat["t"]) and flat["nw_applied"] is False

    empty = block_t(np.array([]), CFG)
    assert np.isnan(empty["t"]) and empty["n"] == 0


# --------------------------------------------------------------------------------------------------
# T9 — _lag1_autocorr recovers a known ρ (construct-validity of the ρ estimator itself).
# --------------------------------------------------------------------------------------------------
def test_lag1_autocorr_recovers_known_rho():
    x = _ar1(n=2000, phi=0.5, mu=0.0, sigma=0.03, seed=9)
    assert _lag1_autocorr(x) == pytest.approx(0.5, abs=0.06)
    iid = np.random.default_rng(99).normal(0, 1, 2000)
    assert abs(_lag1_autocorr(iid)) < 0.06
