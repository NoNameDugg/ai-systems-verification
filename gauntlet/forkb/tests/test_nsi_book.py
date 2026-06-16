"""TDI — harness.nsi_book (the BC gated/diagnostic net-share-issuance book). DEC-333 Block A."""
from __future__ import annotations
import numpy as np
import pandas as pd
import pytest

import synth_fixtures as sf
from _schema import ForkBConfig
import harness
import nsi as nsi_mod


@pytest.fixture(scope="module")
def edge():
    return sf.make_snapshot(seed=1, n_names=200, start="2010-01-04", end="2019-12-31",
                            nsi_edge_frac=0.6, nsi_alpha_ann=0.15)


@pytest.fixture(scope="module")
def flat():
    return sf.make_snapshot(seed=2, n_names=200, start="2010-01-04", end="2019-12-31", nsi_edge_frac=0.0)


_GATE_KEYS = ["daily_raw", "long_leg_daily", "short_leg_daily", "formation_labels", "name_daily",
              "turnover_daily", "short_gross_daily", "n_formations", "max_short_weight", "active_mask"]


def test_gate_key_contract(edge):
    bk = harness.nsi_book(edge[0], ForkBConfig(), cadence="annual_june")
    for k in _GATE_KEYS:
        assert k in bk, k
    assert bk["n_formations"] > 0


def test_direction_low_nsi_is_long(edge):
    cfg = ForkBConfig()
    bk = harness.nsi_book(edge[0], cfg, cadence="annual_june")
    nsi = nsi_mod.compute_nsi(edge[0], cfg)
    D, lab = next(iter(bk["formation_labels"].items()))
    longs = list(lab[lab == 1].index)
    shorts = list(lab[lab == -1].index)
    asof = nsi[nsi.datekey <= D].sort_values("datekey").groupby("ticker").tail(1).set_index("ticker")["nsi"]
    assert asof.reindex(longs).dropna().mean() < asof.reindex(shorts).dropna().mean()   # buyback (low NSI) is LONG


def test_cap_binds_and_emits_max_short_weight(edge):
    cfg = ForkBConfig(single_name_weight_cap=0.10, min_leg_names=11)        # cap binds when leg>10
    bk = harness.nsi_book(edge[0], cfg, cadence="annual_june")
    if bk["n_formations"] == 0:
        pytest.skip("fixture too small to form a >=11-name leg")
    assert bk["max_short_weight"] <= cfg.single_name_weight_cap + 1e-9


def test_active_block_honesty(edge):
    """★ S2 #3 / D-HARD B: annual-June holds ~63 active td/yr; active blocks must be FEWER than the
    full-index block count (i.e. zero-filled flat days are excluded from the M2 block unit)."""
    cfg = ForkBConfig()
    bk = harness.nsi_book(edge[0], cfg, cadence="annual_june")
    active = int(bk["active_mask"].sum())
    full = len(bk["daily_raw"])
    assert active > 0
    assert active // cfg.block_td < full // cfg.block_td


def test_qoq_nonoverlapping_cohorts(edge):
    cfg = ForkBConfig()
    bk = harness.nsi_book(edge[0], cfg, cadence="qoq_nonoverlap")
    dts = sorted(bk["formation_labels"].keys())
    if len(dts) < 2:
        pytest.skip("too few qoq formations")
    dates = bk["daily_raw"].index
    pos = np.array([dates.get_loc(d) for d in dts])
    assert (np.diff(pos) >= cfg.hold_td).all()                              # non-overlapping 63-td cohorts


def test_injected_nsi_edge_recovered_positive(edge, flat):
    cfg = ForkBConfig()
    e = harness.nsi_book(edge[0], cfg, cadence="annual_june", weighting="EW")
    f = harness.nsi_book(flat[0], cfg, cadence="annual_june", weighting="EW")
    em = float(e["daily_raw"][e["active_mask"]].mean())
    fm = float(f["daily_raw"][f["active_mask"]].mean())
    assert em > 0.0          # buyback-long book earns the injected NSI drift
    assert em > fm           # ... and more than the no-edge fixture
