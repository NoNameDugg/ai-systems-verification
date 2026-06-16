"""Shallow, regularized GBM ranker scored under t1-aware panel CPCV. HistGBM is NaN-native (no imputation).

build_long  : panels -> long (date,name) matrix with features + per-date percentile-rank target y.
cpcv_scores : run CPCV (mlx_cpcv) over the rebalance grid; for each split fit on train rebalances, predict test
              rebalances; average OOS predictions per (date,name) across the combos -> {date: score Series}. No look-ahead
              (t1 purge + symmetric embargo on the rebalance grid).
"""
from __future__ import annotations

import numpy as np
import pandas as pd
from sklearn.ensemble import HistGradientBoostingRegressor

import mlx_cpcv as cv


def build_long(panels: dict) -> pd.DataFrame:
    feats, fwd, names = panels["features"], panels["fwd"], panels["feature_names"]
    frames = []
    for D in panels["rebalances"]:
        df = pd.DataFrame({f: feats[f][D] for f in names})
        df["fwd"] = fwd[D]
        df = df.dropna(subset=["fwd"])
        if len(df) < 2:
            continue
        df["y"] = df["fwd"].rank(pct=True)                  # per-date cross-sectional percentile rank target
        df["date"] = D
        df["name"] = df.index
        frames.append(df.reset_index(drop=True))
    return pd.concat(frames, ignore_index=True)


def cpcv_scores(panels: dict, cfg) -> dict:
    long = build_long(panels)
    reb = list(panels["rebalances"])
    didx = {d: i for i, d in enumerate(reb)}
    di = long["date"].map(didx).values
    feat_cols = panels["feature_names"]
    X = long[feat_cols].to_numpy(dtype=float)
    y = long["y"].to_numpy(dtype=float)

    purge = max(1, round(cfg.horizon_td / 21))
    embargo = max(1, round(cfg.cpcv_embargo_td / 21))
    pred_sum = np.zeros(len(long))
    pred_cnt = np.zeros(len(long))
    for tr_g, te_g in cv.cpcv_splits(len(reb), cfg.cpcv_n_groups, cfg.cpcv_k_test, purge, embargo):
        tr_mask = np.isin(di, tr_g)
        te_mask = np.isin(di, te_g)
        if tr_mask.sum() < 500 or te_mask.sum() < 50:
            continue
        m = HistGradientBoostingRegressor(
            max_depth=cfg.gbm_max_depth, max_iter=cfg.gbm_n_estimators, learning_rate=cfg.gbm_learning_rate,
            min_samples_leaf=cfg.gbm_min_samples_leaf, l2_regularization=cfg.gbm_l2,
            random_state=cfg.seed, early_stopping=False)
        m.fit(X[tr_mask], y[tr_mask])
        idx = np.where(te_mask)[0]
        pred_sum[idx] += m.predict(X[te_mask])
        pred_cnt[idx] += 1
    long["score"] = np.where(pred_cnt > 0, pred_sum / np.maximum(pred_cnt, 1), np.nan)
    scored = long.dropna(subset=["score"])
    return {D: pd.Series(g["score"].values, index=g["name"].values) for D, g in scored.groupby("date")}
