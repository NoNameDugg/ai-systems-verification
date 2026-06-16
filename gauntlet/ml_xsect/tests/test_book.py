"""TDI for mlx_book — book weights, net-turnover, drawdown/Calmar, PSR/DSR, P3 kill-test."""
import numpy as np
import pandas as pd

import mlx_book as mb


def test_signed_weights_long_plus1_short_minus1():
    nm = [f"n{i}" for i in range(100)]
    scores = {"d": pd.Series(np.arange(100.0), index=nm)}
    weights = {"d": pd.Series(1.0, index=nm)}
    sw = mb.signed_book_weights(scores, weights, q=5)["d"]
    assert abs(sw[sw > 0].sum() - 1.0) < 1e-9 and abs(sw[sw < 0].sum() + 1.0) < 1e-9
    assert sw["n99"] > 0 and sw["n0"] < 0          # top score long, bottom short


def test_net_turnover_zero_when_unchanged():
    nm = [f"n{i}" for i in range(100)]
    s = pd.Series(np.arange(100.0), index=nm)
    held = mb.signed_book_weights({"d1": s, "d2": s}, {"d1": pd.Series(1.0, index=nm), "d2": pd.Series(1.0, index=nm)}, 5)
    to = mb.net_turnover_series(held)
    assert abs(to.iloc[1]) < 1e-9                  # identical book -> no turnover


def test_net_turnover_high_on_flip():
    nm = [f"n{i}" for i in range(100)]
    s = pd.Series(np.arange(100.0), index=nm)
    held = mb.signed_book_weights({"d1": s, "d2": s[::-1].reset_index(drop=True).set_axis(nm)},
                                  {"d1": pd.Series(1.0, index=nm), "d2": pd.Series(1.0, index=nm)}, 5)
    assert mb.net_turnover_series(held).iloc[1] > 3.0   # full long<->short flip ~ 4.0


def test_max_drawdown_known():
    r = pd.Series([0.1, -0.5, 0.1])
    assert abs(mb.max_drawdown(r) - (0.55 / 1.1 - 1.0)) < 1e-9


def test_calmar_positive():
    rng = np.random.default_rng(0)
    r = pd.Series(rng.normal(0.01, 0.02, size=120))     # positive drift
    assert mb.calmar(r) > 0


def test_psr_high_for_strong_sharpe():
    rng = np.random.default_rng(1)
    r = pd.Series(rng.normal(0.02, 0.01, size=120))     # SR ~ 2/period -> PSR ~ 1
    assert mb.probabilistic_sharpe(r) > 0.99


def test_psr_half_for_zero_mean():
    rng = np.random.default_rng(2)
    r = pd.Series(rng.normal(0.0, 0.02, size=200))
    assert 0.3 < mb.probabilistic_sharpe(r) < 0.7


def test_deflated_lower_than_psr_with_trials():
    rng = np.random.default_rng(3)
    r = pd.Series(rng.normal(0.01, 0.02, size=200))
    assert mb.deflated_sharpe(r, n_trials=50) < mb.probabilistic_sharpe(r, 0.0)


def test_p3_survives_high_calmar():
    rng = np.random.default_rng(4)
    r = pd.Series(rng.normal(0.012, 0.015, size=150),
                  index=pd.date_range("2013-07-31", periods=150, freq="ME"))
    assert mb.p3_killtest(r, calmar_floor=0.50)["survives_p3"] is True


def test_p3_fails_deep_drawdown():
    idx = pd.date_range("2013-07-31", periods=150, freq="ME")
    r = pd.Series(np.full(150, 0.003), index=idx)
    r.iloc[80:84] = -0.30                                # a brutal multi-month crash -> deep DD, low Calmar
    assert mb.p3_killtest(r, calmar_floor=0.50)["survives_p3"] is False
