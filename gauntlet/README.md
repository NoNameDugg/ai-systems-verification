# gauntlet — a falsification gauntlet for statistical and ML models

Three self-contained Python packages whose job is to **kill** a candidate result, not bless it.
Each one runs on synthetic fixtures with a known answer and has to pass its own self-proof first:
recover an effect that was deliberately planted, and reject one that was deliberately leaked or is
pure noise. None of them needs a licensed dataset or anything outside its own directory; CI runs
all three on a clean Linux runner under Python 3.12 and 3.14.

## The packages

**`forkb/` — the core falsification battery.** Takes a frozen snapshot of single-name fundamentals
and prices (synthetic here), builds a long-short book, and runs it through an ordered chain of
gates: a total-return canary that halts on a wrong rebuild, transaction costs, factor
residualization with the alpha retained, a block-level significance test on non-overlapping return
blocks, a probabilistic-Sharpe gate, a formation-label permutation null, an effective-breadth
screen, crash survival, and an out-of-sample power check, ending in a terminal verdict table
(NULL, PROMISING-…, DEPLOY). Its synthetic fixture generator produces a snapshot whose
data-generating process is known, plus the truth the tests assert against.

**`rank_ic_gate/` — signal quality, point-in-time validity, and power.** A powered rank-IC
micro-test: Spearman rank-IC with Fisher-z error bands, a corrected variance-inflation power gate
(minimum detectable IC from an honest effective sample size), a point-in-time validator that
refuses to run on look-ahead-contaminated pairs, a PIT-invariant signal/forward-return pairer, and
Holm family-wise deflation across cells.

**`ml_xsect/` — a non-linear model under leakage-safe cross-validation.** A shallow, regularized
gradient-boosted ranker over a monthly cross-sectional panel, scored under purged and embargoed
combinatorial cross-validation (CPCV) on rebalance indices; construction-matched factor returns
with an alpha-retained residual; fractional differencing selected on the training partition only;
an IC time-series power module; and the same net-turnover / crash-survival kill-test as the book
in `forkb`.

## Quick start

```bash
cd gauntlet/forkb        && pip install -r requirements.txt && python -m pytest   # ~11–15 min
cd gauntlet/rank_ic_gate && pip install -r requirements.txt && python -m pytest
cd gauntlet/ml_xsect     && pip install -r requirements.txt && python -m pytest
```

Test counts are emitted by the CI `gauntlet` jobs (`.github/workflows/ci.yml`), not typed here.

## The self-proof tests, by name

These are the argument. A harness that cannot catch a bug you planted on purpose cannot be
trusted to catch one you didn't.

Recover a planted edge, and stay silent when there is none (`forkb`):
- `tests/test_integration.py::test_injected_edge_recovered_as_positive_alpha`
- `tests/test_integration.py::test_no_edge_no_spurious_edge_and_no_false_deploy`
- `tests/test_permnull.py::test_correlated_labels_real_edge_passes`
- `tests/test_permnull.py::test_noise_labels_not_significant` (parametrized over four seeds)

Catch a deliberately wrong rebuild (`forkb`):
- `tests/test_tr_canary.py::test_div_forgotten_rebuild_caught_on_exdates`

Reject a deliberately leaked, look-ahead pair (`rank_ic_gate`):
- `test_pit_validator.py::test_pairs_pit_flags_lookahead`
- `test_signal_return_pairer.py::test_assert_no_lookahead_raises_on_violation`

Recover a non-linear signal, and return null on pure noise without leaking (`ml_xsect`):
- `tests/test_model.py::test_cpcv_recovers_nonlinear_signal`
- `tests/test_model.py::test_cpcv_pure_noise_is_null_no_leakage`

Run any one of them with `python -m pytest <path>::<name>` from the package directory.

## Provenance note

Comments cite an internal decision log as `DEC-nnn`, sprint codenames such as `BC`, `BD`, `AV`,
and an independent-review role as `S2`; they are left in place as provenance.
