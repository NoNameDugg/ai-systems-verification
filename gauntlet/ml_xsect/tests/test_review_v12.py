"""External review (2026-10) reproductions — ML cross-section gauntlet.

  #4  keys in the frozen, sha256-hashed ``MLXConfig`` are never read by the code.
      The gate thresholds the config carries (``z_sum``, ``ic_power_ceiling``,
      ``p3_calmar_floor``, ``known_*_beta_threshold`` ...) are duplicated as
      hard-coded literal defaults in the leaf functions, so the freeze hash moves
      when a dead key changes and stays put when a live literal changes.

  #2  (the leakage fixture) is reproduced as a command + output in the sprint
      record and fixed by replacing the fixture in ``test_model.py``; see the
      tests named ``*_leak*`` there once the fix lands.

Committed FAILING (``xfail(strict=True)``) before any fix.
"""
import ast
import dataclasses
import pathlib

import pytest

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


@pytest.mark.xfail(strict=True, reason="review #4: 26 of 36 MLXConfig keys are hashed but never read")
def test_every_frozen_mlx_key_is_read_by_non_test_code():
    fields = {f.name for f in dataclasses.fields(MLXConfig)}
    unread = sorted(fields - consumed_config_attrs(PKG, "mlx_config.py"))
    assert unread == [], f"frozen+hashed but never read: {unread}"
