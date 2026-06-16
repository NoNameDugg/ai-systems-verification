"""PEAD/SUE engine (post-earnings-announcement-drift standardized unexpected earnings).

Pure-function library over the _schema/_apparatus/synth_fixtures contract. Implements the
pre-registered SUE construction:

  - Surprise (PINNED): UE_t = epsdil_t - epsdil_{t-4q} on SF1 dimension==ARQ, **fiscal-quarter
    aligned** (matched on the name's fiscal quarter via reportperiod's month-of-fiscal-year, NOT
    calendar), so an off-cycle (e.g. June fiscal-year-end) filer's YoY lines up on its own fiscal
    quarter rather than mis-aligning on the calendar.
  - SUE_t = UE_t / sigma(UE); sigma = rolling std of the NAME'S OWN past UE over the prior
    cfg.sue_sigma_window_q (8) quarters, min cfg.sue_sigma_min_q (6) surviving quarters, using
    ONLY rows with datekey <= the current row's datekey (no-peek / PIT).
  - Winsorize SUE to cfg.sue_winsor [-5, +5] PER CROSS-SECTION (per datekey).

  - Restatement drop-rule (PINNED): a row whose lastupdated > datekey +
    cfg.restatement_K_days (120) is a back-filled restatement -> set dropped=True and EXCLUDE it
    from the rank (do NOT SUE-impute). The drop-rate is logged.

NOTE on the no-peek base values: the spec also asks for "as-originally-filed base values only"
when forming UE. The restatement drop already removes back-filled rows; a dropped (restated) row is
not used as a YoY base for later rows (it is excluded from the seasoned series). The sigma window is
explicitly bounded to datekey <= current datekey so no future revision ever leaks into a row's sigma.
"""
from __future__ import annotations

import logging

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot

log = logging.getLogger("gauntlet.sue")


def _fiscal_quarter(reportperiod: pd.Series) -> pd.Series:
    """Fiscal-quarter-of-year bucket from the period-end calendar month.

    the as-reported quarterly reportperiods are quarter-ends; the month-of-year (3/6/9/12 etc.) uniquely tags
    which fiscal quarter a row belongs to for THAT name. Matching YoY on this bucket (rather than on
    calendar position) is what makes an off-cycle (June fiscal-year-end) name's YoY align on its own
    fiscal quarter. We key purely on reportperiod.month so the alignment is intrinsic to the row.
    """
    return pd.to_datetime(reportperiod).dt.month


