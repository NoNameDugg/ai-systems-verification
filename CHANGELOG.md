# Changelog

Versions are git tags; each has a GitHub Release. This file is written by hand and says what changed;
test counts and coverage figures are never typed here (CI emits them).

## v1.2.0 — 2026-10-05

An independent code review of v1.1.0 (a fresh AI session asked by a third party for a brutally honest
review; it built and ran everything) reported that **every component had a gap in its main promise that
the green tests did not catch**. This release answers it the only honest way: each finding was first
reproduced as a test committed *failing* (Python `xfail(strict=True)`, Rust `#[ignore]`), then fixed with
the smallest correct change, then the test was flipped to a plain passing test so a regression fails it.
No gate threshold was loosened. Twenty deliberate breaks (five per component) were then applied one at a
time to confirm the suites can say no; every one was caught. Hand-typed test counts have been removed
from the component READMEs: CI emits them.

**gauntlet**
- *The gauntlet had never reached its own DEPLOY verdict end-to-end.* The "recover planted edge" test
  asserted only `alpha_daily > 0`. Root cause: `p3._maxdd_recovery_months` reported a drawdown still open
  on the last day as *infinite* recovery, so any book one trading day off its high failed crash-survival.
  The open spell is now counted at its elapsed length (a lower bound) and flagged `recovery_censored`; it
  fails only if that bound already exceeds the 24-month bar. New test: a strong planted edge now passes
  every gate at its frozen default and comes out `DEPLOY`
  (`forkb/tests/test_review_v12.py::test_strong_planted_edge_reaches_deploy_end_to_end`); the result's
  `P3` block now carries the stressed metrics and per-floor booleans so a failing P3 names its cause.
  No published verdict in `RESEARCH_RECORD.md` rested on the recovery-months clause: every crash-survival
  failure recorded there cites the Calmar floor or the max-drawdown floor, which are unchanged.
- *The ML leakage test could not detect leakage.* Its fixture had i.i.d. per-date features and
  one-period labels, so nothing could leak and it passed identically with the purge disabled. Replaced
  by `ml_xsect/tests/test_model.py::test_purge_removes_the_leak_it_is_for`: overlapping label windows
  plus persistent features; the purged run is null and the same seeds with purge = embargo = 0 inflate
  the out-of-sample IC on five of six seeds (paired mean +0.05).
- *Holm existed but no gauntlet path called it.* `forkb/gauntlet.py` gains `run_gauntlet_family` /
  `apply_family_correction`: Holm step-down across the cells of a family, re-gating the permutation
  null, `full_series_pass` and the verdict on the corrected p. A 1-of-20 lucky cell (p = 0.03) now
  comes out `NULL`.
- *Frozen, sha256-hashed config keys that nothing read.* Fourteen `ForkBConfig` keys and fifteen
  `MLXConfig` keys were removed (the hashes changed); `eps_field` is now read by the SUE leaf; the ML
  gate thresholds (`z_sum`, `ic_power_ceiling`, `ic_glimmer_bar`, `ic_strong_bar`, `block_t_crit`,
  `min_xsection`, `n_quantiles`, `p3_calmar_floor`, `known_*_beta_threshold`, `charspace_ic_min`) are
  read by the leaves through a `cfg=` keyword instead of living as hard-coded literal defaults. Each
  package has a test that enumerates the config fields and fails if one is not consumed by code.

**correlation-gate**
- *Pending-approval off-by-one.* With at least one approval pending, the projection was rebuilt from
  open + pending positions and **dropped the signal being evaluated**, so a limit of 3 let a third
  correlated trade through with a `SOFT_WARNING`. The new trade is now counted as slot N+M+1. One
  existing race test had enshrined the off-by-one (10 approvals under a limit of 10) and was corrected.
- *The OANDA provider never sent the API token.* Every request now carries `Authorization: Bearer`.
- *Gold sized at a $1 placeholder.* A proposed XAU trade was projected at `entry_price = 1.0`, so its
  notional could never trip a size gate. A gold signal is now refused (fail closed) until a spot price is
  supplied through `CorrelationGate.update_spot_prices()`. The 1.0 placeholder remains for fiat pairs
  and is labelled an approximation.
- *Race tests that could not detect races.* All 28 shipped race tests passed with the gate's lock
  replaced by a no-op. A new test built on the existing `verify_no_over_exposure` oracle fails under that
  mutation ("Over-exposure: 20 allowed, threshold 2").
- README: the gate coordinates concurrent callers in one process, not processes; the environment
  variables and YAML it documented were never read by the code and are now described as such.

**blackbox**
- *Two checkpoint formats.* `JournalTap` wrote the bare 32-byte state hash; the verifier required the
  48-byte `sequence | timestamp | hash` layout, so no tap-written checkpoint could ever be verified. The
  tap now writes the 48-byte layout with a sequence counter.
