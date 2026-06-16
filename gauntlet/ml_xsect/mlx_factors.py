"""Construction-matched factor returns + alpha-retained residual + char-space cross-check. PURE.

The make-or-break orthogonality apparatus, built so the factor returns use the SAME binning/weighting as the ML book
(quintile / VW) — NOT EW-tercile interpolated returns. Two reads:
  - returns-space      : regress the book's realized L-S spread on the construction-matched 7-factor L-S spreads, keep the
                         intercept = alpha (alpha-retained). _known flags |rmw_beta| OR |cma_beta| OR explained_frac
                         (the known-factor test checks rmw_beta, not just cma_beta).
  - char-space        : per rebalance, residualize the ML score on the 7 characteristics cross-sectionally; the residual
                         score's incremental rank-IC = signal beyond the linear characteristic combination (robust to L-S
                         construction mismatch; diagnoses the trained score post-hoc, does NOT neutralize the label).
"""
from __future__ import annotations

import numpy as np
import pandas as pd


def quantile_ls_return(char: pd.Series, fwd: pd.Series, weight: pd.Series | None, q: int = 5) -> float:
    """Top-minus-bottom q-quantile spread of `fwd`, ranked by `char`, weighted by `weight` (VW; None → EW). One rebalance."""
    df = pd.concat([char.rename("c"), fwd.rename("r")], axis=1).dropna()
    if weight is not None:
        df = df.join(weight.rename("w")).dropna()
    else:
        df["w"] = 1.0
    if len(df) < 2 * q:
        return float("nan")
    ranks = df["c"].rank(method="first")
    lo = ranks <= len(df) / q                       # bottom quintile
    hi = ranks > len(df) * (q - 1) / q              # top quintile
    def wmean(m):
        w = df.loc[m, "w"]
        return float(np.average(df.loc[m, "r"], weights=w)) if w.sum() > 0 else float("nan")
    return wmean(hi) - wmean(lo)


def ls_return_series(char_by_date: dict, fwd_by_date: dict, weight_by_date: dict | None, q: int = 5) -> pd.Series:
    """{date → top-minus-bottom q-quantile spread}. Used for BOTH the ML book (rank by score) and each factor."""
    out = {}
    for d, ch in char_by_date.items():
        if d not in fwd_by_date:
            continue
        w = (weight_by_date or {}).get(d)
        v = quantile_ls_return(ch, fwd_by_date[d], w, q=q)
        if np.isfinite(v):
            out[d] = v
    return pd.Series(out, dtype=float).sort_index()


def rt_a_alpha(book_ret: pd.Series, factor_df: pd.DataFrame) -> dict:
    """RT-A: regress book_ret on the factor returns, KEEP the intercept (alpha retained). resid = book - factors@betas.
    Returns alpha (mean retained = intercept), betas (dict), explained_frac, resid (Series). NEVER subtract alpha."""
    df = pd.concat([book_ret.rename("y"), factor_df], axis=1).dropna()
    if len(df) < factor_df.shape[1] + 5:
        return {"alpha": float("nan"), "betas": {}, "explained_frac": float("nan"), "resid": pd.Series(dtype=float)}
    Y = df["y"].values
    F = df.drop(columns=["y"])
    X = np.column_stack([np.ones(len(df)), F.values])
    beta, *_ = np.linalg.lstsq(X, Y, rcond=None)
    intercept = float(beta[0])
    betas = {c: float(b) for c, b in zip(F.columns, beta[1:])}
    fitted_factor_only = F.values @ beta[1:]                 # exposure component (no intercept)
    resid = Y - (X @ beta)                                    # epsilon
    alpha_series = intercept + resid                          # alpha RETAINED: mean(alpha_series) == intercept
    var_y = float(np.var(Y, ddof=1))
    explained_frac = float(np.var(fitted_factor_only, ddof=1) / var_y) if var_y > 0 else float("nan")
    return {"alpha": intercept, "betas": betas, "explained_frac": explained_frac,
            "resid": pd.Series(alpha_series, index=df.index)}


def known_factor_flag(betas: dict, explained_frac: float, rmw_thr: float = 0.5, cma_thr: float = 0.5,
                      explained_thr: float = 0.5) -> bool:
    """KNOWN-FACTOR (construction-suspect) iff |rmw_beta| OR |cma_beta| exceeds thr OR explained_frac > thr.
    ★ v4-verify: BH tests BOTH rmw_beta AND cma_beta (the apparatus _known tested cma only)."""
    rb, cb = abs(betas.get("rmw", 0.0)), abs(betas.get("cma", 0.0))
    return bool(rb > rmw_thr or cb > cma_thr or (np.isfinite(explained_frac) and explained_frac > explained_thr))


def char_space_incremental_ic(score_by_date: dict, char_panels: dict, fwd_by_date: dict, min_n: int = 50) -> pd.Series:
    """Per rebalance: residualize the ML score on the 7 characteristics (cross-sectional OLS), then IC(resid_score, fwd).
    char_panels: {factor_name → {date → Series}}; the incremental rank-IC = signal beyond the linear char combination."""
    import mlx_ic_power as icp
    out = {}
    for d, sc in score_by_date.items():
        if d not in fwd_by_date:
            continue
        cols = {f: panels[d] for f, panels in char_panels.items() if d in panels}
        if not cols:
            continue
        df = pd.concat([sc.rename("score")] + [v.rename(f) for f, v in cols.items()], axis=1).dropna()
        if len(df) < min_n:
            continue
        X = np.column_stack([np.ones(len(df)), df.drop(columns=["score"]).values])
        b, *_ = np.linalg.lstsq(X, df["score"].values, rcond=None)
        resid_score = pd.Series(df["score"].values - X @ b, index=df.index)
        ic = icp.cross_sectional_ic(resid_score, fwd_by_date[d].reindex(df.index), min_n=min_n)
        if np.isfinite(ic):
            out[d] = ic
    return pd.Series(out, dtype=float).sort_index()
