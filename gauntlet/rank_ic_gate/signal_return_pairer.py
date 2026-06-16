"""
signal_return_pairer — PIT-invariant pairing of a bar-stamped signal with a forward return.

  For each signal observation (signal_time, signal_value, available_at), the forward window
  starts at the FIRST target price bar STRICTLY AFTER available_at (you can only act once the
  signal is knowable), and the forward return is taken over the next H bars from there.

  PIT MACHINE-INVARIANT (not a 20-row eyeball): every emitted pair satisfies
    available_at(signal_t) < forward_window_start(entry_time)  -- raise on ANY violation.
  By construction `searchsorted(side="right")` guarantees this; the assertion is the safety net
  that catches an upstream alignment/`available_at` bug rather than letting look-ahead leak in.

PURE: numpy-only, no I/O. Deterministic.
"""
from __future__ import annotations

import numpy as np

__all__ = ["assert_no_lookahead", "pair_signal_forward_return"]


def assert_no_lookahead(available_at, forward_window_start) -> None:
    """Raise if any available_at >= its forward_window_start (the PIT machine-invariant)."""
    a = np.asarray(available_at)
    f = np.asarray(forward_window_start)
    if a.shape != f.shape:
        raise ValueError("available_at and forward_window_start must align")
    bad = a >= f
    if np.any(bad):
        k = int(np.argmax(bad))
        raise ValueError(f"PIT violation at pair {k}: available_at {a[k]!s} >= "
                         f"forward_window_start {f[k]!s}")


def pair_signal_forward_return(signal_time, signal_value, available_at,
                               price_time, price, H, *, log_return: bool = True,
                               max_entry_gap_days: float | None = 7.0) -> dict:
    """
    Pair each signal value with its causal H-bar forward return on the target price series.

    Returns dict of aligned arrays: signal, fwd_ret, entry_time, exit_time, available_at, n_pairs.
    Observations are dropped (not errored) when: signal NaN; no price bar strictly after
    available_at; no full H-bar forward window; non-finite/non-positive entry or exit price; OR
    the entry bar is more than `max_entry_gap_days` after available_at (freshness/coverage guard).

    ★ The `max_entry_gap_days` guard (default 7d) is load-bearing: a signal whose available_at
    predates the ENTIRE price history searchsorts to index 0, so EVERY such signal collapses onto
    the first price bar -> runs of identical fwd_ret that spuriously inflate the forward-return
    autocorrelation (and thus wreck the power gate). It also drops any stale mapping across a long
    data gap. Set to None to disable.
    """
    st = np.asarray(signal_time, dtype="datetime64[ns]")
    sv = np.asarray(signal_value, dtype=float)
    aa = np.asarray(available_at, dtype="datetime64[ns]")
    pt = np.asarray(price_time, dtype="datetime64[ns]")
    pp = np.asarray(price, dtype=float)

    if not (st.shape == sv.shape == aa.shape):
        raise ValueError(f"signal arrays must align: {st.shape}/{sv.shape}/{aa.shape}")
    if pt.shape != pp.shape:
        raise ValueError(f"price arrays must align: {pt.shape}/{pp.shape}")
    if int(H) < 1:
        raise ValueError("H must be >= 1")
    H = int(H)
    if pt.size >= 2 and np.any(np.diff(pt) <= np.timedelta64(0)):
        raise ValueError("price_time must be strictly increasing")

    n_price = pt.size
    max_gap = None if max_entry_gap_days is None else np.timedelta64(int(round(max_entry_gap_days * 24)), "h")
    sig_o, fwd_o, ent_o, exi_o, ava_o = [], [], [], [], []
    for i in range(st.size):
        if not np.isfinite(sv[i]):
            continue
        entry_idx = int(np.searchsorted(pt, aa[i], side="right"))  # first bar STRICTLY after available_at
        exit_idx = entry_idx + H
        if entry_idx >= n_price or exit_idx >= n_price:
            continue
        if max_gap is not None and (pt[entry_idx] - aa[i]) > max_gap:
            continue                       # freshness/coverage guard (drops pre-history collapse + stale gaps)
        p0, p1 = pp[entry_idx], pp[exit_idx]
        if not (np.isfinite(p0) and np.isfinite(p1)) or p0 <= 0.0 or p1 <= 0.0:
            continue
        r = float(np.log(p1 / p0)) if log_return else float(p1 / p0 - 1.0)
        sig_o.append(float(sv[i])); fwd_o.append(r)
        ent_o.append(pt[entry_idx]); exi_o.append(pt[exit_idx]); ava_o.append(aa[i])

    signal = np.asarray(sig_o, dtype=float)
    fwd_ret = np.asarray(fwd_o, dtype=float)
    entry_time = np.asarray(ent_o, dtype="datetime64[ns]")
    exit_time = np.asarray(exi_o, dtype="datetime64[ns]")
    avail = np.asarray(ava_o, dtype="datetime64[ns]")

    # PIT machine-invariant over every emitted pair (Q5).
    assert_no_lookahead(avail, entry_time)

    return {"signal": signal, "fwd_ret": fwd_ret, "entry_time": entry_time,
            "exit_time": exit_time, "available_at": avail, "n_pairs": int(signal.size)}
