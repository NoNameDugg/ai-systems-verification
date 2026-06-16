"""
TDI tests for deflation (Sprint AV-ALTDATA-IC Track-1, charter T1.1/D-HARD-2/NIT-3).

Run:  python -m pytest scripts/backtester/rank_ic_gate/test_deflation.py -q
"""
import os
import sys

import numpy as np
import pytest

sys.path.insert(0, os.path.dirname(__file__))
from deflation import holm, holm_adjusted_pvalues, holm_reject  # noqa: E402


def test_n1_holm_is_vacuous_equals_raw():
    # NIT-3: at N=1 Holm reduces to the raw permutation p
    assert holm_adjusted_pvalues([0.037]) == pytest.approx([0.037])
    assert holm_reject([0.037], alpha=0.05).tolist() == [True]
    assert holm_reject([0.06], alpha=0.05).tolist() == [False]


def test_known_holm_example():
    adj = holm_adjusted_pvalues([0.01, 0.04, 0.03])
    np.testing.assert_allclose(adj, [0.03, 0.06, 0.06])
    assert holm_reject([0.01, 0.04, 0.03], alpha=0.05).tolist() == [True, False, False]


def test_adjusted_monotone_in_sorted_order():
    p = np.array([0.2, 0.001, 0.05, 0.009, 0.5])
    adj = holm_adjusted_pvalues(p)
    assert np.all(np.diff(adj[np.argsort(p)]) >= -1e-12)   # non-decreasing along sorted p


def test_adjusted_never_below_raw():
    p = [0.01, 0.02, 0.03, 0.5]
    adj = holm_adjusted_pvalues(p)
    assert np.all(adj >= np.asarray(p) - 1e-12)


def test_smallest_p_adj_equals_bonferroni_at_min():
    p = [0.004, 0.2, 0.3, 0.9]
    adj = holm_adjusted_pvalues(p)
    i = int(np.argmin(p))
    assert adj[i] == pytest.approx(min(1.0, len(p) * p[i]))   # m * p_min


def test_all_significant_rejects_all():
    assert holm_reject([0.001, 0.001, 0.001], alpha=0.05).tolist() == [True, True, True]


def test_input_order_preserved():
    p = [0.9, 0.001, 0.5]
    adj = holm_adjusted_pvalues(p)
    assert adj[1] < adj[0] and adj[1] < adj[2]   # the small one stays at index 1


def test_capped_at_one():
    assert np.all(holm_adjusted_pvalues([0.8, 0.9, 0.95]) <= 1.0)


def test_holm_convenience_dicts():
    rows = holm([0.01, 0.04, 0.03], alpha=0.05)
    assert rows[0] == {"p_raw": 0.01, "p_holm": pytest.approx(0.03), "reject": True}
    assert rows[1]["reject"] is False


def test_empty_input():
    assert holm_adjusted_pvalues([]).size == 0


def test_out_of_range_raises():
    with pytest.raises(ValueError):
        holm_adjusted_pvalues([0.5, 1.2])


def test_nan_raises():
    with pytest.raises(ValueError):
        holm_adjusted_pvalues([0.5, np.nan])
