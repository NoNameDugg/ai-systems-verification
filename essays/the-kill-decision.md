# The Kill-Decision

The most valuable thing I did in this entire program was turn it off.

I built a complete, end-to-end automated trading system — market-data ingestion, signal generation,
a stack of fail-closed risk gates, live order execution, deterministic journaling, and a CI pipeline
that regression-tested the decision logic on every change. It ran on a real broker account with real
money. By the usual measure of a portfolio project — "look at this thing I built, and it runs" — it
was a success.

It was also, in the only way that ultimately matters, a failure: it had no edge. And the work I'm
actually proud of is that I proved that, believed the proof, and shut it down.

---

## What the instrumentation said

I didn't build the system to trade. I built it to *find out whether it could*. So it was wired to
measure its own truth, and the truth it measured was unambiguous. On the small live account (~$230),
the realized numbers were:

- **33% win rate.**
- **72% of exits taken by the loss-cutting "smart exit."**
- **0.1% of trades reaching their take-profit target.**

Read together, those say one thing: the *entries weren't catching anything*. The exit logic was doing
all the work, and all it could do was cut losses back toward break-even. There was no alpha at the
front of the pipeline for the rest of the machine to harvest.

The first proper backtest, which replayed the live signal pipeline against a randomized-entry null,
confirmed it at the source: the strategy's entry timing had **no detectable edge versus random**
(excess ≈ −0.006R; it beat random only ~31% of the time) and was break-even *gross* — meaning even
before costs there was nothing there, and after costs it bled the spread. The system worked
flawlessly. It just had nothing to do.

---

## Why I didn't just keep looking

The honest temptation, at that point, is to keep digging — surely *some* configuration, *some*
market, *some* signal works. So I did keep digging, for a long time, across roughly two dozen distinct
hypotheses: FX and cross-asset trend-following, several flavors of carry, crypto momentum and funding,
event-driven equity strategies, and a machine-learning cross-sectional ranker on licensed fundamental
data. The full record is in `RESEARCH_RECORD.md`. Almost all of them returned null. The two that came
closest — a cross-asset trend book (real gross edge of +11%/yr, but null after costs) and the ML
ranker (a genuinely real, leakage-clean signal) — both failed a crash-survival test, with drawdowns
too deep to hold.

The danger in a search that long is that you stop trusting your own nulls. So I commissioned an
adversarial audit of my *own* negative results — its explicit job was to find a shared bug that could
be manufacturing fake nulls. It didn't find one. The kills reproduced at their frozen sources, spread
across eight unrelated failure modes, with zero false-nulls. The closure was confirmed four
independent times at ~0.93 confidence.

And I kept the conclusion precise, because precision is the difference between honesty and despair:
**no edge deployable at small capital, as of mid-2026, by test.** Not "markets are efficient and alpha
is a myth." Some of the kills were capital walls, not statistical ones. But within what I could
actually reach and actually trade, an exhaustive, audited search found nothing that survived.

---

## The shutdown

So I decommissioned it. The scheduled jobs were disabled and the live broker credential was revoked.
Nothing is running against real money. The system that took the most work to build is the one I
deliberately switched off.

I think this is the part of the story that's worth hiring for. Anyone motivated enough can build a
trading system. The rarer thing — and the thing that an evaluation, verification, or research role
actually runs on — is the willingness to build the apparatus that can tell you you're wrong, to look
at what it says, and to act on a negative result instead of rationalizing your way past it. A system
that can't be killed by its own evidence isn't being tested; it's being believed. I tested mine, and
it didn't survive, and that's the result I'm reporting.
