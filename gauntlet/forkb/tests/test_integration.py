"""Fork-B end-to-end gauntlet integration test (charter §4): the apparatus must (a) run end-to-end,
(b) recover an INJECTED edge as a positive factor-residual alpha, and (c) NOT manufacture an edge on
no-edge data (the real-but-uneconomic / false-positive guard). Synthetic ground-truth only."""
import numpy as np
import pytest

from _schema import ForkBConfig
from synth_fixtures import make_snapshot
from gauntlet import run_gauntlet

_VERDICTS = {"DEPLOY", "PROMISING-UNCONFIRMED", "OOS-FALSIFIED-NULL", "NULL", "HALT-CANARY"}


@pytest.fixture(scope="module")
def edge():
    snap, truth = make_snapshot(seed=1, n_names=60, start="2003-01-02", end="2020-12-31",
                                edge_frac=0.8, alpha_ann=0.20)
    return snap, truth, run_gauntlet(snap, ForkBConfig(), weighting="VW", perm_n=50)


def test_gauntlet_runs_end_to_end(edge):
    _, _, res = edge
    assert res["verdict"] in _VERDICTS
    assert res["canary"]["halt"] is False            # correct fixtures -> Canary passes
    assert res["n_blocks"] > 10                       # block unit populated
    assert res["n_formations"] >= 4                   # cohorts formed
    # all the gate components ran + are reported
    for k in ("M2", "PSR", "perm_null", "eff_n", "P3", "OOS"):
        assert k in res, f"missing gate {k}"


def test_injected_edge_recovered_as_positive_alpha(edge):
    _, _, res = edge
    # the SUE-ranked long-short book should capture the injected post-earnings-drift alpha,
    # and the factor-residual must RETAIN it (RT-A) -> alpha_daily > 0
    assert res["alpha_daily"] > 0, f"injected edge not recovered: alpha_daily={res['alpha_daily']}"


def test_no_edge_no_spurious_edge_and_no_false_deploy():
    snap0, _ = make_snapshot(seed=2, n_names=60, start="2003-01-02", end="2020-12-31",
                             edge_frac=0.0, alpha_ann=0.0)
    cfg = ForkBConfig()
    res0 = run_gauntlet(snap0, cfg, weighting="VW", perm_n=50)
    assert res0["canary"]["halt"] is False
    # No injected edge -> NO spurious DEPLOYABLE positive edge: the net factor-residual alpha must stay
    # below the +5%/yr M2 floor. (Costs correctly drag the net NEGATIVE here = the real-but-uneconomic
    # signature the program expects, a feature not a bug; the "alpha is retained, not manufactured" property
    # is unit-proven in test_factor_resid's RT-A test.)
    assert res0["alpha_daily"] * 252 < cfg.m2_net_ann_floor, f"spurious deployable edge: {res0['alpha_daily']*252:.4f}/yr"
    assert res0["M2"]["pass"] is False
    assert res0["verdict"] in {"NULL", "OOS-FALSIFIED-NULL"}, f"no-edge should NULL, got {res0['verdict']}"


def test_config_is_frozen_and_hashable():
    res = ForkBConfig().sha256()
    assert isinstance(res, str) and len(res) == 64
