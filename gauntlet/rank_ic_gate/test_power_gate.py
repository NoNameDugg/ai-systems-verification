"""
TDI tests for power_gate (Sprint AV-ALTDATA-IC Track-1, charter T1.3/D-HARD-1/v2-DS-1).

Run:  python -m pytest scripts/backtester/rank_ic_gate/test_power_gate.py -q
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from power_gate import (ar1, mde_ic, n_eff_correlation, power_gate,  # noqa: E402
                        se_ic, Z_SUM_DEFAULT)


def _ar1_series(phi, n, seed):
    rng = np.random.default_rng(seed)
    e = rng.normal(size=n)
    x = np.empty(n)
    x[0] = e[0]
    for t in range(1, n):
        x[t] = phi * x[t - 1] + e[t]
    return x


def _overlapping_fwd(n, H, seed):
    """Overlapping H-day forward returns from iid daily returns -> MA(H-1), lag-1 acf ~ (H-1)/H."""
    rng = np.random.default_rng(seed)
    daily = rng.normal(size=n + H)
    return np.array([daily[t:t + H].sum() for t in range(n)])


# ---- ar1 -----------------------------------------------------------------
def test_ar1_white_noise_near_zero():
    assert abs(ar1(np.random.default_rng(0).normal(size=5000))) < 0.05


def test_ar1_persistent_series_near_phi():
    assert ar1(_ar1_series(0.9, 5000, 1)) == pytest.approx(0.9, abs=0.05)


def test_ar1_overlapping_returns_high():
    # lag-1 acf of overlapping 21-day returns ~ 20/21 ~ 0.95
    assert ar1(_overlapping_fwd(5000, 21, 2)) == pytest.approx(20 / 21, abs=0.05)


def test_ar1_too_short_is_nan():
    assert np.isnan(ar1([1.0, 2.0]))


# ---- N_eff / SE / MDE closed forms --------------------------------------
def test_n_eff_zero_autocorr_equals_T():
    assert n_eff_correlation(3000, 0.0, 0.0) == pytest.approx(3000.0)


def test_n_eff_positive_product_below_T():
    assert n_eff_correlation(1000, 0.5, 0.5) == pytest.approx(600.0)  # 1000*0.75/1.25


def test_n_eff_symmetric_in_rho():
    assert n_eff_correlation(1000, 0.5, 0.3) == pytest.approx(n_eff_correlation(1000, 0.3, 0.5))


def test_n_eff_caps_at_T_for_negative_product():
    assert n_eff_correlation(1000, 0.5, -0.5, cap_at_T=True) == pytest.approx(1000.0)
    assert n_eff_correlation(1000, 0.5, -0.5, cap_at_T=False) > 1000.0


def test_se_and_mde_closed_form():
    assert se_ic(3136) == pytest.approx(1 / 56)
    assert mde_ic(3136, z_sum=2.80) == pytest.approx(2.80 / 56)        # == 0.05
    assert Z_SUM_DEFAULT == pytest.approx(2.80)


# ---- the v2-DS-1 horizon<->power coupling (the substantive behaviour) -----
def test_h1d_persistent_signal_iid_returns_passes():
    # rho_x ~ 0.99 (spread level), rho_y ~ 0 (non-overlapping daily) -> product ~ 0 -> N_eff ~ T
    sig = _ar1_series(0.99, 4000, 10)
    fwd = np.random.default_rng(11).normal(size=4000)   # iid 1-day returns
    g = power_gate(sig, fwd)
    assert g["rho_y"] == pytest.approx(0.0, abs=0.06)
    assert g["n_eff"] > 3500
    assert g["mde_ic"] < 0.05
    assert g["gated_out"] is False


def test_long_overlapping_horizon_collapses_neff_and_gates_out():
    # rho_x ~ 0.99 AND rho_y ~ 0.95 (overlapping 21d) -> product ~ 0.94 -> N_eff collapses
    sig = _ar1_series(0.99, 4000, 12)
    fwd = _overlapping_fwd(4000, 21, 13)
    g = power_gate(sig, fwd)
    assert g["rho_x"] > 0.9 and g["rho_y"] > 0.85
    assert g["n_eff"] < 500            # collapsed far below T=4000
    assert g["mde_ic"] > 0.05
    assert g["gated_out"] is True


def test_mde_increases_monotonically_with_horizon():
    sig = _ar1_series(0.99, 4000, 14)
    mde_5d = power_gate(sig, _overlapping_fwd(4000, 5, 15))["mde_ic"]
    mde_21d = power_gate(sig, _overlapping_fwd(4000, 21, 16))["mde_ic"]
    assert mde_21d > mde_5d > 0.04


# ---- rho-sensitivity band + plumbing ------------------------------------
def test_rho_band_brackets_point_estimate():
    sig = _ar1_series(0.95, 3000, 17)
    fwd = _overlapping_fwd(3000, 5, 18)
    g = power_gate(sig, fwd)
    assert g["mde_band_low"] <= g["mde_ic"] <= g["mde_band_high"]
    assert g["mde_band_low"] < g["mde_band_high"]     # a genuine band, not a point


def test_gated_out_matches_ceiling_comparison():
    sig = _ar1_series(0.99, 4000, 19)
    fwd = _overlapping_fwd(4000, 21, 20)
    g = power_gate(sig, fwd, ceiling=0.05)
    assert g["gated_out"] == (g["mde_ic"] > 0.05)


def test_shape_mismatch_raises():
    with pytest.raises(ValueError):
        power_gate([1.0, 2.0, 3.0], [1.0, 2.0])
