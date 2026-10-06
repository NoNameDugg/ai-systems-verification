#!/usr/bin/env python
"""
validate_your_strategy.py -- point the gauntlet's rank-IC gate at YOUR signal and let it try to kill it.

Input: a CSV with one row per bar and the columns

    timestamp     the bar (ISO-8601); `price` is that bar's close
    signal        your signal's value computed for that bar (higher = expect a higher forward return)
    available_at  when you could actually have known that value -- MUST be after `timestamp`
    price         the bar's close, used for the forward return
    cost_bps      (optional) one-way trading cost in basis points of notional; a round trip is charged twice.
                  You may give it with --cost-bps instead. Without any cost the cost hurdle fails closed.

Seven checks run in order, each printing a one-line verdict; the final line is

    VERDICT: PASS        or        VERDICT: FAIL-<REASON> (check N: <name>)

  1  point-in-time stamps     every available_at strictly after its own bar          (pit_validator)
  2  causal pairing           entry is the first bar strictly after available_at;
                              raises on any violation                                (signal_return_pairer)
  3  rank-IC + power          Spearman rank-IC with CI; the minimum detectable IC from
                              an honest effective sample size                         (rank_ic_calculator, power_gate)
  4  two nulls                random-entry (random signs, same horizon, N draws) and
                              shuffled-label (signal permuted in horizon-sized blocks)
  5  lag sensitivity          rank-IC with every signal delayed 1 and 2 extra bars;
                              an effect that collapses is look-ahead-suspect           (lag_sensitivity)
  6  cost hurdle              mean per-trade return net of the round-trip cost
  7  multiple testing         Holm correction of the headline p for --n-variants-tried  (deflation)

If check 1 or 2 fails the rest are skipped: a statistic computed on mis-timed data is not evidence.
Only numpy and pandas are needed; everything else is imported from ../rank_ic_gate.
"""
from __future__ import annotations

import argparse
import os
import sys

import numpy as np
import pandas as pd

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.normpath(os.path.join(HERE, "..", "rank_ic_gate")))

from deflation import holm_adjusted_pvalues  # noqa: E402
from lag_sensitivity import lag_sensitivity  # noqa: E402
from pit_validator import audit_macro_pit, audit_pairs_pit  # noqa: E402
from power_gate import power_gate  # noqa: E402
from rank_ic_calculator import average_rank, rank_ic  # noqa: E402
from signal_return_pairer import pair_signal_forward_return  # noqa: E402

REQUIRED = ("timestamp", "signal", "available_at", "price")
MIN_PAIRS = 50
CHECK_NAMES = {
    1: "point-in-time stamps",
    2: "causal pairing",
    3: "rank-IC + power",
    4: "random-entry + shuffled-label nulls",
    5: "lag sensitivity",
    6: "cost hurdle",
    7: "multiple testing (Holm)",
}


# --------------------------------------------------------------------------------------------------
# input
# --------------------------------------------------------------------------------------------------
def load_csv(path: str) -> pd.DataFrame:
    """Read and validate the input CSV. Raises ValueError with a plain message on a malformed file."""
    df = pd.read_csv(path)
    missing = [c for c in REQUIRED if c not in df.columns]
    if missing:
        raise ValueError(f"missing required column(s): {', '.join(missing)}; need {', '.join(REQUIRED)}")
    if len(df) < 2:
        raise ValueError("need at least two rows")
    for col in ("timestamp", "available_at"):
        try:
            df[col] = pd.to_datetime(df[col])
        except (ValueError, TypeError) as e:
            raise ValueError(f"column {col!r} is not parseable as timestamps: {e}") from e
        if df[col].isna().any():
            raise ValueError(f"column {col!r} has missing values")
    df = df.sort_values("timestamp", kind="mergesort").reset_index(drop=True)
    if (df["timestamp"].diff().dropna() <= pd.Timedelta(0)).any():
        raise ValueError("timestamp must be strictly increasing (duplicate bars?)")
    df["price"] = pd.to_numeric(df["price"], errors="coerce")
    if not np.all(np.isfinite(df["price"].to_numpy()) & (df["price"].to_numpy() > 0)):
        raise ValueError("price must be finite and positive on every row")
    df["signal"] = pd.to_numeric(df["signal"], errors="coerce")
    if "cost_bps" in df.columns:
        df["cost_bps"] = pd.to_numeric(df["cost_bps"], errors="coerce")
        if df["cost_bps"].isna().any() or (df["cost_bps"] < 0).any():
            raise ValueError("cost_bps must be present and non-negative on every row when the column exists")
    return df


def _bar_spacing_days(ts: np.ndarray) -> float:
    d = np.diff(ts.astype("datetime64[ns]").astype("int64")) / 1e9 / 86400.0
    return float(np.median(d)) if d.size else 1.0


