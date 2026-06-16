"""TDI for tr_canary — total-return build + Canary HALT-gate (charter §2 CANARY; RT-A/closeadj).

Asserts against the synthetic ground-truth: the fixture constructs close/closeadj/dividends to EXACTLY satisfy
total return on both paths (synth_fixtures lines ~138-144), so:
  - closeadj = close[0] * cumprod(1+tr)            -> Path A = closeadj.pct_change() == tr
  - close[k] = close[k-1]*(1+tr[k]) - div[k]       -> Path B = (close.diff()+div)/close.shift() == tr
Both paths recover the SAME tr -> the Canary reconciles (gap ~0, halt=False). A dividend-forgotten rebuild
(close.pct_change()) diverges on ex-dates by div/close.shift() -> the Canary catches it.
"""
import numpy as np
import pandas as pd

from _schema import ForkBConfig
from synth_fixtures import small
from tr_canary import build_tr, run_canary, run_canary_against


def test_path_a_equals_path_b_no_halt():
    """(1) Path A == Path B on the fixtures (max gap < 1e-9) -> halt False."""
    snap, _ = small()
    cfg = ForkBConfig()
    res = run_canary(snap, cfg)
    assert res["halt"] is False, f"correct rebuild must not HALT; got {res['detail']}"
    assert res["max_path_gap"] < 1e-9, f"A vs B must reconcile to ~0; got {res['max_path_gap']}"
    assert res["detail"]["status"] == "PASS"
    assert res["detail"]["n_disagree"] == 0
    assert res["detail"]["n_eligible"] > 0
    # the per-ex-day signed check is ~0 for a correct rebuild
    assert res["detail"]["n_exday"] > 0, "fixture must have dividend ex-days to exercise the signed check"
    assert abs(res["exday_t"]) < 1e-9


def test_div_forgotten_rebuild_caught_on_exdates():
    """(2) A div-FORGOTTEN rebuild (close.pct_change, ignoring dividends) reconciles WORSE on ex-dates ->
    the Canary catches it: assert the gap > 1e-4 for a dividend-paying name + halt True."""
    snap, _ = small()
    cfg = ForkBConfig()

    # reconcile Path A against the price-only (dividend-forgotten) rebuild
    bad = run_canary_against(snap, cfg, rebuild_path="price_only")
    assert bad["halt"] is True, "the div-forgotten rebuild MUST trip the HALT-gate"
    assert bad["max_path_gap"] > 1e-4, f"div-forgotten gap must be material; got {bad['max_path_gap']}"

    # localize: pick a dividend-paying name and confirm its ex-day gap is the discrepancy div/close.shift()
    sep = snap.sep.sort_values(["ticker", "date"], kind="mergesort")
    div_names = sep.loc[sep["dividends"] > 0, "ticker"].unique()
    assert len(div_names) > 0, "fixture must contain a dividend-paying name"
    t = div_names[0]
    g = sep[sep["ticker"] == t].reset_index(drop=True)
    tr_a = g["closeadj"].pct_change()
    tr_price_only = g["close"].diff() / g["close"].shift()
    exmask = g["dividends"] > 0
    # the discrepancy on ex-days equals div_t / close_{t-1}; must exceed 1e-4
    exgap = (tr_a - tr_price_only).abs()[exmask]
    assert exgap.max() > 1e-4, f"per-name ex-day gap must be > 1e-4; got {exgap.max()}"
    expected = (g["dividends"] / g["close"].shift())[exmask]
    np.testing.assert_allclose(exgap.values, expected.values, rtol=1e-9, atol=1e-12)
    # and a correct Path B for the SAME name has ~0 ex-day gap (proves it's the missing div, not noise)
    tr_b = (g["close"].diff() + g["dividends"]) / g["close"].shift()
    assert (tr_a - tr_b).abs()[exmask].max() < 1e-9


def test_build_tr_matches_closeadj_pct_change_for_sample_name():
    """(3) build_tr's daily TR matches closeadj.pct_change() for a sample name (Path A primary by default)."""
    snap, _ = small()
    cfg = ForkBConfig()
    assert cfg.PHASE0_closeadj_is_total_return is None  # default -> Path A primary
    out = build_tr(snap, cfg)
    assert list(out.columns) == ["ticker", "date", "tr"]

    sep = snap.sep.sort_values(["ticker", "date"], kind="mergesort")
    t = sep["ticker"].iloc[0]
    g = sep[sep["ticker"] == t].reset_index(drop=True)
    expected = g["closeadj"].pct_change()

    got = out[out["ticker"] == t].sort_values("date")["tr"].reset_index(drop=True)
    # compare ignoring the leading NaN
    np.testing.assert_allclose(got.values[1:], expected.values[1:], rtol=1e-10, atol=1e-12)
    assert np.isnan(got.values[0]), "first day of a name has no prior close -> NaN tr"