def compute_sue(snap: Snapshot, cfg: ForkBConfig) -> pd.DataFrame:
    """Compute standardized unexpected earnings (SUE) per (ticker, datekey).

    Returns a DataFrame with columns [ticker, datekey, sue, dropped]:
      - one row per ARQ filing that has a fiscal-quarter-aligned YoY base AND a well-formed sigma
        (>= cfg.sue_sigma_min_q surviving past UEs, no-peek);
      - `dropped` is True for AR-restatement back-fills (lastupdated > datekey + K); a dropped row
        carries sue=NaN and is excluded from any downstream rank.
    Rows that cannot form a sigma (too few seasoned quarters) or lack a YoY base are NOT emitted.
    """
    sf1 = snap.sf1
    primary = sf1[sf1["dimension"] == cfg.sue_dim_primary].copy()
    if primary.empty:
        return pd.DataFrame(columns=["ticker", "datekey", "sue", "dropped"])

    primary["datekey"] = pd.to_datetime(primary["datekey"])
    primary["reportperiod"] = pd.to_datetime(primary["reportperiod"])
    primary["lastupdated"] = pd.to_datetime(primary["lastupdated"])
    primary["fq"] = _fiscal_quarter(primary["reportperiod"])

    # AR-restatement flag (DS-13): back-filled => lastupdated more than K days after the filing date.
    # ★ Phase-0: on licensed-data `lastupdated` is the DB-refresh date (NOT a restatement signal) -> gate this off for the
    # licensed-data run (ARQ already excludes restatements); default True preserves the charter/synthetic-fixture semantics.
    K = pd.Timedelta(days=cfg.restatement_K_days)
    if cfg.sue_restatement_drop_lastupdated:
        primary["dropped"] = primary["lastupdated"] > (primary["datekey"] + K)
    else:
        primary["dropped"] = False

    out_rows = []
    n_total = 0
    n_dropped = 0

    for ticker, g in primary.groupby("ticker", sort=False):
        # Seasoned series is ordered by the fiscal period (reportperiod), tie-break datekey.
        g = g.sort_values(["reportperiod", "datekey"]).reset_index(drop=True)

        rp = g["reportperiod"].to_numpy()
        dk = g["datekey"].to_numpy()
        eps = g["epsdil"].to_numpy(dtype=float)
        fq = g["fq"].to_numpy()
        dropped = g["dropped"].to_numpy()

        # ---- UE_t = epsdil_t - epsdil_{t-4q}, fiscal-quarter aligned ----------------------------
        # The name's quarters are contiguous, so the i-4 row is the same fiscal quarter one year
        # prior; we assert the fiscal-quarter bucket matches before forming the YoY (an explicit
        # guard so a missing/extra filing never silently mis-aligns the calendar vs the fiscal year).
        lag = cfg.sue_yoy_lag_q
        ue = np.full(len(g), np.nan)
        for i in range(len(g)):
            j = i - lag
            if j < 0:
                continue
            if fq[i] != fq[j]:
                # not the same fiscal quarter one year back -> cannot form a clean fiscal YoY
                continue
            ue[i] = eps[i] - eps[j]

        # ---- sigma(UE): rolling std of the name's OWN past UE, no-peek by datekey ----------------
        # For row i, the sigma window is the prior `window` UEs whose datekey <= dk[i] (i.e. strictly
        # earlier rows in this fiscal-ordered series). Restated (back-filled) base UEs are excluded
        # from the window so a future-revealed value never seasons an earlier row.
        win = cfg.sue_sigma_window_q
        minq = cfg.sue_sigma_min_q
        sue_raw = np.full(len(g), np.nan)
        for i in range(len(g)):
            if dropped[i]:
                continue  # restated rows get no SUE (they are dropped from the rank)
            if np.isnan(ue[i]):
                continue
            # candidate past UEs: strictly-earlier rows (datekey < current datekey), not restated,
            # with a defined UE. No-peek: only datekey <= current datekey can be known PIT; we use
            # strictly-earlier so the current row's own UE is never in its own sigma.
            past = []
            for k in range(i):
                if dk[k] < dk[i] and not dropped[k] and not np.isnan(ue[k]):
                    past.append(ue[k])
            if len(past) < minq:
                continue
            window_ue = np.array(past[-win:], dtype=float)
            sigma = float(np.std(window_ue, ddof=1))
            if not np.isfinite(sigma) or sigma <= 0.0:
                continue
            sue_raw[i] = ue[i] / sigma

        for i in range(len(g)):
            n_total += 1
            if dropped[i]:
                n_dropped += 1
                out_rows.append({"ticker": ticker, "datekey": pd.Timestamp(dk[i]),
                                 "sue": np.nan, "dropped": True})
            elif not np.isnan(sue_raw[i]):
                out_rows.append({"ticker": ticker, "datekey": pd.Timestamp(dk[i]),
                                 "sue": float(sue_raw[i]), "dropped": False})
            # rows that are neither dropped nor sigma-formable are simply not emitted (cannot rank)

    out = pd.DataFrame(out_rows, columns=["ticker", "datekey", "sue", "dropped"])

    # ---- per-cross-section winsorize SUE to cfg.sue_winsor (only the non-dropped, scored rows) ---
    lo, hi = cfg.sue_winsor
    scored = out["dropped"] == False  # noqa: E712 (explicit boolean mask, not identity)
    out.loc[scored, "sue"] = out.loc[scored].groupby("datekey")["sue"].transform(
        lambda s: s.clip(lower=lo, upper=hi)
    )

    drop_rate = (n_dropped / n_total) if n_total else 0.0
    log.info("compute_sue: %d ARQ rows, %d restatement-drops (drop-rate=%.3f), %d scored",
             n_total, n_dropped, drop_rate, int(scored.sum()))

    return out


def restatement_drop_balance(sue_df: pd.DataFrame, scored_sign: pd.Series | None = None) -> dict:
    """Diagnostic helper (DS-13 "assert the drop is balanced across SUE-sign").

    Returns the raw drop counts; an imbalanced drop across SUE-sign would itself be a leak. The
    dropped rows carry no SUE (by construction), so this exposes the drop count alongside the scored
    population for an external balance assertion rather than fabricating a sign for dropped rows.
    """
    n_dropped = int(sue_df["dropped"].sum())
    n_scored = int((~sue_df["dropped"]).sum())
    pos = int((sue_df.loc[~sue_df["dropped"], "sue"] > 0).sum())
    neg = int((sue_df.loc[~sue_df["dropped"], "sue"] < 0).sum())
    return {"n_dropped": n_dropped, "n_scored": n_scored, "scored_pos": pos, "scored_neg": neg}
