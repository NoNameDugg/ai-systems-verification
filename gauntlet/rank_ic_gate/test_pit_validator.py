"""
TDI tests for pit_validator (Sprint AV-ALTDATA-IC Track-1, charter T1.1 backtest PIT).

Run:  python -m pytest scripts/backtester/rank_ic_gate/test_pit_validator.py -q
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from pit_validator import assert_clean, audit_macro_pit, audit_pairs_pit  # noqa: E402


def test_macro_pit_clean():
    d = np.array(["2020-01-01", "2020-01-02"], dtype="datetime64[ns]")
    a = d + np.timedelta64(1, "D")
    r = audit_macro_pit(d, a)
    assert r["n_violations"] == 0 and r["first_violation"] is None


def test_macro_pit_flags_available_on_or_before_obs():
    d = np.array(["2020-01-01", "2020-01-02"], dtype="datetime64[ns]")
    a = np.array(["2020-01-02", "2020-01-02"], dtype="datetime64[ns]")   # 2nd: available == obs (bad)
    r = audit_macro_pit(d, a)
    assert r["n_violations"] == 1 and r["first_violation"] == 1


def test_pairs_pit_clean():
    a = np.array(["2020-01-01T21:00"], dtype="datetime64[ns]")
    e = np.array(["2020-01-02T00:00"], dtype="datetime64[ns]")
    r = audit_pairs_pit(a, e, np.array([0.5]), np.array([0.01]))
    assert r["n_lookahead"] == 0 and r["n_nan"] == 0


def test_pairs_pit_flags_lookahead():
    a = np.array(["2020-01-03"], dtype="datetime64[ns]")
    e = np.array(["2020-01-02"], dtype="datetime64[ns]")     # entry before available -> look-ahead
    r = audit_pairs_pit(a, e, np.array([0.5]), np.array([0.01]))
    assert r["n_lookahead"] == 1


def test_pairs_pit_flags_nan():
    a = np.array(["2020-01-01"], dtype="datetime64[ns]")
    e = np.array(["2020-01-02"], dtype="datetime64[ns]")
    r = audit_pairs_pit(a, e, np.array([np.nan]), np.array([0.01]))
    assert r["n_nan"] == 1


def test_assert_clean_raises_then_passes():
    bad = {"check": "x", "n_violations": 1}
    with pytest.raises(ValueError):
        assert_clean(bad)
    assert_clean({"check": "ok", "n_violations": 0, "n_lookahead": 0, "n_nan": 0})   # no raise
