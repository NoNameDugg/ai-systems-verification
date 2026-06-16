"""
rank_ic_calculator — Spearman rank-IC with Fisher-z standard error and tanh CI.

  IC      = Spearman rank correlation of signal_t vs forward return
  SE      = Fisher-z standard error 1/sqrt(n-3)  (the Spearman large-sample SE)
  CI      = tanh back-transform of [z +/- z_crit*SE] to the raw-IC scale

PURE: numpy-only, no I/O, no engine/network coupling. Deterministic.

Design notes:
  - Spearman rho = Pearson correlation of the average-ranked inputs (ties -> average rank,
    matching scipy.stats.rankdata default), implemented in numpy so we carry no scipy dep.
  - The Fisher z-transform z = arctanh(rho) is undefined at |rho|=1 and the SE is undefined
    for n<4 (n-3<=0). Both are handled explicitly (clip for z; NaN SE/CI for n<4) so the
    function never raises on degenerate input -- the *gate* (power_gate / thresholds) decides
    usability via n_pairs >= 50 etc., not this calculator.
"""
from __future__ import annotations

import numpy as np

__all__ = ["rank_ic", "average_rank"]


def average_rank(a: np.ndarray) -> np.ndarray:
    """Average ranks (1-based), ties broken by mean rank (== scipy.stats.rankdata 'average')."""
    a = np.asarray(a, dtype=float)
    n = a.size
    if n == 0:
        return np.empty(0, dtype=float)
    sorter = np.argsort(a, kind="mergesort")
    inv = np.empty(n, dtype=np.intp)
    inv[sorter] = np.arange(n, dtype=np.intp)
    a_sorted = a[sorter]
    obs = np.r_[True, a_sorted[1:] != a_sorted[:-1]]
    dense = obs.cumsum()[inv]                       # 1..#distinct
    count = np.r_[np.nonzero(obs)[0], n]            # boundary indices of each distinct value
    # average 1-based rank for each element's tie-group
    return 0.5 * (count[dense] + count[dense - 1] + 1.0)


def rank_ic(signal, forward_return, *, drop_na: bool = True, z_crit: float = 1.96) -> dict:
    """
    Spearman rank-IC of `signal` vs `forward_return` with Fisher-z SE and tanh CI.

    Returns a dict:
      ic        : float  -- Spearman rank correlation (NaN if undefined / n<2 / zero variance)
      n_pairs   : int    -- number of valid (non-NaN) pairs used
      z_ic      : float  -- arctanh(ic)  (Fisher z), NaN if SE undefined
      se_z      : float  -- 1/sqrt(n-3)  (NaN if n<4)
      ci_low    : float  -- tanh(z_ic - z_crit*se_z)  (raw-IC scale)
      ci_high   : float  -- tanh(z_ic + z_crit*se_z)
      se_ic     : float  -- delta-method SE on the IC scale: se_z*(1-ic**2) (diagnostic only)
    """
    s = np.asarray(signal, dtype=float).ravel()
    f = np.asarray(forward_return, dtype=float).ravel()
    if s.shape != f.shape:
        raise ValueError(f"signal and forward_return must align: {s.shape} vs {f.shape}")

    if drop_na:
        mask = np.isfinite(s) & np.isfinite(f)
        s, f = s[mask], f[mask]

    n = int(s.size)
    out = {"ic": float("nan"), "n_pairs": n, "z_ic": float("nan"),
           "se_z": float("nan"), "ci_low": float("nan"), "ci_high": float("nan"),
           "se_ic": float("nan")}
    if n < 2:
        return out

    rs, rf = average_rank(s), average_rank(f)
    # zero variance in either ranking (all ties) -> correlation undefined
    if np.std(rs) == 0.0 or np.std(rf) == 0.0:
        return out

    ic = float(np.corrcoef(rs, rf)[0, 1])
    out["ic"] = ic

    if n >= 4:
        se_z = 1.0 / np.sqrt(n - 3.0)
        ic_clip = float(np.clip(ic, -1.0 + 1e-12, 1.0 - 1e-12))
        z_ic = float(np.arctanh(ic_clip))
        out["se_z"] = float(se_z)
        out["z_ic"] = z_ic
        out["ci_low"] = float(np.tanh(z_ic - z_crit * se_z))
        out["ci_high"] = float(np.tanh(z_ic + z_crit * se_z))
        out["se_ic"] = float(se_z * (1.0 - ic_clip ** 2))
    return out