# --------------------------------------------------------------------------------------------------
# the two nulls (block-aware so overlapping horizons do not fake significance)
# --------------------------------------------------------------------------------------------------
def _block_index(n: int, block: int, rng: np.random.Generator) -> np.ndarray:
    nb = int(np.ceil(n / block))
    order = rng.permutation(nb)
    return np.concatenate([np.arange(b * block, min((b + 1) * block, n)) for b in order])


def _block_signs(n: int, block: int, rng: np.random.Generator) -> np.ndarray:
    nb = int(np.ceil(n / block))
    s = rng.choice(np.array([-1.0, 1.0]), size=nb)
    return np.repeat(s, block)[:n]


def positions_from_signal(signal: np.ndarray) -> np.ndarray:
    """+1 above the median signal, -1 below, 0 at it: the simplest sign rule, no fitted parameters."""
    return np.sign(signal - np.median(signal))


def _perm_p(null: np.ndarray, observed: float) -> float:
    """One-sided add-one permutation p: P(null >= observed)."""
    return float((1.0 + np.sum(null >= observed)) / (null.size + 1.0))


def random_entry_null(positions, fwd_ret, horizon: int, n_draws: int, rng) -> dict:
    pos = np.asarray(positions, float)
    f = np.asarray(fwd_ret, float)
    observed = float(np.mean(pos * f))
    null = np.empty(n_draws)
    for i in range(n_draws):
        null[i] = np.mean(_block_signs(f.size, horizon, rng) * f)
    return {"observed": observed, "p": _perm_p(null, observed), "null_mean": float(null.mean()),
            "null_sd": float(null.std(ddof=1))}


def shuffled_label_null(signal, fwd_ret, horizon: int, n_draws: int, rng) -> dict:
    """Permute the signal in horizon-sized blocks and recompute the rank-IC (ranks computed once:
    permuting the signal permutes its ranks)."""
    rs = average_rank(np.asarray(signal, float))
    rf = average_rank(np.asarray(fwd_ret, float))
    rs = (rs - rs.mean()) / rs.std()
    rf = (rf - rf.mean()) / rf.std()
    observed = float(np.mean(rs * rf))
    null = np.empty(n_draws)
    for i in range(n_draws):
        null[i] = np.mean(rs[_block_index(rs.size, horizon, rng)] * rf)
    return {"observed": observed, "p": _perm_p(null, observed), "null_sd": float(null.std(ddof=1))}


