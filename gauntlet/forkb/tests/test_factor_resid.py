"""TDI for factor_resid — PIT factor attribution with alpha RETAINED (charter §4; D-HARD-6; ★ RT-A).

The load-bearing assertion (RT-A scar): the factor-residual must RETAIN alpha. Construct a synthetic
book raw = alpha0 + factors @ beta0 + noise with KNOWN alpha0>0 and KNOWN beta0, then assert
mean(residualize(...)['resid']) ≈ alpha0 (NOT ≈ 0) — and ≈ 0 ONLY when alpha0 == 0. A module that
(wrongly) used abnormal_returns would mean-ZERO the residual regardless of the true alpha -> a
guaranteed false-NULL. These tests fail loudly on that mistake.
"""
import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from synth_fixtures import small, make_snapshot
from factor_resid import pit_factor_returns, residualize


CFG = ForkBConfig()


def _synthetic_book(alpha0, beta0, n=2000, seed=7, noise_sd=0.004):
    """raw = alpha0 + factors @ beta0 + iid noise, with a KNOWN alpha0 and beta0 (dict factor->beta)."""
    rng = np.random.default_rng(seed)
    dates = pd.bdate_range("2005-01-03", periods=n)
    factor_cols = list(CFG.factors)
    F = pd.DataFrame({k: rng.normal(0, 0.008, n) for k in factor_cols}, index=dates)
    beta_vec = np.array([beta0[k] for k in factor_cols], dtype="float64")
    factor_part = F.to_numpy() @ beta_vec
    noise = rng.normal(0, noise_sd, n)
    raw = pd.Series(alpha0 + factor_part + noise, index=dates, name="raw_net")
    return raw, F, beta0


# ----------------------------------------------------------------------------------------------------
# ★ RT-A — the load-bearing test: resid RETAINS alpha (mean(resid) ≈ alpha0, NOT 0)
# ----------------------------------------------------------------------------------------------------
def test_rt_a_resid_retains_positive_alpha():
    """alpha0 > 0  ->  mean(resid) ≈ alpha0  (the deployable edge survives the factor strip)."""
    alpha0 = 0.0008  # ~+20%/yr daily alpha — clearly nonzero
    beta0 = {"size": 0.7, "value": -0.4, "mom": 0.3, "str": 0.15, "beta": 1.1}
    raw, F, _ = _synthetic_book(alpha0, beta0)

    out = residualize(raw, F, CFG)
    resid_mean = out["resid"].mean()

    # resid mean RETAINS the injected alpha (RT-A) — NOT mean-zero
    assert resid_mean == pytest.approx(alpha0, abs=2e-4), f"resid mean {resid_mean} != alpha0 {alpha0}"
    # and it is unambiguously far from zero (the false-NULL trap)
    assert abs(resid_mean) > alpha0 * 0.5
    # mean(resid) == alpha (the intercept) — the two definitions agree
    assert resid_mean == pytest.approx(out["alpha"], abs=1e-9)


def test_rt_a_resid_zero_only_when_alpha_zero():
    """alpha0 == 0  ->  mean(resid) ≈ 0  (the residual is mean-zero ONLY in the genuine no-edge case)."""
    beta0 = {"size": 0.5, "value": 0.2, "mom": -0.6, "str": 0.4, "beta": 0.9}
    raw, F, _ = _synthetic_book(0.0, beta0)

    out = residualize(raw, F, CFG)
    resid_mean = out["resid"].mean()
    assert resid_mean == pytest.approx(0.0, abs=2e-4), f"no-alpha resid mean {resid_mean} not ~0"


def test_betas_and_alpha_recovered():
    """OLS recovers the injected betas ≈ beta0 and alpha ≈ alpha0 for every factor."""
    alpha0 = 0.0005
    beta0 = {"size": 0.8, "value": -0.3, "mom": 0.5, "str": -0.2, "beta": 1.25}
    raw, F, _ = _synthetic_book(alpha0, beta0, noise_sd=0.003, seed=11)

    out = residualize(raw, F, CFG)
    assert out["alpha"] == pytest.approx(alpha0, abs=2e-4)
    for k in CFG.factors:
        assert out["betas"][k] == pytest.approx(beta0[k], abs=0.05), f"beta[{k}] off: {out['betas'][k]} vs {beta0[k]}"


