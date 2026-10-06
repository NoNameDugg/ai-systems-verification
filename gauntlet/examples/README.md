# Validate your own strategy in five minutes

The rest of this repository shows the gauntlet proving itself on fixtures with a known answer. This
directory is the entry point for pointing the same machinery at *your* signal without rewriting
anything: one CSV in, seven one-line verdicts out, and a final `VERDICT: PASS` or
`VERDICT: FAIL-<reason>` that names the check that killed it.

```bash
cd gauntlet/examples && pip install -r requirements.txt
python validate_your_strategy.py data/clean_synthetic.csv     # a planted real edge      -> VERDICT: PASS
python validate_your_strategy.py data/leaky_synthetic.csv     # a mis-stamped signal      -> VERDICT: FAIL-PIT
```

Only numpy and pandas are required; every check is imported from [`../rank_ic_gate`](../rank_ic_gate).

## Your CSV

One row per bar, these columns (any order, extra columns ignored):

| column | meaning |
|---|---|
| `timestamp` | the bar, ISO-8601. `price` is that bar's close. |
| `signal` | your signal's value for that bar. Convention: higher signal, higher expected forward return. Blank = no signal. |
| `available_at` | when you could *actually* have known that value. Must be strictly after `timestamp`. The forward return starts at the first bar strictly after this. |
| `price` | the bar's close; forward returns are log returns over `--horizon` bars from the entry bar. |
| `cost_bps` | optional: one-way cost in basis points of notional. A round trip is charged twice. `--cost-bps` overrides it. With neither, the cost hurdle fails closed. |

## What each check catches

| # | check | what it kills | module |
|---|---|---|---|
| 1 | point-in-time stamps | the stamp error: a signal computed from a bar's close but recorded as known *at* that bar. The leaky demo dies here. | `pit_validator` |
| 2 | causal pairing | any pair whose entry bar is not strictly after `available_at`; raises rather than scoring look-ahead. Also refuses to read fewer than 50 pairs. | `signal_return_pairer` |
| 3 | rank-IC + power | a signal with no positive rank correlation to its forward return, or a sample too small to detect an IC of 0.05 once the autocorrelation of overlapping returns is counted (minimum detectable IC above `--ic-ceiling` = underpowered, not "no edge"). | `rank_ic_calculator`, `power_gate` |
| 4 | two nulls | luck. Random-entry: your per-trade mean against 2,000 draws of random signs over the same horizon. Shuffled-label: your rank-IC against 2,000 block permutations of the signal. Either p above `--alpha` fails. | this script |
| 5 | lag sensitivity | leakage *inside* the signal's values, which no timestamp audit can see: the rank-IC is recomputed with every signal delayed one and two extra bars. A real signal degrades gracefully; one that secretly contains the forward return collapses to nothing at lag 1 and is flagged `LOOKAHEAD-SUSPECT`. A real edge fully exhausted within one bar is indistinguishable from look-ahead by this test, so SUSPECT means "check your timestamps", not "guilty". | `lag_sensitivity` |
| 6 | cost hurdle | an edge smaller than its own round-trip cost: mean per-trade return net of `2 x cost_bps`. | this script |
| 7 | multiple testing | the variant you kept out of the N you tried: `--n-variants-tried N` Holm-corrects the headline (shuffled-label) p. At N = 1 this is vacuous by construction. | `deflation` |

Checks 1 and 2 are gates: if either fails, the rest are skipped, because a statistic computed on
mis-timed data is not evidence of anything. From check 3 on, every check runs and the verdict names
the *first* failure.

## The two demo files

Both are 4,000 synthetic daily bars, regenerated deterministically by `make_demo_data.py`; nothing
in them is market data.

- **`clean_synthetic.csv`** -- a planted real edge: a slow-moving signal (AR(1), persistence 0.9)
  known 17 hours after its bar; the return of the bar after the entry bar carries 0.3% per unit of
  signal on top of 1% noise; 5 bps one-way cost. It passes all seven checks.
- **`leaky_synthetic.csv`** -- the stamp error: the signal is the bar's own standardised return,
  computed from that bar's close and stamped `available_at` = the bar's timestamp. No planted edge.
  It fails check 1 and nothing else is computed.

Three things to try on the clean file, each of which should fail exactly where it says:

```bash
python validate_your_strategy.py data/clean_synthetic.csv --cost-bps 20            # VERDICT: FAIL-COST
python validate_your_strategy.py data/clean_synthetic.csv --n-variants-tried 200    # VERDICT: FAIL-MULTIPLE-TESTING
python validate_your_strategy.py data/clean_synthetic.csv --ic-ceiling 0.03        # VERDICT: FAIL-UNDERPOWERED
```

## Reading the output

Exit code 0 is PASS, 1 is any FAIL, 2 is a malformed input. The per-check lines carry the numbers
(rank-IC with its 95% interval, effective sample size, minimum detectable IC, both null p-values,
the retained share at each lag, gross and net bps per trade, the Holm-adjusted p) so the verdict is
checkable, not just stated.

A PASS here means: correctly timed, not luck at the 5% level, not look-ahead-suspect, net of your
cost, and still significant once your search is counted. It does **not** mean deployable -- there is
no drawdown, capacity, or out-of-sample test in this script. For those, see the `forkb` battery.

## Tests

`python -m pytest` in this directory runs `test_examples.py`: the leaky file must fail at check 1 or
2, the clean file must pass, a clean file with cost above the edge must fail at the cost hurdle, a
file with clean stamps but a leaky signal must be caught by check 5, and the shipped CSVs must match
their generator. CI runs this as the `examples` entry of the gauntlet matrix.
