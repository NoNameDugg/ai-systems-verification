"""
lag_sensitivity — does the effect survive being told about the signal one bar late?

A cheap, blunt look-ahead probe. Take the headline statistic (Spearman rank-IC of signal vs forward
return, paired point-in-time by `signal_return_pairer`), then recompute it with every signal's
`available_at` pushed back so that the entry bar moves k bars later (k = 1, 2 by default), and
report the share of the lag-0 statistic that survives each delay.

Why it works: a signal that secretly contains the forward return it is being scored against (a
same-bar close, a mis-stamped release, an off-by-one window) is perfectly informative at lag 0 and
worthless one bar later -- the leaked information has already been "used up". A genuine signal with
any persistence degrades gracefully instead: a slow-moving spread, trend, or fundamental still says
roughly the same thing one bar later.

Verdicts:
  LOOKAHEAD-SUSPECT  lag-0 rank-IC is distinguishable from zero (Fisher-z CI excludes 0) AND one bar
                     of delay removes more than `collapse_share` of it (default 70%).
  ROBUST             lag-0 effect present and the lag-1 statistic keeps >= (1 - collapse_share) of it.
  NO-EFFECT          lag-0 CI covers zero; the ratio is noise over noise and is deliberately NOT read.

Honest limits, stated up front:
  - This is a heuristic, not a proof. A real edge that is fully exhausted within one bar (a pure
    next-bar reversal, say) is indistinguishable from look-ahead by this test. SUSPECT means "go and
    check the timestamps", not "guilty".
  - It is only as good as the pairing it sits on: `signal_return_pairer` already guarantees entry is
    strictly after `available_at`, so what this catches is leakage *inside* the signal's values, which
    no timestamp audit can see.

PURE: numpy-only, no I/O. Deterministic.
"""
from __future__ import annotations

import numpy as np

from rank_ic_calculator import rank_ic
from signal_return_pairer import pair_signal_forward_return

__all__ = ["delay_available_at", "lag_sensitivity"]


def delay_available_at(available_at, price_time, k: int) -> np.ndarray:
    """
    Push every `available_at` back so its entry bar moves exactly k bars later.

    The pairer enters at the first price bar STRICTLY after available_at (index e). Returning
    price_time[e + k - 1] makes the first bar strictly after it index e + k. Observations whose
    delayed entry would fall past the end of the price history get available_at = the last bar, which
    the pairer then drops (no bar strictly after) rather than raising.
    """
    k = int(k)
    if k < 1:
        raise ValueError("k must be >= 1")
    aa = np.asarray(available_at, dtype="datetime64[ns]")
    pt = np.asarray(price_time, dtype="datetime64[ns]")
    if pt.size == 0:
        raise ValueError("price_time must be non-empty")
    entry_idx = np.searchsorted(pt, aa, side="right")
    target = np.minimum(entry_idx + k - 1, pt.size - 1)
    return pt[target]


def lag_sensitivity(signal_time, signal_value, available_at, price_time, price, H, *,
                    lags=(1, 2), collapse_share: float = 0.70, log_return: bool = True,
                    max_entry_gap_days: float | None = None) -> dict:
    """
    Rank-IC at lag 0 and at each extra delay in `lags`, the retained share, and a verdict.

    `max_entry_gap_days` is passed straight to the pairer for every lag (default None: the delayed
    available_at is by construction one bar before its entry, so the freshness guard would only bite
    on the lag-0 run; pass the guard you use elsewhere if your lag-0 data needs it).

    Returns dict:
      verdict        : "LOOKAHEAD-SUSPECT" | "ROBUST" | "NO-EFFECT"
      effect_present : bool  -- lag-0 Fisher-z CI excludes zero
      ic_lag0, n_pairs_lag0, ci_low_lag0, ci_high_lag0
      lags           : list of {lag, ic, n_pairs, retained_share}  (retained_share = ic_k / ic_0)
      collapse_share : the threshold used
    """
    if not (0.0 < float(collapse_share) < 1.0):
        raise ValueError("collapse_share must be in (0, 1)")
    lags = tuple(int(k) for k in lags)
    if not lags or min(lags) < 1:
        raise ValueError("lags must be a non-empty tuple of positive ints")

    base = pair_signal_forward_return(signal_time, signal_value, available_at, price_time, price, H,
                                      log_return=log_return, max_entry_gap_days=max_entry_gap_days)
    r0 = rank_ic(base["signal"], base["fwd_ret"])
    ic0 = r0["ic"]
    effect_present = bool(np.isfinite(r0["ci_low"]) and np.isfinite(r0["ci_high"])
                          and (r0["ci_low"] > 0.0 or r0["ci_high"] < 0.0))

    rows = []
    for k in lags:
        aa_k = delay_available_at(available_at, price_time, k)
        pk = pair_signal_forward_return(signal_time, signal_value, aa_k, price_time, price, H,
                                        log_return=log_return, max_entry_gap_days=max_entry_gap_days)
        rk = rank_ic(pk["signal"], pk["fwd_ret"])
        ratio = (rk["ic"] / ic0) if (np.isfinite(ic0) and ic0 != 0.0 and np.isfinite(rk["ic"])) else float("nan")
        rows.append({"lag": k, "ic": float(rk["ic"]), "n_pairs": int(rk["n_pairs"]),
                     "retained_share": float(ratio)})

    if not effect_present:
        verdict = "NO-EFFECT"
    else:
        r1 = rows[0]["retained_share"]
        verdict = "LOOKAHEAD-SUSPECT" if (not np.isfinite(r1) or r1 < 1.0 - collapse_share) else "ROBUST"

    return {"verdict": verdict, "effect_present": effect_present,
            "ic_lag0": float(ic0), "n_pairs_lag0": int(r0["n_pairs"]),
            "ci_low_lag0": float(r0["ci_low"]), "ci_high_lag0": float(r0["ci_high"]),
            "lags": rows, "collapse_share": float(collapse_share)}
