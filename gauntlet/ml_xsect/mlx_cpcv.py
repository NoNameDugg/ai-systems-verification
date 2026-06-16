"""t1-aware purged + symmetric-embargoed combinatorial CV (CPCV) for a monthly cross-sectional panel. PURE.

The unit is the REBALANCE (monthly month-end). A rebalance's label spans the forward horizon (~1 step at 21td/monthly),
so a train rebalance whose label window overlaps a test block is PURGED, and a SYMMETRIC embargo buffers BOTH sides.
Date-level / name-invariant: excluding a rebalance excludes ALL its names. A naive integer-ROW-distance purge is
panel-unsuitable; this operates on rebalance indices, so row distance != label-time overlap is moot.

Steps are in REBALANCE units (the caller maps trading-day horizon/embargo -> steps via the monthly cadence, e.g. 21td~=1).
"""
from __future__ import annotations

import itertools

import numpy as np


def contiguous_groups(n: int, n_groups: int):
    """Partition range(n) into n_groups near-equal CONTIGUOUS groups (preserves time order)."""
    return [np.asarray(g) for g in np.array_split(np.arange(n), n_groups)]


def cpcv_splits(n: int, n_groups: int, k_test: int, purge: int, embargo: int):
    """Yield (train_idx, test_idx) for each combination of k_test of n_groups contiguous groups.

    train excludes test AND any index within (purge + embargo) of a test index (symmetric). purge = label-window
    overlap in steps; embargo = extra serial-correlation buffer in steps.
    """
    groups = contiguous_groups(n, n_groups)
    buffer = int(purge) + int(embargo)
    for combo in itertools.combinations(range(n_groups), k_test):
        test_idx = np.sort(np.concatenate([groups[g] for g in combo]))
        excl = np.zeros(n, dtype=bool)
        for j in test_idx:
            lo, hi = max(0, j - buffer), min(n, j + buffer + 1)
            excl[lo:hi] = True
        train_idx = np.asarray([i for i in range(n) if not excl[i]], dtype=int)
        yield train_idx, test_idx


def n_combinations(n_groups: int, k_test: int) -> int:
    from math import comb
    return comb(n_groups, k_test)
