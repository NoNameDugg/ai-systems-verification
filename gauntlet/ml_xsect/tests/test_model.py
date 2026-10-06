"""TDI for mlx_model — the binding integration test: CPCV recovers a known non-linear conditional signal; pure noise →
IC≈0; and the purge/embargo guard is tested against a fixture where leakage is POSSIBLE (overlapping label windows +
persistent features), so removing the purge measurably inflates the out-of-sample IC. The pre-v1.2 "leakage" test used
i.i.d. per-date features and one-period labels, where nothing can leak, so it passed identically with the purge
disabled (external review 2026-10, finding #2)."""
import numpy as np
import pandas as pd

import mlx_cpcv as cv
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


def test_cpcv_pure_noise_is_null():
    # pure i.i.d. noise: the OOS IC is centered on 0 across seeds. (This is a sanity check, NOT a leakage test: with
    # i.i.d. per-date features and one-period labels nothing CAN leak — see test_purge_removes_the_leak_it_is_for.)
    means = []
    for sd in range(6):
        panels = make_panels(signal=False, seed=sd)
        scores = mm.cpcv_scores(panels, MLXConfig())
        means.append(icp.ic_series(scores, panels["fwd"], min_n=50).mean())
    assert abs(np.mean(means)) < 0.012


def make_leaky_panels(n_dates=36, n_names=120, seed=0, rho=0.999, overlap=3):
    """Pure-noise labels that CAN leak across dates: each date's label is the sum of the next `overlap` period shocks
    (overlapping label windows), and features are near-static per name (rho), so a model trained on a neighbouring
    date can memorise name -> shared future shock. With purge >= overlap the shared shocks never cross the split."""
    rng = np.random.default_rng(seed)
    dates = list(pd.date_range("2014-01-31", periods=n_dates, freq="ME"))
    names = [f"n{i}" for i in range(n_names)]
    feats = {"f1": {}, "f2": {}, "f3": {}}
    fwd, weights = {}, {}
    fchars = {f: {} for f in FACTORS}
    e = rng.normal(scale=0.03, size=(n_dates + overlap, n_names))
    f = rng.normal(size=(3, n_names))
    for i, D in enumerate(dates):
        f = rho * f + np.sqrt(1 - rho ** 2) * rng.normal(size=(3, n_names))
        for k, nm in enumerate(("f1", "f2", "f3")):
            feats[nm][D] = pd.Series(f[k], index=names)
        fwd[D] = pd.Series(e[i:i + overlap].sum(axis=0), index=names)
        weights[D] = pd.Series(1.0 / n_names, index=names)
        for fc in FACTORS:
            fchars[fc][D] = pd.Series(rng.normal(size=n_names), index=names)
    return {"rebalances": dates, "features": feats, "fwd": fwd, "weights": weights,
            "factor_chars": fchars, "feature_names": ["f1", "f2", "f3"]}


# a 3-step label overlap (horizon 63 td -> purge = round(63/21) = 3 steps) and a model flexible enough to memorise
LEAKY_CFG = MLXConfig(horizon_td=63, gbm_min_samples_leaf=20, gbm_max_depth=6)


def _mean_noise_ic(seeds=6):
    means = []
    for sd in range(seeds):
        panels = make_leaky_panels(seed=sd)
        scores = mm.cpcv_scores(panels, LEAKY_CFG)
        means.append(icp.ic_series(scores, panels["fwd"], min_n=50).mean())
    return np.asarray(means)


def test_purge_removes_the_leak_it_is_for(monkeypatch):
    # (a) with the purge/embargo derived from the horizon, the overlapping-label noise fixture is NULL
    purged = _mean_noise_ic()
    assert abs(purged.mean()) < 0.03, purged

    # (b) the SAME fixture and seeds with purge = embargo = 0 leak: the OOS IC inflates on (nearly) every seed
    orig = cv.cpcv_splits
    monkeypatch.setattr(cv, "cpcv_splits", lambda n, g, k, purge, embargo: orig(n, g, k, 0, 0))
    unpurged = _mean_noise_ic()
    paired = unpurged - purged
    assert paired.mean() > 0.02, (purged, unpurged)            # probe on 2026-10-05: +0.051 (per-seed 0.03..0.07)
    assert (paired > 0).sum() >= 5, paired                     # leak shows on at least 5 of 6 seeds


def test_scores_are_out_of_sample_for_every_rebalance():
    panels = make_panels(signal=True, seed=3)
    scores = mm.cpcv_scores(panels, MLXConfig())
    # every rebalance gets an OOS score (each date is in test in >=1 CPCV combo)
    assert len(scores) >= len(panels["rebalances"]) - 1
