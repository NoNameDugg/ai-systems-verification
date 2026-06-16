"""The block unit: the time-series-independent observation primitive.

The deployability series is aggregated into NON-OVERLAPPING 63-td blocks (the single frozen
time-series-N that the M2-t, the PSR gate, and the OOS power module all consume). Block-aggregation
removes the BULK of the 63-td-overlap autocorrelation so the M2 t is a PLAIN t on the block series
by default — the *daily*-series Newey-West HAC is neither needed nor exists.

Residual autocorrelation: block independence is NOT exact — 63-td cohorts on a 21-td rebalance
  straddle block boundaries, so adjacent blocks share exposure -> a residual lag-1 block
  autocorrelation rho that would inflate a plain-t ANTI-CONSERVATIVELY. rho is a mandatory output;
  iff |rho| > cfg.block_rho_threshold (0.2) apply a 1-lag (Bartlett) BLOCK-LEVEL Newey-West variance
  correction (cfg.block_nw_lag = 1) to the t — a small, well-defined BLOCK-level correction, NOT the
  nonexistent daily HAC. The same rho-rule applies to the ~56-block OOS-t (same cohort straddle).

Block grid: anchored at the first deployable day (cfg.block_anchor) and the trailing partial block
  is DROPPED (cfg.drop_trailing_partial_block).

Pure-function library over the contract: depends ONLY on _schema + numpy/pandas. The std convention
(ddof=1, sample std) matches the shared apparatus (event_study_car plain-t, carry_gate PSR-SR) so the
block-t is consistent with every gate that consumes the block series.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig


# ----------------------------------------------------------------------------------------------------
# to_blocks — aggregate the daily deployability series into non-overlapping 63-td blocks.
# ----------------------------------------------------------------------------------------------------
def to_blocks(daily_returns: pd.Series, cfg: ForkBConfig) -> np.ndarray:
    """Aggregate a daily return series into NON-OVERLAPPING cfg.block_td (63-td) blocks.

    - block return = COMPOUNDED daily within the block: prod(1 + r) - 1 (NOT a sum; geometric).
    - anchored at the FIRST day of the series (cfg.block_anchor = first_deployable_day_of_regime — RT-C):
      block 0 = days [0:63], block 1 = days [63:126], ...
    - DROPS the trailing partial block (cfg.drop_trailing_partial_block — RT-C): only full
      cfg.block_td-length blocks are kept.

    Returns a 1-D np.ndarray of block returns (length = floor(n_days / block_td) when the trailing
    partial is dropped). The caller hands this to block_t / psr_spell / the OOS power module — it IS the
    frozen time-series-independent observation unit.
    """
    bt = int(cfg.block_td)
    if bt <= 0:
        raise ValueError(f"cfg.block_td must be positive (got {bt})")

    # The series is the deployability daily P&L; order by its (date) index so the grid is anchored at
    # the first deployable day, then read the raw daily returns in chronological order.
    r = pd.Series(daily_returns).sort_index()
    vals = np.asarray(r.values, dtype=float)
    n = vals.size

    n_full = n // bt
    if cfg.drop_trailing_partial_block:
        n_keep = n_full                      # only full blocks; trailing partial dropped (RT-C)
    else:
        n_keep = n_full + (1 if (n % bt) else 0)

    blocks = np.empty(n_keep, dtype=float)
    for b in range(n_keep):
        seg = vals[b * bt:(b + 1) * bt]      # the trailing block may be partial iff not dropped
        blocks[b] = float(np.prod(1.0 + seg) - 1.0)   # compounded daily within-block
    return blocks


# ----------------------------------------------------------------------------------------------------
# block_t — the PLAIN t on the block series, with the conditional 1-lag block-NW correction (RT-B).
# ----------------------------------------------------------------------------------------------------
def _lag1_autocorr(x: np.ndarray) -> float:
    """Lag-1 autocorrelation ρ of the block series (Pearson on the (x_t, x_{t-1}) pairs).

    ρ = cov(x_t, x_{t-1}) / var(x), measured on the residual block series itself (RT-B). NaN when
    n < 2 or the series has zero variance (degenerate — no autocorrelation defined).
    """
    x = np.asarray(x, dtype=float)
    n = x.size
    if n < 2:
        return float("nan")
    xm = x.mean()
    dx = x - xm
    denom = float(np.dot(dx, dx))            # n * population variance (the var normaliser cancels)
    if denom == 0.0:
        return float("nan")
    num = float(np.dot(dx[1:], dx[:-1]))     # sum of lag-1 cross-products
    return num / denom


def block_t(block_returns: np.ndarray, cfg: ForkBConfig) -> dict:
    """PLAIN t on the block series, with the conditional lag-1 BLOCK-LEVEL Newey-West correction.

    - plain t  = mean / (std / sqrt(n)), std = sample std (ddof=1) — the apparatus convention
      (event_study_car.py:100).
    - rho      = lag-1 autocorrelation of the block series (mandatory output — RT-B).
    - nw_applied: iff |rho| > cfg.block_rho_threshold (0.2), the t is recomputed with a 1-lag
      (Bartlett) Newey-West variance: Var_NW(mean) = (1/n)[γ0 + 2·w1·γ1], w1 = 1 − 1/(L+1) = 0.5 at
      L = cfg.block_nw_lag = 1  ⇒  Var_NW(mean) = (γ0 + γ1)/n. With ρ > 0 this is LARGER than the
      plain Var(mean) = γ0/n ⇒ |t_NW| < |t_plain| (the anti-conservative inflation is corrected).
      This is a BLOCK-level correction, NOT a daily HAC (DH-A / Finding-2).
    - n        = the number of blocks (the time-series-independent observation count).

    Returns dict{t, rho, nw_applied, n}. t / rho are NaN for a degenerate series (n < 2 or zero var).
    """
    x = np.asarray(block_returns, dtype=float)
    n = int(x.size)

    if n < 2:
        return {"t": float("nan"), "rho": float("nan"), "nw_applied": False, "n": n}

    mean = float(x.mean())
    sd = float(x.std(ddof=1))                 # sample std — apparatus convention
    if sd == 0.0:
        return {"t": float("nan"), "rho": float("nan"), "nw_applied": False, "n": n}

    t_plain = mean / (sd / np.sqrt(n))
    rho = _lag1_autocorr(x)

    # default: plain block-t stands (block-aggregation removed the bulk of the overlap autocorrelation)
    if np.isnan(rho) or abs(rho) <= float(cfg.block_rho_threshold):
        return {"t": float(t_plain), "rho": float(rho), "nw_applied": False, "n": n}

    # |ρ| > 0.2 → the lag-1 Bartlett block-level Newey-West variance correction (RT-B).
    L = int(cfg.block_nw_lag)                  # = 1 (Bartlett L=1 on the ~104-pt residual block series)
    dx = x - mean
    gamma0 = float(np.dot(dx, dx) / n)         # population autocovariance at lag 0 (γ0)
    s_nw = gamma0
    for k in range(1, L + 1):
        w = 1.0 - k / (L + 1.0)                # Bartlett weight (w1 = 0.5 at L=1)
        gamma_k = float(np.dot(dx[k:], dx[:-k]) / n)   # autocovariance at lag k (γk)
        s_nw += 2.0 * w * gamma_k
    # NW variance of the mean; guard the rare negative-PSD edge (small samples) by falling back to plain.
    if s_nw <= 0.0:
        return {"t": float(t_plain), "rho": float(rho), "nw_applied": False, "n": n}
    var_mean_nw = s_nw / n
    t_nw = mean / np.sqrt(var_mean_nw)
    return {"t": float(t_nw), "rho": float(rho), "nw_applied": True, "n": n}
