"""Event-study core: 2-factor market model, abnormal returns, CAR over the traded window, BMP
cross-sectional statistic.

Pure / numpy / deterministic. The market model is an always-alive 2-factor model (zero
survivorship-in-control). Betas estimated by OLS on the DISJOINT estimation window [-150,-30];
CAR scored on the INVESTABLE hold (-10 -> 0] (gate window == trade window). CAR is long-basis
abnormal return; if a thesis predicts CAR < 0 (short profit = -CAR), the gate tests "real CAR
more negative than placebo," so the sign lives in the gate, not here.
"""
from __future__ import annotations

import numpy as np

EST_PRE, EST_POST = 150, 30     # estimation window [-150, -30)
HOLD_ENTRY, HOLD_COVER = -10, 0  # short entry at -10 close, cover at 0 close


def ols_market_model(y: np.ndarray, factors: np.ndarray):
    """OLS y ~ 1 + factors (factors: (n,k)). Returns (alpha, betas:(k,)). NaN rows dropped pairwise.
    Returns (nan, nan-vector) if <5 clean rows or singular."""
    F = np.atleast_2d(factors)
    if F.shape[0] != y.shape[0]:
        F = F.T
    m = np.isfinite(y) & np.all(np.isfinite(F), axis=1)
    if m.sum() < 5:
        return float("nan"), np.full(F.shape[1], np.nan)
    X = np.column_stack([np.ones(m.sum()), F[m]])
    try:
        coef, *_ = np.linalg.lstsq(X, y[m], rcond=None)
    except Exception:
        return float("nan"), np.full(F.shape[1], np.nan)
    return float(coef[0]), coef[1:]


def abnormal_returns(tok_ret: np.ndarray, factors: np.ndarray, alpha: float, betas: np.ndarray) -> np.ndarray:
    """AR_t = r_token,t - (alpha + betas . factors_t)."""
    F = np.atleast_2d(factors)
    if F.shape[0] != tok_ret.shape[0]:
        F = F.T
    pred = alpha + F @ betas
    return tok_ret - pred


def _returns(close: np.ndarray) -> np.ndarray:
    close = np.asarray(close, dtype="float64")
    r = np.full_like(close, np.nan)
    r[1:] = close[1:] / close[:-1] - 1.0
    return r


def event_car(tok_close: np.ndarray, factor_closes: np.ndarray, t0_pos: int, *,
              est_pre: int = EST_PRE, est_post: int = EST_POST,
              entry: int = HOLD_ENTRY, cover: int = HOLD_COVER):
    """CAR for ONE event. factor_closes: (n,k) close matrix (BTC, ETH). t0_pos = index position of t=0.
    Estimate betas on [t0-est_pre, t0-est_post); AR over the captured hold offsets (entry, cover] (for a short
    held entry->cover, the realized abnormal moves are days entry+1..cover); CAR = sum of those AR.
    Returns dict(car, alpha, betas, n_est, ok)."""
    tok_close = np.asarray(tok_close, dtype="float64")
    F = np.atleast_2d(factor_closes)
    if F.shape[0] != tok_close.shape[0]:
        F = F.T
    n = len(tok_close)
    lo, hi = t0_pos - est_pre, t0_pos - est_post
    if lo < 1 or t0_pos + cover >= n or hi <= lo:
        return {"car": float("nan"), "alpha": float("nan"), "betas": None, "n_est": 0, "ok": False}
    tok_ret = _returns(tok_close)
    fac_ret = np.column_stack([_returns(F[:, j]) for j in range(F.shape[1])])
    alpha, betas = ols_market_model(tok_ret[lo:hi], fac_ret[lo:hi])
    if not np.isfinite(alpha) or not np.all(np.isfinite(betas)):
        return {"car": float("nan"), "alpha": alpha, "betas": betas, "n_est": int(hi - lo), "ok": False}
    ar = abnormal_returns(tok_ret, fac_ret, alpha, betas)
    cap = ar[t0_pos + entry + 1: t0_pos + cover + 1]      # (entry, cover]
    if not np.all(np.isfinite(cap)):
        return {"car": float("nan"), "alpha": alpha, "betas": betas, "n_est": int(hi - lo), "ok": False}
    return {"car": float(np.sum(cap)), "alpha": alpha, "betas": betas, "n_est": int(hi - lo), "ok": True,
            "ar_est_sd": float(np.nanstd(ar[lo:hi], ddof=1)), "n_cap": int(len(cap))}


def aggregate_car(cars) -> float:
    """Mean CAR across events (the GATING aggregate statistic, tested vs placebo)."""
    a = np.asarray([c for c in cars if np.isfinite(c)], dtype="float64")
    return float(a.mean()) if len(a) else float("nan")


def bmp_statistic(event_results) -> float:
    """Boehmer-Musumeci-Poulsen standardized cross-sectional t (SUPPORTING, non-gating). Standardize each
    event's CAR by its estimation-window AR sd scaled to the capture length, then cross-sectional t of the
    SCARs. Robust to event-induced variance."""
    scar = []
    for r in event_results:
        if not r.get("ok") or not np.isfinite(r.get("car", np.nan)):
            continue
        sd = r.get("ar_est_sd", np.nan)
        ncap = r.get("n_cap", 0)
        if np.isfinite(sd) and sd > 0 and ncap > 0:
            scar.append(r["car"] / (sd * np.sqrt(ncap)))
    s = np.asarray(scar, dtype="float64")
    if len(s) < 2 or s.std(ddof=1) == 0:
        return float("nan")
    return float(s.mean() / (s.std(ddof=1) / np.sqrt(len(s))))
