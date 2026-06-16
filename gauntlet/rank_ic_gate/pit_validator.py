"""
pit_validator — BACKTEST point-in-time audit.

This validates the historical replay: every signal is available strictly after its own observation
bar (release lag) AND strictly before the forward window it is paired against. Returns a structured
report; `assert_clean` raises on any violation (the gate refuses to run on PIT-undefensible data).

PURE: numpy/pandas, no I/O. Deterministic.
"""
from __future__ import annotations

import numpy as np

__all__ = ["audit_macro_pit", "audit_pairs_pit", "assert_clean"]


def audit_macro_pit(obs_date, available_at) -> dict:
    """available_at must be STRICTLY after the observation bar (release lag > 0) for every row."""
    d = np.asarray(obs_date, dtype="datetime64[ns]")
    a = np.asarray(available_at, dtype="datetime64[ns]")
    if d.shape != a.shape:
        raise ValueError("obs_date and available_at must align")
    bad = a <= d
    return {"check": "available_at_after_obs", "n": int(d.size),
            "n_violations": int(bad.sum()),
            "first_violation": (None if not bad.any() else int(np.argmax(bad)))}


def audit_pairs_pit(available_at, entry_time, signal, fwd_ret) -> dict:
    """Paired data: available_at < entry_time (no look-ahead) AND no NaN in the decisional columns."""
    a = np.asarray(available_at, dtype="datetime64[ns]")
    e = np.asarray(entry_time, dtype="datetime64[ns]")
    s = np.asarray(signal, dtype=float)
    f = np.asarray(fwd_ret, dtype=float)
    look = a >= e
    nan = ~(np.isfinite(s) & np.isfinite(f))
    return {"check": "pairs_pit", "n": int(a.size),
            "n_lookahead": int(look.sum()), "n_nan": int(nan.sum()),
            "first_lookahead": (None if not look.any() else int(np.argmax(look)))}


def assert_clean(*reports: dict) -> None:
    """Raise if any audit report shows a violation."""
    for r in reports:
        bad = r.get("n_violations", 0) + r.get("n_lookahead", 0) + r.get("n_nan", 0)
        if bad:
            raise ValueError(f"PIT audit FAILED ({r.get('check')}): {r}")