def test_build_tr_recovers_injected_tr_via_path_b_equivalence():
    """(4) build_tr (Path A) equals an independent Path-B reconstruction for EVERY name (the fixture's TR
    identity holds name-wide), confirming the build is the true total return, not just for one sample."""
    snap, _ = small()
    cfg = ForkBConfig()
    out = build_tr(snap, cfg).rename(columns={"tr": "tr_a"})

    sep = snap.sep.sort_values(["ticker", "date"], kind="mergesort").reset_index(drop=True)
    prev = sep.groupby("ticker", sort=False)["close"].shift()
    sep["tr_b"] = (sep.groupby("ticker", sort=False)["close"].diff() + sep["dividends"]) / prev
    merged = out.merge(sep[["ticker", "date", "tr_b"]], on=["ticker", "date"], how="inner")
    elig = merged["tr_a"].notna() & merged["tr_b"].notna()
    gap = (merged.loc[elig, "tr_a"] - merged.loc[elig, "tr_b"]).abs()
    assert gap.max() < 1e-9, f"Path A must equal Path B name-wide; max gap {gap.max()}"


def test_path_b_selected_when_closeadj_not_total_return():
    """(5) Config switch: when PHASE0_closeadj_is_total_return is False, build_tr uses Path B; on the fixture
    Path A == Path B so the numbers are identical, but this pins the dispatch (faithfulness to the §3 switch)."""
    snap, _ = small()
    cfg_a = ForkBConfig()                                              # None -> Path A
    cfg_b = ForkBConfig(PHASE0_closeadj_is_total_return=False)         # False -> Path B
    out_a = build_tr(snap, cfg_a)
    out_b = build_tr(snap, cfg_b)
    m = out_a.rename(columns={"tr": "a"}).merge(
        out_b.rename(columns={"tr": "b"}), on=["ticker", "date"])
    elig = m["a"].notna() & m["b"].notna()
    assert (m.loc[elig, "a"] - m.loc[elig, "b"]).abs().max() < 1e-9


def test_unexplained_gross_gap_halts_and_action_explains_it():
    """Real-data refinement: an UNEXPLAINED gross A-vs-B gap on a LIQUID (traded), ORDINARY (no-corporate-action)
    day HALTs; the same gap becomes EXPLAINED (no HALT) once an action (here a spinoff) sits on that date — and
    a non-liquid or near-a-dividend gap is NOT counted (Path-B blind spots / out-of-scope names)."""
    from tr_canary import _liquid_eligible
    snap, _ = small()
    cfg = ForkBConfig()
    liq = _liquid_eligible(snap, cfg)
    sep = snap.sep.copy()
    div = snap.sep.loc[snap.sep["dividends"] > 0, ["ticker", "date"]]
    # find a LIQUID name + a liquid, dividend-free (ordinary) day to inject the gross gap
    chosen_t = chosen_d = None
    for t in [c for c in liq.columns if int(liq[c].sum()) > 100]:
        tdiv = list(div.loc[div["ticker"] == t, "date"])
        for d in sep.loc[sep["ticker"] == t, "date"].sort_values().iloc[20:-5]:
            if not bool(liq.at[d, t]):
                continue
            if any(abs((d - dd).days) <= 10 for dd in tdiv):       # ordinary: no dividend within +-10 days
                continue
            chosen_t, chosen_d = t, d
            break
        if chosen_t is not None:
            break
    assert chosen_t is not None, "fixture must have a liquid, dividend-free (ordinary) day"

    # corrupt closeadj from chosen_d onward (x1.30) -> +30% Path-A step on chosen_d (no action) = unexplained gross gap
    m = (sep["ticker"] == chosen_t) & (sep["date"] >= chosen_d)
    sep.loc[m, "closeadj"] = sep.loc[m, "closeadj"] * 1.30
    snap.sep = sep
    r = run_canary(snap, cfg)
    assert r["halt"] is True, "an unexplained gross gap on a liquid ordinary day must HALT"
    assert r["detail"]["max_unexplained_gap"] > cfg.canary_gross_gap

    # placing a spinoff action on that date -> EXPLAINED -> no gross unexplained gap remains
    snap.actions = pd.concat([snap.actions, pd.DataFrame(
        [{"ticker": chosen_t, "date": chosen_d, "action": "spinoff", "value": 1.0, "contraticker": None}])],
        ignore_index=True)
    r2 = run_canary(snap, cfg)
    assert r2["detail"]["max_unexplained_gap"] < cfg.canary_gross_gap


def test_zero_eligible_names_is_warning_not_halt():
    """Charter §2: a zero-eligible-names case is a WARNING, not a HALT."""
    snap, _ = small()
    # empty the SEP table -> no eligible name-days
    snap.sep = snap.sep.iloc[0:0].copy()
    cfg = ForkBConfig()
    res = run_canary(snap, cfg)
    assert res["halt"] is False
    assert res["detail"]["status"] == "WARNING"
    assert res["detail"]["n_eligible"] == 0
