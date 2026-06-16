"""Leaf — total-return build + the Canary HALT-gate.

Two independent total-return (TR) paths per the CANARY pin:
  - **Path A** (primary, if `closeadj` is total-return — the Phase-0 likely resolution): `closeadj.pct_change()`
    per ticker. `closeadj` is the cash-dividend-adjusted (total-return), split-adjusted series.
  - **Path B**: rebuild TR from the raw split-adjusted price + cash dividend on one consistent basis:
    `(close.diff() + dividends) / close.shift()` per ticker. (`close`/OHLC are split+stock-div adjusted but
    NOT cash-div adjusted; `dividends` = the cash dividend on the ex-date, 0 otherwise.)

The Canary reconciles A vs B **per-name-per-day** with a zero-disagreement count: a correct rebuild
reconciles to ~0 everywhere; a dividend-forgotten rebuild (price-only `close.pct_change()`) diverges
on ex-dates by exactly `dividends/close.shift()` and is caught. HALT (`halt=True`) when the paths do
NOT reconcile (max abs gap > tolerance). A zero-eligible-names case is a WARNING, not a HALT.

The never-subtract-alpha scar is honored elsewhere; this module is pure TR-construction integrity — no
factor residualization happens here. Pure-function library over the contract (_schema/synth_fixtures + np/pd).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot

# per-name-per-day reconciliation tolerance for flagging a DISAGREEMENT day. On exact synthetic ground-truth the
# correct paths agree to float precision (~3e-16); on REAL data ordinary days agree to ~rounding, so 1e-3 (10bps)
# sits above microstructure/rounding yet below a real distribution gap or a genuine corruption (Phase-0 calibrated).
_RECONCILE_TOL = 1e-3
# ★ Real-data Canary refinement (Phase-0): Path B (close + CASH dividend) CANNOT see the NON-CASH distributions that
# `closeadj` DOES bake in (spinoffs/splits/mergers per the INDICATORS dict) -> those days legitimately diverge. So the
# HALT fires only on UNEXPLAINED disagreements (gap > tol AND no non-cash distribution within _DIST_WINDOW_DAYS for the
# ticker); spinoff/split/merger days are EXPECTED, not a corruption. (Synthetic has no such actions -> behaves as before.)
_NON_CASH_DISTRIBUTION_ACTIONS = frozenset({
    "spinoff", "spinoffdividend", "split", "adrratiosplit",
    "mergerfrom", "mergerto", "acquisitionby", "acquisitionof", "spunofffrom"})
_DIST_WINDOW_DAYS = 7   # calendar days around a non-cash distribution within which an A-vs-B gap is EXPLAINED


def _per_ticker_tr(g: pd.DataFrame, path: str) -> pd.Series:
    """Daily TR for one already-date-sorted ticker frame `g` via `path` in {'A','B','price_only'}."""
    if path == "A":
        return g["closeadj"].pct_change()
    prev_close = g["close"].shift()
    if path == "B":
        # (close_t - close_{t-1} + div_t) / close_{t-1}  — raw price + cash dividend, one split-adj basis
        return (g["close"].diff() + g["dividends"]) / prev_close
    if path == "price_only":
        # dividend-FORGOTTEN rebuild: pure price change, ignores the cash dividend (the bug the Canary catches)
        return g["close"].diff() / prev_close
    raise ValueError(f"unknown TR path: {path!r}")


def build_tr(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """Per-name daily TOTAL return.

    Path A (primary, default) when `cfg.PHASE0_closeadj_is_total_return is not False` — i.e. None (unresolved,
    the charter's likely resolution) or True both select Path A = `closeadj.pct_change()` per ticker.
    Otherwise Path B = `(close.diff() + dividends) / close.shift()`.

    Returns a long DataFrame with columns [ticker, date, tr] sorted by (ticker, date). The first day of each
    name carries NaN tr (no prior close) — kept as-is so downstream alignment is explicit.
    """
    use_path_a = cfg.PHASE0_closeadj_is_total_return is not False
    path = "A" if use_path_a else "B"
    return _build_path(snap, path)


def _build_path(snap: Snapshot, path: str) -> pd.DataFrame:
    """Internal: build a long [ticker, date, tr] frame for an arbitrary path (A / B / price_only)."""
    sep = snap.sep.sort_values(["ticker", "date"], kind="mergesort")
    out = sep[["ticker", "date"]].copy()
    if len(sep) == 0:
        out["tr"] = pd.Series(dtype="float64")
        return out.reset_index(drop=True)
    tr = sep.groupby("ticker", group_keys=False, sort=False).apply(
        lambda g: _per_ticker_tr(g, path), include_groups=False
    )
    out["tr"] = np.asarray(tr).reshape(-1)
    return out.reset_index(drop=True)


def _liquid_eligible(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """date x ticker bool: passes the TRADED-universe liquid screen as-of date — marketcap>=floor & close>=price_floor
    & 60-td median $ADV>=floor. Mirrors harness.liquid_panel (kept local to avoid a circular import). TR integrity is
    only required where the strategy actually trades; obscure micro-caps / pumps are out of the Canary's scope."""
    mcap = snap.daily.pivot_table(index="date", values="marketcap", columns="ticker")
    close = snap.sep.pivot_table(index="date", values="close", columns="ticker")
    closeadj = snap.sep.pivot_table(index="date", values="closeadj", columns="ticker").reindex_like(mcap)
    sep = snap.sep.assign(_dv=snap.sep["volume"] * snap.sep["close"])
    adv = (sep.pivot_table(index="date", values="_dv", columns="ticker")
           .rolling(cfg.adv_window_td, min_periods=cfg.adv_window_td // 2).median().reindex_like(mcap))
    return ((mcap >= cfg.marketcap_floor) & (close >= cfg.price_floor) & (adv >= cfg.adv_floor)
            & (closeadj >= cfg.closeadj_floor)).fillna(False)   # ★ closeadj precision screen (DEC-329)


def _classify_disagreements(disagree: pd.DataFrame, actions: pd.DataFrame, sep: pd.DataFrame):
    """Split gap>tol name-days (`disagree`=[ticker, date, gap]) into EXPLAINED vs UNEXPLAINED. A disagreement is
    EXPLAINED if within _DIST_WINDOW_DAYS of ANY corporate-action day for that ticker — a non-cash distribution
    (spinoff/split/merger) OR a CASH DIVIDEND. Path B (close + cash-dividend) cannot faithfully reconstruct
    distribution days that `closeadj` encodes (special/large/date-misaligned dividends, spinoffs); integrity there
    rests on the external anchor + the ordinary-day ex-reconciliation, not this per-day max gap. Returns
    (n_explained, unexplained_df[ticker,date,gap])."""
    if len(disagree) == 0:
        return 0, disagree
    nc = actions[actions["action"].isin(_NON_CASH_DISTRIBUTION_ACTIONS)][["ticker", "date"]]
    cd = sep.loc[sep["dividends"].fillna(0.0) > 0.0, ["ticker", "date"]]      # cash-dividend days (Path-B blind spot)
    ev = pd.concat([nc, cd], ignore_index=True).rename(columns={"date": "edate"})
    if len(ev) == 0:
        return 0, disagree
    win = pd.Timedelta(days=_DIST_WINDOW_DAYS)
    j = disagree.merge(ev, on="ticker", how="left")
    j["near"] = j["edate"].notna() & ((j["date"] - j["edate"]).abs() <= win)
    flag = j.groupby(["ticker", "date"], as_index=False)["near"].any()
    out = disagree.merge(flag, on=["ticker", "date"], how="left")
    out["near"] = out["near"].fillna(False)
    n_explained = int(out["near"].sum())
    unexplained = out.loc[~out["near"], ["ticker", "date", "gap"]]
    return n_explained, unexplained


def run_canary(snap: Snapshot, cfg: ForkBConfig) -> dict:
    """Reconcile Path A vs Path B per-name-per-day; HALT if they don't reconcile.

    Returns dict:
      max_path_gap : float — the largest |TR_A - TR_B| over all eligible (non-NaN) name-days (np.nan if none).
      exday_t      : float — signed mean of (A - B) over ex-day rows only (dividends > 0); the per-ex-day
                     signed check (charter §2 (i)). ~0 for a correct rebuild; np.nan if no ex-days.
      halt         : bool  — True iff max_path_gap > _RECONCILE_TOL (the paths disagree). A zero-eligible-names
                     case is a WARNING (halt=False, status='WARNING'), NOT a HALT (charter §2).
      detail       : dict  — n_eligible, n_exday, n_disagree (>tol), tol, status, signed exday max gap.
    """
    a = _build_path(snap, "A").rename(columns={"tr": "tr_a"})
    b = _build_path(snap, "B").rename(columns={"tr": "tr_b"})
    m = a.merge(b, on=["ticker", "date"], how="inner")

    # ex-day flag + close (for the tradeable-universe filter) from SEP
    sep = snap.sep[["ticker", "date", "dividends", "close"]]
    m = m.merge(sep, on=["ticker", "date"], how="left")

    # ★ reconcile only on the TRADEABLE universe (close >= price_floor): sub-$5 price-rounding dominates the gap on
    # penny names the strategy never trades; TR integrity only needs to hold where we actually trade.
    eligible = m["tr_a"].notna() & m["tr_b"].notna() & (m["close"] >= cfg.price_floor)
    elig = m.loc[eligible]
    n_eligible = int(eligible.sum())

    if n_eligible == 0:
        return {
            "max_path_gap": np.nan,
            "exday_t": np.nan,
            "halt": False,
            "detail": {"n_eligible": 0, "n_exday": 0, "n_disagree": 0,
                       "tol": _RECONCILE_TOL, "status": "WARNING",
                       "reason": "zero eligible names — WARNING not HALT (charter §2)"},
        }

    tol = cfg.canary_perday_tol
    elig = elig.assign(gap=(elig["tr_a"] - elig["tr_b"]).abs())
    max_path_gap = float(elig["gap"].max())
    disagree = elig.loc[elig["gap"] > tol, ["ticker", "date", "gap"]]
    n_disagree_all = int(len(disagree))
    # ★ restrict disagreements to the TRADED (liquid) universe — TR integrity is only required where the strategy
    # trades; obscure micro-caps / pumps (fail the liquid screen) are out of the Canary's scope.
    if len(disagree):
        liq = _liquid_eligible(snap, cfg)
        keep = disagree.apply(
            lambda r: bool(liq.at[r["date"], r["ticker"]]) if (r["ticker"] in liq.columns and r["date"] in liq.index)
            else False, axis=1)
        disagree = disagree.loc[keep]
    n_disagree = int(len(disagree))
    # ★ classify: EXPLAINED (near a cash dividend OR a non-cash distribution — Path-B blind spots) vs UNEXPLAINED
    n_explained, unexpl = _classify_disagreements(disagree, snap.actions, snap.sep)
    n_unexplained = int(len(unexpl))
    max_unexplained_gap = float(unexpl["gap"].max()) if n_unexplained else 0.0
    unexplained_rate = n_unexplained / n_eligible
    unexplained_sample = (unexpl.sort_values("gap", ascending=False).head(20)
                          .assign(date=lambda d: d["date"].astype(str))[["ticker", "date", "gap"]].to_dict("records"))

    exday_mask = elig["dividends"].fillna(0.0) > 0.0
    exrows = elig.loc[exday_mask]
    n_exday = int(exday_mask.sum())
    if n_exday > 0:
        signed = exrows["tr_a"] - exrows["tr_b"]
        exday_t = float(signed.mean())
        exday_max_abs = float(signed.abs().max())
    else:
        exday_t = np.nan
        exday_max_abs = np.nan

    # HALT on a GROSS unexplained gap (real corruption on a tradeable name) OR an elevated unexplained RATE
    # (systematic) — NOT on a sparse benign tail (recent-data closeadj lag / untyped specials).
    halt = (max_unexplained_gap > cfg.canary_gross_gap) or (unexplained_rate > cfg.canary_unexplained_rate_max)
    status = "HALT" if halt else "PASS"

    return {
        "max_path_gap": max_path_gap,
        "exday_t": exday_t,
        "halt": halt,
        "detail": {
            "n_eligible": n_eligible,
            "n_exday": n_exday,
            "n_disagree_all": n_disagree_all,
            "n_disagree": n_disagree,
            "n_explained": n_explained,
            "n_unexplained": n_unexplained,
            "max_unexplained_gap": max_unexplained_gap,
            "unexplained_rate": unexplained_rate,
            "unexplained_sample": unexplained_sample,
            "tol": tol,
            "status": status,
            "exday_signed_max_abs": exday_max_abs,
        },
    }


def run_canary_against(snap: Snapshot, cfg: ForkBConfig, rebuild_path: str) -> dict:
    """Reconcile Path A vs an arbitrary `rebuild_path` (e.g. 'price_only' for the div-forgotten bug).

    Same contract as run_canary but lets a caller / TDI substitute the second path. Used by the canary's own
    integrity test: a price-only rebuild must reconcile WORSE on ex-dates (gap > 1e-4 for a div-paying name).
    """
    a = _build_path(snap, "A").rename(columns={"tr": "tr_a"})
    b = _build_path(snap, rebuild_path).rename(columns={"tr": "tr_b"})
    m = a.merge(b, on=["ticker", "date"], how="inner")
    sep = snap.sep[["ticker", "date", "dividends"]]
    m = m.merge(sep, on=["ticker", "date"], how="left")

    eligible = m["tr_a"].notna() & m["tr_b"].notna()
    elig = m.loc[eligible]
    n_eligible = int(eligible.sum())
    if n_eligible == 0:
        return {"max_path_gap": np.nan, "exday_t": np.nan, "halt": False,
                "detail": {"n_eligible": 0, "status": "WARNING"}}

    gap = (elig["tr_a"] - elig["tr_b"]).abs()
    max_path_gap = float(gap.max())
    halt = max_path_gap > _RECONCILE_TOL
    return {
        "max_path_gap": max_path_gap,
        "exday_t": np.nan,
        "halt": halt,
        "detail": {"n_eligible": n_eligible, "status": "HALT" if halt else "PASS",
                   "rebuild_path": rebuild_path},
    }


def run_canary_external(snap: Snapshot, cfg: ForkBConfig) -> dict:
    """Canary part (ii): reconcile the apparatus cumulative TR vs the >=3 hand-verified anchors sourced
    INDEPENDENTLY of the price vendor (the common-mode guard — A and B both source the same vendor, so the
    A/B reconciliation is BLIND to a shared vendor TR error). HALT if any
    |apparatus_cum_TR - known_cum_TR| > cfg.canary_anchor_tol_bps, OR if
    the anchor set does NOT span >=1 'ordinary' AND >=1 'special' dividend (the error hides in special-div
    handling). Phase-0 supplies the values; an empty set -> UNRESOLVED (freeze_ready() blocks the freeze).

    Each anchor = (ticker, start, end, known_cum_TR, kind), kind in {'ordinary','special'}.
    """
    anchors = cfg.PHASE0_canary_external_anchor
    if not anchors:
        return {"status": "UNRESOLVED", "halt": False, "n": 0,
                "detail": "external anchors not set — Phase-0 manual step; freeze_ready() blocks the freeze"}
    tr = build_tr(snap, cfg)
    breaches, kinds = [], set()
    for row in anchors:
        ticker, start, end, known_cum_tr, kind = row[0], row[1], row[2], float(row[3]), row[4]
        kinds.add(kind)
        g = tr[(tr["ticker"] == ticker) & (tr["date"] >= pd.Timestamp(start)) & (tr["date"] <= pd.Timestamp(end))]
        if g.empty or g["tr"].isna().all():
            breaches.append((ticker, "no apparatus TR in span")); continue
        appar_cum = float((1.0 + g["tr"].fillna(0.0)).prod() - 1.0)
        gap_bps = abs(appar_cum - known_cum_tr) * 1e4
        if gap_bps > cfg.canary_anchor_tol_bps:
            breaches.append((ticker, f"{gap_bps:.2f}bps > {cfg.canary_anchor_tol_bps}bps tol"))
    span_ok = ("ordinary" in kinds) and ("special" in kinds)
    halt = bool(breaches) or (not span_ok)
    return {"status": "HALT" if halt else "PASS", "halt": halt, "n": len(anchors),
            "breaches": breaches, "span_ok": span_ok, "kinds": sorted(kinds)}
