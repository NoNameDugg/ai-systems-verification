"""TDI for the S2 freeze-item folds (D + E-1):
  D   — Canary part (ii): the external-anchor reconcile-and-HALT + the >=1-ordinary AND >=1-special span.
  E-1 — cost: deployable-grade REFUSES the CS×1.75 placeholder (requires a fitted decile multiplier).
"""
import dataclasses

import pytest

from _schema import ForkBConfig
from synth_fixtures import small
import tr_canary
import cost as cost_mod


def _anchor(tr, t, lo_i, hi_i):
    g = tr[tr["ticker"] == t].dropna(subset=["tr"]).sort_values("date")
    start, end = g["date"].iloc[lo_i], g["date"].iloc[hi_i]
    span = g[(g["date"] >= start) & (g["date"] <= end)]
    return start, end, float((1.0 + span["tr"]).prod() - 1.0)


def test_canary_external_reconcile_and_span_halt():            # S2 D
    snap, _ = small()
    cfg0 = ForkBConfig()
    # empty anchors (pre-Phase-0) -> UNRESOLVED, not a HALT (freeze_ready blocks the freeze instead)
    assert tr_canary.run_canary_external(snap, cfg0)["status"] == "UNRESOLVED"

    tr = tr_canary.build_tr(snap, cfg0)
    ts = list(tr["ticker"].unique()[:3])
    a = [_anchor(tr, ts[0], 1, 40), _anchor(tr, ts[1], 1, 30), _anchor(tr, ts[2], 1, 25)]
    good = ((ts[0], *a[0], "ordinary"), (ts[1], *a[1], "special"), (ts[2], *a[2], "ordinary"))
    cfg = dataclasses.replace(cfg0, PHASE0_canary_external_anchor=good)
    res = tr_canary.run_canary_external(snap, cfg)
    assert res["status"] == "PASS" and res["halt"] is False and res["span_ok"]          # matching + spanning -> PASS

    # a breached anchor (known TR off by 50%) -> HALT (the common-mode guard bites)
    bad = ((ts[0], a[0][0], a[0][1], a[0][2] + 0.5, "ordinary"), (ts[1], *a[1], "special"), (ts[2], *a[2], "ordinary"))
    assert tr_canary.run_canary_external(snap, dataclasses.replace(cfg0, PHASE0_canary_external_anchor=bad))["halt"]

    # a non-spanning set (all ordinary) -> HALT (special-div handling is where the common-mode error hides)
    allord = ((ts[0], *a[0], "ordinary"), (ts[1], *a[1], "ordinary"), (ts[2], *a[2], "ordinary"))
    r = tr_canary.run_canary_external(snap, dataclasses.replace(cfg0, PHASE0_canary_external_anchor=allord))
    assert r["halt"] and r["span_ok"] is False

    # freeze_ready: empty -> not ready; a resolved (spanning) anchor + closeadj path -> ready
    assert cfg0.freeze_ready()[0] is False
    assert dataclasses.replace(cfg, PHASE0_closeadj_is_total_return=True).freeze_ready()[0] is True


def test_cost_calibration_required_for_deployable_grade():     # S2 E-1
    snap, _ = small()
    cfg = ForkBConfig()
    band = cost_mod.conservative_band_bps(snap.sep, snap.daily, cfg)        # placeholder OK in build/synthetic mode
    assert len(band) > 0 and band.notna().any()
    with pytest.raises(ValueError):                                        # deployable-grade refuses the placeholder
        cost_mod.conservative_band_bps(snap.sep, snap.daily, cfg, require_calibration=True)
    calib = [3.0, 2.5, 2.0, 1.8, 1.6, 1.5, 1.4, 1.3, 1.2, 1.1]             # decile 0 illiquid (high) .. 9 liquid (low)
    band_c = cost_mod.conservative_band_bps(snap.sep, snap.daily, cfg, calibration_set=calib, require_calibration=True)
    assert (band_c >= band - 1e-9).all()                                   # the fitted illiquid-tail multiplier never lowers
    assert band_c.sum() > band.sum()                                       # and it lifts the illiquid tail (calibration matters)
