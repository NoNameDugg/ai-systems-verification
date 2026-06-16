"""
TDI tests for signal_return_pairer (Sprint AV-ALTDATA-IC Track-1, charter T1.1, Q5 PIT).

Run:  python -m pytest scripts/backtester/rank_ic_gate/test_signal_return_pairer.py -q
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from signal_return_pairer import (assert_no_lookahead,  # noqa: E402
                                  pair_signal_forward_return)


def _days(start, n):
    return np.array([np.datetime64(start) + np.timedelta64(d, "D") for d in range(n)])


def test_basic_pairing_log_return():
    pt = _days("2020-01-01", 10)
    price = np.array([100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0])
    # signal known at 2020-01-01 16:15 -> entry is the 2020-01-02 bar (first strictly after)
    st = np.array([np.datetime64("2020-01-01")])
    sv = np.array([0.5])
    aa = np.array([np.datetime64("2020-01-01T16:15")])
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    assert out["n_pairs"] == 1
    assert out["entry_time"][0] == np.datetime64("2020-01-02")
    assert out["exit_time"][0] == np.datetime64("2020-01-03")
    assert out["fwd_ret"][0] == pytest.approx(np.log(102.0 / 101.0))
    assert out["signal"][0] == 0.5


def test_pit_invariant_holds_for_every_pair():
    pt = _days("2021-01-01", 60)
    price = 100.0 + np.arange(60.0)
    rng = np.random.default_rng(0)
    idx = np.arange(50)
    st = pt[idx]
    sv = rng.normal(size=50)
    aa = pt[idx] + np.timedelta64(10, "h")          # same-day, before next bar
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    assert np.all(out["available_at"] < out["entry_time"])   # the Q5 invariant


def test_available_at_exactly_on_a_bar_excludes_that_bar():
    pt = _days("2020-01-01", 6)
    price = np.array([10.0, 11.0, 12.0, 13.0, 14.0, 15.0])
    st = np.array([np.datetime64("2020-01-01")])
    sv = np.array([1.0])
    aa = np.array([np.datetime64("2020-01-02")])     # exactly the 01-02 bar
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    # side='right' -> entry is 01-03 (strictly after), never the equal bar (no look-ahead)
    assert out["entry_time"][0] == np.datetime64("2020-01-03")


def test_horizon_H_used_for_exit():
    pt = _days("2020-01-01", 12)
    price = 100.0 * (1.01 ** np.arange(12.0))
    st = np.array([np.datetime64("2020-01-01")])
    sv = np.array([1.0])
    aa = np.array([np.datetime64("2020-01-01T12:00")])
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=5)
    e = list(pt).index(out["entry_time"][0])
    assert out["exit_time"][0] == pt[e + 5]
    assert out["fwd_ret"][0] == pytest.approx(np.log(price[e + 5] / price[e]))


def test_drops_obs_without_full_forward_window():
    pt = _days("2020-01-01", 5)
    price = np.array([1.0, 2.0, 3.0, 4.0, 5.0])
    st = np.array([np.datetime64("2020-01-04T12:00")])   # near the end
    sv = np.array([1.0])
    aa = np.array([np.datetime64("2020-01-04T12:00")])
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=3)
    assert out["n_pairs"] == 0


def test_drops_nan_signal():
    pt = _days("2020-01-01", 6)
    price = np.arange(1.0, 7.0)
    st = np.array([np.datetime64("2020-01-01"), np.datetime64("2020-01-02")])
    sv = np.array([np.nan, 2.0])
    aa = np.array([np.datetime64("2020-01-01T12:00"), np.datetime64("2020-01-02T12:00")])
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    assert out["n_pairs"] == 1 and out["signal"][0] == 2.0


def test_simple_vs_log_return():
    pt = _days("2020-01-01", 5)
    price = np.array([100.0, 110.0, 121.0, 133.1, 146.41])
    st = np.array([np.datetime64("2020-01-01")])
    sv = np.array([1.0])
    aa = np.array([np.datetime64("2020-01-01T12:00")])
    log_out = pair_signal_forward_return(st, sv, aa, pt, price, H=1, log_return=True)
    sim_out = pair_signal_forward_return(st, sv, aa, pt, price, H=1, log_return=False)
    assert log_out["fwd_ret"][0] == pytest.approx(np.log(121.0 / 110.0))
    assert sim_out["fwd_ret"][0] == pytest.approx(121.0 / 110.0 - 1.0)


def test_assert_no_lookahead_raises_on_violation():
    a = np.array([np.datetime64("2020-01-02T00:00")])
    f = np.array([np.datetime64("2020-01-01T00:00")])   # window starts BEFORE available -> look-ahead
    with pytest.raises(ValueError):
        assert_no_lookahead(a, f)


def test_assert_no_lookahead_passes_when_strictly_before():
    a = np.array([np.datetime64("2020-01-01T00:00")])
    f = np.array([np.datetime64("2020-01-02T00:00")])
    assert_no_lookahead(a, f)   # no raise


def test_non_increasing_price_time_raises():
    pt = np.array([np.datetime64("2020-01-02"), np.datetime64("2020-01-01")])
    with pytest.raises(ValueError):
        pair_signal_forward_return(np.array([np.datetime64("2020-01-01")]), np.array([1.0]),
                                   np.array([np.datetime64("2020-01-01T12:00")]),
                                   pt, np.array([1.0, 2.0]), H=1)


def test_H_less_than_one_raises():
    pt = _days("2020-01-01", 3)
    with pytest.raises(ValueError):
        pair_signal_forward_return(np.array([np.datetime64("2020-01-01")]), np.array([1.0]),
                                   np.array([np.datetime64("2020-01-01T12:00")]),
                                   pt, np.array([1.0, 2.0, 3.0]), H=0)


def test_signal_shape_mismatch_raises():
    pt = _days("2020-01-01", 3)
    with pytest.raises(ValueError):
        pair_signal_forward_return(np.array([np.datetime64("2020-01-01")]), np.array([1.0, 2.0]),
                                   np.array([np.datetime64("2020-01-01T12:00")]),
                                   pt, np.array([1.0, 2.0, 3.0]), H=1)


# ---- freshness/coverage guard (the rho_y-inflation bug regression) -------
def test_signals_predating_price_history_are_dropped():
    # price starts 2020-06-01; signals from 2020-01 all searchsort to index 0 -> must be dropped,
    # NOT collapsed onto the first price bar (the bug that spuriously inflated rho_y to 0.44).
    pt = _days("2020-06-01", 10)
    price = 100.0 + np.arange(10.0)
    st = _days("2020-01-01", 5)                       # Jan signals, long before price history
    sv = np.array([1.0, 2.0, 3.0, 4.0, 5.0])
    aa = st + np.timedelta64(12, "h")
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    assert out["n_pairs"] == 0                         # all dropped (gap >> 7d), none collapse to pt[0]


def test_no_duplicate_entries_for_dense_in_range_signals():
    pt = _days("2020-06-01", 40)
    price = 100.0 + np.arange(40.0)
    st = _days("2020-06-02", 30)                       # in-range, one per day
    sv = np.arange(30.0)
    aa = st + np.timedelta64(12, "h")
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1)
    assert out["n_pairs"] == len(np.unique(out["entry_time"]))   # 1:1, no collapse


def test_guard_disabled_keeps_predating_signals():
    pt = _days("2020-06-01", 10)
    price = 100.0 + np.arange(10.0)
    st = _days("2020-01-01", 3)
    sv = np.array([1.0, 2.0, 3.0])
    aa = st + np.timedelta64(12, "h")
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1, max_entry_gap_days=None)
    assert out["n_pairs"] == 3 and len(np.unique(out["entry_time"])) == 1   # the OLD buggy collapse


def test_within_gap_kept_beyond_gap_dropped():
    pt = _days("2020-06-01", 10)
    price = 100.0 + np.arange(10.0)
    # one signal 1 day before price start (kept: gap ~1d), one 30 days before (dropped)
    st = np.array([np.datetime64("2020-05-31"), np.datetime64("2020-05-01")])
    sv = np.array([1.0, 2.0])
    aa = np.array([np.datetime64("2020-05-31T12:00"), np.datetime64("2020-05-01T12:00")])
    out = pair_signal_forward_return(st, sv, aa, pt, price, H=1, max_entry_gap_days=7.0)
    assert out["n_pairs"] == 1 and out["signal"][0] == 1.0
