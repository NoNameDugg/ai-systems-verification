"""
Tests for the bring-your-own-strategy example.

The contract: the leaky demo CSV fails at check 1 or 2 (timing), the clean demo CSV passes all
seven checks, a clean CSV whose cost exceeds its edge fails at the cost hurdle, a CSV with clean
stamps but a leaky signal is caught by the lag-sensitivity check, and the shipped CSVs match their
generator. Run:  python -m pytest -q   (from this directory)
"""
import os
import subprocess
import sys

import numpy as np
import pandas as pd
import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import make_demo_data as demo  # noqa: E402
import validate_your_strategy as v  # noqa: E402

DATA = os.path.join(HERE, "data")
CLEAN = os.path.join(DATA, "clean_synthetic.csv")
LEAKY = os.path.join(DATA, "leaky_synthetic.csv")
SCRIPT = os.path.join(HERE, "validate_your_strategy.py")


def _statuses(res):
    return {row["check"]: row["status"] for row in res["checks"]}


# ---- the three contractual cases ------------------------------------------------
def test_leaky_csv_fails_at_check_1_or_2():
    res = v.run(LEAKY)
    assert res["verdict"].startswith("FAIL-")
    assert res["failed_check"] in (1, 2)
    st = _statuses(res)
    assert st[1] == "FAIL" or st[2] == "FAIL"
    assert all(st[k] == "SKIPPED" for k in range(res["failed_check"] + 1, 8))


def test_clean_csv_passes_every_check():
    res = v.run(CLEAN)
    assert res["verdict"] == "PASS"
    assert res["failed_check"] is None
    assert all(s == "PASS" for s in _statuses(res).values())
    assert res["ic"] > 0.0 and res["p_shuffled_label"] <= 0.05 and res["p_random_entry"] <= 0.05
    assert res["net_bps_per_trade"] > 0.0
    assert res["lag"]["verdict"] == "ROBUST"


def test_clean_csv_with_cost_above_the_edge_fails_at_the_cost_hurdle(tmp_path):
    df = pd.read_csv(CLEAN)
    df["cost_bps"] = 100.0                      # far above the planted ~20 bps gross per trade
    p = tmp_path / "expensive.csv"
    df.to_csv(p, index=False)
    res = v.run(str(p))
    assert res["verdict"] == "FAIL-COST"
    assert res["failed_check"] == 6
    st = _statuses(res)
    assert all(st[k] == "PASS" for k in (1, 2, 3, 4, 5))    # it is the cost, nothing else
    assert res["net_bps_per_trade"] <= 0.0 < res["gross_bps_per_trade"]


# ---- the other knobs --------------------------------------------------------------
def test_cost_flag_overrides_the_column():
    assert v.run(CLEAN, cost_bps=20.0)["verdict"] == "FAIL-COST"


def test_no_cost_at_all_fails_closed(tmp_path):
    df = pd.read_csv(CLEAN).drop(columns=["cost_bps"])
    p = tmp_path / "nocost.csv"
    df.to_csv(p, index=False)
    res = v.run(str(p))
    assert res["verdict"] == "FAIL-COST-UNSPECIFIED" and res["failed_check"] == 6


def test_many_variants_tried_fails_multiple_testing():
    res = v.run(CLEAN, n_variants_tried=200)
    assert res["verdict"] == "FAIL-MULTIPLE-TESTING" and res["failed_check"] == 7
    assert res["p_holm"] == pytest.approx(min(1.0, 200 * res["p_headline"]))


def test_tighter_ic_ceiling_is_underpowered_not_no_edge():
    res = v.run(CLEAN, ic_ceiling=0.03)
    assert res["verdict"] == "FAIL-UNDERPOWERED" and res["failed_check"] == 3


