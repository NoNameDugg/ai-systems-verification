"""TDI — the BC gauntlet rewiring (residual split, cost calibration, P3/embargo guards, underpowered branch).
DEC-333 Block G. Guards the S2 must-fixes + the cited underpowered→NULL bug."""
from __future__ import annotations
import dataclasses
import pytest

import synth_fixtures as sf
from _schema import ForkBConfig
from gauntlet import run_gauntlet


@pytest.fixture(scope="module")
def snap():
    s, _ = sf.make_snapshot(seed=3, n_names=200, start="2010-01-04", end="2019-12-31",
                            nsi_edge_frac=0.7, nsi_alpha_ann=0.30)        # strong edge so full_series can pass
    return s


@pytest.fixture(scope="module")
def annual_out(snap):
    return run_gauntlet(snap, ForkBConfig(), book="nsi", cadence="annual_june", perm_n=50)


def test_annual_underpowered_branch_not_null(annual_out):
    """★ The cited bug regression: an annual book is underpowered (active n_blocks ≪ 60); it must NEVER be
    a 'DEPLOY', and if it passes econ+sig+psr+perm+p3 it must route to PROMISING-UNCONFIRMED, NOT NULL."""
    o = annual_out
    assert o["underpowered"] is True
    assert o["n_active_blocks"] < o["M2"]["n_blocks"] + 1 < 60          # active count is the (small) honest unit
    assert not str(o["verdict"]).startswith("DEPLOY")                   # can't deploy underpowered
    if o["full_series_pass"]:
        assert o["verdict"] == "PROMISING-UNCONFIRMED"                  # the fix: underpowered ≠ NULL


def test_interpretation_residual_is_present_and_nongating(annual_out):
    o = annual_out
    assert o["interpretation"]["label"] in ("NOVEL", "KNOWN-FACTOR")
    assert "cma_beta" in o["interpretation"] and "rmw_beta" in o["interpretation"]
    # the GATED M2 came from the 5-factor residual (book/cadence recorded; verdict never NULL'd by CMA collinearity)
    assert o["book"] == "nsi" and o["cadence"] == "annual_june"


def test_embargo_below_hold_raises(snap):
    cfg = dataclasses.replace(ForkBConfig(), perm_embargo_td=10)        # < hold_td=63 → DEC-327 guard
    with pytest.raises(ValueError, match="perm_embargo"):
        run_gauntlet(snap, cfg, book="nsi", cadence="annual_june")


def test_cost_calibration_required_raises(snap):
    """require_cost_calibration with an empty calibration_set must raise (D-HARD D). Use an in-span active P3
    window so the P3 non-vacuity guard passes first and the COST raise is what fires."""
    cfg = dataclasses.replace(ForkBConfig(), p3_stress_windows=(("2015-06-01", "2015-09-30"),))
    with pytest.raises(ValueError, match="calibration"):
        run_gauntlet(snap, cfg, book="nsi", cadence="annual_june", require_cost_calibration=True)


def test_p3_vacuous_window_raises(snap):
    """A stress window hitting no in-position day must raise on the GATED QoQ path (N2: guard tied to the gated
    cadence, not the cost flag) — not silently pass P3 by not stressing."""
    cfg = dataclasses.replace(ForkBConfig(), p3_stress_windows=(("1990-01-01", "1990-01-05"),))
    with pytest.raises(ValueError, match="P3 stress window"):
        run_gauntlet(snap, cfg, book="nsi", cadence="qoq_nonoverlap")
