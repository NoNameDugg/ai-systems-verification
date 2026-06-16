"""The gated gauntlet: compose the leaf gates into the verdict terminal table.

Per-gate return series: perm-null / P3 / eff-N on RAW; M2-t + PSR + the deployability
verdict on the FACTOR-RESIDUAL block-net; OOS-M2 on the residual with beta RE-ESTIMATED in-regime.
Order: Canary (HALT) -> book -> costs (net) -> factor-residual (alpha RETAINED) -> block unit ->
{M2, PSR, perm-null, eff-N, P3} -> calendar-regime OOS power -> terminal table.
Runs ONCE on the frozen snapshot. Verdict numbers from source.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot, regime_of
import tr_canary
import factor_resid
import cost as cost_mod
import blocks as blocks_mod
import psr_gate as psr_mod
import permnull
import oos_power as oos_mod
import p3 as p3_mod
import harness
import dataclasses
import fundamental_factors as ff_mod


def _eff_n(name_daily: pd.DataFrame, labels_union: set) -> float:
    """Cross-sectional eff-N (eigenvalue participation ratio) of the traded names' return corr matrix.
    Noise screen (necessary-not-sufficient; charter §4). >3.84 = more than ~1 factor above noise."""
    cols = [c for c in name_daily.columns if c in labels_union]
    R = name_daily[cols].dropna(how="all").dropna(axis=1, how="any")
    if R.shape[1] < 2 or R.shape[0] < 10:
        return float("nan")
    C = np.corrcoef(R.values, rowvar=False)
    w = np.linalg.eigvalsh(C)
    w = w[w > 0]
    return float((w.sum() ** 2) / (w ** 2).sum())


def _block_sharpe(block_returns: np.ndarray) -> float:
    b = np.asarray(block_returns, float)
    return float(b.mean() / b.std(ddof=1)) if b.size > 2 and b.std(ddof=1) > 0 else float("nan")


def _verdict_terminal(full_series_pass: bool, m2_power: bool, oos_conf: bool, oos_indeterminate: bool,
                      label: str, book: str, cost_mode) -> str:
    """The charter §4/§7 terminal verdict table (pure + testable). ★ DEC-340 carry-forward (S2): a KNOWN-FACTOR book
    (nsi≈CMA, prof=RMW) that the interp residual mislabels NOVEL is a CONSTRUCTION ARTIFACT (BD: VW-capped-quintile book
    vs EW-tercile factor → near-zero rmw_beta + negative explained_frac), NOT a genuine discovery — it must NOT be
    crowned a bare DEPLOY; route it to PROMISING-PENDING-INVESTIGATION (reconcile the construction before any deploy)."""
    if not full_series_pass:
        return "NULL"
    if not m2_power:                                          # econ+sig+psr+perm+p3 pass but underpowered (active n<60)
        return "PROMISING-UNCONFIRMED"
    if oos_conf:
        if book in ("nsi", "prof") and label == "NOVEL":     # ★ known-factor book mislabeled NOVEL ⇒ construction artifact
            return "PROMISING-PENDING-INVESTIGATION"
        verdict = "DEPLOY-as-known-factor" if label == "KNOWN-FACTOR" else "DEPLOY"
        if book in ("nsi", "prof") and str(cost_mode or "").startswith("PLACEHOLDER"):   # fence (a): placeholder ⇒ conditional
            verdict += "-PENDING-COST-CALIBRATION"
        return verdict
    if oos_indeterminate:
        return "PROMISING-UNCONFIRMED"
    return "OOS-FALSIFIED-NULL"


def run_gauntlet(snap: Snapshot, cfg: ForkBConfig, weighting: str = "VW", perm_n: int | None = None,
                 book: str = "pead", cadence: str = "annual_june",
                 require_cost_calibration: bool = False) -> dict:
    out: dict = {"config_sha256": cfg.sha256(), "book": book, "cadence": cadence}

    # ★ BC pre-flight (DEC-333): fail loud, not silent. perm-embargo must cover the hold (DEC-327 embargo-leak bug).
    if cfg.perm_embargo_td < cfg.hold_td:
        raise ValueError(f"perm_embargo_td ({cfg.perm_embargo_td}) must be >= hold_td ({cfg.hold_td}) (DEC-327)")

    # 0) Canary HALT-gate (gates everything)
    canary = tr_canary.run_canary(snap, cfg)
    out["canary"] = canary
    if canary.get("halt"):
        out["verdict"] = "HALT-CANARY"
        return out

    # 1) book — H1 PEAD (pead_book) / BC NSI (nsi_book) / BD profitability (profitability_book) — both emit active_mask
    if book == "nsi":
        bk = harness.nsi_book(snap, cfg, weighting=weighting, cadence=cadence)
    elif book == "prof":
        bk = harness.profitability_book(snap, cfg, weighting=weighting)
    else:
        bk = harness.pead_book(snap, cfg, weighting=weighting)
    active_mask = bk.get("active_mask")                       # None for pead (always-in-position overlapping cohorts)
    out["n_formations"] = bk["n_formations"]
    out["max_short_weight"] = bk["max_short_weight"]        # S2 C2: report the C-2 squeeze-injector weight input
    if "stale_dropped_frac" in bk:                          # ★ BD S2-AMEND: stale-incidence (how many slots the cap dropped)
        out["stale_dropped_frac"] = bk["stale_dropped_frac"]
        out["stale_dropped_total"] = bk["stale_dropped_total"]
        out["n_cand_pool_total"] = bk["n_cand_pool_total"]
    raw = bk["daily_raw"].dropna()
    if raw.empty or bk["n_formations"] < 4:
        out["verdict"] = "NULL"; out["reason"] = "no book"; return out

    # ★ G.3 P3 non-vacuity (S2 N2: tie to the GATED QoQ path, not the cost flag — the annual diagnostic is
    #   capped-PROMISING so its vacuous-COVID window is acceptable; the gated verdict must NOT be vacuously stressed).
    if (book == "nsi" and cadence == "qoq_nonoverlap") or book == "prof":   # ★ prof is rolling/continuous → must stress real windows
        act_dates = raw.index[active_mask.reindex(raw.index).fillna(False).values] if active_mask is not None else raw.index
        for (lo, hi) in cfg.p3_stress_windows:
            if not ((act_dates >= pd.Timestamp(lo)) & (act_dates <= pd.Timestamp(hi))).any():
                raise ValueError(f"P3 stress window {lo}..{hi} hits no in-position day (vacuous P3); re-pin windows")

    # 2) costs -> net (raw book, charged turnover + short borrow)
    band = cost_mod.conservative_band_bps(snap.sep, snap.daily, cfg,
                                          calibration_set=(cfg.cost_calibration_set or None),
                                          require_calibration=require_cost_calibration)   # ★ D-HARD D (BC gated run)
    # ★ E-2 (S2): charge the PER-LEG band (weight the illiquid short leg in), NOT the global-median scalar
    fl = list(bk["formation_labels"].values())
    long_names = set().union(*[set(l[l > 0].index) for l in fl]) if fl else set()
    short_names = set().union(*[set(l[l < 0].index) for l in fl]) if fl else set()

    def _legband(names):
        b = band.reindex(list(names)).dropna() if names else band.dropna()
        return float(b.mean()) if len(b) else 50.0
    long_band, short_band = _legband(long_names), _legband(short_names)
    band_bps = 0.5 * (long_band + short_band)            # dollar-neutral: ~half long / half short turnover
    out["cost_band_bps"] = band_bps; out["cost_long_band_bps"] = long_band; out["cost_short_band_bps"] = short_band
    # ★ E-1 (S2): the data-derived decile-uplift is a Phase-0/run-time fit; until then this is the CS+anchor PLACEHOLDER
    # (CS×1.75 + the external-anchor max() floor). A deployable-grade verdict REQUIRES a fitted decile multiplier.
    out["cost_mode"] = ("CALIBRATED(fitted decile-multiplier)" if (cfg.cost_calibration_set and require_cost_calibration)
                        else "PLACEHOLDER(CSx1.75+anchor; data-derived decile-uplift calibration REQUIRED for deployable-grade)")
    net = cost_mod.apply_costs(raw, bk["turnover_daily"].reindex(raw.index).fillna(0.0),
                               bk["short_gross_daily"].reindex(raw.index).fillna(0.0), band_bps, cfg)

    # 3) factor-residual (alpha RETAINED — RT-A) on the NET book — the GATED 5-factor residual (feeds M2/PSR/verdict)
    fac = factor_resid.pit_factor_returns(snap, cfg)
    res = factor_resid.residualize(net, fac, cfg)
    resid = res["resid"].dropna()
    out["alpha_daily"] = float(res["alpha"])
    out["alpha_t"] = float(res["alpha_t"])
    out["resid_betas"] = res["betas"]            # ★ §4 deliverable (S2-folded): the GATED 5-factor loadings (no RMW/CMA)

    # ★ G.1 INTERPRETATION residual (NON-GATING): adds CMA/RMW only to LABEL novel-vs-known-factor. NSI≈CMA, so
    #   gating on it would strip the signal → a by-construction false-NULL (inverse-RT-A). It NEVER feeds the verdict.
    interp_F = ff_mod.interp_factor_returns(snap, cfg)
    interp = factor_resid.residualize(net, interp_F, dataclasses.replace(cfg, factors=cfg.interp_factors))
    _cb, _rb = float(interp["betas"].get("cma", 0.0)), float(interp["betas"].get("rmw", 0.0))
    _expl = (1.0 - abs(float(interp["alpha"])) / abs(float(res["alpha"]))) if abs(float(res["alpha"])) > 1e-12 else 0.0
    _known = (abs(_cb) > cfg.known_factor_beta_threshold) or (_expl > cfg.known_factor_explained_frac)
    out["interpretation"] = {"cma_beta": _cb, "rmw_beta": _rb, "explained_frac": _expl,
                             "label": "KNOWN-FACTOR" if _known else "NOVEL"}

    # 4) block unit (the time-series-independent N) on the residual net. ★ For the NSI book, count ACTIVE blocks
    #    ONLY (S2 #3 / D-HARD B): zero-filled flat days must not inflate the count past m2_min_blocks (landmine #2:
    #    the count gates M2 on the RESIDUAL active series, not raw).
    if active_mask is not None:
        am = active_mask.reindex(resid.index).fillna(False).values
        resid_active = resid[am]
    else:
        resid_active = resid
    resid_blocks = blocks_mod.to_blocks(resid_active, cfg)
    bt = blocks_mod.block_t(resid_blocks, cfg)
    out["n_blocks"] = bt["n"]; out["n_active_blocks"] = bt["n"]
    out["block_rho"] = bt["rho"]; out["block_nw_applied"] = bt["nw_applied"]
    net_ann = float(resid_active.mean() * 252)
    out["net_ann"] = net_ann; out["resid_block_t"] = bt["t"]

    # ★ S2 fence (b): report GROSS + the realistic-35bps net alongside the (conservative/placeholder) gated net, so a
    #   cost-driven NULL is distinguishable from a no-signal NULL (the DEC-330 cost-inflation caveat, sharper on QoQ).
    if book in ("nsi", "prof"):
        gross_active = raw.reindex(resid.index)
        if active_mask is not None:
            gross_active = gross_active[active_mask.reindex(resid.index).fillna(False).values]
        out["gross_ann"] = float(gross_active.mean() * 252) if len(gross_active) else float("nan")
        net35 = cost_mod.apply_costs(raw, bk["turnover_daily"].reindex(raw.index).fillna(0.0),
                                     bk["short_gross_daily"].reindex(raw.index).fillna(0.0), 35.0, cfg)
        r35 = factor_resid.residualize(net35, fac, cfg)["resid"].dropna()
        if active_mask is not None:
            r35 = r35[active_mask.reindex(r35.index).fillna(False).values]
        out["net_ann_realistic_35bps"] = float(r35.mean() * 252) if len(r35) else float("nan")
        # ★ BD §5.1 / S2 BLOCK-1: the gated `net_ann` already uses NET turnover (turnover_daily). ALSO report the OLD
        #   per-cohort gross-turnover net — shows the BLOCK-1 over-charge (gate is on net-turnover; this is context only).
        if "turnover_daily_gross" in bk:
            net_gt = cost_mod.apply_costs(raw, bk["turnover_daily_gross"].reindex(raw.index).fillna(0.0),
                                          bk["short_gross_daily"].reindex(raw.index).fillna(0.0), band_bps, cfg)
            rgt = factor_resid.residualize(net_gt, fac, cfg)["resid"].dropna()
            if active_mask is not None:
                rgt = rgt[active_mask.reindex(rgt.index).fillna(False).values]
            out["net_ann_gross_turnover"] = float(rgt.mean() * 252) if len(rgt) else float("nan")

    # 5) gates
    # ★ G.5 split M2: econ+sig (deployability economics) vs power (block floor, on ACTIVE blocks). Conflating them
    #   (the old `and n>=60`) maps an underpowered-but-real book to NULL; here power gates DEPLOY, not full_series_pass.
    m2_econ_sig = (net_ann >= cfg.m2_net_ann_floor) and (bt["t"] >= cfg.m2_t_floor)
    m2_power = (bt["n"] >= cfg.m2_min_blocks)
    m2 = m2_econ_sig and m2_power
    psr = psr_mod.psr_gate(resid_blocks, cfg)
    raw_blocks = blocks_mod.to_blocks(raw.reindex(resid.index).fillna(0.0), cfg)
    out["raw_block_t"] = blocks_mod.block_t(raw_blocks, cfg)["t"]   # reported raw-vs-residual gradient (charter §5)
    pn = permnull.perm_null_p(bk["formation_labels"], bk["name_daily"], cfg,
                             **({"perm_n": perm_n} if perm_n else {}))
    effn = _eff_n(bk["name_daily"], set().union(*[set(l[l != 0].index) for l in bk["formation_labels"].values()]))
    p3 = p3_mod.p3_battery(net.reindex(raw.index).fillna(0.0), bk["short_leg_daily"].reindex(raw.index).fillna(0.0), cfg,
                           short_name_weight=bk.get("max_short_weight"))   # C-2: book impact = gap × squeezed-name weight
    out["M2"] = {"pass": bool(m2), "econ_sig": bool(m2_econ_sig), "power": bool(m2_power),
                 "net_ann": net_ann, "t": bt["t"], "n_blocks": bt["n"]}
    out["PSR"] = psr
    out["perm_null"] = {"p": pn.get("p"), "pass": bool(pn.get("p", 1.0) <= cfg.perm_p_threshold)}
    out["eff_n"] = {"value": effn, "pass": bool(effn > cfg.effn_floor) if effn == effn else False}
    out["P3"] = {"pass": bool(p3.get("passed")), **{k: p3.get(k) for k in ("calmar", "maxdd", "worst_episode", "sortino")}}

    # ★ G.5: full_series_pass uses econ+sig (NOT the power floor) so an underpowered-but-real book routes to
    #   PROMISING-UNCONFIRMED, not NULL. The power floor (m2_power) gates DEPLOY in the verdict terminal below.
    full_series_pass = bool(m2_econ_sig and psr.get("passed") and out["perm_null"]["pass"] and p3.get("passed"))

    # 6) calendar-regime OOS (the cfg.oos_primary_regime leg — BC/pead = >=2010, BD = >=2013), beta re-estimated in-regime
    reg = regime_of(resid.index.to_series(), cfg)
    is_mask = (reg != cfg.oos_primary_regime).values
    oos_mask = (reg == cfg.oos_primary_regime).values
    is_blocks = blocks_mod.to_blocks(resid[is_mask], cfg) if is_mask.sum() > cfg.block_td else np.array([])
    is_sharpe = _block_sharpe(is_blocks) if is_blocks.size else _block_sharpe(resid_blocks)
    # OOS-M2 on the oos_primary_regime NET, beta re-estimated in-regime (align net to resid.index before masking)
    oos_net = net.reindex(resid.index)[oos_mask]
    oos_conf = False; oos_pw = {"power": float("nan"), "indeterminate": True}
    if oos_mask.sum() > cfg.block_td and len(oos_net) > cfg.block_td:
        oos_res = factor_resid.residualize(oos_net, fac.reindex(oos_net.index), cfg)["resid"].dropna()
        oos_blocks = blocks_mod.to_blocks(oos_res, cfg)
        oos_bt = blocks_mod.block_t(oos_blocks, cfg)
        oos_net_ann = float(oos_res.mean() * 252)
        oos_pw = oos_mod.oos_power(is_sharpe if is_sharpe == is_sharpe else 0.0, oos_bt["n"], cfg)
        oos_conf = oos_mod.oos_confirms(oos_net_ann, oos_bt["t"], oos_pw["power"], cfg)
        out["OOS"] = {"n_blocks": oos_bt["n"], "net_ann": oos_net_ann, "t": oos_bt["t"],
                      "power": oos_pw["power"], "indeterminate": oos_pw["indeterminate"], "confirms": oos_conf}
    else:
        out["OOS"] = {"n_blocks": int(oos_mask.sum() // cfg.block_td), "indeterminate": True, "confirms": False,
                      "note": "insufficient OOS span"}
    out["is_sharpe"] = is_sharpe

    # 7) verdict terminal table (charter §4 / N5 / DH-B; ★ G.5 underpowered branch BEFORE oos — landmine #1;
    #    ★ DEC-340: NOVEL-on-known-factor → PROMISING-PENDING-INVESTIGATION, not bare DEPLOY)
    out["verdict"] = _verdict_terminal(full_series_pass, m2_power, oos_conf, bool(out["OOS"].get("indeterminate")),
                                       out["interpretation"]["label"], book, out.get("cost_mode"))
    out["full_series_pass"] = full_series_pass
    out["underpowered"] = bool(not m2_power)
    return out