# --------------------------------------------------------------------------------------------------
# the run
# --------------------------------------------------------------------------------------------------
def run(source, *, horizon: int = 1, n_draws: int = 2000, alpha: float = 0.05, ic_ceiling: float = 0.05,
        n_variants_tried: int = 1, cost_bps: float | None = None, seed: int = 0,
        collapse_share: float = 0.70, out=None) -> dict:
    """
    Run the seven checks on a CSV path or a prepared DataFrame. Returns a dict with
    `verdict` ("PASS" or "FAIL-<REASON>"), `failed_check` (int or None), `checks` (list of rows) and
    the key numbers. `out` is a print-like callable for the one-line verdicts (None = silent).
    """
    say = out if out is not None else (lambda *_: None)
    if int(horizon) < 1:
        raise ValueError("--horizon must be >= 1")
    if int(n_variants_tried) < 1:
        raise ValueError("--n-variants-tried must be >= 1")
    if int(n_draws) < 100:
        raise ValueError("--n-draws must be >= 100")
    df = load_csv(source) if isinstance(source, (str, os.PathLike)) else source
    rng = np.random.default_rng(seed)
    checks: list[dict] = []
    result: dict = {"verdict": None, "failed_check": None, "checks": checks, "n_rows": int(len(df))}

    def record(n, status, detail):
        checks.append({"check": n, "name": CHECK_NAMES[n], "status": status, "detail": detail})
        say(f" {n}  {CHECK_NAMES[n]:<36s} {status:<8s} {detail}")

    def fail(n, reason):
        result["verdict"] = f"FAIL-{reason}"
        result["failed_check"] = n

    ts = df["timestamp"].to_numpy(dtype="datetime64[ns]")
    aa = df["available_at"].to_numpy(dtype="datetime64[ns]")
    sig = df["signal"].to_numpy(dtype=float)
    px = df["price"].to_numpy(dtype=float)
    spacing = _bar_spacing_days(ts)
    gap_guard = max(7.0, 3.0 * spacing)

    # 1 -- point-in-time stamps
    r1 = audit_macro_pit(ts, aa)
    if r1["n_violations"]:
        k = r1["first_violation"]
        record(1, "FAIL", f"{r1['n_violations']} of {r1['n']} rows have available_at on/before their bar "
                          f"(first: row {k}, {pd.Timestamp(ts[k])} vs available_at {pd.Timestamp(aa[k])})")
        fail(1, "PIT")
        _skip(record, 2)
        return _finish(result, say)
    record(1, "PASS", f"{r1['n']} rows; every available_at is strictly after its bar")

    # 2 -- causal pairing
    try:
        pairs = pair_signal_forward_return(ts, sig, aa, ts, px, horizon, log_return=True,
                                           max_entry_gap_days=gap_guard)
    except ValueError as e:
        record(2, "FAIL", f"pairer raised: {e}")
        fail(2, "PAIRING")
        _skip(record, 3)
        return _finish(result, say)
    r2 = audit_pairs_pit(pairs["available_at"], pairs["entry_time"], pairs["signal"], pairs["fwd_ret"])
    if r2["n_lookahead"] or r2["n_nan"]:
        record(2, "FAIL", f"{r2['n_lookahead']} look-ahead pair(s), {r2['n_nan']} NaN pair(s)")
        fail(2, "PAIRING")
        _skip(record, 3)
        return _finish(result, say)
    n_pairs = pairs["n_pairs"]
    if n_pairs < MIN_PAIRS:
        record(2, "FAIL", f"only {n_pairs} usable pairs (< {MIN_PAIRS}); the gate will not read a result this thin")
        fail(2, "TOO-FEW-PAIRS")
        _skip(record, 3)
        return _finish(result, say)
    dropped = int(np.isfinite(sig).sum()) - n_pairs
    record(2, "PASS", f"{n_pairs} pairs, horizon {horizon} bar(s); every entry strictly after available_at"
                      + (f"; {dropped} signal row(s) dropped (no full forward window / stale)" if dropped else ""))
    s, f = pairs["signal"], pairs["fwd_ret"]
    result["n_pairs"] = int(n_pairs)

    # 3 -- rank-IC + power
    ic = rank_ic(s, f)
    pg = power_gate(s, f, ceiling=ic_ceiling)
    result.update({"ic": ic["ic"], "ic_ci": (ic["ci_low"], ic["ci_high"]), "n_eff": pg["n_eff"],
                   "mde_ic": pg["mde_ic"]})
    detail3 = (f"rank-IC {ic['ic']:+.4f} [95% CI {ic['ci_low']:+.4f}, {ic['ci_high']:+.4f}]; "
               f"N_eff {pg['n_eff']:.0f} of {pg['T']}; min detectable IC {pg['mde_ic']:.4f} (ceiling {ic_ceiling})")
    if pg["gated_out"]:
        record(3, "FAIL", detail3 + " -- underpowered: cannot test")
        if result["failed_check"] is None:
            fail(3, "UNDERPOWERED")
    elif not (ic["ic"] > 0.0):
        record(3, "FAIL", detail3 + " -- rank-IC is not positive (negate your signal if you mean the reverse)")
        if result["failed_check"] is None:
            fail(3, "NO-EDGE")
    else:
        record(3, "PASS", detail3)

    # 4 -- the two nulls
    pos = positions_from_signal(s)
    re_null = random_entry_null(pos, f, horizon, n_draws, rng)
    sl_null = shuffled_label_null(s, f, horizon, n_draws, rng)
    result.update({"p_random_entry": re_null["p"], "p_shuffled_label": sl_null["p"],
                   "gross_per_trade": re_null["observed"]})
    detail4 = (f"random-entry p {re_null['p']:.4f} (mean per-trade {re_null['observed'] * 1e4:+.2f} bps vs null sd "
               f"{re_null['null_sd'] * 1e4:.2f}); shuffled-label p {sl_null['p']:.4f}; {n_draws} draws each")
    if re_null["p"] > alpha or sl_null["p"] > alpha:
        record(4, "FAIL", detail4 + f" -- not distinguishable from luck at alpha {alpha}")
        if result["failed_check"] is None:
            fail(4, "LUCK")
    else:
        record(4, "PASS", detail4)

    # 5 -- lag sensitivity
    lag = lag_sensitivity(ts, sig, aa, ts, px, horizon, lags=(1, 2), collapse_share=collapse_share,
                          max_entry_gap_days=gap_guard)
    result["lag"] = lag
    kept = ", ".join(f"lag {r['lag']}: IC {r['ic']:+.4f} ({r['retained_share'] * 100:+.0f}% kept)" for r in lag["lags"])
    if lag["verdict"] == "LOOKAHEAD-SUSPECT":
        record(5, "FAIL", f"{kept} -- one bar of delay removes >{collapse_share * 100:.0f}% of the effect: "
                          f"check your timestamps")
        if result["failed_check"] is None:
            fail(5, "LOOKAHEAD-SUSPECT")
    elif lag["verdict"] == "NO-EFFECT":
        record(5, "PASS", f"{kept} -- no lag-0 effect to test (CI covers zero)")
    else:
        record(5, "PASS", f"{kept} -- degrades gracefully")

    # 6 -- cost hurdle
    if cost_bps is not None:
        cost = np.full(f.size, float(cost_bps))
        cost_src = f"--cost-bps {float(cost_bps):g}"
    elif "cost_bps" in df.columns:
        cost = df.set_index("timestamp")["cost_bps"].reindex(pd.DatetimeIndex(pairs["entry_time"])).to_numpy(float)
        cost_src = f"cost_bps column (median {np.median(cost):g})"
    else:
        cost = None
        cost_src = "none supplied"
    if cost is None:
        record(6, "FAIL", "no cost supplied (add a cost_bps column or pass --cost-bps); a strategy without a "
                          "cost model has not been tested")
        if result["failed_check"] is None:
            fail(6, "COST-UNSPECIFIED")
    else:
        gross = pos * f
        net = gross - 2.0 * cost / 1e4 * np.abs(pos)
        g_bps, n_bps = float(gross.mean()) * 1e4, float(net.mean()) * 1e4
        result.update({"gross_bps_per_trade": g_bps, "net_bps_per_trade": n_bps})
        detail6 = (f"mean per-trade gross {g_bps:+.2f} bps, net {n_bps:+.2f} bps after a round trip at "
                   f"{cost_src}; {int(np.count_nonzero(pos))} trades")
        if n_bps <= 0.0:
            record(6, "FAIL", detail6 + " -- costs eat the edge")
            if result["failed_check"] is None:
                fail(6, "COST")
        else:
            record(6, "PASS", detail6)

    # 7 -- multiple testing
    p_head = sl_null["p"]
    p_holm = float(holm_adjusted_pvalues(np.r_[p_head, np.ones(int(n_variants_tried) - 1)])[0])
    result.update({"p_headline": p_head, "p_holm": p_holm, "n_variants_tried": int(n_variants_tried)})
    detail7 = (f"headline p {p_head:.4f} (shuffled-label) over {n_variants_tried} variant(s) tried -> "
               f"Holm-adjusted {p_holm:.4f}")
    if p_holm > alpha:
        record(7, "FAIL", detail7 + f" -- not significant at alpha {alpha} once the search is counted")
        if result["failed_check"] is None:
            fail(7, "MULTIPLE-TESTING")
    else:
        record(7, "PASS", detail7)

    if result["failed_check"] is None:
        result["verdict"] = "PASS"
    return _finish(result, say)


