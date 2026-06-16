"""ML cross-sectional config — the PRE-REGISTRATION object (sha256-frozen BEFORE the verdict).

Pins every researcher-degree-of-freedom. `sha256()` is the freeze hash. The data window is set by
institutional-holdings coverage (13F-derived series start 2013-06-30). Pure dataclass — no I/O.
"""
from __future__ import annotations

import dataclasses
import hashlib
import json


@dataclasses.dataclass(frozen=True)
class MLXConfig:
    # --- data window (PINNED: institutional 13F coverage starts 2013-06-30 → full-blend window) ---
    primary_start: str = "2013-07-01"
    primary_end: str = "2026-06-05"

    # --- liquid universe floors (raw $) ---
    marketcap_floor: float = 300_000_000.0
    price_floor: float = 5.0
    adv_floor: float = 1_000_000.0
    closeadj_floor: float = 1.0
    adv_window_td: int = 60

    # --- target / rebalance / binning (n_quantiles pinned in ONE place for BOTH book + factor returns) ---
    horizon_td: int = 21          # forward 21-trading-day cross-sectional return label
    rebalance: str = "ME"         # month-end
    n_quantiles: int = 5          # quintile L-S book AND construction-matched factor returns (top/bottom 20%)
    weighting: str = "VW"         # value-weighted (cap-restored), matching the gating factor returns
    weight_cap: float = 0.05      # per-name cap (breadth-restoring)

    # --- curated orthogonal feature blend (~8-15; features << effective-N) ---
    features: tuple = (
        "value_pe", "value_pb", "value_ev_ebit",                 # value (DAILY)
        "qual_roa", "qual_gpa", "qual_accruals", "qual_gm",      # quality (gpa/accruals DERIVED)
        "mom_12_1", "lowvol_120", "rev_1m",                      # momentum / low-vol / reversal
        "insider_breadth_1y",                                    # insider (filingdate D-1)
        "inst_holders_chg",                                      # institutional (13F holder change, calendardate+45d)
        "size_mktcap",                                           # size (DAILY) — control feature
    )

    # --- frac-diff — DISABLED: FFD is a TIME-SERIES transform; these features are per-date
    #     CROSS-SECTIONAL robust-z (the standardization), so frac-diff is N/A here. The mlx_fracdiff leaf
    #     stays banked + tested for a future time-series application. Declared False so the freeze is truthful. ---
    fracdiff_enable: bool = False

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

    # --- orthogonality (construction-matched 7-factor + char-space cross-check) ---
    factor_set: tuple = ("size", "value", "mom", "str", "beta", "rmw", "cma")   # 7-factor, built at the BOOK's quintile/VW
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

    # --- verdict rule + kill-test (IN the freeze hash) ---
    verdict_rule_version: str = "v2-cilo-gated"   # CANNOT-TEST only when ci_lo<=0; never veto a significant detection
    p3_calmar_floor: float = 0.50    # stressed Calmar floor (P3 crash-survival kill-test)
    cost_band_bps: float = 10.0      # per-unit net-turnover cost (one-way band, bps)

    def sha256(self) -> str:
        payload = json.dumps(dataclasses.asdict(self), sort_keys=True, default=str)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()
