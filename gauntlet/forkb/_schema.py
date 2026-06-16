"""Cross-sectional gauntlet apparatus — the frozen config + snapshot schema contract.

This is the SINGLE interface every leaf module builds against. It encodes the pre-registered
parameters so the gate is sha256-freezable: change a value here = a new freeze. Nothing here
peeks at data; the apparatus is built and tested on synthetic ground-truth (synth_fixtures.py)
BEFORE any data spend, so the post-subscribe path collapses to pull -> verify -> data checks ->
freeze -> run.

Provenance of every value is the pre-registration pin it implements (cited inline). The handful of
data-semantic switches that resolve at subscribe-time empirical checks (the Canary primary
total-return path; sector-is-current-only) are flagged `PHASE0_` and default to the likely
resolution — they flip a flag, not the apparatus structure.
"""
from __future__ import annotations

from dataclasses import dataclass, field, asdict
import hashlib
import json
from typing import Optional

import pandas as pd


# ----------------------------------------------------------------------------------------------------
# Snapshot — the survivorship-clean PIT licensed-data tables (DataFrames), with pinned column contracts.
# synth_fixtures.py produces EXACTLY these columns; the real pull (the production loader) produces a superset.
# ----------------------------------------------------------------------------------------------------
# Column contracts (the columns the apparatus reads; real licensed-data tables carry more):
#   sep      : ticker, date, open, high, low, close, closeadj, closeunadj, dividends, volume
#              - close/OHLC = split + STOCK-div adjusted (NOT cash-div); closeadj = total-return
#                (cash-div adj) [PHASE0-VERIFY]; dividends = cash-div on the ex-date (raw, 0 else)
#   sf1      : ticker, dimension, datekey, reportperiod, lastupdated, epsdil, eps, sharesbas
#              - dimension in {ARQ,ARY,ART,MRQ,...}; datekey = availability/filing date (PIT);
#                reportperiod = fiscal period end; lastupdated = last revision (restatement detector)
#   actions  : ticker, date, action, value, contraticker
#              - action in {dividend, split, delisted, spinoff, ...}; date = ex-date for dividends;
#                value = cash-div amount / delist terminal value / deal price; contraticker = acquirer
#   daily    : ticker, date, marketcap, pb, pe, ev
#   tickers  : ticker, isdelisted, firstpricedate, lastpricedate, sector, industry, category
#   sp500    : date, action, ticker   (action in {added, removed}) — PIT membership
@dataclass
class Snapshot:
    sep: pd.DataFrame
    sf1: pd.DataFrame
    actions: pd.DataFrame
    daily: pd.DataFrame
    tickers: pd.DataFrame
    sp500: pd.DataFrame

    REQUIRED = {
        "sep": ["ticker", "date", "open", "high", "low", "close", "closeadj", "closeunadj", "dividends", "volume"],
        "sf1": ["ticker", "dimension", "datekey", "reportperiod", "lastupdated", "epsdil", "eps", "sharesbas",
                "assets", "revenue", "equity", "gp"],   # assets -> CMA (asset-growth); gp/assets -> RMW (Novy-Marx)
        "actions": ["ticker", "date", "action", "value", "contraticker"],
        "daily": ["ticker", "date", "marketcap", "pb", "pe", "ev"],
        "tickers": ["ticker", "isdelisted", "firstpricedate", "lastpricedate", "sector", "industry", "category"],
        "sp500": ["date", "action", "ticker"],
    }

    def validate(self) -> None:
        """Assert every required column is present + date columns are datetime64. Raises on contract breach."""
        for tbl, cols in self.REQUIRED.items():
            df = getattr(self, tbl)
            missing = [c for c in cols if c not in df.columns]
            if missing:
                raise ValueError(f"Snapshot.{tbl} missing columns: {missing}")
            for dcol in ("date", "datekey", "reportperiod", "lastupdated", "firstpricedate", "lastpricedate"):
                if dcol in df.columns and not pd.api.types.is_datetime64_any_dtype(df[dcol]):
                    raise ValueError(f"Snapshot.{tbl}.{dcol} must be datetime64 (got {df[dcol].dtype})")


