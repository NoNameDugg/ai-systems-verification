# Changelog

Versions are git tags; each has a GitHub Release. This file is written by hand and says what changed;
test counts and coverage figures are never typed here (CI emits them).

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