def _skip(record, start: int) -> None:
    for n in range(start, 8):
        record(n, "SKIPPED", "fix the timing failure above first")


def _finish(result: dict, say) -> dict:
    v = result["verdict"]
    if v == "PASS":
        say("VERDICT: PASS")
    else:
        k = result["failed_check"]
        say(f"VERDICT: {v} (check {k}: {CHECK_NAMES[k]})")
    return result


# --------------------------------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------------------------------
def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("csv", help="path to the CSV (timestamp, signal, available_at, price[, cost_bps])")
    ap.add_argument("--horizon", type=int, default=1, help="forward-return horizon in bars (default 1)")
    ap.add_argument("--n-draws", type=int, default=2000, help="draws per null (default 2000)")
    ap.add_argument("--alpha", type=float, default=0.05, help="significance level (default 0.05)")
    ap.add_argument("--ic-ceiling", type=float, default=0.05,
                    help="largest acceptable minimum-detectable IC before the test is called underpowered (default 0.05)")
    ap.add_argument("--n-variants-tried", type=int, default=1,
                    help="how many signal variants you tried before this one; Holm-corrects the headline p (default 1)")
    ap.add_argument("--cost-bps", type=float, default=None,
                    help="one-way cost in bps of notional (overrides a cost_bps column)")
    ap.add_argument("--collapse-share", type=float, default=0.70,
                    help="share of the effect that one bar of delay may remove before it is look-ahead-suspect (default 0.70)")
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)
    try:
        print(f"validate_your_strategy: {a.csv}")
        print(f" #  {'check':<36s} {'result':<8s} detail")
        res = run(a.csv, horizon=a.horizon, n_draws=a.n_draws, alpha=a.alpha, ic_ceiling=a.ic_ceiling,
                  n_variants_tried=a.n_variants_tried, cost_bps=a.cost_bps, seed=a.seed,
                  collapse_share=a.collapse_share, out=print)
    except (ValueError, FileNotFoundError, pd.errors.ParserError) as e:
        print(f"ERROR: {e}")
        return 2
    return 0 if res["verdict"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
