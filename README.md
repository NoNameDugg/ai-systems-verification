# AI Systems Verification

**I build the systems that prove whether things actually work.**

This repository is a working portfolio of tested, production-grade components and the verification
discipline around them. They were extracted from a complete, multi-component software platform that I
designed, built, and instrumented solo — by directing AI coding agents — and then, once the evidence
showed it did not do what it needed to do, deliberately shut down.

Every component builds and tests from a clean checkout on a Linux CI runner where the original
repository and its licensed data do not exist; every test count is emitted by that CI, not typed into
this file; and nothing here contains a credential or a byte of licensed data.

---

## What this project demonstrates

- **Architecting and shipping a real multi-component system, solo, by directing AI coding agents.**
  Four independently tested components — two in Rust, two in Python — plus a CI pipeline, a leak-scanning
  gate, and the documentation, all built through multi-agent workflows with an adversarial review step.
  I design the architecture, direct the build, and verify the result; AI coding agents write much of
  the code under that direction.
- **A self-proving verification harness — proof-of-concept-grade rigor.** The centerpiece evaluates
  statistical and machine-learning models, and it must first prove *itself*: recover an answer
  deliberately planted in synthetic data and reject one deliberately leaked, before it is trusted on
  anything real.
- **CI and cross-component integration.** A sibling-free Linux pipeline builds and tests every component
  from scratch on each push, and the two Rust components integrate through an optional, separately
  tested feature seam.
- **Honest documentation for non-technical readers.** The research record, the architecture document,
  and two short essays explain what was built, what was tested, and what the evidence said — including
  the decision to shut the system down.
- **Translating a complex domain into plain language.** The platform came from quantitative trading, a
  domain full of jargon. The method — test hard, report honestly, act on a negative result — transfers to
  any system whose value has to be demonstrated before it is trusted.

---

## Start here

If you read three things, read these — they are the argument this repository makes:

- **[`METHODOLOGY.md`](METHODOLOGY.md)** — how I decide whether a system works: three-role adversarial
  review, test-driven implementation, "green tests are not a working system," and falsification over
  confirmation.
- **[`RESEARCH_RECORD.md`](RESEARCH_RECORD.md)** — how I test and report honestly: the map of ~26
  hypotheses tested, almost all of which returned null, with every number read back from source.
- **[`ARCHITECTURE.md`](ARCHITECTURE.md)** — the live system these components came from, how the pieces
  fit together, and why I switched it off.

Two short essays make it concrete: **[the kill-decision](essays/the-kill-decision.md)** (building was
the easy part; shutting it down on the evidence was the point) and **[what independent review actually
catches](essays/what-independent-review-catches.md)** (four cases where an adversarial second pass
changed the answer).

---

## The components

Four runnable components, each self-contained (clone → install → test), each demonstrating a distinct
capability. **Test counts are produced by [the CI workflow](.github/workflows/ci.yml) on every push** —
clone it and run them yourself.

### `gauntlet/` — a falsification gauntlet for statistical and ML models *(the centerpiece)*
A validation harness whose job is to **kill** a candidate result, not bless it: it tests for look-ahead
leakage, overfitting, and statistical fragility using permutation nulls, purged cross-validation,
factor residualization, and a statistical-power gate. Crucially it is **self-proving** — it runs on
synthetic fixtures with a known answer and must **recover a deliberately planted edge** and **reject a
deliberately leaked one** before it is trusted on real data. A harness that can't catch a bug you
planted on purpose can't be trusted to catch one you didn't. Three Python packages, each runnable on
synthetic data alone with no licensed dataset: `forkb` (the core falsification battery),
`rank_ic_gate` (signal-quality, point-in-time validity, and power gating), and `ml_xsect` (a non-linear
machine-learning model with leakage-safe cross-validation). This is the AI/ML part of the repository:
the tooling that decides whether a model's result is real.
**Demonstrates: model evaluation, statistical rigor, leakage detection.**

### `blackbox/` — a deterministic flight-recorder
A lock-free Rust journaling engine with microsecond timestamps and SHA-256-verified replay, so any live
session can be reconstructed exactly. This is data infrastructure, not AI — and it is what makes the
rest verifiable: you cannot verify a system you cannot replay.
**Demonstrates: Rust systems programming, lock-free concurrency, deterministic replay.**

### `flash/` — an async real-time data adapter
A zero-allocation async Rust engine that ingests and normalizes a live streaming feed — a broker's
(OANDA) Level-2 price data — with a PyO3 Python binding. It is the low-latency front door of the data
path. It optionally taps into the `blackbox/` flight-recorder (the `blackbox` feature) for
deterministic replay — two independently tested components in this repo integrating through a clean
seam.
**Demonstrates: async Rust, high-throughput stream processing, FFI.**

### `correlation-gate/` — a fail-closed safety gate
A thread-safe semaphore that stops concurrent processes from stacking correlated exposure. It fails
*closed*: if it cannot prove an action is safe, it rejects it. Extensively tested (unit, integration,
concurrency, chaos) with 99% line coverage measured by `pytest --cov=src`; the wall-clock perf benchmarks are opt-in (`--runperf`).
**Demonstrates: concurrency, defensive systems design, rule-based safety controls.**

---

## Also built (described, not shipped here)

To keep this repository to a core that is bulletproof end-to-end, a number of other genuine
components are described but not included — density of quality over volume. Among them: a clean
src-layout broker **adapter** with a real CLI; an options-exposure (**gamma/GEX**) engine with a FastAPI
surface; a **portfolio** construction module; a **research** harness; a **sentiment**/news pipeline; a
tick→OHLCV **candle** builder; a trade **ledger**; a genetic-algorithm **evolver**; and a **carry**
signal library. They are real, test-first-built, and tested — but a portfolio is only as strong as its
weakest exhibit, so the shipped set is deliberately small.

---

## What this is and isn't (the honest scope)

- **The platform's central question was answered "no," by test.** It was built to find out whether a
  durable, deployable edge existed in its market at small capital. It did not, as of mid-2026, and four
  independent audits confirmed that conclusion. This is not a profitable trading system, and it is not
  presented as one. (See `RESEARCH_RECORD.md`.)
- **Nothing here runs against real money.** The live system was decommissioned; its broker credential
  was revoked.
- **No secrets, no licensed data.** This is a fresh-history repository (the original carried
  credentials and licensed data and is not published). Two independent scanners — `gitleaks` plus a
  custom licensed-string / host-path scanner (`scripts/scan_leaks.sh`) — gate every push.
- **Built with AI coding agents, verified by me.** The code was produced largely by AI agents working
  under my direction, with an independent review pass on every change. I say so plainly because the
  point of this repository is how the work was verified, not who typed it.
- **Honest framing throughout.** Every claim here is meant to match the code; if you find one that
  doesn't, that's a bug and I want to know.

---

## Running it

```bash
# Python components (per package):
cd gauntlet/forkb && pip install -r requirements.txt && python -m pytest
cd correlation-gate && pip install -r requirements.txt && python -m pytest

# Rust components:
cd blackbox && cargo test
cd flash    && cargo test
```

The [CI workflow](.github/workflows/ci.yml) runs these same commands (flash with `--tests --lib`, i.e.
without doctests) on a clean Linux runner, across Python 3.12 and 3.14 — so the numbers it reports are
the numbers you'll get.