def test_resid_equals_raw_minus_F_dot_beta_not_abnormal_returns():
    """resid == raw - F @ beta EXACTLY (RT-A); it is NOT raw - (alpha + F@beta) = the mean-zero epsilon."""
    alpha0 = 0.0006
    beta0 = {"size": 0.4, "value": 0.1, "mom": 0.2, "str": 0.3, "beta": 1.0}
    raw, F, _ = _synthetic_book(alpha0, beta0, seed=3)

    out = residualize(raw, F, CFG)
    beta_vec = np.array([out["betas"][k] for k in CFG.factors])
    expected_resid = raw.to_numpy() - F[list(CFG.factors)].to_numpy() @ beta_vec
    np.testing.assert_allclose(out["resid"].to_numpy(), expected_resid, atol=1e-12)

    # the WRONG (abnormal_returns) path would be raw - (alpha + F@beta) -> mean ~0; prove resid is NOT that
    wrong = raw.to_numpy() - (out["alpha"] + F[list(CFG.factors)].to_numpy() @ beta_vec)
    assert abs(np.mean(wrong)) < abs(np.mean(out["resid"].to_numpy()))   # wrong path is the false-NULL
    assert np.mean(out["resid"].to_numpy()) == pytest.approx(alpha0, abs=2e-4)


def test_alpha_t_significant_for_real_edge_and_insignificant_for_none():
    """alpha_t = plain t of the intercept: large for a real injected alpha, ~0 for no alpha."""
    beta0 = {"size": 0.6, "value": -0.2, "mom": 0.4, "str": 0.1, "beta": 1.0}
    raw_edge, F_e, _ = _synthetic_book(0.0008, beta0, n=2000, seed=21, noise_sd=0.004)
    raw_none, F_n, _ = _synthetic_book(0.0, beta0, n=2000, seed=22, noise_sd=0.004)

    t_edge = residualize(raw_edge, F_e, CFG)["alpha_t"]
    t_none = residualize(raw_none, F_n, CFG)["alpha_t"]

    assert t_edge > 2.0, f"real-edge alpha_t {t_edge} should be significant"
    assert abs(t_none) < 2.0, f"no-edge alpha_t {t_none} should be insignificant"


# ----------------------------------------------------------------------------------------------------
# pit_factor_returns — PIT panel contract (N6) against the synthetic snapshot
# ----------------------------------------------------------------------------------------------------
def test_pit_factor_returns_contract():
    """pit_factor_returns returns a date x factor panel matching cfg.factors, finite, non-degenerate."""
    snap, truth = small()
    F = pit_factor_returns(snap, CFG)

    assert list(F.columns) == list(CFG.factors)
    assert pd.api.types.is_datetime64_any_dtype(F.index)
    # PIT formation needs warm-up (12-1 mom rolling window) -> later dates must be populated
    tail = F.dropna()
    assert len(tail) > 100, f"too few formed factor dates: {len(tail)}"
    # each factor varies (a non-degenerate spread series), not all-constant
    for k in CFG.factors:
        assert tail[k].std() > 0, f"factor {k} is degenerate (zero variance)"


def test_residualize_consumes_pit_panel_end_to_end():
    """End-to-end: a book built as alpha + (PIT factor panel)@beta residualizes back to alpha."""
    snap, truth = small()
    Fpanel = pit_factor_returns(snap, CFG).dropna()
    factor_cols = list(CFG.factors)

    alpha0 = 0.0007
    beta0 = {"size": 0.5, "value": -0.3, "mom": 0.4, "str": 0.2, "beta": 0.8}
    beta_vec = np.array([beta0[k] for k in factor_cols])
    rng = np.random.default_rng(99)
    raw = pd.Series(
        alpha0 + Fpanel[factor_cols].to_numpy() @ beta_vec + rng.normal(0, 0.003, len(Fpanel)),
        index=Fpanel.index, name="raw_net",
    )
    out = residualize(raw, Fpanel, CFG)
    assert out["resid"].mean() == pytest.approx(alpha0, abs=3e-4)
    for k in factor_cols:
        assert out["betas"][k] == pytest.approx(beta0[k], abs=0.08)


def test_residualize_aligns_and_drops_nans():
    """raw_net with NaNs and a non-overlapping date is handled by pairwise inner-join alignment."""
    alpha0 = 0.0005
    beta0 = {"size": 0.3, "value": 0.2, "mom": 0.1, "str": 0.0, "beta": 1.0}
    raw, F, _ = _synthetic_book(alpha0, beta0, n=600, seed=5)
    raw2 = raw.copy()
    raw2.iloc[:20] = np.nan                                   # leading NaNs must be dropped
    out = residualize(raw2, F, CFG)
    assert len(out["resid"]) == len(raw) - 20
    assert np.isfinite(out["resid"]).all()
    assert out["resid"].mean() == pytest.approx(alpha0, abs=3e-4)
