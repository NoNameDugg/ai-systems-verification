"""Fractional differencing — López-de-Prado fixed-width-window (FFD). PURE numpy/pandas, deterministic.

Stationarize a series while preserving memory. d is selected as the MINIMUM order that makes the series stationary
(ADF t-stat < the 5% critical value) — selected on the TRAIN partition only (NIT-1, no look-ahead). Dependency-free ADF
(numpy OLS vs the MacKinnon-constant 5% critical value ≈ −2.86); statsmodels is not installed here.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

ADF_CRIT_5PCT = -2.86      # MacKinnon 5% critical value, constant, large-n


def ffd_weights(d: float, tau: float = 1e-4, max_k: int = 10_000) -> np.ndarray:
    """FFD weights w_k (w_0=1, w_k = -w_{k-1}·(d-k+1)/k), truncated when |w_k| < tau. Index 0 = most recent obs."""
    w = [1.0]
    for k in range(1, max_k + 1):
        wk = -w[-1] * (d - k + 1) / k
        if abs(wk) < tau:
            break
        w.append(wk)
    return np.asarray(w, dtype=float)


def frac_diff_ffd(series: pd.Series, d: float, tau: float = 1e-4) -> pd.Series:
    """FFD_t = Σ_k w_k · y_{t-k}. NaN for the first (len(w)-1) points. d=0 → unchanged; d=1 → first difference."""
    w = ffd_weights(d, tau)
    y = series.astype(float).values
    K = len(w)
    out = np.full(len(y), np.nan)
    for t in range(K - 1, len(y)):
        window = y[t - K + 1: t + 1][::-1]      # [y_t, y_{t-1}, ..., y_{t-K+1}]
        if np.isnan(window).any():
            continue
        out[t] = float(np.dot(w, window))
    return pd.Series(out, index=series.index)


def adf_tstat(x, maxlag: int = 1) -> float:
    """ADF t-stat on the lagged-level coefficient (regression with constant + `maxlag` lagged differences). numpy OLS."""
    y = np.asarray(x, dtype=float)
    y = y[np.isfinite(y)]
    n = len(y)
    if n < maxlag + 10:
        return float("nan")
    dy = np.diff(y)                              # Δy_t
    ylag = y[:-1]                                # y_{t-1}
    # build lagged Δy regressors
    rows = len(dy) - maxlag
    if rows < 10:
        return float("nan")
    Z = [np.ones(rows), ylag[maxlag:]]          # const, y_{t-1}
    for L in range(1, maxlag + 1):
        Z.append(dy[maxlag - L: -L] if L != 0 else dy)
    X = np.column_stack(Z)
    Y = dy[maxlag:]
    beta, *_ = np.linalg.lstsq(X, Y, rcond=None)
    resid = Y - X @ beta
    dof = max(rows - X.shape[1], 1)
    sigma2 = float(resid @ resid) / dof
    xtx_inv = np.linalg.pinv(X.T @ X)
    se_beta1 = float(np.sqrt(sigma2 * xtx_inv[1, 1]))
    return float(beta[1] / se_beta1) if se_beta1 > 0 else float("nan")


def is_stationary(series: pd.Series, maxlag: int = 1, crit: float = ADF_CRIT_5PCT) -> bool:
    t = adf_tstat(series.dropna().values, maxlag=maxlag)
    return bool(np.isfinite(t) and t < crit)


def min_ffd_order(series: pd.Series, adf_crit: float = ADF_CRIT_5PCT, tau: float = 1e-4,
                  d_grid=None, maxlag: int = 1, d_max: float = 1.0) -> float:
    """Smallest d in d_grid s.t. the FFD'd series is stationary (ADF t < crit). Returns d_max if none pass.
    ★ Call on the TRAIN slice only — the returned d is then frozen and applied to the full series."""
    if d_grid is None:
        d_grid = [round(x, 2) for x in np.arange(0.0, d_max + 1e-9, 0.1)]
    for d in d_grid:
        fd = frac_diff_ffd(series, float(d), tau=tau).dropna()
        if len(fd) < maxlag + 10:
            continue
        if is_stationary(fd, maxlag=maxlag, crit=adf_crit):
            return float(d)
    return float(d_max)
