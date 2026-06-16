"""TDI for nsi.compute_nsi — net-share-issuance (H2 DIAGNOSTIC; charter §2 H2).

Spec assertions:
  (1) sharesbas x2 YoY -> nsi ~= ln(2); shrank to half -> ln(0.5).  [the construction is correct]
  (2) datekey present + PIT (uses only past rows; no future leakage into a NSI_t row).
  (3) handles missing YoY base (drops the under-4q-history rows; no NaN explosion).

We assert against KNOWN ground-truth: for the ratio test we inject a controlled name with a hand-set
sharesbas ladder (the random-uniform fixture sharesbas can't be asserted exactly); for PIT / structure
/ missing-base we drive the standard small() synthetic snapshot.
"""
import copy

import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig, Snapshot
from synth_fixtures import small
from nsi import compute_nsi


CFG = ForkBConfig()


# --------------------------------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------------------------------
def _arq_row(ticker, datekey, reportperiod, sharesbas):
    return {
        "ticker": ticker, "dimension": "ARQ",
        "datekey": pd.Timestamp(datekey), "reportperiod": pd.Timestamp(reportperiod),
        "lastupdated": pd.Timestamp(datekey), "epsdil": 1.0, "eps": 1.0,
        "sharesbas": float(sharesbas),
        # BC (DEC-333): the SF1 contract now also requires CMA/RMW inputs (dummy positives; compute_nsi ignores them)
        "assets": 1e9, "revenue": 6e8, "equity": 5e8, "gp": 3e8,
    }


def _snap_with_sf1(base_snap, sf1_rows):
    """Clone a snapshot but swap in a hand-built sf1 (other tables irrelevant for compute_nsi)."""
    sf1 = pd.DataFrame(sf1_rows)
    # match the contract dtypes the validator expects
    for c in ("datekey", "reportperiod", "lastupdated"):
        sf1[c] = pd.to_datetime(sf1[c])
    snap = Snapshot(sep=base_snap.sep, sf1=sf1, actions=base_snap.actions,
                    daily=base_snap.daily, tickers=base_snap.tickers, sp500=base_snap.sp500)
    snap.validate()
    return snap


# --------------------------------------------------------------------------------------------------
# (1) construction: x2 YoY -> ln(2), half -> ln(0.5)
# --------------------------------------------------------------------------------------------------
def test_nsi_ratio_doubling_and_halving():
    base_snap, _ = small()
    # 8 fiscal quarters for a doubler (DBL) and a halver (HLV). reportperiods on a clean fiscal cycle.
    rps = pd.date_range("2015-03-31", periods=8, freq="QE")
    rows = []
    # DBL: shares = 100,110,120,130 then doubled YoY at the 4q-back base -> exactly x2 vs t-4q.
    #   t-4q base = first 4 quarters; t = each base * 2 -> log ratio == ln(2) for the last 4 rows.
    dbl_base = [100.0, 110.0, 120.0, 130.0]
    dbl = dbl_base + [b * 2.0 for b in dbl_base]
    # HLV: t = base / 2 -> log ratio == ln(0.5).
    hlv_base = [400.0, 380.0, 360.0, 340.0]
    hlv = hlv_base + [b * 0.5 for b in hlv_base]
    for qi, rp in enumerate(rps):
        dk = rp + pd.Timedelta(days=45)
        rows.append(_arq_row("DBL", dk, rp, dbl[qi]))
        rows.append(_arq_row("HLV", dk, rp, hlv[qi]))
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)

    dbl_vals = nsi[nsi.ticker == "DBL"]["nsi"].to_numpy()
    hlv_vals = nsi[nsi.ticker == "HLV"]["nsi"].to_numpy()
    # exactly 4 rows survive each (8 quarters - 4q YoY base)
    assert len(dbl_vals) == 4
    assert len(hlv_vals) == 4
    np.testing.assert_allclose(dbl_vals, np.log(2.0), rtol=0, atol=1e-12)
    np.testing.assert_allclose(hlv_vals, np.log(0.5), rtol=0, atol=1e-12)


