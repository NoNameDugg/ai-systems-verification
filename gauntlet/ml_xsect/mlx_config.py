"""ML cross-sectional config — the PRE-REGISTRATION object (sha256-frozen BEFORE the verdict).

Every key here is READ by a leaf in this package (tests/test_review_v12.py enumerates the fields and fails if
one is not consumed by non-test code), so `sha256()` covers exactly what runs. v1.2.0 removed 15 keys that
were hashed but read by nothing shipped here (the licensed-universe filters, data window, feature list,
rebalance/weighting pins, fracdiff flag, factor_set, verdict_rule_version, cost_band_bps) and wired the gate
thresholds into the leaves (`cfg=` keyword), which until then carried them as hard-coded literal defaults.
Pure dataclass — no I/O.
"""
from __future__ import annotations

import dataclasses
import hashlib
import json


@dataclasses.dataclass(frozen=True)
class MLXConfig:
    # --- target / binning (n_quantiles pinned in ONE place for BOTH book + factor returns) ---
    horizon_td: int = 21          # forward 21-trading-day cross-sectional return label
    n_quantiles: int = 5          # quintile L-S book AND construction-matched factor returns (top/bottom 20%)

    # --- t1-aware panel CPCV (symmetric, >= horizon, date-level/name-invariant) ---
    cpcv_n_groups: int = 6
    cpcv_k_test: int = 2
    cpcv_embargo_td: int = 21         # >= horizon, applied BOTH sides of each test block

    # --- IC-IR power gate ---
    z_sum: float = 2.80               # 1.96 + 0.84 (two-sided alpha=0.05 + 80% power)
    ic_power_ceiling: float = 0.02    # MDE ceiling = the GLIMMER decision bar — CANNOT-TEST iff MDE > this
    ic_glimmer_bar: float = 0.02      # mean OOS rank-IC >= this (with block-t CI lower > 0) -> GLIMMER
    ic_strong_bar: float = 0.05
    block_t_crit: float = 1.96
    min_xsection: int = 50            # minimum names per usable rebalance (thin-date guard)

    # --- orthogonality (char-space cross-check) ---
    charspace_ic_min: float = 0.0     # incremental rank-IC > this with block-t >= block_t_crit -> survives
    known_rmw_beta_threshold: float = 0.5   # the known-factor test ALSO checks |rmw_beta|, not just cma_beta
    known_cma_beta_threshold: float = 0.5

    # --- model (deliberately shallow / heavily regularized) ---
    gbm_max_depth: int = 3
    gbm_n_estimators: int = 200
    gbm_learning_rate: float = 0.03
    gbm_min_samples_leaf: int = 200
    gbm_l2: float = 1.0               # HistGBM l2_regularization (subsample dropped — HistGBM has no such param)
    seed: int = 7

    # --- kill-test (IN the freeze hash) ---
    p3_calmar_floor: float = 0.50    # realized Calmar floor (P3 crash-survival kill-test)

    def sha256(self) -> str:
        payload = json.dumps(dataclasses.asdict(self), sort_keys=True, default=str)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()