# ----------------------------------------------------------------------------------------------------
# ForkBConfig — every frozen, sha256-able parameter, cited to the charter §/pin it implements.
# ----------------------------------------------------------------------------------------------------
@dataclass(frozen=True)
class ForkBConfig:
    # --- liquid screen (formation-once; §2 H1 / D-HARD-5) -------------------------------------------
    marketcap_floor: float = 300e6          # §2 H1: marketcap >= $300M, as-of formation
    price_floor: float = 5.0                # §2 H1: close >= $5
    adv_floor: float = 1e6                  # §2 H1: 60-td median $ADV >= $1M
    adv_window_td: int = 60
    closeadj_floor: float = 1.0             # ★ Phase-0 (DEC-329): closeadj >= $1 precision screen — heavily-adjusted
    #   names get a tiny closeadj (NSTR ~$0.009) where the 0.001 rounding step (~11%) corrupts the Path-A TR; exclude
    #   those name-days from the TRADED universe + the Canary (13/11,800 names, ~0.1% of name-days — immaterial sample)
    tr_clip_daily: Optional[float] = None   # ★ Phase-0 (DEC-330, S2 verdict-validation): clip per-name daily BOOK TR to
    #   +-this before book construction — delist terminal-value overrides + closeadj reissue/Q-stub discontinuities
    #   produce absurd returns (SDOCQ +1.2M%, daily_raw std 139% for a dollar-neutral book). None = charter/synthetic
    #   (clean); the licensed-data run sets 0.50 (a real daily equity TR >50% is an event/artifact, not a PEAD signal)

    # --- H1 PEAD / SUE (§2 H1; DS-13; RT-A) --------------------------------------------------------
    sue_dim_primary: str = "ARQ"            # As-Reported Quarterly; ARY for annual-only filers
    sue_dim_annual: str = "ARY"
    eps_field: str = "epsdil"               # diluted (conservative; avoids buyback inflation) — S2 open-Q2
    sue_yoy_lag_q: int = 4                  # UE = epsdil_t - epsdil_{t-4} (fiscal-quarter aligned)
    sue_sigma_window_q: int = 8             # rolling std of own past UE over prior 8q
    sue_sigma_min_q: int = 6                # min surviving quarters to form sigma
    sue_winsor: tuple = (-5.0, 5.0)         # per-cross-section
    restatement_K_days: int = 120           # lastupdated > datekey + K -> back-filled -> DROP the name (DS-13)
    sue_restatement_drop_lastupdated: bool = True  # ★ Phase-0: licensed-data `lastupdated` is the DB-REFRESH date (median
    #   lag ~4600d, 100% of pre-2015 rows > K), NOT a per-filing restatement signal -> SET FALSE for the licensed-data run
    #   (ARQ = "excluding restatements" already gives as-first-reported PIT safety; the lastupdated-drop is vendor-specific)
    rank_datekey_lag_td: int = 1            # rank set = firms with datekey <= D-1 (no same-day-after-close peek)
    entry_lag_td: int = 1                   # gated entry = close of D+1
    n_quantiles: int = 5                    # quintiles (top/bottom 20%)
    hold_td: int = 63                       # ~3-month hold
    rebalance_td: int = 21                  # overlapping monthly cohorts (LdP uniqueness-weighted returns)
    single_name_weight_cap: Optional[float] = None  # ★ Phase-0 (S2-RATIFIED 0.05/both legs/pro-rata+EW-fallback):
    #   None = pure VW (charter/synthetic default + DS-12 diagnostic); on real data VW concentrates the leg into ~1
    #   mega-cap (weight median 26%, max 93%) -> P3-non-vacuity + construct-validity failure (NOT an eff-N-gate issue,
    #   which is weighting-invariant). The licensed-data run sets 0.05 (excess redistributed pro-rata). DS-12: EW/cap/VW gradient.
    min_leg_names: int = 2                   # ★ S2 C2 / §7-pin-1: min names per quintile leg (floor = min_leg_names*q
    #   candidates). Default 2 = charter/synthetic; the licensed-data run sets 21 so the 0.05 cap BINDS (leg>20) -> max
    #   single-name weight <=0.05 -> the C-2 P3 squeeze stays ~3.8% (the EW-fallback can't un-bound it on a thin cohort).

    # --- H2 NSI / buyback (DIAGNOSTIC; §2 H2; DS-5) ------------------------------------------------
    nsi_shares_field: str = "sharesbas"     # split-adjusted period-end shares (Pontiff-Woodgate) — S2 open-Q2
    nsi_yoy_q: int = 4
    nsi_rebalance_month: int = 6            # end-of-June (Fama-French standard, NOT Dec-31)

    # --- BC-NSI-BUYBACK gauntlet (DEC-333; S2 charter-review v2) ------------------------------------
    nsi_yoy_q_quarterly: int = 1            # QoQ-non-overlap GATED cadence: genuinely-independent 1q share change
    #   (vs the autocorrelated YoY-measured-quarterly false-power-pass S2 banned). nsi_book(cadence='qoq_nonoverlap').
    # ★ Interpretation residual (S2 #1) — NON-GATING. The 5-tuple `factors` stays the GATING set (resid → M2/verdict);
    #   these 7 only LABEL the verdict NOVEL vs KNOWN-FACTOR. NSI ≈ CMA, so CMA/RMW must NEVER enter the gated residual
    #   (that would strip the signal → a by-construction false-NULL, the inverse of the RT-A scar).
    interp_factors: tuple = ("size", "value", "mom", "str", "beta", "cma", "rmw")
    cma_field: str = "assets"               # CMA (investment) = log(assets_t / assets_{t-4q}); conservative-minus-aggressive
    cma_yoy_q: int = 4
    rmw_gp_field: str = "gp"                # RMW (profitability, Novy-Marx) = gp / assets; robust-minus-weak
    rmw_assets_field: str = "assets"
    fundamental_max_stale_days: int | None = None   # ★ BD S2-AMEND (DEC-338): drop fundamental chars staler than N
    #   CALENDAR days (the wave-2 screen capped at 400d; default None = unbounded ffill, so BC/interp are unaffected)
    known_factor_beta_threshold: float = 0.5    # |cma_beta| > this OR explained-frac > this → label KNOWN-FACTOR (else NOVEL)
    known_factor_explained_frac: float = 0.5
    cost_calibration_set: tuple = ()        # ★ D-HARD D: fitted decile cost multipliers; require_calibration=True at the
    #   gauntlet call. Empty = placeholder (CS×1.75) which "must not produce a deployable verdict" — freeze_ready blocks it.

    # --- H3 tax-loss (DIAGNOSTIC; §2 H3) -----------------------------------------------------------
    tl_size_bottom_deciles: int = 2         # small/micro-cap, as-of formation
    tl_loser_decile: int = 10               # most-negative trailing-TR decile
    tl_price_floor_primary: float = 5.0     # $5 primary; $1 reported
    tl_price_floor_reported: float = 1.0
    tl_lookback_skip_recent_month: bool = True   # ~11-mo TR through end-Oct

    # --- block unit (DH-A / RT-B / RT-C) -----------------------------------------------------------
    block_td: int = 63                      # non-overlapping 63-td blocks = the time-series-independent unit
    block_anchor: str = "first_deployable_day_of_regime"   # RT-C
    drop_trailing_partial_block: bool = True               # RT-C
    block_rho_threshold: float = 0.2        # RT-B: |lag-1 block-rho| > 0.2 -> apply the 1-lag block-NW
    block_nw_lag: int = 1                   # Bartlett L=1, on the ~104-pt residual block series (NOT daily HAC)

    # --- M2 deployability gate (§4; v3.8-H; DS-4) --------------------------------------------------
    m2_net_ann_floor: float = 0.05          # net >= +5%/yr (residual daily series annualized)
    m2_t_floor: float = 2.0                 # plain t on the residual block series (block-agg obviates HAC)
    m2_min_blocks: int = 60                 # >=60 TIME-SERIES independent blocks (full-sample; WAIVED for OOS — RT-D)

    # --- overfit / significance (N1 / DH-A / DS-4) -------------------------------------------------
    psr_threshold: float = 0.95             # PSR(SR*=0) via carry_gate.psr_spell on the residual block series
    psr_sr_star: float = 0.0
    dsr_is_gate: bool = False               # DSR = REPORTED, not a gate (RT-E)

    # --- eff-N (necessary-not-sufficient noise screen; §4) -----------------------------------------
    effn_floor: float = 3.84                # chi2(1,.05); cross-sectional, on RAW; NOT the binding breadth gate

    # --- permutation null (D-HARD-4 / R3 / DS-C) ---------------------------------------------------
    perm_n: int = 1000                      # iterations
    perm_p_threshold: float = 0.05          # one-sided: real block-Sharpe > 95th pctile of the embargoed null
    perm_embargo_td: int = 63               # >= hold horizon

    # --- OOS (D-HARD-3 / R1 / N2 / RT-D) -----------------------------------------------------------
    oos_regime_bounds: tuple = ("2001-01-01", "2010-01-01")   # 3 regimes: pre-2001 / 2001-09 / >=2010
    oos_primary_regime: str = ">=2010"
    oos_effect_haircut: float = 0.5         # FIXED post-decay effect = IS-Sharpe * 0.5 (NOT IS-slope-derived)
    oos_power_z: float = 1.645              # one-sided alpha=0.05 (the literal z; power_gate Z_SUM precedent)
    oos_power_floor: float = 0.5            # power < 0.5 -> INDETERMINATE (not falsified)
    oos_waive_block_floor: bool = True      # RT-D: the >=60-block floor is WAIVED for the ~56-block OOS leg

    # --- factor attribution (D-HARD-6 / N3 / N6 / open-Q3) -----------------------------------------
    factors: tuple = ("size", "value", "mom", "str", "beta")  # PIT-formed; verdict on the factor-RESIDUAL
    pit_factor_formation: bool = True       # N6: formed on as-of-date characteristics
    report_mom_only_leg: bool = True        # open-Q3: PEAD-residual-vs-MOM-only attribution leg

    # --- cost model (§5; DS-2/6/7/8) ---------------------------------------------------------------
    cost_uplift_floor_mult: float = 1.75    # max(CS*1.75, external-anchor, data-derived-uplift)
    cost_neg_spread_two_day_correction: bool = True   # NOT floor-to-zero
    cost_opposite_side_fills: bool = True   # buy ask / sell bid -> bounce paid, not harvested
    borrow_bps_floor: float = 200.0         # >= 200 bps small-cap HTB (marketcap-decile blend)
    # external decade anchor (mandatory max() floor) — bps by decade, set at Phase-0; placeholder pinned here
    cost_external_anchor_bps: tuple = (320.0, 180.0, 110.0, 70.0, 45.0, 28.0, 18.0, 12.0, 8.0, 5.0)  # ★ decile 0 =
    #   LEAST liquid = MOST expensive ... decile 9 = most liquid = cheapest — MUST match _liquidity_decile's 0=least
    #   (was ascending = REVERSED: it charged microcaps 5bps + megacaps 320bps; masked by CS dominating on synth)

    # --- P3 crash-survival (DS-3; per-book) --------------------------------------------------------
    # dollar-neutral-recalibrated thresholds (the charter flags the inherited set as "false comfort";
    # the recalibrated values are a freeze-time pin — these are the pinned defaults, override at Step-0c).
    p3_calmar_floor: float = 0.5            # RATIFY (S2 C-1): market-neutral deployability min on the stressed series
    p3_worst_episode_floor: float = 0.20    # AMEND (S2 C-1): == the |maxdd| floor magnitude (same metric — NIT)
    p3_maxdd_floor: float = -0.20           # AMEND (S2 C-1): binds the ~10-20% quant-quake region (was -0.25 "false comfort")
    p3_recovery_max_months: int = 24        # RATIFY (S2 C-1)
    p3_sortino_floor: float = 0.0           # RATIFY (S2 C-1): survival gate, not performance (M2 owns performance)
    p3_ann: int = 252
    p3_stress_windows: tuple = (("2007-08-06", "2007-08-10"), ("2021-01-25", "2021-02-01"))
    p3_single_name_gap: tuple = (0.50, 1.00)   # 50-100% SINGLE-NAME squeeze gap; book impact = gap × short-leg weight (C-2)
    p3_short_name_weight_default: float = 0.10  # C-2 fallback single-name short-leg weight (gauntlet passes the actual max)

    # --- gated family (frozen pre-data; DS-9) ------------------------------------------------------
    gated: tuple = ("H1",)                  # {H1 PEAD} only; H2/H3/H4 = diagnostics (M2-exempt, NOT Holm)
    diagnostics: tuple = ("H2", "H3", "H4")

    # --- Phase-0 data-semantic switches (resolve at subscribe-time; flip a flag, not the structure) -
    PHASE0_closeadj_is_total_return: Optional[bool] = None   # Canary resolves: True -> Path A primary
    # ★ Canary part (ii): the MANDATORY independent external TR anchor (>=3 hand-verified total returns
    # from a second free source) — guards the A/B common-mode error (both A and B source the same vendor). The
    # VALUES are a Phase-0 manual step; this slot reserves the hook. canary_external_anchor_resolved()
    # must be True before the Step-0c freeze (a WARNING pre-spend; a HALT at freeze if still empty).
    PHASE0_canary_external_anchor: tuple = ()   # ((ticker, start, end, known_cum_TR, kind), ...) Phase-0; kind in
    #   {'ordinary','special'}; MUST span >=1 ordinary AND >=1 special (the common-mode error hides in special-div — S2 D)
    canary_anchor_tol_bps: float = 50.0         # Phase-0 calibrated: cross-vendor cum-TR agrees to ~20bps (KO 1/PG 18/
    #   ABT 23 vs Yahoo) -> 50bps catches a systematic error with margin w/o false-HALT on vendor noise (was 1bp; S2 D)
    # --- Canary real-data reconciliation (Phase-0 calibrated on the live snapshot) ---
    canary_perday_tol: float = 1e-2             # per-day A-vs-B tol on the TRADEABLE (close>=price_floor) universe;
    #   above $5-stock price-rounding (~1e-3) yet below a real distribution/corruption gap
    canary_gross_gap: float = 0.10             # an UNEXPLAINED gap > this on a tradeable name = real corruption -> HALT
    canary_unexplained_rate_max: float = 5e-4   # HALT if the unexplained-disagreement RATE exceeds this fraction of
    #   tradeable name-days (a systematic problem); a sparse benign tail (recent-data lag / untyped specials) does not HALT

    def sha256(self) -> str:
        """Deterministic config hash for the Step-0c freeze (verdict-numbers-from-source discipline)."""
        return hashlib.sha256(json.dumps(asdict(self), sort_keys=True, default=str).encode()).hexdigest()

    def canary_external_anchor_resolved(self) -> bool:
        """Canary part (ii) — S2 D: >=3 independent external TR anchors set AND spanning >=1 ordinary AND
        >=1 special dividend (presence alone is insufficient; the common-mode error hides in special-div handling)."""
        a = self.PHASE0_canary_external_anchor
        if len(a) < 3:
            return False
        kinds = {row[4] for row in a if len(row) >= 5}
        return ("ordinary" in kinds) and ("special" in kinds)

    def freeze_ready(self, require_cost_calibration: bool = False) -> tuple[bool, list[str]]:
        """Step-0c gate: every Phase-0 data-semantic switch resolved. Returns (ok, unresolved-list).
        A WARNING pre-spend; must be (True, []) before the sha256 freeze + the once-only run.
        `require_cost_calibration=True` (the BC-NSI gated run, DEC-333) also requires the fitted cost calibration_set."""
        unresolved = []
        if self.PHASE0_closeadj_is_total_return is None:
            unresolved.append("PHASE0_closeadj_is_total_return (Canary primary TR path)")
        if not self.canary_external_anchor_resolved():
            unresolved.append("PHASE0_canary_external_anchor (>=3 anchors spanning ordinary+special — S2 D)")
        if self.perm_embargo_td < self.hold_td:                       # ★ DEC-327 embargo-on-long-hold bug guard
            unresolved.append(f"perm_embargo_td ({self.perm_embargo_td}) >= hold_td ({self.hold_td})")
        if require_cost_calibration and not self.cost_calibration_set:  # ★ D-HARD D (BC gated run)
            unresolved.append("cost_calibration_set (D-HARD D: require_calibration for a deployable-grade verdict)")
        return (len(unresolved) == 0, unresolved)


# regime labels derived from oos_regime_bounds, for the OOS / calendar-regime split (N2)
def regime_of(dates: pd.Series, cfg: ForkBConfig) -> pd.Series:
    lo, hi = (pd.Timestamp(b) for b in cfg.oos_regime_bounds)
    d = pd.to_datetime(dates)
    out = pd.Series(f"pre{lo.year}", index=d.index)            # ★ S2 AMEND-3: labels DERIVED from the bounds (no hard-code)
    out[d >= lo] = f"{lo.year}-{hi.year - 1}"                  #   default bounds (2001,2010) -> identical "2001-2009"/">=2010"
    out[d >= hi] = f">={hi.year}"                              #   BD bounds (2001,2013) -> "2001-2012"/">=2013"
    return out
