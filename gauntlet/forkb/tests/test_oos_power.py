"""TDI for oos_power — charter §4 OOS power/INDETERMINATE (D-HARD-3 / R1 / N2 / RT-D).

Asserts against the pinned formula + the named spec assertions:
  (1) lo2002_se matches sqrt((1+0.5*sr^2)/n) on hand values.
  (2) tiny n_blocks -> low power -> indeterminate True; large n with a real effect -> indeterminate False.
  (3) oos_confirms requires all 3 AND, and does NOT impose a >=60-block floor (a 56-block OOS can confirm).
  (4) the regimes split at 2001 / 2010 correctly.
Plus: effect = is_sharpe*0.5 FIXED haircut; mde = z*SE (z=1.645); power = Phi((effect-mde)/SE).
"""
import math

import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from oos_power import oos_regime, lo2002_se, oos_power, oos_confirms

CFG = ForkBConfig()


def _phi(x):
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


# ---- (1) lo2002_se formula on hand values --------------------------------------------------------
def test_lo2002_se_matches_formula_hand_values():
    # SR=0, n=100 -> sqrt((1+0)/100) = 0.1
    assert lo2002_se(0.0, 100) == pytest.approx(0.1)
    # SR=1.0, n=50 -> sqrt((1+0.5)/50) = sqrt(0.03)
    assert lo2002_se(1.0, 50) == pytest.approx(math.sqrt(1.5 / 50.0))
    # SR=2.0, n=56 -> sqrt((1+2)/56) = sqrt(3/56)
    assert lo2002_se(2.0, 56) == pytest.approx(math.sqrt(3.0 / 56.0))
    # symmetric in sign of SR (sr**2)
    assert lo2002_se(-1.5, 80) == pytest.approx(lo2002_se(1.5, 80))


# ---- (2) power -> indeterminate behavior ---------------------------------------------------------
def test_tiny_nblocks_low_power_indeterminate_true():
    # tiny OOS (20 blocks) with a modest IS Sharpe -> low power -> INDETERMINATE
    res = oos_power(is_sharpe=0.4, oos_n_blocks=20, cfg=CFG)
    assert res["indeterminate"] is True
    assert res["power"] < CFG.oos_power_floor


def test_large_nblocks_real_effect_indeterminate_false():
    # large OOS (1000 blocks) with a strong IS Sharpe -> high power -> NOT indeterminate
    res = oos_power(is_sharpe=2.0, oos_n_blocks=1000, cfg=CFG)
    assert res["indeterminate"] is False
    assert res["power"] >= CFG.oos_power_floor


def test_oos_power_matches_pinned_formula_exactly():
    # re-derive the whole chain by hand and assert each field, incl. the FIXED 0.5 haircut + z=1.645
    is_sharpe, n = 1.2, 56
    effect = is_sharpe * 0.5            # cfg.oos_effect_haircut FIXED
    assert CFG.oos_effect_haircut == 0.5
    assert CFG.oos_power_z == 1.645
    se = math.sqrt((1.0 + 0.5 * effect ** 2) / n)
    mde = 1.645 * se
    power = _phi((effect - mde) / se)
    res = oos_power(is_sharpe=is_sharpe, oos_n_blocks=n, cfg=CFG)
    assert res["mde"] == pytest.approx(mde)
    assert res["power"] == pytest.approx(power)
    # (effect - mde)/se == effect/se - 1.645, so power depends only on effect/se vs the z floor
    assert res["power"] == pytest.approx(_phi(effect / se - 1.645))


def test_power_monotonic_in_nblocks():
    # more blocks -> tighter SE -> higher power for a fixed real effect
    lo = oos_power(is_sharpe=1.0, oos_n_blocks=30, cfg=CFG)["power"]
    hi = oos_power(is_sharpe=1.0, oos_n_blocks=400, cfg=CFG)["power"]
    assert hi > lo


# ---- (3) oos_confirms: 3-way AND + the WAIVED >=60-block floor (RT-D) -----------------------------
def test_oos_confirms_requires_all_three():
    floors = dict(oos_net_ann=0.06, oos_t=2.5, power=0.7, cfg=CFG)  # all pass
    assert oos_confirms(**floors) is True
    # drop net below +5%/yr
    assert oos_confirms(oos_net_ann=0.04, oos_t=2.5, power=0.7, cfg=CFG) is False
    # drop t below 2.0
    assert oos_confirms(oos_net_ann=0.06, oos_t=1.9, power=0.7, cfg=CFG) is False
    # drop power below 0.5
    assert oos_confirms(oos_net_ann=0.06, oos_t=2.5, power=0.4, cfg=CFG) is False


def test_oos_confirms_boundaries_inclusive():
    # exactly at each floor confirms (>= semantics)
    assert oos_confirms(oos_net_ann=CFG.m2_net_ann_floor, oos_t=CFG.m2_t_floor,
                        power=CFG.oos_power_floor, cfg=CFG) is True


def test_oos_confirms_does_not_impose_60_block_floor():
    # ★ RT-D: a 56-block OOS leg with net + t + power must still CONFIRM.
    # oos_confirms takes no block-count arg at all -> the >=60 floor (m2_min_blocks=60) is WAIVED.
    assert CFG.m2_min_blocks == 60
    # simulate the ~56-block OOS: compute power on 56 blocks from a healthy IS Sharpe...
    p = oos_power(is_sharpe=2.0, oos_n_blocks=56, cfg=CFG)["power"]
    assert p >= CFG.oos_power_floor   # 56 blocks is enough power at this effect
    # ...and confirm with passing net + t despite < 60 blocks
    assert oos_confirms(oos_net_ann=0.08, oos_t=2.3, power=p, cfg=CFG) is True


# ---- (4) regime split at 2001 / 2010 -------------------------------------------------------------
def test_oos_regime_splits_at_bounds():
    dates = pd.to_datetime([
        "1999-06-30",   # pre2001
        "2000-12-31",   # pre2001
        "2001-01-01",   # boundary -> 2001-2009 (inclusive lower edge)
        "2005-07-04",   # 2001-2009
        "2009-12-31",   # 2001-2009
        "2010-01-01",   # boundary -> >=2010 (inclusive lower edge)
        "2020-03-23",   # >=2010
    ])
    reg = oos_regime(dates, CFG)
    assert list(reg) == ["pre2001", "pre2001", "2001-2009", "2001-2009",
                         "2001-2009", ">=2010", ">=2010"]
    # only the three pinned labels appear, and they match the schema bounds
    assert set(reg.unique()) <= {"pre2001", "2001-2009", ">=2010"}
    assert CFG.oos_regime_bounds == ("2001-01-01", "2010-01-01")


def test_oos_regime_on_synthetic_span():
    # an end-to-end-ish span across all three regimes lands in all three buckets
    dates = pd.bdate_range("1999-01-04", "2024-12-31")
    reg = oos_regime(dates, CFG)
    counts = reg.value_counts()
    assert counts["pre2001"] > 0 and counts["2001-2009"] > 0 and counts[">=2010"] > 0
    # the >=2010 holdout is the primary regime per cfg
    assert CFG.oos_primary_regime in set(reg.unique())
