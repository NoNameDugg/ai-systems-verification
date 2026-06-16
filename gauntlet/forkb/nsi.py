"""Leaf module `nsi` — net-share-issuance.

NSI is a richly-reported DIAGNOSTIC (M2-exempt, NOT Holm): the best cost-surviving prior, but the
signal is annual (NSI = YoY share-count change driven by aggregate IPO/SEO waves -> ~26 temporally-
independent annual draws) -> demoted from gated. This leaf computes the raw NSI feature.

Construction (PINNED):
    NSI_t = log(sharesbas_t / sharesbas_{t-4q})
on SF1 `dimension == 'ARQ'` split-adjusted `sharesbas` (the period-end share stock — the correct
economic object per Pontiff-Woodgate / Daniel-Titman; cfg.nsi_shares_field='sharesbas',
cfg.nsi_yoy_q=4), fiscal-quarter aligned (the YoY base is 4 quarters back on the NAME's OWN fiscal
cycle, ordered by reportperiod — NOT calendar Dec-31), PIT with `datekey <= D-1`.

This is a PURE function library over the contract: depends only on _schema + numpy/pandas.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import ForkBConfig, Snapshot


def compute_nsi(snap: Snapshot, cfg: ForkBConfig, quarterly: bool = False) -> pd.DataFrame:
    """Net-share-issuance per ARQ filing: NSI_t = log(sharesbas_t / sharesbas_{t-Kq}).

    quarterly=False (default, annual-YoY): K = cfg.nsi_yoy_q = 4 — the Pontiff-Woodgate YoY signal (DIAGNOSTIC
    cadence; ~annual information → un-gateable). quarterly=True (BC GATED cadence): K = cfg.nsi_yoy_q_quarterly = 1
    — a genuinely-independent 1-quarter share change (the only honestly-gateable form; S2 banned YoY-measured-quarterly
    as an autocorrelated false-power-pass). The default path is bit-identical to before (existing TDI unaffected).

    Returns a DataFrame with columns [ticker, datekey, nsi], one row per ARQ filing that has a valid
    YoY (fiscal-quarter aligned, 4q-back) base. The datekey column is the PIT availability date — a
    consumer uses NSI_t only on dates D where datekey <= D-1 (the row itself IS the as-of-datekey
    fact). Rows with no YoY base (first ~4 quarters of a name, gaps) are DROPPED, not NaN-filled.

    Honors the charter's frozen params (cfg.nsi_shares_field, cfg.nsi_yoy_q) — no invented thresholds.
    """
    shares_field = cfg.nsi_shares_field   # 'sharesbas'
    yoy_q = cfg.nsi_yoy_q_quarterly if quarterly else cfg.nsi_yoy_q   # 1 (QoQ gated) vs 4 (annual diagnostic)

    sf1 = snap.sf1
    # ARQ dimension only (ARY for annual-only filers is the H1/SUE concern; H2 NSI is pinned to ARQ).
    arq = sf1[sf1["dimension"] == cfg.sue_dim_primary].copy()

    out_frames = []
    for ticker, g in arq.groupby("ticker", sort=False):
        # fiscal-quarter alignment: order by reportperiod (the fiscal-period end), NOT datekey/calendar.
        # ties / dup reportperiods (e.g. an as-of-filed + a back-filled restatement) -> keep the most
        # recently-known by stable sort on (reportperiod, datekey).
        g = g.sort_values(["reportperiod", "datekey"], kind="stable")

        shares = g[shares_field].to_numpy(dtype=float)
        datekey = g["datekey"].to_numpy()

        n = len(g)
        if n <= yoy_q:
            # not enough fiscal quarters to form any YoY base -> nothing survives for this name.
            continue

        cur = shares[yoy_q:]          # sharesbas_t for t with a valid base
        base = shares[:n - yoy_q]     # sharesbas_{t-4q}, fiscal-quarter aligned
        dk = datekey[yoy_q:]

        # missing / non-positive base or current -> log undefined; DROP (no NaN explosion).
        with np.errstate(divide="ignore", invalid="ignore"):
            valid = (
                np.isfinite(cur) & np.isfinite(base) & (cur > 0.0) & (base > 0.0)
            )
            nsi = np.log(cur[valid] / base[valid])

        if valid.sum() == 0:
            continue

        out_frames.append(pd.DataFrame({
            "ticker": ticker,
            "datekey": dk[valid],
            "nsi": nsi,
        }))

    if not out_frames:
        return pd.DataFrame({"ticker": pd.Series(dtype=object),
                             "datekey": pd.Series(dtype="datetime64[ns]"),
                             "nsi": pd.Series(dtype=float)})

    res = pd.concat(out_frames, ignore_index=True)
    res["datekey"] = pd.to_datetime(res["datekey"])
    res = res.sort_values(["ticker", "datekey"], kind="stable").reset_index(drop=True)
    return res[["ticker", "datekey", "nsi"]]
