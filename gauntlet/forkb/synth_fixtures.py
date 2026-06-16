"""Synthetic ground-truth snapshot generator (the test foundation).

Produces a `Snapshot` whose data-generating process is KNOWN, plus a `truth` dict the tests assert
against. Deterministic (seeded). No real data is touched; the whole apparatus is built and tested
on this synthetic ground-truth BEFORE any data spend.

The load-bearing constructions (each underwrites a binding test):
  - close / closeadj / dividends EXACTLY satisfy total return: closeadj-return = TR (Path A);
    (close_t - close_{t-1} + div_t)/close_{t-1} = TR (Path B). Both recover TR -> the Canary
    reconciles for a CORRECT rebuild and a price-only (div-forgotten) rebuild FAILS it.
  - factor model: TR = alpha + sum_k beta_k f_k + eps, with KNOWN per-name alpha + betas -> the
    factor-residual must retain alpha (resid = raw - F.beta = alpha + eps; mean ~= alpha).
  - SUE: epsdil_t = epsdil_{t-4} + sue_injected * sigma_UE, fiscal-quarter aligned -> the computed
    standardized surprise ~= sue_injected. "edge" names get post-earnings drift alpha correlated with
    SUE sign -> the PEAD harness recovers a real injected edge end-to-end.
  - delisted names carry an ACTIONS terminal value (truth["delist_terminal"]) -> survivorship +
    intra-hold terminal value.
  - a few AR rows are back-filled (lastupdated > datekey + 120d) -> the restatement-drop.
  - a few off-cycle fiscal years -> the fiscal-quarter (not calendar) YoY alignment.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from _schema import Snapshot

_FACTORS = ("size", "value", "mom", "str", "beta")


def make_snapshot(seed: int = 0, n_names: int = 120, start: str = "1999-01-04", end: str = "2024-12-31",
                  edge_frac: float = 0.5, alpha_ann: float = 0.08,
                  nsi_edge_frac: float = 0.0, nsi_alpha_ann: float = 0.0,
                  rmw_edge_frac: float = 0.0, rmw_alpha_ann: float = 0.0):
    """Build a synthetic survivorship-clean PIT snapshot + a `truth` dict.

    edge_frac  = fraction of names that carry a real SUE->drift alpha (the injected edge).
    alpha_ann  = the injected post-earnings-drift alpha (annualized) for high-SUE edge names.
    nsi_edge_frac = fraction of names carrying a real NSI->drift alpha (buyback names drift UP). Default 0
                    keeps the legacy snapshot bit-identical for the PEAD/factor tests.
    nsi_alpha_ann = the injected NSI drift (annualized) for nsi-edge names (sign = -sign(NSI) = buyback long).
    Returns (Snapshot, truth).
    ★ BC (DEC-333): `sharesbas` is now a per-name YoY ladder (NSI = log(1+nsi_rate)); SF1 carries
    assets/revenue/equity/gp ladders → known CMA (asset-growth) + RMW (gp/assets) characteristics.
    """
    rng = np.random.default_rng(seed)
    days = pd.bdate_range(start, end)
    T = len(days)
    tickers = [f"SYN{i:04d}" for i in range(n_names)]

    # ---- factor returns (daily) -------------------------------------------------------------------
    F = pd.DataFrame({k: rng.normal(0, 0.008, T) for k in _FACTORS}, index=days)

    # ---- per-name betas (known) -------------------------------------------------------------------
    betas = {t: {k: rng.normal(0, 0.6) for k in _FACTORS} for t in tickers}

    # ---- delisting + listing windows (survivorship) -----------------------------------------------
    is_delisted = {t: (rng.random() < 0.25) for t in tickers}      # ~25% delisted
    first_idx = {t: 0 for t in tickers}
    last_idx = {t: T - 1 for t in tickers}
    delist_terminal = {}
    for t in tickers:
        if is_delisted[t]:
            li = int(rng.integers(int(T * 0.3), T - 5))            # delists somewhere mid-panel
            last_idx[t] = li
            # terminal: bankruptcy (~0) or acquisition (premium) — KNOWN
            delist_terminal[t] = 0.0 if rng.random() < 0.6 else float(rng.uniform(1.05, 1.4))

    # ---- earnings calendar + injected SUE ---------------------------------------------------------
    # fiscal-year-end month: most December (12); a few off-cycle (June=6) to test fiscal alignment
    fy_end_month = {t: (6 if rng.random() < 0.15 else 12) for t in tickers}
    is_edge = {t: (rng.random() < edge_frac) for t in tickers}
    alpha_daily_target = alpha_ann / 252.0

    # ---- BC: per-name share-count + fundamental characteristics (NSI ladder, CMA, RMW) -------------
    is_nsi_edge = {t: (rng.random() < nsi_edge_frac) for t in tickers}
    nsi_rate = {t: float(rng.normal(0.0, 0.10)) for t in tickers}    # YoY share change: <0 buyback, >0 issuance
    cma_rate = {t: float(rng.normal(0.05, 0.10)) for t in tickers}   # YoY asset growth (CMA characteristic)
    rmw_level = {t: float(rng.uniform(0.05, 0.45)) for t in tickers} # gp/assets profitability (RMW characteristic)
    shares_base = {t: float(rng.uniform(1e7, 5e8)) for t in tickers}
    assets_base = {t: float(rng.uniform(5e8, 5e10)) for t in tickers}
    nsi_alpha_daily = nsi_alpha_ann / 252.0
    # ★ BD: profitability (gp/assets = rmw_level) -> drift edge — HIGH-prof names drift UP (sign vs the x-sec median)
    is_rmw_edge = {t: (rng.random() < rmw_edge_frac) for t in tickers}
    rmw_alpha_daily = rmw_alpha_ann / 252.0
    _rmw_med = float(np.median(list(rmw_level.values())))

    sf1_rows, truth_sue, restated = [], {}, set()
    eps_hist = {t: {} for t in tickers}                            # (fiscal_q_index) -> epsdil
    # per-name UE scale (sigma_UE)
    sigma_ue = {t: float(rng.uniform(0.05, 0.20)) for t in tickers}
    # drift schedule: name -> list of (datekey_idx, sue_value, drift_sign) for alpha injection
    drift_windows = {t: [] for t in tickers}

    for t in tickers:
        # quarter-end dates on the name's fiscal cycle, within its listed window
        fy = fy_end_month[t]
        q_ends = pd.date_range(start, end, freq="QE")
        q_ends = q_ends[((q_ends.month - fy) % 3) == 0]           # quarters aligned to the fiscal year
        base_eps = float(rng.uniform(0.2, 2.0))
        for qi, rp in enumerate(q_ends):
            dk = rp + pd.Timedelta(days=int(rng.integers(35, 55)))  # filed ~35-55d after period end
            if dk < days[0] or dk > days[-1]:
                continue
            dk_idx = int(days.searchsorted(dk))
            if dk_idx <= first_idx[t] or dk_idx >= last_idx[t]:
                continue
            sue = float(rng.normal(0, 1.0))                        # injected standardized surprise
            ue = sue * sigma_ue[t]
            prior = eps_hist[t].get(qi - 4, base_eps)              # YoY (fiscal) base
            eps_t = prior + ue
            eps_hist[t][qi] = eps_t
            lastupd = dk
            # ~8% of rows are back-filled restatements (lastupdated >> datekey) -> must be DROPPED
            if rng.random() < 0.08:
                lastupd = dk + pd.Timedelta(days=int(rng.integers(130, 400)))
                restated.add((t, pd.Timestamp(rp).normalize()))
            yfrac = qi / 4.0                                       # YoY ladders: factor compounds (1+rate) per 4q
            shares_t = shares_base[t] * (1.0 + nsi_rate[t]) ** yfrac
            assets_t = assets_base[t] * (1.0 + cma_rate[t]) ** yfrac
            sf1_rows.append({"ticker": t, "dimension": "ARQ", "datekey": dk, "reportperiod": rp,
                             "lastupdated": lastupd, "epsdil": eps_t, "eps": eps_t,
                             "sharesbas": shares_t, "assets": assets_t,
                             "revenue": 0.6 * assets_t, "equity": 0.5 * assets_t,
                             "gp": rmw_level[t] * assets_t})
            truth_sue[(t, pd.Timestamp(dk).normalize())] = sue
            # edge names: high |SUE| -> a post-earnings drift over the next hold, sign = sign(SUE)
            if is_edge[t] and abs(sue) > 0.5 and (qi - 4) in eps_hist[t]:
                drift_windows[t].append((dk_idx, sue, np.sign(sue)))

    # ---- build per-name daily total return TR, then close/closeadj/dividends -----------------------
    sep_rows, actions_rows, daily_rows = [], [], []
    truth_alpha = {}                                              # ticker -> np.array(T) injected alpha
    div_amt = {t: float(rng.uniform(0.0, 0.5)) for t in tickers}  # per-ex-date cash dividend
    pays_div = {t: (rng.random() < 0.6) for t in tickers}

    for t in tickers:
        fi, li = first_idx[t], last_idx[t]
        n = li - fi + 1
        if n < 80:
            continue
        idx = days[fi:li + 1]
        eps_idio = rng.normal(0, 0.012, n)
        alpha = np.zeros(n)
        for (dk_idx, sue, sgn) in drift_windows[t]:
            s = dk_idx - fi + 1                                   # drift starts day AFTER datekey (D+1)
            e = min(s + 63, n)                                    # over the ~63-td hold
            if 0 < s < n:
                alpha[s:e] += sgn * alpha_daily_target
        if is_nsi_edge[t]:                                        # BC: NSI drift — buyback (nsi<0) drifts UP
            alpha += -np.sign(nsi_rate[t]) * nsi_alpha_daily
        if is_rmw_edge[t]:                                        # BD: profitability drift — HIGH gp/assets drifts UP
            alpha += np.sign(rmw_level[t] - _rmw_med) * rmw_alpha_daily
        truth_alpha[t] = (idx, alpha.copy())
        factor_part = sum(betas[t][k] * F[k].values[fi:li + 1] for k in _FACTORS)
        tr = alpha + factor_part + eps_idio                      # daily TOTAL return
        # ex-dates: quarterly, on a fixed offset; cash dividend div_amt[t]
        div = np.zeros(n)
        if pays_div[t]:
            ex_positions = np.arange(40, n, 63)                  # ~quarterly ex-dates
            div[ex_positions] = div_amt[t]
        # splits: ~10% of names get one 2:1 split (affects closeunadj only; close/closeadj split-adj)
        split_factor = np.ones(n)
        if rng.random() < 0.10:
            sp = int(rng.integers(n // 3, 2 * n // 3))
            split_factor[sp:] = 2.0
        # close (price-only, split-adjusted): close_t = close_{t-1}*(1+TR_t) - div_t
        close = np.empty(n)
        close[0] = float(rng.uniform(10, 100))
        for k in range(1, n):
            close[k] = close[k - 1] * (1.0 + tr[k]) - div[k]
        # closeadj (total-return, split-adjusted): grows at TR
        closeadj = close[0] * np.cumprod(1.0 + tr)
        closeunadj = close * split_factor                        # raw as-traded (splits NOT removed)
        vol = rng.uniform(5e4, 5e6, n)
        # marketcap drives the liquid screen + the size factor; some names below the $300M floor
        shares = float(rng.uniform(2e6, 5e8))
        mcap = close * shares
        for k in range(n):
            sep_rows.append({"ticker": t, "date": idx[k], "open": close[k], "high": close[k] * 1.01,
                             "low": close[k] * 0.99, "close": close[k], "closeadj": closeadj[k],
                             "closeunadj": closeunadj[k], "dividends": div[k], "volume": vol[k]})
            daily_rows.append({"ticker": t, "date": idx[k], "marketcap": mcap[k],
                               "pb": float(abs(rng.normal(2, 1)) + 0.1), "pe": float(abs(rng.normal(15, 8)) + 1),
                               "ev": mcap[k] * 1.1})
        # ACTIONS: dividends (ex-date rows) + the delist terminal
        if pays_div[t]:
            for p in ex_positions:
                actions_rows.append({"ticker": t, "date": idx[p], "action": "dividend",
                                     "value": div_amt[t], "contraticker": None})
        if is_delisted[t]:
            actions_rows.append({"ticker": t, "date": idx[-1], "action": "delisted",
                                 "value": delist_terminal[t] * close[-1], "contraticker": None})

    sep = pd.DataFrame(sep_rows)
    sf1 = pd.DataFrame(sf1_rows)
    actions = pd.DataFrame(actions_rows) if actions_rows else pd.DataFrame(
        columns=["ticker", "date", "action", "value", "contraticker"])
    daily = pd.DataFrame(daily_rows)
    tickers_df = pd.DataFrame([{
        "ticker": t, "isdelisted": "Y" if is_delisted[t] else "N",
        "firstpricedate": days[first_idx[t]], "lastpricedate": days[last_idx[t]],
        "sector": rng.choice(["Tech", "Fin", "Health", "Energy", "Cons"]),
        "industry": "X", "category": "Domestic Common Stock",
    } for t in tickers])
    # SP500: a few add/remove events (reconstitution is CUT, but the table exists)
    sp_rows = []
    for t in rng.choice(tickers, size=min(20, n_names), replace=False):
        sp_rows.append({"date": days[int(rng.integers(0, T // 2))], "action": "added", "ticker": t})
    sp500 = pd.DataFrame(sp_rows)

    snap = Snapshot(sep=sep, sf1=sf1, actions=actions, daily=daily, tickers=tickers_df, sp500=sp500)
    snap.validate()
    truth = {
        "factors": F, "betas": betas, "alpha": truth_alpha, "sue": truth_sue,
        "restated": restated, "delist_terminal": delist_terminal, "is_edge": is_edge,
        "is_delisted": is_delisted, "fy_end_month": fy_end_month, "sigma_ue": sigma_ue,
        "alpha_daily_target": alpha_daily_target, "factor_cols": list(_FACTORS),
        # ★ BC ground truth (DEC-333): the known NSI / CMA / RMW characteristics + the NSI edge
        "is_nsi_edge": is_nsi_edge, "nsi_alpha_daily": nsi_alpha_daily,
        "is_rmw_edge": is_rmw_edge, "rmw_alpha_daily": rmw_alpha_daily,
        "nsi_char": {t: float(np.log1p(nsi_rate[t])) for t in tickers},      # annual NSI = log(1+nsi_rate)
        "cma_growth": {t: float(np.log1p(cma_rate[t])) for t in tickers},    # annual CMA = log(1+cma_rate)
        "rmw_level": rmw_level,                                              # gp/assets profitability
    }
    return snap, truth


def small(seed: int = 0):
    """A small, fast fixture for leaf-module unit tests (~8 years, 40 names)."""
    return make_snapshot(seed=seed, n_names=40, start="2012-01-03", end="2020-12-31")


if __name__ == "__main__":
    s, tr = make_snapshot()
    print("sep", s.sep.shape, "sf1", s.sf1.shape, "actions", s.actions.shape,
          "daily", s.daily.shape, "tickers", s.tickers.shape, "sp500", s.sp500.shape)
    print("edge names", sum(tr["is_edge"].values()), "delisted", sum(tr["is_delisted"].values()),
          "restated rows", len(tr["restated"]), "sue events", len(tr["sue"]))