def test_nsi_fiscal_quarter_aligned_yoy_base():
    """The YoY base is 4 quarters back on the name's OWN fiscal cycle (not calendar Dec-31).

    Build an off-Dec fiscal cycle (June FY-end ladder). A purely-calendar implementation would mis-pick
    the base; a fiscal-quarter-aligned one picks the 4th-prior fiscal quarter. Set sharesbas so the
    correct 4q-back ratio is a distinct, known value.
    """
    base_snap, _ = small()
    # off-cycle fiscal quarters: period ends Aug/Nov/Feb/May (June FY-end name)
    rps = pd.to_datetime(["2015-08-31", "2015-11-30", "2016-02-29", "2016-05-31",
                          "2016-08-31", "2016-11-30", "2017-02-28", "2017-05-31"])
    # known ladder: first 4 = 200; next 4 = 200 * e (so 4q-back log ratio == 1.0 exactly)
    shares = [200.0, 200.0, 200.0, 200.0,
              200.0 * np.e, 200.0 * np.e, 200.0 * np.e, 200.0 * np.e]
    rows = [_arq_row("OFFCY", rp + pd.Timedelta(days=50), rp, s) for rp, s in zip(rps, shares)]
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)
    vals = nsi[nsi.ticker == "OFFCY"]["nsi"].to_numpy()
    assert len(vals) == 4
    np.testing.assert_allclose(vals, 1.0, atol=1e-12)


# --------------------------------------------------------------------------------------------------
# (2) datekey present + PIT
# --------------------------------------------------------------------------------------------------
def test_datekey_present_and_is_the_current_filing_date():
    """Each NSI_t row carries datekey_t (the CURRENT, latest filing's availability date) — the PIT key.

    Verify (a) the column exists + is datetime, (b) the datekey on a row equals the CURRENT quarter's
    datekey (not the base quarter's) — i.e. NSI_t is keyed to when sharesbas_t became known, so a
    consumer applying datekey <= D-1 never sees it before it exists.
    """
    base_snap, _ = small()
    rps = pd.date_range("2015-03-31", periods=8, freq="QE")
    rows = []
    for qi, rp in enumerate(rps):
        dk = rp + pd.Timedelta(days=40 + qi)  # distinct datekeys
        rows.append(_arq_row("PIT", dk, rp, 100.0 + 10.0 * qi))
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)
    assert "datekey" in nsi.columns
    assert pd.api.types.is_datetime64_any_dtype(nsi["datekey"])

    g = nsi[nsi.ticker == "PIT"].sort_values("datekey").reset_index(drop=True)
    # current quarters with a base = quarters 5..8 (0-indexed 4..7); their datekeys are the CURRENT ones
    expected_current_datekeys = [rps[qi] + pd.Timedelta(days=40 + qi) for qi in range(4, 8)]
    assert list(g["datekey"]) == [pd.Timestamp(d) for d in expected_current_datekeys]


def test_pit_no_future_rows_leak_into_a_row():
    """PIT: NSI_t is built ONLY from rows at/before t (current + its 4q-back base). Truncating the
    panel to before a future filing must NOT change earlier NSI values (no look-ahead)."""
    snap_full, _ = small()
    full = compute_nsi(snap_full, CFG)

    # pick a real name with several filings; cut its sf1 to only the first 6 filings (drop the future)
    tkr = full["ticker"].iloc[0]
    sf1 = snap_full.sf1
    g = sf1[sf1.ticker == tkr].sort_values(["reportperiod", "datekey"])
    cutoff_dk = g["datekey"].iloc[5]
    sf1_cut = sf1[(sf1.ticker != tkr) | (sf1.datekey <= cutoff_dk)].copy()
    snap_cut = Snapshot(sep=snap_full.sep, sf1=sf1_cut, actions=snap_full.actions,
                        daily=snap_full.daily, tickers=snap_full.tickers, sp500=snap_full.sp500)

    cut = compute_nsi(snap_cut, CFG)

    f = full[(full.ticker == tkr) & (full.datekey <= cutoff_dk)].sort_values("datekey").reset_index(drop=True)
    c = cut[(cut.ticker == tkr) & (cut.datekey <= cutoff_dk)].sort_values("datekey").reset_index(drop=True)
    # the surviving early NSI values are identical whether or not future filings are present
    pd.testing.assert_frame_equal(f, c)