def test_clean_stamps_but_leaky_signal_is_caught_by_lag_sensitivity(tmp_path):
    """Valid-looking timestamps, but the signal is the forward return it will be scored against:
    checks 1-4 cannot see it (the IC is huge and 'significant'); check 5 can."""
    df = pd.read_csv(CLEAN, parse_dates=["timestamp", "available_at"])
    price = df["price"].to_numpy()
    fwd = np.full(len(df), np.nan)
    fwd[:-2] = np.log(price[2:] / price[1:-1])          # return of (entry bar t+1 -> t+2)
    df["signal"] = fwd + np.random.default_rng(0).normal(0.0, 0.002, size=len(df))
    p = tmp_path / "content_leak.csv"
    df.to_csv(p, index=False, date_format="%Y-%m-%dT%H:%M:%S")
    res = v.run(str(p))
    assert res["verdict"] == "FAIL-LOOKAHEAD-SUSPECT" and res["failed_check"] == 5
    st = _statuses(res)
    assert st[1] == st[2] == st[3] == st[4] == "PASS"
    assert res["ic"] > 0.8 and abs(res["lag"]["lags"][0]["retained_share"]) < 0.30


# ---- provenance of the shipped data -----------------------------------------------
def test_shipped_csvs_match_their_generator():
    for path, build in ((CLEAN, demo.build_clean), (LEAKY, demo.build_leaky)):
        disk = pd.read_csv(path, parse_dates=["timestamp", "available_at"])
        gen = build()
        assert len(disk) == len(gen) == demo.N_BARS
        assert (disk["timestamp"].to_numpy() == gen["timestamp"].to_numpy()).all()
        assert (disk["available_at"].to_numpy() == gen["available_at"].to_numpy()).all()
        np.testing.assert_allclose(disk["price"].to_numpy(), gen["price"].to_numpy(), atol=1e-6)
        np.testing.assert_allclose(disk["signal"].to_numpy(), gen["signal"].to_numpy(), atol=1e-6)
        np.testing.assert_allclose(disk["cost_bps"].to_numpy(), gen["cost_bps"].to_numpy(), atol=1e-6)


def test_leaky_demo_is_the_stamp_error_by_construction():
    df = pd.read_csv(LEAKY, parse_dates=["timestamp", "available_at"])
    assert (df["available_at"] == df["timestamp"]).all()


# ---- input validation + CLI -------------------------------------------------------
def test_missing_column_is_rejected(tmp_path):
    p = tmp_path / "bad.csv"
    pd.read_csv(CLEAN).drop(columns=["available_at"]).to_csv(p, index=False)
    with pytest.raises(ValueError, match="available_at"):
        v.run(str(p))


def test_bad_arguments_raise():
    with pytest.raises(ValueError):
        v.run(CLEAN, horizon=0)
    with pytest.raises(ValueError):
        v.run(CLEAN, n_variants_tried=0)


def test_run_is_deterministic_for_a_seed():
    a, b = v.run(CLEAN, n_draws=200, seed=3), v.run(CLEAN, n_draws=200, seed=3)
    assert a["p_shuffled_label"] == b["p_shuffled_label"] and a["p_random_entry"] == b["p_random_entry"]


@pytest.mark.parametrize("path, code, prefix", [
    (CLEAN, 0, "VERDICT: PASS"),
    (LEAKY, 1, "VERDICT: FAIL-PIT"),
])
def test_cli_exit_code_and_final_line(path, code, prefix):
    proc = subprocess.run([sys.executable, SCRIPT, path, "--n-draws", "200"],
                          capture_output=True, text=True, cwd=HERE)
    assert proc.returncode == code, proc.stdout + proc.stderr
    last = [line for line in proc.stdout.strip().splitlines() if line.strip()][-1]
    assert last.startswith(prefix), last


def test_cli_missing_file_is_a_usage_error():
    proc = subprocess.run([sys.executable, SCRIPT, os.path.join(HERE, "does_not_exist.csv")],
                          capture_output=True, text=True, cwd=HERE)
    assert proc.returncode == 2
