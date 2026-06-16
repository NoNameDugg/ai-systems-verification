"""
TDI tests for rank_ic_calculator (Sprint AV-ALTDATA-IC Track-1, charter T1.1/D-SOFT-7).

Run:  python -m pytest scripts/backtester/rank_ic_gate/test_rank_ic_calculator.py -q
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from rank_ic_calculator import average_rank, rank_ic  # noqa: E402


# ---- average_rank --------------------------------------------------------
def test_average_rank_no_ties():
    np.testing.assert_array_equal(average_rank([30.0, 10.0, 20.0]), [3.0, 1.0, 2.0])


def test_average_rank_ties_get_mean_rank():
    # two tied at the bottom -> mean of ranks 1,2 = 1.5; top -> 3
    np.testing.assert_array_equal(average_rank([10.0, 10.0, 20.0]), [1.5, 1.5, 3.0])


def test_average_rank_all_tied():
    np.testing.assert_array_equal(average_rank([5.0, 5.0, 5.0]), [2.0, 2.0, 2.0])


# ---- core IC -------------------------------------------------------------
def test_perfect_monotonic_increasing_ic_is_one():
    r = rank_ic(np.arange(50.0), np.arange(50.0) ** 2)  # strictly monotone -> rank-IC == 1
    assert r["ic"] == pytest.approx(1.0)
    assert r["n_pairs"] == 50


def test_perfect_monotonic_decreasing_ic_is_minus_one():
    r = rank_ic(np.arange(50.0), -np.arange(50.0) ** 3)
    assert r["ic"] == pytest.approx(-1.0)


def test_positive_relationship_has_positive_ic_within_ci():
    rng = np.random.default_rng(7)
    x = rng.normal(size=400)
    y = 0.4 * x + rng.normal(size=400)  # genuine positive association
    r = rank_ic(x, y)
    assert r["ic"] > 0.0
    assert r["ci_low"] <= r["ic"] <= r["ci_high"]   # CI brackets the point estimate


def test_matches_scipy_spearman_when_available():
    scipy_stats = pytest.importorskip("scipy.stats")
    rng = np.random.default_rng(11)
    x = rng.normal(size=120)
    y = rng.normal(size=120) + 0.3 * x
    expected = scipy_stats.spearmanr(x, y).statistic
    assert rank_ic(x, y)["ic"] == pytest.approx(expected, abs=1e-9)


# ---- Fisher-z SE + tanh CI (the D-SOFT-7 spec) ---------------------------
def test_se_z_is_one_over_sqrt_n_minus_3():
    r = rank_ic(np.random.default_rng(1).normal(size=103),
                np.random.default_rng(2).normal(size=103))
    assert r["se_z"] == pytest.approx(1.0 / np.sqrt(103 - 3))


def test_ci_is_tanh_backtransform_of_z_interval():
    rng = np.random.default_rng(3)
    x = rng.normal(size=200)
    y = 0.5 * x + rng.normal(size=200)
    r = rank_ic(x, y, z_crit=1.96)
    z, se = r["z_ic"], r["se_z"]
    assert r["ci_low"] == pytest.approx(np.tanh(z - 1.96 * se))
    assert r["ci_high"] == pytest.approx(np.tanh(z + 1.96 * se))
    assert r["z_ic"] == pytest.approx(np.arctanh(np.clip(r["ic"], -1 + 1e-12, 1 - 1e-12)))


# ---- NaN / degenerate handling ------------------------------------------
def test_drops_nan_pairs_and_counts_valid():
    s = np.array([1.0, 2.0, np.nan, 4.0, 5.0])
    f = np.array([1.0, np.nan, 3.0, 4.0, 5.0])
    r = rank_ic(s, f)                      # only indices 0,3,4 survive -> n=3
    assert r["n_pairs"] == 3
    assert np.isfinite(r["ic"])            # ic defined at n=3...
    assert np.isnan(r["se_z"])             # ...but SE undefined for n<4


def test_n_less_than_4_has_nan_se_and_ci_but_defined_ic():
    r = rank_ic([1.0, 3.0, 2.0], [10.0, 30.0, 20.0])
    assert r["n_pairs"] == 3
    assert r["ic"] == pytest.approx(1.0)
    assert np.isnan(r["se_z"]) and np.isnan(r["ci_low"]) and np.isnan(r["ci_high"])


def test_zero_variance_signal_returns_nan_ic():
    r = rank_ic([5.0, 5.0, 5.0, 5.0, 5.0], [1.0, 2.0, 3.0, 4.0, 5.0])
    assert np.isnan(r["ic"])


def test_shape_mismatch_raises():
    with pytest.raises(ValueError):
        rank_ic([1.0, 2.0, 3.0], [1.0, 2.0])


def test_pure_no_clock_same_inputs_same_output():
    x = np.linspace(-1, 1, 80)
    y = np.sin(x)
    assert rank_ic(x, y) == rank_ic(x, y)   # deterministic, no hidden state
