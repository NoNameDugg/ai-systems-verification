"""Leaf — calendar-regime OOS split + Lo-2002 Sharpe-SE power / INDETERMINATE grade.

Implements the OOS split + the OOS-CONFIRMS predicate.

Pins honored here (do NOT invent thresholds — every value comes from ForkBConfig):
  - 3 calendar regimes split at `cfg.oos_regime_bounds` = ("2001-01-01","2010-01-01") ->
    {pre2001 / 2001-2009 / >=2010}; primary holdout = >=2010 (separates the 2001 decimalization
    cost break from the ~2005 Reg-NMS signal break). A profitability book re-bounds to
    ("2001-01-01","2013-01-01") -> primary holdout >=2013 to isolate RMW's post-2013 publication
    decay (regime_of derives the label).
  - Lo-2002 Sharpe standard error: SE(SR) = sqrt((1 + 0.5*SR**2) / n_blocks), n_blocks = the
    in-regime non-overlapping-63-td block count.
  - post-decay effect size = IS-Sharpe * cfg.oos_effect_haircut (0.5 FIXED — NOT IS-slope-derived,
    which is circular; the realized slope is reported as a diagnostic elsewhere).
  - MDE = cfg.oos_power_z * SE (z = 1.645, the literal one-sided alpha=0.05 value).
  - power = Phi((effect - MDE)/SE).
  - power < cfg.oos_power_floor (0.5) -> INDETERMINATE, NOT falsified (deterministic, non-tunable).
  - ★ RT-D: OOS-CONFIRMS = [OOS net >= +5%/yr ∧ OOS t >= 2.0 ∧ power >= 0.5]; the >=60-block floor
    (cfg.m2_min_blocks) is EXPLICITLY WAIVED for the OOS leg — it is carried by the full-series M2,
    and the ~56-block OOS underpower is exactly what the power/INDETERMINATE grade handles. So this
    module does NOT impose m2_min_blocks; a 56-block OOS with net+t+power can still confirm.

Pure-function library over the contract: depends ONLY on _schema (ForkBConfig, regime_of) + numpy.
"""
from __future__ import annotations

import math

import numpy as np
import pandas as pd

from _schema import ForkBConfig, regime_of


def _phi(x: float) -> float:
    """Standard-normal CDF Phi(x), via erf (no scipy dependency)."""
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def oos_regime(dates, cfg: ForkBConfig) -> pd.Series:
    """Map dates -> the calendar regime label {'pre2001','2001-2009','>=2010'} per cfg.oos_regime_bounds.

    Thin wrapper over _schema.regime_of (the single owner of the bound semantics): the split is at
    the lower bound (2001-01-01) and the upper bound (2010-01-01), inclusive of each lower edge.
    """
    return regime_of(pd.Series(pd.to_datetime(dates)), cfg)


def lo2002_se(sr: float, n_blocks: int) -> float:
    """Lo-2002 standard error of a Sharpe ratio: sqrt((1 + 0.5*sr**2) / n_blocks).

    `sr` is the (per-block) Sharpe; `n_blocks` is the time-series-independent block count.
    """
    return math.sqrt((1.0 + 0.5 * float(sr) ** 2) / float(n_blocks))


def oos_power(is_sharpe: float, oos_n_blocks: int, cfg: ForkBConfig) -> dict:
    """Lo-2002 OOS power + the INDETERMINATE grade (charter §4 / D-HARD-3 R1).

    effect = is_sharpe * cfg.oos_effect_haircut (0.5 FIXED post-decay haircut)
    SE     = lo2002_se(effect, oos_n_blocks)
    mde    = cfg.oos_power_z * SE        (z = 1.645)
    power  = Phi((effect - mde)/SE)
    indeterminate = power < cfg.oos_power_floor (0.5)

    Returns {'power': float, 'mde': float, 'indeterminate': bool}.
    """
    effect = float(is_sharpe) * cfg.oos_effect_haircut
    se = lo2002_se(effect, oos_n_blocks)
    mde = cfg.oos_power_z * se
    power = _phi((effect - mde) / se)
    return {
        "power": power,
        "mde": mde,
        "indeterminate": bool(power < cfg.oos_power_floor),
    }


def oos_confirms(oos_net_ann: float, oos_t: float, power: float, cfg: ForkBConfig) -> bool:
    """RT-D OOS-CONFIRMS predicate: all three AND-required; the >=60-block floor is WAIVED for OOS.

    (oos_net_ann >= cfg.m2_net_ann_floor) AND (oos_t >= cfg.m2_t_floor) AND (power >= cfg.oos_power_floor).
    ★ Deliberately does NOT apply cfg.m2_min_blocks (RT-D) — a ~56-block OOS can still confirm.
    """
    return bool(
        (float(oos_net_ann) >= cfg.m2_net_ann_floor)
        and (float(oos_t) >= cfg.m2_t_floor)
        and (float(power) >= cfg.oos_power_floor)
    )
