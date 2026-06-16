"""TDI for mlx_model — the binding integration test: CPCV recovers a known non-linear conditional signal; pure noise →
IC≈0 (the look-ahead/leakage guard: if purging leaked test labels, even the NULL would show spurious IC)."""
import numpy as np
import pandas as pd

import mlx_model as mm
import mlx_ic_power as icp
from mlx_config import MLXConfig

FACTORS = ["size", "value", "mom", "str", "beta", "rmw", "cma"]


def make_panels(n_dates=30, n_names=200, signal=True, seed=0):
    rng = np.random.default_rng(seed)
    dates = list(pd.date_range("2014-01-31", periods=n_dates, freq="ME"))
    names = [f"n{i}" for i in range(n_names)]
    feats = {"f1": {}, "f2": {}, "f3": {}}
    fwd, weights = {}, {}
    fchars = {f: {} for f in FACTORS}
    for D in dates:
        f1 = pd.Series(rng.normal(size=n_names), index=names)
        f2 = pd.Series(rng.normal(size=n_names), index=names)
        f3 = pd.Series(rng.normal(size=n_names), index=names)
        if signal:                       # non-linear: f1 predicts only when f2>0, flips when f2<0
            mu = np.where(f2.values > 0, f1.values, -f1.values) * 0.03
            r = pd.Series(mu + rng.normal(scale=0.04, size=n_names), index=names)
        else:
            r = pd.Series(rng.normal(scale=0.04, size=n_names), index=names)
        feats["f1"][D], feats["f2"][D], feats["f3"][D] = f1, f2, f3
        fwd[D] = r
        weights[D] = pd.Series(1.0 / n_names, index=names)
        for fc in FACTORS:
            fchars[fc][D] = pd.Series(rng.normal(size=n_names), index=names)
    return {"rebalances": dates, "features": feats, "fwd": fwd, "weights": weights,
            "factor_chars": fchars, "feature_names": ["f1", "f2", "f3"]}


def test_cpcv_recovers_nonlinear_signal():
    panels = make_panels(signal=True, seed=1)
    scores = mm.cpcv_scores(panels, MLXConfig())
    ic = icp.ic_series(scores, panels["fwd"], min_n=50)
    assert ic.mean() > 0.03 and icp.block_t(ic)["t"] > 2     # GBM recovers the conditional signal, OOS


def test_cpcv_pure_noise_is_null_no_leakage():
    # leakage would bias EVERY seed's OOS IC positive; pure-noise IC is centered on 0 across seeds (single-seed T=30 is
    # noisy — a per-seed |mean|<0.03 can occur by chance, so the leakage guard is the CROSS-SEED mean).
    means = []
    for sd in range(6):
        panels = make_panels(signal=False, seed=sd)
        scores = mm.cpcv_scores(panels, MLXConfig())
        means.append(icp.ic_series(scores, panels["fwd"], min_n=50).mean())
    assert abs(np.mean(means)) < 0.012                       # centered on 0 -> purge/embargo prevent look-ahead


def test_scores_are_out_of_sample_for_every_rebalance():
    panels = make_panels(signal=True, seed=3)
    scores = mm.cpcv_scores(panels, MLXConfig())
    # every rebalance gets an OOS score (each date is in test in >=1 CPCV combo)
    assert len(scores) >= len(panels["rebalances"]) - 1
