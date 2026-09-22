"""Single-name loader + cohort harness (the gated PEAD book).

Builds the gated PEAD long-short book from a Snapshot, with the pinned PIT/survivorship discipline:
  - total-return panel via tr_canary.build_tr (+ intra-hold delist terminal value applied)
  - formation-once liquid screen (marketcap>=$300M, close>=$5, 60-td median $ADV>=$1M) — a name passing
    at formation is held to its full 63-td term, NEVER dropped mid-hold for a screen breach
  - rank on SUE using only firms with datekey <= D-1 (no same-day-after-close peek); enter at D+1 close
  - quintile long/short, dollar-neutral; overlapping cohorts rebalanced every 21 td (LdP uniqueness =
    average over the ~3 live cohorts); VW gated leg (EW reported diagnostic)
Outputs the daily raw book return, the short-leg sub-P&L (P3), the formation-label dict (perm-null),
the name-daily TR panel, and turnover/short-gross (costs).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot
import tr_canary
import sue as sue_mod
import nsi as nsi_mod
import fundamental_factors as ff_mod


def tr_panel(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """date x ticker daily TOTAL return, with the intra-hold delist terminal value applied (D-HARD-5).

    A delisted name's last in-panel day gets the terminal return (terminal_value/last_close - 1) from
    ACTIONS, so a held position realizes the survivorship-correct terminal (≈0 bankruptcy / deal price).
    """
    tr = tr_canary.build_tr(snap, cfg)
    panel = tr.pivot_table(index="date", values="tr", columns="ticker")
    # apply delist terminal value — ★ BOUNDED to a sane delisting range [-1, +1] (S2/DEC-330): the raw
    # terminal_value/last_close-1 blows up on penny denominators / unit mismatches (SDOCQ +1.2M%).
    dels = snap.actions[snap.actions["action"] == "delisted"]
    last_close = snap.sep.sort_values("date").groupby("ticker")["close"].last()
    for _, row in dels.iterrows():
        t = row["ticker"]
        if t not in panel.columns:
            continue
        col = panel[t].dropna()
        if col.empty or t not in last_close.index or last_close[t] == 0:
            continue
        d = col.index[-1]
        term_ret = float(row["value"]) / float(last_close[t]) - 1.0
        panel.loc[d, t] = max(-1.0, min(term_ret, 1.0))
    # ★ name-level daily-TR clip (S2 verdict-validation / DEC-330) — the robust catch-all for delist + closeadj
    # reissue/Q-stub discontinuities (a real daily equity TR > tr_clip_daily is an event/artifact, not a PEAD signal).
    if cfg.tr_clip_daily is not None:
        panel = panel.clip(lower=-cfg.tr_clip_daily, upper=cfg.tr_clip_daily)
    return panel


def _adv(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """date x ticker rolling 60-td median dollar-ADV (volume * close)."""
    sep = snap.sep.copy()
    sep["dvol"] = sep["volume"] * sep["close"]
    dv = sep.pivot_table(index="date", values="dvol", columns="ticker")
    return dv.rolling(cfg.adv_window_td, min_periods=cfg.adv_window_td // 2).median()


def liquid_panel(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """date x ticker boolean: passes the liquid screen AS-OF that date (marketcap/price/$ADV)."""
    mcap = snap.daily.pivot_table(index="date", values="marketcap", columns="ticker")
    close = snap.sep.pivot_table(index="date", values="close", columns="ticker")
    adv = _adv(snap, cfg).reindex_like(mcap)
    closeadj = snap.sep.pivot_table(index="date", values="closeadj", columns="ticker").reindex_like(mcap)
    ok = ((mcap >= cfg.marketcap_floor) & (close >= cfg.price_floor) & (adv >= cfg.adv_floor)
          & (closeadj >= cfg.closeadj_floor))   # ★ closeadj precision screen (DEC-329) — exclude rounding-noisy TR
    return ok.fillna(False)


def _cap_weights(w: pd.Series, cap: float) -> pd.Series:
    """Cap each name's weight at `cap`, redistributing the excess PRO-RATA to uncapped names (iterate to
    convergence). Returns weights summing to 1. If cap*n <= 1 (cap too tight to sum to 1) -> equal weights.
    Restores breadth: pure VW on the equity power-law concentrates the leg into ~1 mega-cap (median 26%)."""
    w = w.clip(lower=0).astype(float)
    s = float(w.sum())
    if s <= 0 or len(w) == 0:
        return pd.Series(1.0 / max(1, len(w)), index=w.index)
    w = w / s
    if cap * len(w) <= 1.0:
        return pd.Series(1.0 / len(w), index=w.index)
    for _ in range(100):
        over = w > cap + 1e-12
        if not over.any():
            break
        excess = float((w[over] - cap).sum())
        w[over] = cap
        under = ~over
        u = float(w[under].sum())
        if u <= 0:
            break
        w[under] = w[under] + excess * w[under] / u
    return w


def pead_book(snap: Snapshot, cfg: ForkBConfig, weighting: str = "VW") -> dict:
    """The gated H1 PEAD book. weighting in {'VW','EW'} (VW = the gated leg, EW = reported diagnostic).

    Returns dict: daily_raw (Series), short_leg_daily (Series), long_leg_daily (Series),
    formation_labels (dict[Timestamp -> Series ticker->{+1,-1,0}]), name_daily (date x ticker TR panel),
    turnover_daily (Series), short_gross_daily (Series), n_formations (int).
    """
    panel = tr_panel(snap, cfg)
    dates = panel.index
    liq = liquid_panel(snap, cfg).reindex(index=dates, columns=panel.columns).fillna(False)
    mcap = snap.daily.pivot_table(index="date", values="marketcap", columns="ticker").reindex(
        index=dates, columns=panel.columns)

    sdf = sue_mod.compute_sue(snap, cfg)
    sdf = sdf[~sdf["dropped"].astype(bool)].dropna(subset=["sue"]).sort_values("datekey")

    H, R = cfg.hold_td, cfg.rebalance_td
    form_idx = list(range(R, len(dates) - 1, R))            # formation rows (need D-1 and D+1)
    cohort_long = np.zeros(len(dates))
    cohort_short = np.zeros(len(dates))
    cohort_count = np.zeros(len(dates))                      # # live cohorts per day (for averaging)
    turnover = np.zeros(len(dates))
    short_gross = np.zeros(len(dates))
    formation_labels: dict = {}
    q = cfg.n_quantiles
    max_short_w = 0.0                                        # largest single-name short-leg weight (C-2 P3 injector)

    for fi in form_idx:
        D = dates[fi]
        d_minus_1 = dates[fi - cfg.rank_datekey_lag_td]      # datekey<=D-1 (config-driven; the freeze value is load-bearing)
        # cohort = names that announced in (D-R, D-1]: most-recent SUE datekey in that window, datekey<=D-1
        win_lo = dates[max(0, fi - R)]
        cand = sdf[(sdf["datekey"] > win_lo) & (sdf["datekey"] <= d_minus_1)]
        cand = cand.sort_values("datekey").groupby("ticker").tail(1)        # most-recent per name
        # formation-once liquid screen as-of D
        cand = cand[cand["ticker"].apply(lambda t: bool(liq.loc[D, t]) if t in liq.columns else False)]
        if len(cand) < cfg.min_leg_names * q:                # need >= min_leg_names per quintile leg (S2 C2 / §7-pin-1)
            continue
        cand = cand.sort_values("sue")
        nper = len(cand) // q
        shorts = cand.head(nper)["ticker"].tolist()          # bottom quintile (low SUE)
        longs = cand.tail(nper)["ticker"].tolist()           # top quintile (high SUE)
        if not longs or not shorts:
            continue
        # weights
        if weighting == "VW":
            wl = mcap.loc[D, longs].fillna(0.0); wl = (wl / wl.sum()) if wl.sum() > 0 else pd.Series(1.0/len(longs), index=longs)
            ws = mcap.loc[D, shorts].fillna(0.0); ws = (ws / ws.sum()) if ws.sum() > 0 else pd.Series(1.0/len(shorts), index=shorts)
            if cfg.single_name_weight_cap is not None:          # ★ cap single-name domination -> restore breadth
                wl = _cap_weights(wl, cfg.single_name_weight_cap)
                ws = _cap_weights(ws, cfg.single_name_weight_cap)
        else:
            wl = pd.Series(1.0 / len(longs), index=longs)
            ws = pd.Series(1.0 / len(shorts), index=shorts)
        max_short_w = max(max_short_w, float(ws.max()) if len(ws) else 0.0)   # C-2: track the squeezed-name weight
        # formation labels for the perm-null (+1 long / -1 short / 0)
        lab = pd.Series(0, index=panel.columns, dtype=int)
        lab.loc[longs] = 1; lab.loc[shorts] = -1
        formation_labels[D] = lab
        # hold from D+entry_lag for H days (formation-once: held to term regardless of later screen/delist)
        s, e = fi + cfg.entry_lag_td, min(fi + cfg.entry_lag_td + H, len(dates))
        for k in range(s, e):
            rl = panel.iloc[k][longs].fillna(0.0)
            rs = panel.iloc[k][shorts].fillna(0.0)
            cohort_long[k] += float((wl * rl).sum())
            cohort_short[k] += float((ws * rs).sum())
            cohort_count[k] += 1
        # turnover: full leg entered at formation + exited at term (charged once per cohort leg)
        turnover[s] += 2.0                                    # long+short legs entered
        if e - 1 < len(dates):
            turnover[min(e, len(dates) - 1)] += 2.0           # exit
        short_gross[s:e] += 1.0                               # 1 unit short notional held over the hold

    cc = np.where(cohort_count > 0, cohort_count, 1.0)
    long_leg = pd.Series(cohort_long / cc, index=dates)
    short_leg = pd.Series(cohort_short / cc, index=dates)
    daily_raw = long_leg - short_leg                          # dollar-neutral long-short
    return {
        "daily_raw": daily_raw, "long_leg_daily": long_leg, "short_leg_daily": short_leg,
        "formation_labels": formation_labels, "name_daily": panel,
        "turnover_daily": pd.Series(turnover / np.maximum(cc, 1.0), index=dates),
        "short_gross_daily": pd.Series(short_gross / cc, index=dates),
        "n_formations": len(formation_labels),
        "max_short_weight": max_short_w,
    }


def nsi_book(snap: Snapshot, cfg: ForkBConfig, weighting: str = "VW", cadence: str = "annual_june") -> dict:
    """Net-share-issuance book: LONG the low-issuance (buyback) quintile / SHORT the high-issuance quintile.

    cadence='annual_june' (DIAGNOSTIC: roughly annual independent draws, too few to clear the block floor, so
    un-gateable) or 'qoq_nonoverlap' (GATED: non-overlapping 63-trading-day quarterly cohorts on the
    quarter-over-quarter NSI, which are genuinely independent blocks). Emits the SAME gate-consumed keys as
    pead_book PLUS `active_mask` (cohort_count>0) so the M2 power gate counts only active days, not the
    zero-filled flat days that would spuriously clear the 60-block floor.
    Direction is inverted vs pead_book (low-NSI long); everything else reuses tr_panel/liquid_panel/_cap_weights.
    """
    panel = tr_panel(snap, cfg)
    dates = panel.index
    liq = liquid_panel(snap, cfg).reindex(index=dates, columns=panel.columns).fillna(False)
    mcap = snap.daily.pivot_table(index="date", values="marketcap", columns="ticker").reindex(
        index=dates, columns=panel.columns)

    quarterly = (cadence == "qoq_nonoverlap")
    sdf = nsi_mod.compute_nsi(snap, cfg, quarterly=quarterly).dropna(subset=["nsi"]).sort_values("datekey")

    H, q, LAG, ENTRY = cfg.hold_td, cfg.n_quantiles, cfg.rank_datekey_lag_td, cfg.entry_lag_td
    if cadence == "annual_june":
        form_idx = []
        for yr in sorted(set(dates.year)):
            c = [i for i, d in enumerate(dates) if d.year == yr and d.month == cfg.nsi_rebalance_month]
            if c:
                form_idx.append(c[-1])
        form_idx = [i for i in form_idx if i + ENTRY + H <= len(dates) and i - LAG >= 0]
        win_days = 400                                       # trailing ~year (the annual NSI)
    elif cadence == "qoq_nonoverlap":
        start = max(LAG, H)
        form_idx = [i for i in range(start, len(dates) - 1, H) if i + ENTRY + H <= len(dates)]   # non-overlapping 63-td
        win_days = None
    else:
        raise ValueError(f"nsi_book: unknown cadence {cadence!r}")

    cohort_long = np.zeros(len(dates)); cohort_short = np.zeros(len(dates)); cohort_count = np.zeros(len(dates))
    turnover = np.zeros(len(dates)); short_gross = np.zeros(len(dates))
    formation_labels: dict = {}
    max_short_w = 0.0

    for fi in form_idx:
        D = dates[fi]
        d_minus_1 = dates[fi - LAG]
        win_lo = dates[max(0, fi - H)] if quarterly else (D - pd.Timedelta(days=win_days))
        cand = sdf[(sdf["datekey"] > win_lo) & (sdf["datekey"] <= d_minus_1)]
        cand = cand.sort_values("datekey").groupby("ticker").tail(1)                # most-recent per name
        cand = cand[cand["ticker"].apply(lambda t: bool(liq.loc[D, t]) if t in liq.columns else False)]
        if len(cand) < cfg.min_leg_names * q:
            continue
        cand = cand.sort_values("nsi")
        nper = len(cand) // q
        longs = cand.head(nper)["ticker"].tolist()           # LOW NSI = buyback = LONG
        shorts = cand.tail(nper)["ticker"].tolist()          # HIGH NSI = issuance = SHORT
        if not longs or not shorts:
            continue
        if weighting == "VW":
            wl = mcap.loc[D, longs].fillna(0.0); wl = (wl / wl.sum()) if wl.sum() > 0 else pd.Series(1.0 / len(longs), index=longs)
            ws = mcap.loc[D, shorts].fillna(0.0); ws = (ws / ws.sum()) if ws.sum() > 0 else pd.Series(1.0 / len(shorts), index=shorts)
            if cfg.single_name_weight_cap is not None:
                wl = _cap_weights(wl, cfg.single_name_weight_cap)
                ws = _cap_weights(ws, cfg.single_name_weight_cap)
        else:
            wl = pd.Series(1.0 / len(longs), index=longs); ws = pd.Series(1.0 / len(shorts), index=shorts)
        max_short_w = max(max_short_w, float(ws.max()) if len(ws) else 0.0)
        lab = pd.Series(0, index=panel.columns, dtype=int); lab.loc[longs] = 1; lab.loc[shorts] = -1
        formation_labels[D] = lab
        s, e = fi + ENTRY, min(fi + ENTRY + H, len(dates))
        for k in range(s, e):
            cohort_long[k] += float((wl * panel.iloc[k][longs].fillna(0.0)).sum())
            cohort_short[k] += float((ws * panel.iloc[k][shorts].fillna(0.0)).sum())
            cohort_count[k] += 1
        turnover[s] += 2.0
        if e - 1 < len(dates):
            turnover[min(e, len(dates) - 1)] += 2.0
        short_gross[s:e] += 1.0

    cc = np.where(cohort_count > 0, cohort_count, 1.0)
    long_leg = pd.Series(cohort_long / cc, index=dates)
    short_leg = pd.Series(cohort_short / cc, index=dates)
    daily_raw = long_leg - short_leg
    return {
        "daily_raw": daily_raw, "long_leg_daily": long_leg, "short_leg_daily": short_leg,
        "formation_labels": formation_labels, "name_daily": panel,
        "turnover_daily": pd.Series(turnover / np.maximum(cc, 1.0), index=dates),
        "short_gross_daily": pd.Series(short_gross / cc, index=dates),
        "n_formations": len(formation_labels),
        "max_short_weight": max_short_w,
        "active_mask": pd.Series(cohort_count > 0, index=dates),     # ★ honest active-block counting (S2 #3)
    }


def _net_turnover(w_agg: dict, cohort_count, dates) -> pd.Series:
    """★ BD §5.1 / S2 BLOCK-1 — NET turnover = the L1 day-over-day change of the NORMALIZED dollar-neutral book.
    w_agg: dict{ticker -> np.array(len(dates))} = signed aggregate held weight (sum over live cohorts, pre-normalization;
    long +, short −). The normalized book book_w = w_agg / cohort_count is always unit-gross per leg, so Σ|Δ book_w| is
    the traded notional (fraction of book traded). A STICKY signal (same names across consecutive cohorts) → ~0 net
    turnover; a fully-rotating signal → the full per-cohort charge. Returns turnover_daily (Series; day-0 = 0)."""
    if not w_agg:
        return pd.Series(0.0, index=dates)
    ccx = np.where(np.asarray(cohort_count) > 0, np.asarray(cohort_count, dtype=float), 1.0)
    M = np.vstack([w_agg[nm] for nm in w_agg]) / ccx                  # names x dates, normalized book weights
    dchg = np.abs(np.diff(M, axis=1)).sum(axis=0)                     # L1 across names, per day-over-day step
    return pd.Series(np.concatenate([[0.0], dchg]), index=dates)


def profitability_book(snap: Snapshot, cfg: ForkBConfig, weighting: str = "VW") -> dict:
    """Gross-profitability book: LONG the high gross-profit/assets quintile / SHORT the low quintile, rolling
    21-trading-day rebalance and 63-trading-day hold.

    Point-in-time: ranks on the gp/assets characteristic as of D-1 (`dates[fi - rank_datekey_lag_td]`);
    `pit_fundamental_chars` broadcasts to datekey <= D, and reading at D-1 is what makes the rank point-in-time.
    Emits `active_mask`, NET `turnover_daily` (what the cost model consumes), `turnover_daily_gross` (the old
    per-cohort full-leg accounting, reported only), and every gate-consumed key."""
    panel = tr_panel(snap, cfg)
    dates = panel.index
    liq = liquid_panel(snap, cfg).reindex(index=dates, columns=panel.columns).fillna(False)
    mcap = snap.daily.pivot_table(index="date", values="marketcap", columns="ticker").reindex(
        index=dates, columns=panel.columns)
    cap = cfg.fundamental_max_stale_days                             # ★ S2-AMEND: drop chars staler than N cal days
    char = ff_mod.pit_fundamental_chars(snap, cfg, max_stale_days=cap)["rmw"].reindex(index=dates, columns=panel.columns)
    char_full = (ff_mod.pit_fundamental_chars(snap, cfg)["rmw"].reindex(index=dates, columns=panel.columns)
                 if cap is not None else char)                       # uncapped, for the stale-incidence report only

    H, R, q, LAG, ENTRY = cfg.hold_td, cfg.rebalance_td, cfg.n_quantiles, cfg.rank_datekey_lag_td, cfg.entry_lag_td
    form_idx = [i for i in range(R, len(dates) - 1, R) if i + ENTRY + H <= len(dates) and i - LAG >= 0]
    cohort_long = np.zeros(len(dates)); cohort_short = np.zeros(len(dates)); cohort_count = np.zeros(len(dates))
    turnover_gross = np.zeros(len(dates)); short_gross = np.zeros(len(dates))
    w_agg: dict = {}                                                  # ticker -> signed aggregate held weight per day
    formation_labels: dict = {}
    max_short_w = 0.0
    stale_dropped_total = 0; n_cand_pool_total = 0                    # ★ S2-AMEND stale-incidence report

    for fi in form_idx:
        D = dates[fi]
        dm1 = dates[fi - LAG]                                         # ★ PIT: char as-of D−1
        liqD = liq.loc[D]
        cand = char.loc[dm1].dropna()
        cand = cand[[t for t in cand.index if t in liqD.index and bool(liqD[t])]]
        if cap is not None:                                          # capped vs uncapped candidate pool (liquid names)
            cf = char_full.loc[dm1].dropna()
            cf = cf[[t for t in cf.index if t in liqD.index and bool(liqD[t])]]
            n_cand_pool_total += len(cf); stale_dropped_total += max(0, len(cf) - len(cand))
        else:
            n_cand_pool_total += len(cand)
        if len(cand) < cfg.min_leg_names * q:
            continue
        cand = cand.sort_values()
        nper = len(cand) // q
        longs = cand.tail(nper).index.tolist()                       # HIGH gp/assets = LONG
        shorts = cand.head(nper).index.tolist()                      # LOW gp/assets = SHORT
        if not longs or not shorts:
            continue
        if weighting == "VW":
            wl = mcap.loc[D, longs].fillna(0.0); wl = (wl / wl.sum()) if wl.sum() > 0 else pd.Series(1.0 / len(longs), index=longs)
            ws = mcap.loc[D, shorts].fillna(0.0); ws = (ws / ws.sum()) if ws.sum() > 0 else pd.Series(1.0 / len(shorts), index=shorts)
            if cfg.single_name_weight_cap is not None:
                wl = _cap_weights(wl, cfg.single_name_weight_cap)
                ws = _cap_weights(ws, cfg.single_name_weight_cap)
        else:
            wl = pd.Series(1.0 / len(longs), index=longs); ws = pd.Series(1.0 / len(shorts), index=shorts)
        max_short_w = max(max_short_w, float(ws.max()) if len(ws) else 0.0)
        lab = pd.Series(0, index=panel.columns, dtype=int); lab.loc[longs] = 1; lab.loc[shorts] = -1
        formation_labels[D] = lab
        s, e = fi + ENTRY, min(fi + ENTRY + H, len(dates))
        for k in range(s, e):
            cohort_long[k] += float((wl * panel.iloc[k][longs].fillna(0.0)).sum())
            cohort_short[k] += float((ws * panel.iloc[k][shorts].fillna(0.0)).sum())
            cohort_count[k] += 1
        for nm in longs:                                             # net-turnover: signed aggregate weight over the hold
            w_agg.setdefault(nm, np.zeros(len(dates)))[s:e] += float(wl[nm])
        for nm in shorts:
            w_agg.setdefault(nm, np.zeros(len(dates)))[s:e] -= float(ws[nm])
        turnover_gross[s] += 2.0                                      # the OLD per-cohort full-leg accounting (reported)
        if e - 1 < len(dates):
            turnover_gross[min(e, len(dates) - 1)] += 2.0
        short_gross[s:e] += 1.0

    cc = np.where(cohort_count > 0, cohort_count, 1.0)
    long_leg = pd.Series(cohort_long / cc, index=dates)
    short_leg = pd.Series(cohort_short / cc, index=dates)
    daily_raw = long_leg - short_leg
    return {
        "daily_raw": daily_raw, "long_leg_daily": long_leg, "short_leg_daily": short_leg,
        "formation_labels": formation_labels, "name_daily": panel,
        "turnover_daily": _net_turnover(w_agg, cohort_count, dates),          # ★ NET (the gated cost; §5.1)
        "turnover_daily_gross": pd.Series(turnover_gross / cc, index=dates),  # old per-cohort accounting (reported)
        "short_gross_daily": pd.Series(short_gross / cc, index=dates),
        "n_formations": len(formation_labels),
        "max_short_weight": max_short_w,
        "active_mask": pd.Series(cohort_count > 0, index=dates),
        "stale_dropped_total": int(stale_dropped_total),             # ★ S2-AMEND stale-incidence (cap dropped these slots)
        "n_cand_pool_total": int(n_cand_pool_total),
        "stale_dropped_frac": float(stale_dropped_total / n_cand_pool_total) if n_cand_pool_total else 0.0,
    }
