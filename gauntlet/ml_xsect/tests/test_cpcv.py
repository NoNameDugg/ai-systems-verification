"""TDI for mlx_cpcv — purge/embargo leakage guards on the rebalance grid."""
import numpy as np

import mlx_cpcv as cp


def test_combination_count():
    splits = list(cp.cpcv_splits(60, n_groups=6, k_test=2, purge=1, embargo=1))
    assert len(splits) == cp.n_combinations(6, 2) == 15


def test_train_test_disjoint():
    for tr, te in cp.cpcv_splits(60, 6, 2, purge=1, embargo=1):
        assert len(np.intersect1d(tr, te)) == 0


def test_no_train_within_buffer_of_test():
    buffer = 1 + 1
    for tr, te in cp.cpcv_splits(60, 6, 2, purge=1, embargo=1):
        for i in tr:
            assert np.min(np.abs(i - te)) > buffer       # symmetric purge+embargo enforced both sides


def test_purge_zero_embargo_zero_is_plain_holdout():
    # with no buffer, train = all non-test indices exactly
    for tr, te in cp.cpcv_splits(30, 6, 1, purge=0, embargo=0):
        assert sorted(np.concatenate([tr, te])) == list(range(30))


def test_each_group_tested_equally():
    n_groups, k = 6, 2
    counts = np.zeros(n_groups, dtype=int)
    groups = cp.contiguous_groups(60, n_groups)
    gset = [set(g.tolist()) for g in groups]
    for _, te in cp.cpcv_splits(60, n_groups, k, purge=1, embargo=1):
        teset = set(te.tolist())
        for gi, gs in enumerate(gset):
            if gs & teset:
                counts[gi] += 1
    # each group appears in test in C(n_groups-1, k-1) = C(5,1) = 5 combinations
    assert set(counts.tolist()) == {5}


def test_larger_embargo_shrinks_train():
    small = next(cp.cpcv_splits(60, 6, 2, purge=1, embargo=1))[0]
    big = next(cp.cpcv_splits(60, 6, 2, purge=1, embargo=4))[0]
    assert len(big) < len(small)
