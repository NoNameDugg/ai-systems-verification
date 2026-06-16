"""
deflation — Holm step-down family-wise correction over per-cell permutation p-values.

Holm is the family-wise correction used in the gate. (The DSR / deflated-Sharpe is REPORTED-ONLY
via the selection_stats module -- NOT a gate, and NOT here.)

At N=1 (a single decisional cell) Holm is VACUOUS -- the (m-k+1) factor is 1, so the adjusted p
reduces to the raw permutation p. The apparatus earns its keep on a multi-signal follow-on.

PURE: numpy-only, no I/O. Deterministic.
"""
from __future__ import annotations

import numpy as np

__all__ = ["holm_adjusted_pvalues", "holm_reject", "holm"]


def _validate(p: np.ndarray) -> np.ndarray:
    p = np.asarray(p, dtype=float).ravel()
    if p.size and (not np.all(np.isfinite(p)) or p.min() < 0.0 or p.max() > 1.0):
        raise ValueError("p-values must be finite and in [0, 1]")
    return p


def holm_adjusted_pvalues(pvalues) -> np.ndarray:
    """
    Holm-Bonferroni step-down adjusted p-values (monotone, in the INPUT order).

    For the k-th smallest raw p (1-indexed k over m tests): factor = (m - k + 1);
    adjusted = cumulative max of min(1, factor * p_(k))  (enforces monotonicity).
    """
    p = _validate(pvalues)
    m = p.size
    if m == 0:
        return np.empty(0, dtype=float)
    order = np.argsort(p, kind="mergesort")
    p_sorted = p[order]
    adj_sorted = np.empty(m, dtype=float)
    running = 0.0
    for k in range(m):
        val = min((m - k) * p_sorted[k], 1.0)   # k is 0-indexed -> factor (m-k) == (m-(k+1)+1)
        running = max(running, val)
        adj_sorted[k] = running
    adj = np.empty(m, dtype=float)
    adj[order] = adj_sorted
    return adj


def holm_reject(pvalues, alpha: float = 0.05) -> np.ndarray:
    """Boolean reject vector at family-wise alpha (input order)."""
    return holm_adjusted_pvalues(pvalues) <= alpha


def holm(pvalues, alpha: float = 0.05) -> list[dict]:
    """Convenience: per-cell {p_raw, p_holm, reject} in input order."""
    p = _validate(pvalues)
    adj = holm_adjusted_pvalues(p)
    rej = adj <= alpha
    return [{"p_raw": float(p[i]), "p_holm": float(adj[i]), "reject": bool(rej[i])}
            for i in range(p.size)]
