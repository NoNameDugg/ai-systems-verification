"""TDI — fundamental_factors (CMA/RMW interpretation factors + the SF1-loader extension). DEC-333 Block E."""
from __future__ import annotations
import dataclasses
import numpy as np
import pandas as pd

import synth_fixtures as sf
from _schema import ForkBConfig
import fundamental_factors as ff
import factor_resid


def test_loader_required_fields_present_on_synth():
    snap, _ = sf.small()
    for c in ["assets", "revenue", "equity", "gp"]:
        assert c in snap.sf1.columns
    snap.validate()                                              # extended REQUIRED must still pass


def test_cma_char_recovers_injected_asset_growth():
    snap, truth = sf.small()
    chars = ff.pit_fundamental_chars(snap, ForkBConfig())
    cma = chars["cma"]
    checked = 0
    for t in cma.columns:
        col = cma[t].dropna()
        if len(col) < 50:
            continue
        assert abs(float(col.iloc[-1]) - truth["cma_growth"][t]) < 1e-3, (t, float(col.iloc[-1]), truth["cma_growth"][t])
        checked += 1
    assert checked >= 5


def test_rmw_char_recovers_injected_profitability():
    snap, truth = sf.small()
    rmw = ff.pit_fundamental_chars(snap, ForkBConfig())["rmw"]
    checked = 0
    for t in rmw.columns:
        col = rmw[t].dropna()
        if len(col) < 50:
            continue
        assert abs(float(col.iloc[-1]) - truth["rmw_level"][t]) < 1e-6, t   # gp/assets == rmw_level exactly
        checked += 1
    assert checked >= 5


def test_pit_no_future_filing_leaks():
    snap, _ = sf.small()
    cfg = ForkBConfig()
    full = ff.pit_fundamental_chars(snap, cfg)["cma"]
    cutoff = snap.sf1["datekey"].quantile(0.6)
    snap2 = dataclasses.replace(snap, sf1=snap.sf1[snap.sf1["datekey"] <= cutoff].copy())
    trunc = ff.pit_fundamental_chars(snap2, cfg)["cma"]
    dts = full.index[full.index <= cutoff]
    a = full.reindex(index=dts).reindex(columns=trunc.columns)
    b = trunc.reindex(index=dts)
    both = a.notna() & b.notna()
    assert int(both.values.sum()) > 0
    assert np.allclose(a.values[both.values], b.values[both.values], atol=1e-9)


def test_cma_rmw_factor_returns_contract():
    fr = ff.cma_rmw_factor_returns(*(sf.small()[0:1] + (ForkBConfig(),)))
    assert list(fr.columns) == ["cma", "rmw"]
    for c in ["cma", "rmw"]:
        v = fr[c].dropna()
        assert len(v) > 100 and bool(np.isfinite(v).all()) and float(v.std()) > 0.0


def test_interp_columns_and_first5_match_gating():
    snap, _ = sf.small()
    cfg = ForkBConfig()
    interp = ff.interp_factor_returns(snap, cfg)
    assert list(interp.columns) == list(cfg.interp_factors)
    base = factor_resid.pit_factor_returns(snap, cfg)
    for c in cfg.factors:                                        # the gating 5 are bit-identical
        common = interp[c].dropna().index.intersection(base[c].dropna().index)
        assert np.allclose(interp[c].reindex(common), base[c].reindex(common), atol=1e-12)


def test_residualize_recovers_interp_betas_end_to_end():
    snap, _ = sf.small()
    cfg = ForkBConfig()
    interp = ff.interp_factor_returns(snap, cfg).dropna()
    cfg_i = dataclasses.replace(cfg, factors=cfg.interp_factors)  # interp residual sees all 7 (NON-gating)
    rng = np.random.default_rng(0)
    known_beta = {c: float(rng.normal(0, 0.5)) for c in cfg.interp_factors}
    alpha_true = 0.0003
    raw = alpha_true + sum(known_beta[c] * interp[c] for c in cfg.interp_factors)
    out = factor_resid.residualize(raw, interp, cfg_i)
    assert abs(out["alpha"] - alpha_true) < 1e-6                  # RT-A: alpha retained
    for c in cfg.interp_factors:
        assert abs(out["betas"][c] - known_beta[c]) < 1e-3
