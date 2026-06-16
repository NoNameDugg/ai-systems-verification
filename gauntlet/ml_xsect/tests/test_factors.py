"""TDI for mlx_factors — construction-matched L-S returns, RT-A alpha-retained, _known, char-space cross-check."""
import numpy as np
import pandas as pd

import mlx_factors as mf


def _names(n=120):
    return [f"n{i}" for i in range(n)]


def test_quantile_ls_positive_when_char_predicts():
    nm = _names()
    char = pd.Series(np.arange(120.0), index=nm)
    fwd = pd.Series(np.arange(120.0) * 0.001, index=nm)        # fwd monotone in char
    assert mf.quantile_ls_return(char, fwd, None, q=5) > 0


def test_quantile_ls_negative_when_char_anti_predicts():
    nm = _names()
    char = pd.Series(np.arange(120.0), index=nm)
    fwd = pd.Series(np.arange(120.0)[::-1] * 0.001, index=nm)
    assert mf.quantile_ls_return(char, fwd, None, q=5) < 0


def test_rt_a_pure_exposure_zero_alpha():
    rng = np.random.default_rng(1)
    f = pd.Series(rng.normal(size=120))
    book = 1.5 * f
    fdf = pd.DataFrame({"rmw": f})
    r = mf.rt_a_alpha(book, fdf)
    assert abs(r["betas"]["rmw"] - 1.5) < 1e-6
    assert abs(r["alpha"]) < 1e-6
    assert r["explained_frac"] > 0.98


def test_rt_a_alpha_retained():
    rng = np.random.default_rng(2)
    f = pd.Series(rng.normal(size=120))
    book = 1.5 * f + 0.01                                       # constant alpha on top of pure exposure
    r = mf.rt_a_alpha(book, pd.DataFrame({"rmw": f}))
    assert abs(r["alpha"] - 0.01) < 1e-3
    assert abs(r["resid"].mean() - r["alpha"]) < 1e-9          # mean(resid) == alpha (RT-A: alpha RETAINED)


def test_rt_a_orthogonal_book_keeps_alpha():
    rng = np.random.default_rng(3)
    f = pd.Series(rng.normal(size=200))
    book = pd.Series(rng.normal(scale=1e-3, size=200)) + 0.02   # independent of f, real alpha
    r = mf.rt_a_alpha(book, pd.DataFrame({"rmw": f}))
    assert abs(r["betas"]["rmw"]) < 0.05 and abs(r["alpha"] - 0.02) < 2e-3 and r["explained_frac"] < 0.1


def test_known_factor_flag_rmw_and_cma():
    assert mf.known_factor_flag({"rmw": 0.8, "cma": 0.0}, 0.1) is True      # rmw exposure (v4-verify: tested!)
    assert mf.known_factor_flag({"rmw": 0.0, "cma": 0.7}, 0.1) is True      # cma exposure
    assert mf.known_factor_flag({"rmw": 0.2, "cma": 0.2}, 0.6) is True      # high explained_frac
    assert mf.known_factor_flag({"rmw": 0.1, "cma": 0.1}, 0.2) is False     # genuinely orthogonal


def test_char_space_zero_when_score_is_linear_combo():
    rng = np.random.default_rng(4)
    nm = _names()
    c1p, c2p, score, fwd = {}, {}, {}, {}
    for d in ["d1", "d2", "d3", "d4"]:
        c1 = pd.Series(rng.normal(size=120), index=nm)
        c2 = pd.Series(rng.normal(size=120), index=nm)
        s = 2 * c1 - c2 + pd.Series(rng.normal(scale=0.01, size=120), index=nm)   # ~pure linear combo
        c1p[d], c2p[d], score[d] = c1, c2, s
        fwd[d] = s + pd.Series(rng.normal(scale=0.5, size=120), index=nm)
    inc = mf.char_space_incremental_ic(score, {"c1": c1p, "c2": c2p}, fwd)
    assert abs(inc.mean()) < 0.15          # residual ~ noise → no incremental signal


def test_char_space_survives_with_orthogonal_signal():
    rng = np.random.default_rng(5)
    nm = _names()
    c1p, c2p, score, fwd = {}, {}, {}, {}
    for d in ["d1", "d2", "d3", "d4", "d5", "d6"]:
        c1 = pd.Series(rng.normal(size=120), index=nm)
        c2 = pd.Series(rng.normal(size=120), index=nm)
        extra = pd.Series(rng.normal(size=120), index=nm)               # orthogonal to chars
        score[d] = c1 + extra
        fwd[d] = extra + pd.Series(rng.normal(scale=0.3, size=120), index=nm)   # fwd driven by extra
        c1p[d], c2p[d] = c1, c2
    inc = mf.char_space_incremental_ic(score, {"c1": c1p, "c2": c2p}, fwd)
    assert inc.mean() > 0.3                 # residual ~ extra → strong incremental IC
