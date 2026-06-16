# Architecture — A Live Execution System, and Why I Shut It Down

The components in this repository were not built as demos. They were extracted from a complete,
real-money automated trading system that ran live against a broker for an extended period — fully
instrumented to measure its own performance. The single most important thing this architecture
demonstrates is the ending: **I built the system, instrumented it to prove whether it had a
tradable edge, found that it did not, and decommissioned it.** Building the thing was the easy part.
Proving the truth about it, and acting on that truth, was the point.

This document is the map: how the system was wired, where the shipped components sit in it, and what
the whole effort demonstrates.

---

## 1. What it was

A solo-built, end-to-end automated execution system for foreign-exchange markets:

- It ingested live market data from a broker API, computed features and signals, passed candidate
  trades through a stack of independent risk gates, and placed real orders on a small live account.
- It was **fully instrumented** — every decision, latency, and fill was journaled for later
  reconstruction, because the entire purpose was to measure whether the strategy actually worked
  rather than to assume it did.
- It ran on a continuous-integration pipeline with a backtester regression suite, so changes to the
  decision logic were checked against a fixed baseline before they could reach the live path.

It was real money, real execution, real consequences — which is exactly why the verification
discipline mattered.

---

## 2. The data path

```
  Broker market-data API
          │
          ▼
  ┌───────────────────┐     low-latency ingest + normalization
  │  Market-data       │ ◄── [ order-book / quote adapter ]
  │  adapter           │
  └───────────────────┘
          │  normalized quotes / book state
          ▼
  ┌───────────────────┐     feature engineering → signal generation
  │  Signal pipeline   │
  └───────────────────┘
          │  candidate trades
          ▼
  ┌───────────────────┐     independent, fail-closed risk gates:
  │  Risk-gate stack   │ ◄── [ correlation / exposure gate ]
  │                    │     carry-bias gate, entry timing, per-pair barriers
  └───────────────────┘
          │  approved orders
          ▼
  ┌───────────────────┐     order creation, account-state caching
  │  Execution         │
  └───────────────────┘
          │  fills + every decision, latency, and reject
          ▼
  ┌───────────────────┐     deterministic journaling for replay
  │  Capture / journal │ ◄── [ flight-recorder ]
  └───────────────────┘
          │  captured history
          ▼
  ┌───────────────────┐     offline research + validation
  │  Falsification     │ ◄── [ the gauntlet ]
  │  apparatus         │
  └───────────────────┘
          │
          ▼
  CI regression suite gates every change to the decision logic
```

The arrows marked `[ ... ]` are the four components shipped in this repository. They are the
load-bearing, reusable pieces of that path — each one self-contained, tested, and extractable.

---

## 3. Where the shipped components sit

- **`flash/` — the market-data adapter.** The low-latency front door: an async Rust order-book /
  market-data engine that ingests and normalizes broker quotes with zero-allocation stream
  processing. This is the top of the data path.

- **`correlation-gate/` — a risk gate.** One of the fail-closed gates in the risk stack: a
  thread-safe semaphore that prevents concurrent strategies from stacking correlated exposure. It
  fails *closed* — if it cannot prove a trade is safe, it rejects it. (455 tests, ~99% coverage.)

- **`blackbox/` — the flight-recorder.** The capture layer: a deterministic, lock-free Rust
  journaling engine with microsecond timestamps and SHA-256-verified replay, so any live session can
  be reconstructed exactly. You cannot verify a system you cannot replay.

- **`gauntlet/` — the falsification apparatus.** The offline validation core: it takes a candidate
  signal and tries to *kill* it — testing for look-ahead bias, overfitting, and statistical fragility,
  and proving itself on synthetic fixtures (it recovers a planted edge and rejects a leaked one)
  before it is ever pointed at real data. This is the component that turned "I think this works" into
  "I have shown whether this works."

---

## 4. Built to find the truth, not to flatter it

The architecture's defining feature is that it was instrumented *against itself*. The system did not
just trade — it measured its own latency at each internal stage, recorded realized slippage against
intended fills, ran a shadow evaluator over its own decisions, and captured enough state to check
that what it learned offline matched what it did live (train/serve parity). Every one of those is a
mechanism for catching the system lying to itself.

That instrumentation is what made the final conclusion trustworthy. A less honest build would have
shown a backtest curve and stopped. This one was wired so that the live, real-money, after-costs
truth was measurable — and then it was measured.

---

## 5. The kill-decision

The verdict from all of that instrumentation, confirmed by an independent review pass, was that the
strategy had **no durable, deployable edge** after real costs. The realized performance was
consistent with no edge, and an exhaustive search across many independent hypotheses (documented in
`RESEARCH_RECORD.md`) failed to surface one.

So the system was **decommissioned**: the scheduled jobs were disabled and the live broker credential
was revoked. Nothing here is running against real money today.

This is the architecture's real lesson, and the reason it leads this document. The valuable skill on
display is not "can build a trading system" — plenty of people can. It is: **can build the
instrumentation to learn the truth about a system, and has the discipline to act on a negative
result instead of rationalizing it.** That is verification work, and it is what this repository is
evidence of.
