"""TDI for mlx_ic_power (BLOCK-1 core) on synthetic ground-truth + mlx_config freeze."""
import numpy as np
import pandas as pd

import mlx_ic_power as ic
from mlx_config import MLXConfig


# ---- cross_sectional_ic ------------------------------------------------------------------------
def test_ic_perfect_monotone_is_one():
    s = pd.Series(np.arange(100.0), index=[f"T{i}" for i in range(100)])
    r = pd.Series(np.arange(100.0), index=s.index)
    assert ic.cross_sectional_ic(s, r) > 0.999


def test_ic_reversed_is_minus_one():
    s = pd.Series(np.arange(100.0), index=[f"T{i}" for i in range(100)])
    r = pd.Series(np.arange(100.0)[::-1], index=s.index)
    assert ic.cross_sectional_ic(s, r) < -0.999


def test_ic_below_min_n_is_nan():
    s = pd.Series(np.arange(10.0), index=[f"T{i}" for i in range(10)])
    r = pd.Series(np.arange(10.0), index=s.index)
    assert np.isnan(ic.cross_sectional_ic(s, r, min_n=50))


def test_ic_random_near_zero():
    rng = np.random.default_rng(7)
    idx = [f"T{i}" for i in range(500)]
    s = pd.Series(rng.normal(size=500), index=idx)
    r = pd.Series(rng.normal(size=500), index=idx)
    assert abs(ic.cross_sectional_ic(s, r)) < 0.15


# ---- ar1 / t_eff -------------------------------------------------------------------------------
def test_ar1_iid_near_zero():
    rng = np.random.default_rng(1)
    assert abs(ic.ar1(rng.normal(size=2000))) < 0.1


def test_ar1_strong_positive():
    rng = np.random.default_rng(1)
    x = np.zeros(2000)
    for i in range(1, 2000):
        x[i] = 0.7 * x[i - 1] + rng.normal()
    assert ic.ar1(x) > 0.55


def test_t_eff_iid_approx_T():
    rng = np.random.default_rng(2)
    s = pd.Series(rng.normal(0, 0.06, size=200))
    assert 150 < ic.t_eff(s) <= 200          # rho≈0 → t_eff≈T, capped at T


def test_t_eff_autocorr_below_T():
    rng = np.random.default_rng(2)
    x = np.zeros(200)
    for i in range(1, 200):
        x[i] = 0.6 * x[i - 1] + rng.normal(0, 0.06)
    te = ic.t_eff(pd.Series(x))
    assert te < 120                          # AR(0.6): T*(1-.6)/(1+.6)=T*0.25≈50


# ---- mde_ic / block_t / power_verdict ----------------------------------------------------------
def test_mde_matches_formula_iid():
    rng = np.random.default_rng(3)
    s = pd.Series(rng.normal(0.0, 0.06, size=200))
    expected = 2.80 * np.std(s.values, ddof=1) / np.sqrt(ic.t_eff(s))
    assert abs(ic.mde_ic(s, z_sum=2.80) - expected) < 1e-9


def test_powered_low_std_long_series():
    rng = np.random.default_rng(4)
    s = pd.Series(rng.normal(0.0, 0.06, size=200))       # iid → t_eff≈200 → mde≈0.012 < 0.02
    v = ic.power_verdict(s, ceiling=0.02)
    assert v["powered"] and v["mde_ic"] < 0.02


def test_not_powered_high_std_short_series():
    rng = np.random.default_rng(5)
    s = pd.Series(rng.normal(0.0, 0.10, size=40))        # mde≈0.044 > 0.02 → CANNOT-TEST
    v = ic.power_verdict(s, ceiling=0.02)
    assert not v["powered"] and v["mde_ic"] > 0.02


def test_block_t_real_mean_is_significant():
    rng = np.random.default_rng(6)
    s = pd.Series(rng.normal(0.03, 0.05, size=200))      # real mean 0.03, low std
    bt = ic.block_t(s)
    assert bt["t"] > 3 and bt["ci_lo"] > 0 and bt["mean"] > 0.02


def test_block_t_noise_not_significant():
    rng = np.random.default_rng(6)
    s = pd.Series(rng.normal(0.0, 0.06, size=200))
    bt = ic.block_t(s)
    assert abs(bt["t"]) < 2 and bt["ci_lo"] < 0 < bt["ci_hi"]


def test_ic_series_drops_thin_dates():
    big = pd.Series(np.arange(100.0), index=[f"T{i}" for i in range(100)])
    small = pd.Series(np.arange(10.0), index=[f"T{i}" for i in range(10)])
    scores = {"d1": big, "d2": small}
    fwd = {"d1": big, "d2": small}
    s = ic.ic_series(scores, fwd, min_n=50)
    assert list(s.index) == ["d1"]           # d2 (n=10<50) dropped


# ---- config freeze -----------------------------------------------------------------------------
def test_config_sha_deterministic_and_hex():
    a, b = MLXConfig().sha256(), MLXConfig().sha256()
    assert a == b and len(a) == 64 and all(c in "0123456789abcdef" for c in a)


def test_config_window_and_binning_pinned():
    c = MLXConfig()
    assert c.primary_start == "2013-07-01"           # Phase-0 SF3A pin
    assert c.n_quantiles == 5                         # ONE place for book + factor returns (BLOCK-2′)
    assert c.ic_power_ceiling == c.ic_glimmer_bar == 0.02   # BLOCK-1b: ceiling = decision bar
    assert set(c.factor_set) == {"size", "value", "mom", "str", "beta", "rmw", "cma"}   # 7-factor
