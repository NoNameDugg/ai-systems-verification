"""TDI for harness helpers — the single-name weight cap (breadth restoration; Phase-0 path-B proposal)."""
import numpy as np
import pandas as pd

from _schema import ForkBConfig
from synth_fixtures import small
import harness
from harness import _cap_weights


def test_tr_panel_clips_extreme_returns_and_bounds_terminal():
    """DEC-330 contamination guard: a reissue/Q-stub-style closeadj discontinuity (+4900% one-day) is clipped to
    +-tr_clip_daily in the BOOK panel; and the delist terminal-value override is bounded to [-1,+1] even un-clipped
    (the raw terminal_value/last_close-1 blows up on penny denominators)."""
    snap, _ = small()
    sep = snap.sep.sort_values(["ticker", "date"], kind="mergesort").reset_index(drop=True)
    t = sep["ticker"].iloc[0]
    idx = sep[sep["ticker"] == t].index[len(sep[sep["ticker"] == t]) // 2]
    sep.loc[idx, "closeadj"] = sep.loc[idx, "closeadj"] * 50.0                 # +4900% one-day discontinuity
    snap.sep = sep
    # inject a blow-up delist (huge terminal value on a name) -> raw override would be astronomical
    t2 = sep["ticker"].unique()[1]
    last_d = sep.loc[sep["ticker"] == t2, "date"].max()
    snap.actions = pd.concat([snap.actions, pd.DataFrame(
        [{"ticker": t2, "date": last_d, "action": "delisted", "value": 1e9, "contraticker": None}])], ignore_index=True)

    clipped = harness.tr_panel(snap, ForkBConfig(tr_clip_daily=0.50))
    assert float(clipped.abs().max().max()) <= 0.50 + 1e-9, "all book TR must be clipped to +-0.50"

    unclipped = harness.tr_panel(snap, ForkBConfig(tr_clip_daily=None))
    assert float(unclipped[t].abs().max()) > 1.0, "without the clip the +4900% discontinuity is present"
    assert float(unclipped[t2].max()) <= 1.0 + 1e-9, "the delist terminal override is bounded to <= +1 even un-clipped"


def test_cap_weights_caps_redistributes_and_restores_breadth():
    # one 90%-weight mega name + 29 tiny names (a VW-concentrated leg)
    w = pd.Series([0.90] + [0.10 / 29] * 29, index=[f"N{i}" for i in range(30)])
    capped = _cap_weights(w, 0.05)
    assert capped.max() <= 0.05 + 1e-9, f"max {capped.max()} exceeds the cap"
    assert abs(capped.sum() - 1.0) < 1e-9, "capped weights must sum to 1"
    eff_before = 1.0 / float((w ** 2).sum())
    eff_after = 1.0 / float((capped ** 2).sum())
    assert eff_after > 5 * eff_before, f"cap must restore breadth (eff {eff_before:.1f} -> {eff_after:.1f})"


def test_cap_weights_infeasible_cap_returns_equal_weight():
    # 4 names, cap*n = 0.05*4 = 0.2 < 1 -> cannot sum to 1 under the cap -> equal-weight is the closest feasible
    w = pd.Series([0.90, 0.05, 0.03, 0.02], index=list("abcd"))
    capped = _cap_weights(w, 0.05)
    assert np.allclose(capped.values, 0.25)


def test_cap_weights_noop_when_all_already_under_cap():
    w = pd.Series([1.0 / 30] * 30, index=[f"N{i}" for i in range(30)])   # EW already < 0.05
    capped = _cap_weights(w, 0.05)
    assert np.allclose(capped.values, w.values)
