"""TDI — harness.profitability_book + _net_turnover (the BD gross-profitability book; DEC-338). The headline test is
the S2 BLOCK-1 net-turnover fix: a sticky signal must charge NET turnover ≪ the old per-cohort gross turnover."""
from __future__ import annotations
import dataclasses
import numpy as np
import pandas as pd
import pytest

import synth_fixtures as sf
from _schema import ForkBConfig
import harness
import fundamental_factors as ff


@pytest.fixture(scope="module")
def edge():
    return sf.make_snapshot(seed=5, n_names=200, start="2010-01-04", end="2019-12-31",
                            rmw_edge_frac=0.6, rmw_alpha_ann=0.15)


_KEYS = ["daily_raw", "long_leg_daily", "short_leg_daily", "formation_labels", "name_daily", "turnover_daily",
         "turnover_daily_gross", "short_gross_daily", "n_formations", "max_short_weight", "active_mask"]


def test_gate_key_contract(edge):
    bk = harness.profitability_book(edge[0], ForkBConfig())
    for k in _KEYS:
        assert k in bk, k
    assert bk["n_formations"] > 0


def test_direction_high_profitability_is_long(edge):
    cfg = ForkBConfig()
    bk = harness.profitability_book(edge[0], cfg)
    char = ff.pit_fundamental_chars(edge[0], cfg)["rmw"]
    D, lab = next(iter(bk["formation_labels"].items()))
    longs = list(lab[lab == 1].index)
    shorts = list(lab[lab == -1].index)
    cv = char.loc[D]
    assert cv.reindex(longs).dropna().mean() > cv.reindex(shorts).dropna().mean()   # HIGH gp/assets is LONG


def test_cap_binds(edge):
    cfg = ForkBConfig(single_name_weight_cap=0.10, min_leg_names=11)
    bk = harness.profitability_book(edge[0], cfg)
    if bk["n_formations"] == 0:
        pytest.skip("fixture too small")
    assert bk["max_short_weight"] <= cfg.single_name_weight_cap + 1e-9


def test_net_turnover_much_less_than_gross(edge):
    """★ S2 BLOCK-1 fix: the profitability quintiles are STICKY → NET turnover (L1 Δ of the held book) must be
    far below the OLD per-cohort full-leg (gross) turnover — else the run would false-NULL on inflated cost."""
    bk = harness.profitability_book(edge[0], ForkBConfig(), weighting="EW")
    net = float(bk["turnover_daily"].sum())
    gross = float(bk["turnover_daily_gross"].sum())
    assert gross > 0.0
    assert net < 0.5 * gross, (net, gross)


def test_injected_rmw_edge_recovered_positive(edge):
    bk = harness.profitability_book(edge[0], ForkBConfig(), weighting="EW")
    assert float(bk["daily_raw"][bk["active_mask"]].mean()) > 0.0   # high-prof-long earns the injected drift


def test_active_mask_emitted_and_rolling_is_mostly_active(edge):
    bk = harness.profitability_book(edge[0], ForkBConfig())
    active = int(bk["active_mask"].sum())
    full = len(bk["daily_raw"])
    assert 0 < active <= full
    assert active // ForkBConfig().block_td >= 30          # rolling/continuous → powered (well above the synth's span/63)


def test_net_turnover_helper_persistent_vs_rotating():
    from harness import _net_turnover
    N = 300
    dates = pd.bdate_range("2015-01-01", periods=N)
    cc = np.ones(N)
    persistent = {"A": np.full(N, 0.5), "B": np.full(N, -0.5)}                      # constant book → ~0 turnover
    flip = (np.arange(N) // 10) % 2 == 0
    rotating = {"A": np.where(flip, 0.5, -0.5), "B": np.where(flip, -0.5, 0.5)}     # flips every 10d → high turnover
    tn_p = float(_net_turnover(persistent, cc, dates).sum())
    tn_r = float(_net_turnover(rotating, cc, dates).sum())
    assert tn_p < 1e-9
    assert tn_r > 1.0
    assert tn_r > 1e6 * max(tn_p, 1e-12)


def test_broadcast_asof_staleness_cap():
    """★ S2 freeze-AMEND: a char goes stale N calendar days after its last datekey. cap=None ffills forever;
    cap=400 masks any cell older than 400d (matches the wave-2 screen). The cutoff is datekey + 400d."""
    dates = pd.bdate_range("2010-01-01", "2012-12-31")
    dk = pd.Timestamp("2010-01-04")
    char_df = pd.DataFrame({"ticker": ["A"], "datekey": [dk], "val": [0.3]})
    uncapped = ff._broadcast_asof(char_df, dates, ["A"], max_stale_days=None)
    capped = ff._broadcast_asof(char_df, dates, ["A"], max_stale_days=400)
    assert uncapped["A"][dates >= dk].notna().all()                      # unbounded ffill from the first filing onward
    assert uncapped["A"][dates < dk].isna().all()                        # nothing known before the first datekey
    cutoff = dk + pd.Timedelta(days=400)
    assert capped["A"][(dates >= dk) & (dates <= cutoff)].notna().all()  # fresh within 400d of the datekey
    assert capped["A"][dates > cutoff].isna().all()                      # masked after the cutoff


def test_staleness_cap_incidence_reported(edge):
    """★ S2-AMEND reporting: with the cap on, the book reports how many candidate slots it dropped (so the verdict
    can show the effect was immaterial). Default (cap=None) reports zero dropped."""
    cfg = dataclasses.replace(ForkBConfig(), fundamental_max_stale_days=400)
    bk = harness.profitability_book(edge[0], cfg)
    assert "stale_dropped_frac" in bk and 0.0 <= bk["stale_dropped_frac"] <= 1.0
    assert bk["n_cand_pool_total"] > 0
    bk0 = harness.profitability_book(edge[0], ForkBConfig())
    assert bk0["stale_dropped_total"] == 0                               # no cap -> nothing dropped
    # a tight 30d cap (< the synth's ~90d quarterly filing cadence) MUST drop slots — proves the cap is non-vacuous
    bk_tight = harness.profitability_book(edge[0], dataclasses.replace(ForkBConfig(), fundamental_max_stale_days=30))
    assert bk_tight["stale_dropped_total"] > 0
    assert bk_tight["stale_dropped_frac"] > bk["stale_dropped_frac"]     # tighter cap drops strictly more
