"""Pytest configuration for correlation-gate.

The performance and benchmark suites assert wall-clock budgets, which are
inherently machine- and load-dependent: they can fail on a slow CI runner or a
busy laptop even when the code is correct. They are therefore skipped by default
so the suite is green on any machine, and run explicitly with ``--runperf``::

    pytest --runperf

The functional unit / integration / concurrency / chaos suites always run.
"""

import pytest


def pytest_addoption(parser):
    parser.addoption(
        "--runperf",
        action="store_true",
        default=False,
        help="run the environment-dependent wall-clock performance/benchmark tests",
    )


def pytest_collection_modifyitems(config, items):
    if config.getoption("--runperf"):
        return
    skip_perf = pytest.mark.skip(
        reason="environment-dependent wall-clock perf test; run with --runperf"
    )
    for item in items:
        path = str(item.fspath).replace("\\", "/")
        if (
            "/tests/performance/" in path
            or "/tests/benchmarks/" in path
            or "TestPerformance" in item.nodeid
        ):
            item.add_marker(skip_perf)
