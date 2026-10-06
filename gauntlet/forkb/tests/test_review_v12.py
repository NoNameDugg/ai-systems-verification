"""External review (2026-10) reproductions — Fork-B gauntlet.

Each test reproduces one finding from an independent code review of v1.1.0. They
were committed FAILING (``xfail(strict=True)``) in 1078a65 before any fix, then
flipped to plain tests by the fix commit. A later regression fails them again.

  #1  the full gauntlet never reaches a DEPLOY verdict end-to-end on its own data.
      Root cause traced to ``p3._maxdd_recovery_months``: a right-censored
      (still-underwater-on-the-last-day) drawdown is reported as *infinite*
      recovery, so any book one day off its high fails crash-survival.
  #3  Holm correction exists (``deflation.py``) but no gauntlet path calls it.
  #4  keys in the frozen, sha256-hashed config are never read by the code.
"""
import ast
import dataclasses
import pathlib

import numpy as np
import pandas as pd
import pytest

from _schema import ForkBConfig
from deflation import holm_adjusted_pvalues
from gauntlet import _verdict_terminal, apply_family_correction, run_gauntlet
from p3 import p3_battery
from synth_fixtures import make_snapshot

CFG = ForkBConfig()
PKG = pathlib.Path(__file__).resolve().parents[1]


# =============================================================================
# #1 — crash-survival: one trailing underwater day is not an infinite recovery
# =============================================================================


def test_one_day_underwater_at_end_is_not_infinite_recovery():
    rng = np.random.default_rng(7)
    idx = pd.bdate_range("2015-01-02", periods=600)  # clear of the pinned stress windows
    benign = pd.Series(0.0010 + rng.normal(0, 0.0006, len(idx)), index=idx)
    short = pd.Series(0.0005 + rng.normal(0, 0.0006, len(idx)), index=idx)

    # the same benign book, but its LAST day is a -0.5% dip: still underwater at the end
    dipped = benign.copy()
    dipped.iloc[-1] = -0.005

    out = p3_battery(dipped, short, CFG)
    assert out["recovery_censored"] is True        # honest: the final spell is unresolved ...
    assert out["recovery_months"] < 1.0            # ... but it is ONE DAY old, not infinite
    assert out["recovery_ok"] is True
    assert out["passed"] is True


def test_strong_planted_edge_reaches_deploy_end_to_end():
    """A planted edge that genuinely clears every gate must come out DEPLOY.

    Nothing here loosens a threshold. ``single_name_weight_cap`` / ``min_leg_names``
    are book-construction pins (how the long-short book is built), the same kind
    the licensed runs use; every gate is evaluated at its frozen default.
    """
    snap, _ = make_snapshot(seed=1, n_names=400, start="2003-01-02", end="2020-12-31",
                            edge_frac=0.8, alpha_ann=0.50)
    cfg = dataclasses.replace(ForkBConfig(), single_name_weight_cap=0.05, min_leg_names=15)
    res = run_gauntlet(snap, cfg, weighting="VW", perm_n=50)

    assert res["canary"]["halt"] is False
    assert res["M2"]["pass"] is True, res["M2"]
    assert res["PSR"]["passed"] is True, res["PSR"]
    assert res["perm_null"]["pass"] is True, res["perm_null"]
    assert res["OOS"]["confirms"] is True, res["OOS"]
    assert res["P3"]["pass"] is True, res["P3"]
    assert res["verdict"] == "DEPLOY", res["verdict"]


# =============================================================================
# #3 — Holm must be applied where cells are compared
# =============================================================================


