"""TDI — the BD gauntlet wiring (book="prof"; DEC-338): the profitability_book route, the un-gated fences
(gross_ann + realistic-35bps + net_ann_gross_turnover), the P3 non-vacuity firing for the ROLLING cadence,
the post-2013 OOS re-bound, and the BLOCK-1 invariant (NET-turnover net >= old gross-turnover net)."""
from __future__ import annotations
import dataclasses
import pandas as pd
import pytest

import synth_fixtures as sf
from _schema import ForkBConfig, regime_of
from gauntlet import run_gauntlet


@pytest.fixture(scope="module")
def snap():
    s, _ = sf.make_snapshot(seed=7, n_names=120, start="2000-01-04", end="2024-12-31",
                            rmw_edge_frac=0.7, rmw_alpha_ann=0.25)     # strong edge + long span -> powered/rolling
    return s


@pytest.fixture(scope="module")
def prof_out(snap):
    cfg = dataclasses.replace(ForkBConfig(), p3_stress_windows=(("2015-06-01", "2015-09-30"),))  # in-span (non-vacuous)
    return run_gauntlet(snap, cfg, weighting="VW", book="prof", perm_n=20)


def test_prof_runs_and_fences_present(prof_out):
    o = prof_out
    assert o["book"] == "prof"
    assert not str(o["verdict"]).startswith("HALT")
    for k in ("gross_ann", "net_ann_realistic_35bps", "net_ann_gross_turnover", "net_ann", "resid_betas"):
        assert k in o, k


def test_gated_betas_are_the_clean_five_factor_set(prof_out):
    """★ §4 deliverable: the gated residual betas are reported AND are the 5 frozen factors — NO RMW/CMA leak
    into the GATING set (that would be a by-construction false-NULL of the profitability premium)."""
    betas = prof_out["resid_betas"]
    assert set(betas) == {"size", "value", "mom", "str", "beta"}
    assert "rmw" not in betas and "cma" not in betas


def test_prof_powered_rolling(prof_out):
    o = prof_out
    assert o["n_active_blocks"] >= 60                 # 25yr rolling -> ~100 blocks -> POWERED (unlike annual NSI)
    assert o["underpowered"] is False


def test_block1_net_turnover_helps_net(prof_out):
    """★ S2 BLOCK-1 end-to-end: NET-turnover cost <= old GROSS-turnover cost -> the gated net_ann (NET) must be
    >= net_ann_gross_turnover (the old per-cohort full-leg accounting). This is the false-NULL fix, observable."""
    o = prof_out
    assert o["net_ann"] >= o["net_ann_gross_turnover"] - 1e-9


def test_prof_interp_residual_known_factor_capable(prof_out):
    o = prof_out
    assert o["interpretation"]["label"] in ("NOVEL", "KNOWN-FACTOR")
    assert "rmw_beta" in o["interpretation"] and "cma_beta" in o["interpretation"]


def test_prof_p3_vacuous_raises_for_rolling(snap):
    """★ S2 AMEND: the P3 non-vacuity guard must fire for the prof ROLLING cadence (not only qoq) — an
    out-of-span window must raise, not silently pass."""
    cfg = dataclasses.replace(ForkBConfig(), p3_stress_windows=(("1990-01-01", "1990-01-05"),))
    with pytest.raises(ValueError, match="P3 stress window"):
        run_gauntlet(snap, cfg, weighting="VW", book="prof")


def test_regime_of_post2013_bounds():
    """★ S2 AMEND-3: the post-2013 OOS re-bound produces a >=2013 regime (label DERIVED, not hard-coded ">=2010")."""
    cfg = dataclasses.replace(ForkBConfig(), oos_regime_bounds=("2001-01-01", "2013-01-01"))
    d = pd.Series(pd.to_datetime(["1999-06-01", "2007-06-01", "2018-06-01"]))
    assert list(regime_of(d, cfg).values) == ["pre2001", "2001-2012", ">=2013"]


def test_verdict_terminal_novel_on_known_factor_not_bare_deploy():
    """★ DEC-340 carry-forward: a known-factor book (prof≈RMW / nsi≈CMA) mislabeled NOVEL (the BD construction artifact)
    must route to PROMISING-PENDING-INVESTIGATION, NOT a bare DEPLOY. All other branches unchanged."""
    from gauntlet import _verdict_terminal as v
    assert v(True, True, True, False, "NOVEL", "prof", "CALIBRATED") == "PROMISING-PENDING-INVESTIGATION"
    assert v(True, True, True, False, "NOVEL", "nsi", "CALIBRATED") == "PROMISING-PENDING-INVESTIGATION"
    assert v(True, True, True, False, "KNOWN-FACTOR", "prof", "CALIBRATED") == "DEPLOY-as-known-factor"
    assert v(True, True, True, False, "KNOWN-FACTOR", "prof", "PLACEHOLDER(x)") == "DEPLOY-as-known-factor-PENDING-COST-CALIBRATION"
    assert v(True, True, True, False, "NOVEL", "pead", "CALIBRATED") == "DEPLOY"           # genuine novel on a non-factor book
    assert v(False, True, True, False, "KNOWN-FACTOR", "prof", "CALIBRATED") == "NULL"
    assert v(True, False, True, False, "KNOWN-FACTOR", "prof", "CALIBRATED") == "PROMISING-UNCONFIRMED"
    assert v(True, True, False, True, "KNOWN-FACTOR", "prof", "CALIBRATED") == "PROMISING-UNCONFIRMED"
    assert v(True, True, False, False, "KNOWN-FACTOR", "prof", "CALIBRATED") == "OOS-FALSIFIED-NULL"