- *`verify` reported PASS without parsing a checkpoint* (`checkpoints_matched = checkpoints_found`). It
  now parses every checkpoint, checks that sequence numbers strictly increase, never claims a match it
  did not make, and reports `Incomplete` (structure sound, state hashes not compared — that needs the
  user's hasher) or `Fail`; `--stop-on-mismatch` is honoured; exit codes 0 / 1 / 2.
- *A crashed journal read back as phantom records.* The pre-allocated zero tail of a journal with no
  footer parsed as an endless run of empty `Unknown` records (43,594 from a 1 MiB file with 5 real
  records). An all-zero record header now ends iteration.
- *"Zero-allocation" was never measured and the write path allocated once per record.* Payloads up to
  256 bytes are now copied into the ring-buffer slot (larger ones still take one heap buffer, and the
  docs say so), and `tests/zero_alloc_test.rs` measures it with a thread-local counting allocator.
- Docs: file replay through the replay engine is labelled a prototype; `USAGE.md` examples that called
  non-existent APIs are corrected.

**flash**
- *A silent connection waited forever.* The OANDA stream loop had no idle timeout; `read_timeout_ms`
  was read only by the unused WebSocket connector. The loop (now `network::oanda_stream`, extracted from
  the binary so it is testable) abandons a stream that delivers nothing for the idle timeout, floored at
  three heartbeat intervals.
- *Reconnect delay reset to zero.* The retry counter was zeroed on HTTP 200 and then used as the delay
  multiplier, so an accepted-then-dropped stream retried with 0 ms forever; connect failures backed off
  linearly under an "exponential backoff" comment. The policy now uses the crate's existing exponential
  `calculate_delay` and resets only when the stream delivers data.
- *Dockerfile env vars the code never read* (`OANDA_API_TOKEN`, `OANDA_ACCOUNT_ID`, `ASTRA_ENV`,
  `ASTRA_LOG_LEVEL`) renamed to the names the code reads; a test parses the Dockerfile and fails if a
  documented variable is read nowhere. Empty credential strings now mean "not configured" instead of
  sending `Authorization: Bearer ` to OANDA.
- *The Python binding could not decode what the binary publishes.* It decoded the Redis stream as the
  internal types; the wire carries the gateway shape. One shared decoder now maps the published JSON back
  to the internal snapshot, the binding tries it first and surfaces snapshots as events (it used to drop
  them), the wire format is pinned to a committed fixture, and CI builds the `python` feature to run the
  round-trip. Adjacent: the OANDA instrument was built with an empty quote, so the published symbol and
  Redis key read `EUR_USD_`; fixed.
- Build: pyo3's `extension-module` moved from the dependency to a `python-ext` cargo feature, which the
  maturin wheel build now enables (`pyproject.toml`); `--features python` builds a library that test binaries
  can link against libpython (the first CI run of the new step failed to link on Linux for this reason).
- README: the binary speaks HTTP chunked streaming, not WebSocket; the connector / heartbeat /
  reconnection-manager / adapter layers are labelled prototypes the binary does not use.

**repo**
- `ci.yml`: the "WDAC" comments were wrong (the crate builds and the doctests run on the author's
  machine; a stale build directory was the problem) and are corrected; flash's library is now clippy
  clean.

**Known open issues** (found while reproducing the above; not fixed in this release)
- `correlation-gate`: `src/security/atomic.py` (`AtomicGateGuard`, `StateVersionTracker`) is not used by the
  gate, which guards itself with a bare `RLock`. Consequence: the only pre-existing tests that genuinely detect
  a race exercise code the gate does not run; the serialisation guarantee rests on the new race test alone.
- `blackbox`: `JournalWriter::write` silently drops a record when the ring buffer is full (fail-open, returns
  `Ok`). Consequence: a journal can be missing records with no error, so a replay can diverge from the live
  session without the writer having said so.
- `blackbox`: `JournalWriter::flush` waits unbounded for the background thread to drain the ring buffer.
  Consequence: a misbehaving buffer hangs the caller instead of returning an error (seen when a mutation
  removed the capacity check during the v1.2 mutation exercise).
- `flash`: after `max_reconnect_attempts` the OANDA stream task exits while the metrics endpoint and the
  Docker health check keep reporting healthy. Consequence: a container can run indefinitely publishing nothing
  and look green to an orchestrator.

## v1.1.0 — 2026-10-05

- **Say plainly what "independent review" means here.** A separately-prompted AI session with no
  shared context, run by the author under a verify-at-source protocol — not an outside reviewer.
  Stated wherever the word is load-bearing: the README scope section, METHODOLOGY §1, both essays,
  ARCHITECTURE §5.
- **Bring-your-own-strategy entry point** — `gauntlet/examples/validate_your_strategy.py`. One CSV
  (`timestamp, signal, available_at, price[, cost_bps]`) in; seven one-line checks out — point-in-time
  stamps, causal pairing, rank-IC with a power read, random-entry and shuffled-label nulls, lag-one-bar
  sensitivity, a cost hurdle, and a Holm correction for the number of variants tried — ending in
  `VERDICT: PASS` or `VERDICT: FAIL-<reason>`. Two synthetic demo CSVs (a planted real edge that passes;
  a mis-stamped signal that fails at check 1), a README, and tests run as the `examples` entry of the
  CI gauntlet matrix.
- **Lag-one-bar sensitivity check** — `gauntlet/rank_ic_gate/lag_sensitivity.py`. Recomputes the
  rank-IC with every signal delayed one and two extra bars and flags `LOOKAHEAD-SUSPECT` when one bar of
  delay removes more than a configurable share (default 70%) of an effect; pure noise is flagged neither
  way. Tested on a planted look-ahead, a planted persistent edge, and noise.
- **Crypto-market research threads withdrawn from the public record (kept private).** The research
  record now lists 22 threads; the FX, cross-asset, equity, single-name and ETF threads are unchanged.
- **Status line** in the README: a versioned portfolio project, not a maintained library.
- **Plain-English docstrings** at the gauntlet entry points (`forkb/gauntlet.py`, `forkb/harness.py`,
  the `rank_ic_gate` test modules): internal decision-log and sprint references rewritten into
  plain language.

## v1.0.0 — 2026-09-22

- First public state: four components (`gauntlet`, `blackbox`, `flash`, `correlation-gate`) building
  and testing green from a clean checkout on a Linux CI runner, two leak gates, the research record,
  the methodology, the architecture document, and two essays.