def test_family_wise_perm_p_is_holm_adjusted():
    from gauntlet import run_gauntlet_family

    snap, _ = make_snapshot(seed=1, n_names=60, start="2003-01-02", end="2020-12-31",
                            edge_frac=0.8, alpha_ann=0.20)
    cells = [{"weighting": "VW"}, {"weighting": "EW"}]
    outs = run_gauntlet_family(snap, CFG, cells, perm_n=50)

    raw = [o["perm_null"]["p"] for o in outs]
    expected = holm_adjusted_pvalues(raw)
    assert len(outs) == 2
    for o, e in zip(outs, expected):
        assert o["perm_null"]["p_holm"] == pytest.approx(e)
        assert o["perm_null"]["family_size"] == 2
        assert o["perm_null"]["pass"] is bool(e <= CFG.perm_p_threshold)


def _passing_cell(p_raw: float) -> dict:
    """A result dict in the shape run_gauntlet returns, passing every gate except (possibly) perm-null."""
    return {"book": "pead", "cost_mode": "CALIBRATED", "underpowered": False,
            "M2": {"pass": True, "econ_sig": True, "power": True},
            "PSR": {"passed": True}, "P3": {"pass": True},
            "OOS": {"confirms": True, "indeterminate": False},
            "interpretation": {"label": "NOVEL"},
            "perm_null": {"p": p_raw, "pass": p_raw <= 0.05},
            "full_series_pass": True, "verdict": "DEPLOY"}


def test_one_lucky_cell_in_twenty_is_rejected_after_holm():
    """20 variants tried, one came out p = 0.03: alone it would DEPLOY; in its family it does not."""
    cells = [_passing_cell(0.03)] + [_passing_cell(0.40 + 0.02 * i) for i in range(19)]
    assert cells[0]["verdict"] == "DEPLOY"                       # the uncorrected story

    outs = apply_family_correction(cells, alpha=0.05)

    lucky = outs[0]["perm_null"]
    assert lucky["family_size"] == 20
    assert lucky["p_raw"] == 0.03
    assert lucky["p_holm"] == pytest.approx(min(1.0, 20 * 0.03))  # smallest p × m under Holm
    assert lucky["pass"] is False
    assert outs[0]["full_series_pass"] is False
    assert outs[0]["verdict"] == "NULL"
    assert all(o["verdict"] == "NULL" for o in outs)


def test_single_cell_family_is_unchanged_by_holm():
    out = apply_family_correction([_passing_cell(0.03)], alpha=0.05)[0]
    assert out["perm_null"]["p_holm"] == 0.03 and out["perm_null"]["pass"] is True
    assert out["verdict"] == "DEPLOY"


def test_a_gauntlet_that_always_says_null_fails_the_deploy_test():
    """Mutation guard: the verdict terminal must be able to say yes."""
    assert _verdict_terminal(True, True, True, False, "NOVEL", "pead", "CALIBRATED") == "DEPLOY"
    assert _verdict_terminal(False, True, True, False, "NOVEL", "pead", "CALIBRATED") == "NULL"


# =============================================================================
# #4 — every frozen, hashed key must be consumed by non-test code
# =============================================================================


def consumed_config_attrs(package_dir: pathlib.Path, schema_module: str) -> set:
    """Attribute names read off a config object anywhere in the package's non-test code.

    Counts ``cfg.<name>`` / ``config.<name>`` reads, ``getattr(cfg, "<name>")``, and
    ``self.<name>`` reads inside the schema module itself (its own methods).
    """
    seen = set()
    for path in sorted(package_dir.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        own = path.name == schema_module
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
                if node.value.id in ("cfg", "config") or (own and node.value.id == "self"):
                    seen.add(node.attr)
            elif (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                  and node.func.id == "getattr" and len(node.args) >= 2
                  and isinstance(node.args[1], ast.Constant) and isinstance(node.args[1].value, str)):
                seen.add(node.args[1].value)
    return seen


def test_every_frozen_forkb_key_is_read_by_non_test_code():
    fields = {f.name for f in dataclasses.fields(ForkBConfig)}
    unread = sorted(fields - consumed_config_attrs(PKG, "_schema.py"))
    assert unread == [], f"frozen+hashed but never read: {unread}"
