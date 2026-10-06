"""External review (2026-10) reproductions — ML cross-section gauntlet.

  #4  keys in the frozen, sha256-hashed ``MLXConfig`` are never read by the code.
      The gate thresholds the config carries (``z_sum``, ``ic_power_ceiling``,
      ``p3_calmar_floor``, ``known_*_beta_threshold`` ...) are duplicated as
      hard-coded literal defaults in the leaf functions, so the freeze hash moves
      when a dead key changes and stays put when a live literal changes.

  #2  (the leakage fixture) was reproduced as a command + output in the sprint
      record and is fixed in ``test_model.py::test_purge_removes_the_leak_it_is_for``.

Committed FAILING (``xfail(strict=True)``) in 1078a65; flipped by the fix.
"""
import ast
import dataclasses
import pathlib

import numpy as np
import pandas as pd
import pytest

import mlx_book as mb
import mlx_factors as mf
import mlx_ic_power as icp
from mlx_config import MLXConfig

PKG = pathlib.Path(__file__).resolve().parents[1]


def consumed_config_attrs(package_dir: pathlib.Path, schema_module: str) -> set:
    """Attribute names read off a config object anywhere in the package's non-test code."""
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


def test_every_frozen_mlx_key_is_read_by_non_test_code():
    fields = {f.name for f in dataclasses.fields(MLXConfig)}
    unread = sorted(fields - consumed_config_attrs(PKG, "mlx_config.py"))
    assert unread == [], f"frozen+hashed but never read: {unread}"


# =============================================================================
# the thresholds in the frozen config are the ones the leaves apply
# =============================================================================


def test_power_verdict_reads_its_bars_from_cfg():
    rng = np.random.default_rng(4)
    s = pd.Series(rng.normal(0.03, 0.06, size=200))          # mde ~0.012; mean ~0.03
    default = icp.power_verdict(s)
    assert default["powered"] is True and default["glimmer"] is True and default["strong"] is False
    strict = icp.power_verdict(s, cfg=MLXConfig(ic_power_ceiling=0.001, ic_glimmer_bar=0.5, ic_strong_bar=0.5))
    assert strict["powered"] is False and strict["glimmer"] is False
    lax = icp.power_verdict(s, cfg=MLXConfig(ic_strong_bar=0.01))
    assert lax["strong"] is True


def test_block_t_uses_cfg_critical_value():
    rng = np.random.default_rng(6)
    s = pd.Series(rng.normal(0.03, 0.05, size=200))
    narrow = icp.block_t(s, cfg=MLXConfig(block_t_crit=1.0))
    wide = icp.block_t(s, cfg=MLXConfig(block_t_crit=3.0))
    assert narrow["ci_lo"] > wide["ci_lo"]
    assert icp.block_t(s, cfg=MLXConfig(block_t_crit=1e9))["significant"] is False


def test_p3_killtest_reads_calmar_floor_from_cfg():
    idx = pd.date_range("2014-01-31", periods=48, freq="ME")
    r = pd.Series(np.r_[np.full(47, 0.01), -0.05], index=idx)   # ann ~0.11, maxDD 0.05 -> calmar ~2
    assert mb.p3_killtest(r)["survives_p3"] is True
    assert mb.p3_killtest(r, cfg=MLXConfig(p3_calmar_floor=100.0))["survives_p3"] is False


def test_known_factor_flag_reads_beta_thresholds_from_cfg():
    assert mf.known_factor_flag({"rmw": 0.3, "cma": 0.0}, 0.1) is False
    assert mf.known_factor_flag({"rmw": 0.3, "cma": 0.0}, 0.1, cfg=MLXConfig(known_rmw_beta_threshold=0.2)) is True


def test_char_space_verdict_reads_its_bars_from_cfg():
    rng = np.random.default_rng(8)
    inc = pd.Series(rng.normal(0.02, 0.03, size=120))
    assert mf.char_space_verdict(inc)["survives"] is True
    assert mf.char_space_verdict(inc, cfg=MLXConfig(charspace_ic_min=0.5))["survives"] is False
