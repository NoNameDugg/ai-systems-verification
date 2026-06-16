"""TDI for mlx_fracdiff on synthetic ground-truth."""
import numpy as np
import pandas as pd

import mlx_fracdiff as fd


def test_weights_d1_is_first_difference():
    w = fd.ffd_weights(1.0, tau=1e-4)
    assert np.allclose(w, [1.0, -1.0])


def test_weights_d0_is_identity():
    w = fd.ffd_weights(0.0, tau=1e-4)
    assert np.allclose(w, [1.0])


def test_ffd_d1_equals_diff():
    s = pd.Series(np.cumsum(np.random.default_rng(1).normal(size=50)))
    out = fd.frac_diff_ffd(s, 1.0)
    assert np.isnan(out.iloc[0])
    assert np.allclose(out.iloc[1:].values, s.diff().iloc[1:].values)


def test_ffd_d0_is_identity():
    s = pd.Series(np.arange(20.0))
    out = fd.frac_diff_ffd(s, 0.0)
    assert np.allclose(out.values, s.values)


def test_adf_iid_is_stationary():
    x = np.random.default_rng(2).normal(size=500)
    assert fd.adf_tstat(x, maxlag=1) < fd.ADF_CRIT_5PCT


def test_adf_random_walk_is_nonstationary():
    x = np.cumsum(np.random.default_rng(3).normal(size=500))
    assert fd.adf_tstat(x, maxlag=1) > fd.ADF_CRIT_5PCT


def test_min_ffd_iid_is_zero():
    s = pd.Series(np.random.default_rng(4).normal(size=500))
    assert fd.min_ffd_order(s) == 0.0


def test_min_ffd_random_walk_is_fractional():
    s = pd.Series(np.cumsum(np.random.default_rng(5).normal(size=800)))
    d = fd.min_ffd_order(s)
    assert 0.0 < d <= 1.0          # needs differencing; fractional d stationarizes
