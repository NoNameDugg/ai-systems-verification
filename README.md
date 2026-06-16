# Quant Verification Framework

**I build the systems that prove whether things actually work.**

This repository is a working portfolio centered on *verification* — the discipline of testing a
claim hard enough to trust the answer, especially when the answer is "no." It was extracted from a
complete, real-money automated trading system I designed, built, instrumented, and — after proving it
had no durable edge — deliberately shut down.

The shipped pieces are the reusable, bulletproof core of that system plus the honest record of the
research behind it. Every component builds and tests from a clean checkout on a Linux CI runner where
the licensed data and the original repository do not exist; every test count is emitted by that CI,
not typed into this file; and nothing here contains a credential or a byte of licensed data.

---

## Start here

If you read three things, read these — they are the argument this repository makes:

- **[`METHODOLOGY.md`](METHODOLOGY.md)** — how I verify systems: three-role adversarial review,
  test-driven implementation, "green tests are not a working system," and falsification over
  confirmation. This is the most on-point document for an evaluation / verification role.
- **[`RESEARCH_RECORD.md`](RESEARCH_RECORD.md)** — the honest map of ~26 hypotheses tested for a
  tradable edge, almost all of which returned null, with every number read back from source.
- **[`ARCHITECTURE.md`](ARCHITECTURE.md)** — the live execution system these components came from,
  and why I switched it off.

Two short essays make it concrete: **[the kill-decision](essays/the-kill-decision.md)** (building was
the easy part; shutting it down on the evidence was the point) and **[what independent review actually
catches](essays/what-independent-review-catches.md)** (four cases where an adversarial second pass
changed the answer).

---

## The components

Four runnable components, each self-contained (clone → install → test), each signalling a distinct
skill. **Test counts are produced by [the CI workflow](.github/workflows/ci.yml) on every push** —
clone it and run them yourself.

### `gauntlet/` — a falsification gauntlet *(the centerpiece)*
A research-validation harness whose job is to **kill** a candidate signal, not bless it: it tests for
look-ahead bias, overfitting, and statistical fragility, with permutation nulls, combinatorial
purged cross-validation, factor-residualization, and a power gate. Crucially it is **self-proving** —
it runs on synthetic fixtures with a known answer and must **recover a deliberately planted edge** and
**reject a deliberately leaked one** before it is trusted on real data. A harness that can't catch a
bug you planted on purpose can't be trusted to catch one you didn't. *(Python; three packages —
`forkb`, `rank_ic_gate`, `ml_xsect` — runnable on synthetic data alone, no licensed dataset.)*
**Skill: research methodology, statistical rigor, leakage detection — i.e. evaluation work.**

### `blackbox/` — a deterministic flight-recorder
A lock-free Rust journaling engine with microsecond timestamps and SHA-256-verified replay, so any
live session can be reconstructed exactly. You cannot verify a system you cannot replay.
**Skill: Rust systems programming, lock-free concurrency, deterministic replay.**

### `flash/` — an async market-data adapter
A zero-allocation async Rust order-book / market-data engine that ingests and normalizes a broker's
(OANDA) Level-2 feed — the low-latency front door of the execution path, with a PyO3 Python binding.
It optionally taps into the `blackbox/` flight-recorder (the `blackbox` feature) for deterministic
replay — two independently-tested components in this repo integrating through a clean seam.
**Skill: async Rust, high-throughput stream processing, FFI.**

### `correlation-gate/` — a fail-closed risk gate
A thread-safe risk semaphore that stops concurrent strategies from stacking correlated exposure. It
fails *closed*: if it cannot prove a trade is safe, it rejects it. 455 tests, ~99% coverage.
**Skill: concurrency, defensive systems design, risk controls.**

---

## Also built (described, not shipped here)

To keep this repository to a core that is bulletproof end-to-end, a number of other genuine
components are described but not included — density of quality over volume. Among them: a clean
src-layout broker **adapter** with a real CLI; a **gamma/GEX** options-exposure engine with a FastAPI
surface; a **portfolio** construction module; a **research** harness; a **sentiment**/news pipeline; a
tick→OHLCV **candle** builder; a trade **ledger**; a genetic-algorithm **evolver**; and a **carry**
signal library. They are real, TDI-built, and tested — but a portfolio is only as strong as its
weakest exhibit, so the shipped set is deliberately small.

---

## What this is and isn't (the honest scope)

- The research frontier was **closed by test**: no edge deployable at small capital, as of mid-2026,
  confirmed by four independent audits. This is not a profitable trading system, and it is not
  presented as one. (See `RESEARCH_RECORD.md`.)
- **Nothing here runs against real money.** The live system was decommissioned; its broker credential
  was revoked.
- **No secrets, no licensed data.** This is a fresh-history repository (the original carried
  credentials and licensed data and is not published). Two independent scanners — `gitleaks` plus a
  custom licensed-string / host-path scanner (`scripts/scan_leaks.sh`) — gate every push.
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

The [CI workflow](.github/workflows/ci.yml) runs exactly this — on a clean Linux runner, across
Python 3.12 and 3.14 — so the numbers it reports are the numbers you'll get.
