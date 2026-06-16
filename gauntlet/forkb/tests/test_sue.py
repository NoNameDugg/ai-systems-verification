"""TDI for sue.compute_sue — asserted against the synthetic ground-truth (truth['sue'] etc.).

Charter §2 H1 (v6.1, lines 84-86): the SUE engine must recover the injected standardized surprise
on well-seasoned non-restated rows, drop every back-filled restatement (DS-13), align YoY on the
FISCAL quarter for off-cycle filers, and never peek (sigma uses only earlier-datekey UEs).
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from sue import compute_sue, restatement_drop_balance
from synth_fixtures import small


@pytest.fixture(scope="module")
def fixture():
    snap, truth = small()
    cfg = ForkBConfig()
    df = compute_sue(snap, cfg)
    df = df.copy()
    df["key"] = list(zip(df["ticker"], df["datekey"].dt.normalize()))
    return snap, truth, cfg, df


# ----------------------------------------------------------------------------------------------------
# (1) computed sue ~= truth['sue'] for non-restated, well-seasoned rows
# ----------------------------------------------------------------------------------------------------
def test_sue_recovers_injected_truth(fixture):
    _, truth, _, df = fixture
    scored = df[~df["dropped"]].copy()
    scored["truth"] = scored["key"].map(truth["sue"])
    scored = scored.dropna(subset=["truth"])
    assert len(scored) > 100, "expected a healthy seasoned scored population"

    # sigma is an ESTIMATE of the per-name constant sigma_ue, so per-row sue is close-but-not-exact;
    # the right faithfulness metric is a strong cross-row correlation + a ~unit slope. Exclude rows
    # near the winsor clip so the per-cross-section clip doesn't distort the correlation.
    mask = scored["truth"].abs() < 4.5
    s_hat = scored.loc[mask, "sue"].to_numpy()
    s_true = scored.loc[mask, "truth"].to_numpy()
    corr = np.corrcoef(s_hat, s_true)[0, 1]
    assert corr > 0.9, f"computed SUE must track injected truth (corr={corr:.3f})"

    # slope ~ 1 (sigma estimate is unbiased-ish): regress s_hat on s_true through origin
    slope = float(np.dot(s_hat, s_true) / np.dot(s_true, s_true))
    assert 0.7 < slope < 1.4, f"SUE scale should be ~1 vs truth (slope={slope:.3f})"


def test_sue_sign_agreement(fixture):
    """The sign of the standardized surprise must agree with truth (PEAD direction is sign-driven)."""
    _, truth, _, df = fixture
    scored = df[~df["dropped"]].copy()
    scored["truth"] = scored["key"].map(truth["sue"])
    scored = scored.dropna(subset=["truth"])
    # ignore near-zero truth (sign is noise there)
    sig = scored[scored["truth"].abs() > 0.3]
    agree = (np.sign(sig["sue"]) == np.sign(sig["truth"])).mean()
    assert agree > 0.9, f"SUE sign must agree with injected surprise (agree={agree:.3f})"


# ----------------------------------------------------------------------------------------------------
# (2) every truth['restated'] row is dropped=True (DS-13 restatement-drop)
# ----------------------------------------------------------------------------------------------------
def test_every_restated_row_is_dropped(fixture):
    snap, truth, _, df = fixture
    sf1 = snap.sf1.copy()
    sf1["datekey"] = pd.to_datetime(sf1["datekey"])
    sf1["reportperiod"] = pd.to_datetime(sf1["reportperiod"])
    sf1["rp"] = sf1["reportperiod"].dt.normalize()

    dropped_keys = set(zip(df.loc[df["dropped"], "ticker"],
                           df.loc[df["dropped"], "datekey"].dt.normalize()))
    not_dropped = 0
    checked = 0
    for (tk, rp) in truth["restated"]:
        row = sf1[(sf1["ticker"] == tk) & (sf1["rp"] == rp)]
        if row.empty:
            continue
        checked += 1
        dkn = row.iloc[0]["datekey"].normalize()
        if (tk, dkn) not in dropped_keys:
            not_dropped += 1
    assert checked > 0
    assert not_dropped == 0, f"{not_dropped}/{checked} restated rows were not dropped"


def test_dropped_rows_have_nan_sue_and_are_excluded(fixture):
    """Dropped (restated) rows must NOT carry an imputed SUE — they are excluded, not filled."""
    _, _, _, df = fixture
    assert df.loc[df["dropped"], "sue"].isna().all(), "dropped rows must have sue=NaN (no impute)"
    assert df["dropped"].sum() > 0, "fixture should contain restatement drops"
    # drop-rate is logged + recoverable; balance helper exposes the scored sign distribution
    bal = restatement_drop_balance(df)
    assert bal["n_dropped"] == int(df["dropped"].sum())
    assert bal["scored_pos"] > 0 and bal["scored_neg"] > 0


# ----------------------------------------------------------------------------------------------------
# (3) off-cycle (fy_end_month==6) names align YoY on fiscal quarters (sue computed, not NaN)
# ----------------------------------------------------------------------------------------------------
def test_offcycle_fiscal_alignment(fixture):
    _, truth, _, df = fixture
    june_names = [t for t, m in truth["fy_end_month"].items() if m == 6]
    assert june_names, "fixture should contain at least one off-cycle June fiscal-year name"
    for t in june_names:
        sub = df[(df["ticker"] == t) & (~df["dropped"])]
        # an off-cycle name must still produce real (non-NaN) SUE: fiscal-quarter YoY aligned,
        # NOT NaN from a calendar mis-alignment
        assert len(sub) > 0, f"off-cycle name {t} produced no scored SUE"
        assert sub["sue"].notna().all(), f"off-cycle name {t} has NaN SUE (calendar mis-align?)"
        # and it must track its injected truth too
        sub = sub.copy()
        sub["truth"] = list(zip(sub["ticker"], sub["datekey"].dt.normalize()))
        sub["truth"] = sub["truth"].map(truth["sue"])
        sub = sub.dropna(subset=["truth"])
        m = sub["truth"].abs() < 4.5
        if m.sum() >= 5:
            corr = np.corrcoef(sub.loc[m, "sue"], sub.loc[m, "truth"])[0, 1]
            assert corr > 0.8, f"off-cycle {t} SUE should track truth (corr={corr:.3f})"


# ----------------------------------------------------------------------------------------------------
# (4) no-peek: a row's sigma uses ONLY earlier-datekey UEs (constructed independent check)
# ----------------------------------------------------------------------------------------------------
def test_no_peek_sigma_uses_only_past(fixture):
    """Independent re-derivation: for a sample of scored rows, recompute sigma from ONLY strictly
    earlier-datekey UEs and confirm sue == ue / that_past_sigma. If the engine peeked at same- or
    future-datekey UEs the recomputed value would diverge."""
    snap, _, cfg, df = fixture
    sf1 = snap.sf1[snap.sf1["dimension"] == cfg.sue_dim_primary].copy()
    sf1["datekey"] = pd.to_datetime(sf1["datekey"])
    sf1["reportperiod"] = pd.to_datetime(sf1["reportperiod"])
    sf1["lastupdated"] = pd.to_datetime(sf1["lastupdated"])
    K = pd.Timedelta(days=cfg.restatement_K_days)
    sf1["dropped"] = sf1["lastupdated"] > (sf1["datekey"] + K)
    sf1["fq"] = sf1["reportperiod"].dt.month

    checked = 0
    for ticker, g in sf1.groupby("ticker", sort=False):
        g = g.sort_values(["reportperiod", "datekey"]).reset_index(drop=True)
        eps = g["epsdil"].to_numpy(dtype=float)
        dk = g["datekey"].to_numpy()
        fq = g["fq"].to_numpy()
        dropped = g["dropped"].to_numpy()
        lag = cfg.sue_yoy_lag_q
        ue = np.full(len(g), np.nan)
        for i in range(len(g)):
            j = i - lag
            if j >= 0 and fq[i] == fq[j]:
                ue[i] = eps[i] - eps[j]
        scored_t = df[(df["ticker"] == ticker) & (~df["dropped"])]
        scored_t = scored_t.set_index(scored_t["datekey"].dt.normalize())
        for i in range(len(g)):
            if dropped[i] or np.isnan(ue[i]):
                continue
            past = [ue[k] for k in range(i)
                    if dk[k] < dk[i] and not dropped[k] and not np.isnan(ue[k])]
            if len(past) < cfg.sue_sigma_min_q:
                continue
            sigma = float(np.std(np.array(past[-cfg.sue_sigma_window_q:]), ddof=1))
            if not np.isfinite(sigma) or sigma <= 0:
                continue
            expected_pre_winsor = ue[i] / sigma
            dkn = pd.Timestamp(dk[i]).normalize()
            if dkn in scored_t.index:
                got = float(scored_t.loc[dkn, "sue"]) if np.ndim(scored_t.loc[dkn, "sue"]) == 0 \
                    else float(scored_t.loc[dkn, "sue"].iloc[0])
                # |expected| may exceed the winsor clip; compare only un-clipped rows
                lo, hi = cfg.sue_winsor
                if lo < expected_pre_winsor < hi and abs(got) < hi:
                    assert abs(got - expected_pre_winsor) < 1e-6, (
                        f"{ticker}@{dkn}: sigma must use only strictly-earlier UEs "
                        f"(got={got}, past-only={expected_pre_winsor})")
                    checked += 1
            if checked >= 60:
                break
        if checked >= 60:
            break
    assert checked >= 20, f"expected to validate the no-peek property on many rows (checked={checked})"
