"""
power_gate — corrected correlation variance-inflation power gate.

The bug this fixes: a CORRELATION's effective sample size uses the product of BOTH
autocorrelations, not the mean's single-series correction:

    N_eff = T * (1 - rho_x*rho_y) / (1 + rho_x*rho_y)        <-- correlation (correct)
        (NOT  T * (1 - rho_1)  / (1 + rho_1) , which is the MEAN's correction)

where rho_x = lag-1 autocorr of the SIGNAL (e.g. a spread level) and
      rho_y = lag-1 autocorr of the FORWARD-RETURN SERIES ACTUALLY USED IN THE IC.

The horizon<->power coupling: rho_y MUST be computed on the same OVERLAPPING H-day forward-return
series the IC pairs on -- NOT a 1-day series. For H=1d (non-overlapping) rho_y~=0 => N_eff~=T =>
MDE_IC~=0.05 (borderline passes). For overlapping H>1 the forward returns carry rho_y ~= (H-k)/H by
construction => N_eff collapses => MDE_IC blows past 0.05 (5d~=0.11, 21d~=0.23 => power-gated-out =>
CANNOT-TEST). So this module, fed the real series, reproduces the horizon dependence automatically;
the discipline is that the HARNESS passes the H-day series.

MDE_IC = Z_SUM * SE_IC ,  SE_IC = 1/sqrt(N_eff) ,  default Z_SUM = 1.96 + 0.84 = 2.80
         (two-sided alpha=0.05 + 80% power; the value is a FREEZE parameter).
Gate-out iff MDE_IC > ceiling (frozen 0.05). A rho-sensitivity BAND brackets MDE_IC under
+/- perturbations of (rho_x, rho_y) so a borderline call is reported honestly, not as a point.

PURE: numpy-only, no I/O. Deterministic.
"""
from __future__ import annotations

import numpy as np

__all__ = ["ar1", "n_eff_correlation", "se_ic", "mde_ic", "power_gate"]

Z_SUM_DEFAULT = 1.96 + 0.84  # two-sided alpha=0.05 + 80% power == 2.80 (FREEZE parameter)


def ar1(series) -> float:
    """Lag-1 autocorrelation (Pearson of x_t vs x_{t-1}) over finite values. NaN if undefined."""
    x = np.asarray(series, dtype=float).ravel()
    x = x[np.isfinite(x)]
    if x.size < 3:
        return float("nan")
    a, b = x[1:], x[:-1]
    if np.std(a) == 0.0 or np.std(b) == 0.0:
        return float("nan")
    return float(np.corrcoef(a, b)[0, 1])


def n_eff_correlation(T: int, rho_x: float, rho_y: float, *, cap_at_T: bool = True) -> float:
    """Corrected correlation-VIF effective N. Clips the product off +/-1; floors at 1."""
    if T <= 0:
        return float("nan")
    prod = float(np.clip(rho_x * rho_y, -1.0 + 1e-12, 1.0 - 1e-12))
    n_eff = T * (1.0 - prod) / (1.0 + prod)
    if cap_at_T:
        n_eff = min(n_eff, float(T))     # cannot gain power beyond the realised sample (conservative)
    return float(max(n_eff, 1.0))


def se_ic(n_eff: float) -> float:
    return float(1.0 / np.sqrt(n_eff)) if n_eff and n_eff > 0 else float("nan")


def mde_ic(n_eff: float, *, z_sum: float = Z_SUM_DEFAULT) -> float:
    return float(z_sum * se_ic(n_eff))


def power_gate(signal, forward_return, *, ceiling: float = 0.05, z_sum: float = Z_SUM_DEFAULT,
               rho_delta: float = 0.10, cap_at_T: bool = True) -> dict:
    """
    Compute the corrected-VIF power verdict for {signal vs forward_return}.

    `forward_return` MUST be the SAME (possibly overlapping H-day) series fed to rank_ic
    (v2-DS-1) -- its autocorrelation is what collapses N_eff at longer horizons.

    Returns dict: T, rho_x, rho_y, n_eff, se_ic, mde_ic, ceiling, gated_out,
                  mde_band_low, mde_band_high (rho-sensitivity), z_sum.
    gated_out == True  => MDE_IC > ceiling => outcome-0 CANNOT-TEST.
    """
    s = np.asarray(signal, dtype=float).ravel()
    f = np.asarray(forward_return, dtype=float).ravel()
    if s.shape != f.shape:
        raise ValueError(f"signal and forward_return must align: {s.shape} vs {f.shape}")
    mask = np.isfinite(s) & np.isfinite(f)
    T = int(mask.sum())

    rho_x, rho_y = ar1(s), ar1(f)
    rx = 0.0 if not np.isfinite(rho_x) else rho_x
    ry = 0.0 if not np.isfinite(rho_y) else rho_y

    n_eff = n_eff_correlation(T, rx, ry, cap_at_T=cap_at_T)
    mde = mde_ic(n_eff, z_sum=z_sum)

    # rho-sensitivity band: perturb BOTH autocorrelations +/- rho_delta (clip to [-0.99,0.99]).
    # higher product -> smaller N_eff -> larger MDE (worst case) and vice-versa.
    mdes = []
    for dx in (-rho_delta, 0.0, rho_delta):
        for dy in (-rho_delta, 0.0, rho_delta):
            rxp = float(np.clip(rx + dx, -0.99, 0.99))
            ryp = float(np.clip(ry + dy, -0.99, 0.99))
            mdes.append(mde_ic(n_eff_correlation(T, rxp, ryp, cap_at_T=cap_at_T), z_sum=z_sum))

    return {
        "T": T, "rho_x": float(rx), "rho_y": float(ry),
        "n_eff": float(n_eff), "se_ic": float(se_ic(n_eff)), "mde_ic": float(mde),
        "ceiling": float(ceiling), "gated_out": bool(mde > ceiling),
        "mde_band_low": float(min(mdes)), "mde_band_high": float(max(mdes)),
        "z_sum": float(z_sum),
    }
