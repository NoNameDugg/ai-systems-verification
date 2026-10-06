"""
Tests for lag_sensitivity: a planted look-ahead signal collapses when delayed one bar, a planted
real edge degrades gracefully, and pure noise is flagged neither way.

Run:  python -m pytest test_lag_sensitivity.py -q   (from this directory)
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from lag_sensitivity import delay_available_at, lag_sensitivity  # noqa: E402
from signal_return_pairer import pair_signal_forward_return  # noqa: E402

N = 3000


def _bars(n, start="2010-01-01"):
    return np.array([np.datetime64(start) + np.timedelta64(d, "D") for d in range(n)],
                    dtype="datetime64[ns]")


def _random_walk(n, seed, sigma=0.01):
    rng = np.random.default_rng(seed)
    r = rng.normal(0.0, sigma, size=n)
    return 100.0 * np.exp(np.cumsum(r)), rng


def _lookahead_fixture(seed=1):
    """Signal at bar t = the log return the pairer will score it against at H=1 (entry t+1 -> exit
    t+2), plus a little noise. Timestamps are PIT-clean (available 12h after the bar), so only the
    signal's *content* is leaky -- exactly what a timestamp audit cannot see."""
    pt = _bars(N)
    price, rng = _random_walk(N, seed)
    fwd = np.full(N, np.nan)
    fwd[:-2] = np.log(price[2:] / price[1:-1])
    sig = fwd + rng.normal(0.0, 0.002, size=N)
    aa = pt + np.timedelta64(12, "h")
    return pt, sig, aa, price


def _real_edge_fixture(seed=2, phi=0.9, beta=0.003, sigma=0.01):
    """Persistent AR(1) signal, unit variance; the return of the bar AFTER the entry bar carries
    beta * signal. Real, modest, and slow-moving -- a delay of one bar costs ~ (1 - phi) of it."""
    rng = np.random.default_rng(seed)
    pt = _bars(N)
    s = np.empty(N)
    s[0] = rng.normal()
    eta = rng.normal(0.0, np.sqrt(1.0 - phi ** 2), size=N)
    for t in range(1, N):
        s[t] = phi * s[t - 1] + eta[t]
    eps = rng.normal(0.0, sigma, size=N)
    r = np.zeros(N)
    # r[t+2] is the return from bar t+1 to bar t+2, i.e. the H=1 forward return of entry bar t+1
    r[2:] = beta * s[:-2] + eps[2:]
    price = 100.0 * np.exp(np.cumsum(r))
    aa = pt + np.timedelta64(12, "h")
    return pt, s, aa, price


def _noise_fixture(seed=3):
    rng = np.random.default_rng(seed)
    pt = _bars(N)
    price, _ = _random_walk(N, seed + 100)
    sig = rng.normal(size=N)
    aa = pt + np.timedelta64(12, "h")
    return pt, sig, aa, price


# ---- delay_available_at ------------------------------------------------------
def test_delay_moves_entry_exactly_k_bars():
    pt = _bars(30)
    price = 100.0 + np.arange(30.0)
    st = pt[:20]
    sv = np.arange(20.0)
    aa = st + np.timedelta64(12, "h")
    base = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    for k in (1, 2, 5):
        lagged = pair_signal_forward_return(st, sv, delay_available_at(aa, pt, k), pt, price, H=1)
        assert lagged["n_pairs"] == base["n_pairs"]
        np.testing.assert_array_equal(lagged["entry_time"], base["entry_time"] + np.timedelta64(k, "D"))


def test_delay_past_history_drops_rather_than_raises():
    pt = _bars(6)
    price = 100.0 + np.arange(6.0)
    st = pt[:5]
    sv = np.arange(5.0)
    aa = st + np.timedelta64(12, "h")
    out = pair_signal_forward_return(st, sv, delay_available_at(aa, pt, 3), pt, price, H=1)
    assert out["n_pairs"] < 5          # the late ones fell off the end


def test_delay_k_below_one_raises():
    with pytest.raises(ValueError):
        delay_available_at(_bars(3), _bars(3), 0)


# ---- the three planted cases -------------------------------------------------
def test_planted_lookahead_collapses_at_lag_one():
    pt, sig, aa, price = _lookahead_fixture()
    out = lag_sensitivity(pt, sig, aa, pt, price, H=1)
    assert out["effect_present"] is True
    assert out["ic_lag0"] > 0.8
    lag1 = out["lags"][0]
    assert lag1["lag"] == 1
    assert abs(lag1["retained_share"]) < 0.30
    assert out["verdict"] == "LOOKAHEAD-SUSPECT"


def test_planted_real_edge_keeps_more_than_half_at_lag_one():
    pt, sig, aa, price = _real_edge_fixture()
    out = lag_sensitivity(pt, sig, aa, pt, price, H=1)
    assert out["effect_present"] is True
    assert out["ic_lag0"] > 0.15
    assert out["lags"][0]["retained_share"] > 0.50
    assert out["verdict"] == "ROBUST"


def test_pure_noise_is_flagged_neither_way():
    pt, sig, aa, price = _noise_fixture()
    out = lag_sensitivity(pt, sig, aa, pt, price, H=1)
    assert out["effect_present"] is False
    assert out["verdict"] == "NO-EFFECT"
    assert abs(out["ic_lag0"]) < 0.05


# ---- knobs and plumbing ------------------------------------------------------
def test_collapse_share_is_the_threshold():
    pt, sig, aa, price = _real_edge_fixture()
    # ask for 99% retention: a phi=0.9 signal retains ~90%, so it is now (correctly) flagged
    strict = lag_sensitivity(pt, sig, aa, pt, price, H=1, collapse_share=0.01)
    assert strict["verdict"] == "LOOKAHEAD-SUSPECT"
    # ask for only 1% retention: the look-ahead signal (~0% retained) is still flagged
    pt2, sig2, aa2, price2 = _lookahead_fixture()
    loose = lag_sensitivity(pt2, sig2, aa2, pt2, price2, H=1, collapse_share=0.99)
    assert loose["verdict"] == "LOOKAHEAD-SUSPECT"


def test_lags_reported_in_order_with_counts():
    pt, sig, aa, price = _real_edge_fixture()
    out = lag_sensitivity(pt, sig, aa, pt, price, H=1, lags=(1, 2, 3))
    assert [row["lag"] for row in out["lags"]] == [1, 2, 3]
    assert all(row["n_pairs"] > N - 10 for row in out["lags"])
    assert out["collapse_share"] == pytest.approx(0.70)


def test_bad_arguments_raise():
    pt, sig, aa, price = _noise_fixture()
    with pytest.raises(ValueError):
        lag_sensitivity(pt, sig, aa, pt, price, H=1, collapse_share=1.5)
    with pytest.raises(ValueError):
        lag_sensitivity(pt, sig, aa, pt, price, H=1, lags=())
    with pytest.raises(ValueError):
        lag_sensitivity(pt, sig[:-1], aa, pt, price, H=1)     # shape mismatch from the pairer


def test_deterministic():
    pt, sig, aa, price = _real_edge_fixture()
    assert lag_sensitivity(pt, sig, aa, pt, price, H=1) == lag_sensitivity(pt, sig, aa, pt, price, H=1)