# --------------------------------------------------------------------------------------------------
# (3) missing YoY base handling: drop, no NaN explosion
# --------------------------------------------------------------------------------------------------
def test_first_four_quarters_have_no_base_and_are_dropped():
    base_snap, _ = small()
    rps = pd.date_range("2015-03-31", periods=8, freq="QE")
    rows = [_arq_row("SHORT", rp + pd.Timedelta(days=45), rp, 100.0 + qi)
            for qi, rp in enumerate(rps)]
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)
    g = nsi[nsi.ticker == "SHORT"]
    # 8 quarters - 4q YoY base = exactly 4 surviving rows; the first 4 (no base) are dropped
    assert len(g) == 4
    # no NaN explosion anywhere
    assert not nsi["nsi"].isna().any()


def test_name_with_too_few_quarters_yields_no_rows():
    base_snap, _ = small()
    rps = pd.date_range("2015-03-31", periods=4, freq="QE")  # only 4 quarters: no YoY base possible
    rows = [_arq_row("TINY", rp + pd.Timedelta(days=45), rp, 100.0) for rp in rps]
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)
    assert (nsi.ticker == "TINY").sum() == 0
    assert not nsi["nsi"].isna().any()


def test_nonpositive_or_missing_shares_dropped_no_nan():
    """A zero/NaN sharesbas (missing base) drops the affected row; log never produces NaN/inf output."""
    base_snap, _ = small()
    rps = pd.date_range("2015-03-31", periods=8, freq="QE")
    shares = [100.0, 0.0, np.nan, 130.0, 200.0, 220.0, 240.0, 260.0]
    rows = [_arq_row("BAD", rp + pd.Timedelta(days=45), rp, s) for rp, s in zip(rps, shares)]
    snap = _snap_with_sf1(base_snap, rows)

    nsi = compute_nsi(snap, CFG)
    vals = nsi[nsi.ticker == "BAD"]["nsi"]
    assert np.isfinite(vals.to_numpy()).all()
    assert not vals.isna().any()
    # the t whose 4q-back base is the 0 (qi=1) and NaN (qi=2) must be excluded:
    #   qi=5 base=qi1=0 -> dropped; qi=6 base=qi2=NaN -> dropped
    g = nsi[nsi.ticker == "BAD"].reset_index(drop=True)
    # surviving currents: qi=4(base100), qi=7(base130) -> 2 rows
    assert len(g) == 2


# --------------------------------------------------------------------------------------------------
# structure / contract: runs end-to-end on the real synthetic snapshot
# --------------------------------------------------------------------------------------------------
def test_endtoend_on_small_snapshot_contract():
    snap, _ = small()
    nsi = compute_nsi(snap, CFG)
    assert list(nsi.columns) == ["ticker", "datekey", "nsi"]
    assert len(nsi) > 0
    assert pd.api.types.is_datetime64_any_dtype(nsi["datekey"])
    assert not nsi["nsi"].isna().any()
    assert np.isfinite(nsi["nsi"].to_numpy()).all()
    # NSI is a YoY log-ratio: with random-walk-ish synthetic shares it must straddle zero (issuers +,
    # repurchasers -) -> the diagnostic has both signs to rank into quintiles.
    assert nsi["nsi"].min() < 0 < nsi["nsi"].max()
