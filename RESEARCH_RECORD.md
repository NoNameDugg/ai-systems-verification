# Research Record — A Map of What Didn't Work

This is the honest output of a multi-year systematic-trading research program: a record of roughly
**two dozen distinct hypotheses tested for a tradable edge, almost all of which returned null.**

I lead with that because it is the point. The skill this repository demonstrates is not "found a
money-printer" — it is the discipline to test an idea rigorously, to attack my own promising results
until they break, and to **state a negative result plainly and act on it** rather than letting hope
launder a weak signal into a deployment. Every number below was read back from the artifact that
produced it; none was transcribed from memory.

---

## The conclusion, stated precisely

After ~26 threads, the frontier was closed. The exact scope of that conclusion matters, and I keep it
honest:

> **No edge deployable at small (sub-$1k) capital, as of mid-2026, by test.**

That is *not* the same as "no alpha exists anywhere." Several of the kills are over-determined by
**capital walls** — minimum-margin requirements, multi-name short legs — that bite before the
statistics even matter. What the record shows is that within the reachable universe (free and
owned-licensed data, retail execution, small capital), an exhaustive search did not surface a signal
that survived realistic costs and a crash-survival test.

The closure was **independently audited and confirmed four times** — including a dedicated
"false-null" audit, commissioned out of my own skepticism after ten-plus consecutive nulls, that went
looking for a shared bug that could have produced fake nulls. It found none: the kills were
reproduced at their frozen sources, with **zero false-nulls and zero verdict-flipping errors**, at
~0.93 confidence. A telling structural fact: the failures spread across **eight distinct kill-classes,
none accounting for more than ~30%** — which is itself evidence against a single methodological
artifact faking the nulls. The kill-classes:

1. **Cost** — break-even gross; spread/funding eats the edge.
2. **Breadth** — too few independent bets (low effective-N) to trust the mean.
3. **Crash-survival** — positive return, but drawdown too deep to hold (Calmar below floor).
4. **Decay** — real in-sample, gone out-of-sample / in recent years.
5. **Un-gateable frequency** — too few non-overlapping draws to gate.
6. **Non-orthogonality** — subsumed by known factors once neutralized.
7. **Look-ahead non-capturability** — the effect exists but can't be captured without future data.
8. **Capital wall** — economically real but unreachable at small size.

---

## The three findings worth knowing

**1. The best signal I ever found — and why I still killed it.**
A non-linear, multi-factor machine-learning cross-sectional ranker (on a licensed point-in-time
US-equity fundamentals dataset) produced the program's *first* genuinely real, leakage-clean,
orthogonal out-of-sample signal: cross-sectional **rank-IC +0.0479 (block-t 3.80)**, clean against a
permutation null. It was real. It still failed the deployability bar: even in the most generous
factor-neutral best case the tradable book's **Calmar was 0.460, under the 0.50 floor**, and an audit
showed the signal was *decaying live* — monthly IC fell from **+0.0566 pre-2024 (t 4.85) to +0.0086
in 2024–2026 (t 0.43)**. A real signal that you cannot deploy is still a null for deployment purposes.
Recovering it did not entitle me to keep it.

**2. The one real-but-uneconomic edge.**
Cross-asset trend-following (a dollar-neutral book across equity/bond/commodity instruments) was real
and significant: **gross random-sign excess +11.10%/yr at p=0.000**, positive in all three asset
classes. But it failed crash-survival (Calmar 0.22, max drawdown −41% unrecovered), and once turnover
costs were applied the **net excess fell to +3.10%/yr at p=0.135 — no longer significant.** Vol-targeting
and a wider universe were tried to rescue it; both failed (a broader universe actually *lowered* the
stressed Calmar, because the added instruments weren't independent — the effective breadth ceiling was
~3.8 bets).

**3. The live ground truth.**
The strategy that actually ran on a small real-money account (~$230) told the same story directly: a
realized **33% win rate, 72% of exits via the loss-cutting "smart exit," and 0.1% take-profit hits.**
The entries weren't catching alpha; the exit logic was merely cutting losses to break-even. The very
first backtest confirmed it: the base strategy's entry timing had **no detectable edge versus random**
(excess ≈ −0.006R; P(beats random) ≈ 0.69) and was break-even *gross* — the loss was structural cost
drag, not bad luck.

---

## The full null-map

Every distinct alpha thread, its verdict, and the single most telling verified number. (Data-source
vendors are described generically; internal codenames are dropped in favor of plain descriptions.)

| Thread | Hypothesis | Method | Verdict | Key number |
|---|---|---|---|---|
| Trend-pullback entries | The live FX strategy's entry timing beats random | Walk-forward replay of the live pipeline vs a randomized-entry null | NULL | excess −0.006R; P(beats random) 0.69 |
| Exit/barrier geometry | Better stop/exit geometry rescues the bleed | 48-cell sweep + selection-bias (CPCV/PBO) gate | NULL | break-even gross; loss = spread (~25–38% of the stop) |
| Information-driven bars | Activity-sampled bars surface the edge clock-time hides | Volume/tick bars vs a random-excess gate | NULL | no cell cleared the gate; best p=0.066 |
| FX carry | The carry premium is investable for retail FX | Decompose carry; crash + significance gates | INCONCLUSIVE | gross +1.09% − 5.42% broker markup = **net −4.77%/yr**; maxDD −63% |
| FX time-series momentum | FX-only trend-following has a directional edge | Random-sign gross-excess test, 2009–2026 | NULL | gross excess +0.0002 at **p=0.504** (coin-flip) |
| Cross-asset trend-following | Trend-following works across equities/bonds/commodities | 15-instrument dollar-neutral TSMOM; random-sign + PBO + crash | REAL BUT UNECONOMIC | gross **+11.10%/yr p=0.000**; Calmar 0.22; net +3.10%/yr p=0.135 |
| Drawdown-managed trend | Vol-targeting lifts it to deployable grade | Constant-vol governor + stressed Calmar | INSUFFICIENT | stressed Calmar **0.149 < 0.50** |
| Broader-universe trend | A wider universe smooths the Calmar | Broaden 15→25 instruments; effective-N | NULL (breadth) | broadening *lowered* Calmar to 0.089; ~3.84 independent bets |
| FX intraday session setups | Popular session/"killzone" setups have edge | ~8 candidates through the validated harness + economic floor | NULL | zero validated; ~11 stacked nulls; best PBO 0.286 |
| Crypto trend | Trend works on crypto where FX failed | 2→8-coin TSMOM; random-sign + PBO + crash | NULL | broadening regressed it: +6.62%→+0.41%/yr, p 0.092→0.481 |
| Crypto funding/basis carry | Perpetual-funding carry is a deployable base | Delta-neutral short-perp/long-spot; stressed crash gate | INCONCLUSIVE | net +6.67%/yr p=0.0000 but **stressed Calmar −1.00, maxDD −100%** |
| Cross-sectional crypto momentum | Relative-strength rotation across coins pays | Rank coins, long top / short bottom; effective-N + perm null | INCONCLUSIVE (thin) | effective-N **2.0 < 3.0**; perm p=0.458 |
| Token-unlock supply shock | Pre-unlock supply shocks are tradeable | Event-study CAR + placebo null + short-cost | NULL | perm p=0.0106 on −0.23% gross, but PBO 0.50, **net −0.39%** |
| Liquidation-cascade reversion | You can fade forced-liquidation cascades | Intraday event-study; block-perm + vol-matched co-gate | NULL (both directions) | beats timing-null but fails vol-matched (p=0.98); ~20× underwater on slippage |
| Macro slope → crypto | A macro slope predicts crypto returns | Powered, point-in-time, deflated rank-IC kill-test | CANNOT-TEST | min detectable IC **0.061 > the 0.05 ceiling** — under-powered |
| Signal-fusion ensemble | The 14-signal fusion is a "foundation of alpha" | On-disk audit of each component's IC + the correlation premise | REFUTED AS FRAMED | **0 of ~14 components had a measured positive IC** |
| Equity flow around macro events | Structural ETF flow around policy meetings is tradeable | Free-data event-study; perm + Holm + post-2015 holdout + crash | NULL (holdout decay) | full-sample net +5.91%/yr (t 2.13) but **holdout t 1.95 < 2.0** |
| Closed-end-fund discount reversion | The discount-to-NAV mean-reverts cross-sectionally | Dollar-neutral monthly L/S; perm null + economic floor + survivorship | REAL BUT UNECONOMIC | perm p=0.0002 but **net +2.60%/yr < 5% floor**; survivorship → +0.55%/yr |
| Buried smart-exit candidate | A previously look-ahead-flagged exit is a real edge | Root-fix the look-ahead; re-freeze; single clean run | NULL (no replication) | base strategy net **−0.48R/trade (t −5.25)** |
| Commodity term-structure | Roll-yield is harvestable and crisis-positive | Feasibility probe on free laddered-vs-front pairs | INCONCLUSIVE (breadth) | real + crisis-positive but only 3 reads → effective-N 2.46 |
| Funding-skew fade | The inverse-of-the-carry-trap fade pays | Phase-0 feasibility probe | NULL (feasibility) | weak + cost-doomed; t 0.70 |
| Post-earnings drift (single-name) | Drift is tradeable on paid single-name data | Survivorship-clean cross-sectional gauntlet on licensed fundamentals | NULL (uneconomic) | correct-direction but **\|t\| ≈ 1.4 < 2.0**; gross +2.62%/yr < cost wall |
| Buyback / net-share-issuance | The issuance premium is deployable | Full gauntlet, annual + non-overlapping quarterly | PROMISING, UN-GATEABLE | annual net +7.02%/yr (t 2.24, perm-p 0.032) but **~27 draws ≪ the 60-block floor**; quarterly null |
| Gross profitability | Profitability is the first powered single-name edge | Full gauntlet incl. factor-residual + factor-unwind crash gate | NULL (POWERED) | **fails the crash gate at every cost band incl. pure gross** (maxDD −37.6%) |
| Index reconstitution forced-flow | Index add/delete forced flow is tradeable | Look-ahead-free event-study + cluster-aware placebo | NULL | gross +0.324%/event, **clustered-t 1.26 (NS)**; placebo p 0.191 |
| ML multi-factor cross-section | A non-linear ML blend finds orthogonal alpha | Cross-sectional ML ranker; perm null + factor-residual + crash gate | NULL (on crash gate) | rank-IC **+0.0479 (t 3.80)** but factor-neutral best-case Calmar **0.460 < 0.50** |
| Spinoff post-spin drift | Post-spinoff drift is tradeable long | Long-only event-study, 550 events 1998–2026 | NULL (undeployable) | gross +12.6%/12mo (t 2.67) but **median −2.9%**; top-10 names = 78% of the signal |
| Non-traditional strategy family | Copy/sentiment/event/niche strategies have edge | 8-agent survey of 28 strategies + external corroboration | NULL | **26 of 28 killed** by lateness/selection/crowding |
| Portfolio ensemble | Blending the real-but-failing sleeves clears the bar | Inverse-vol blend of the three least-bad sleeves | NULL (confirms closure) | combined Calmar **0.19–0.33 < 0.50** |

*(Measurement and infrastructure work — corpus-faithfulness fixes, execution-cost and latency
attribution — is excluded above; those were not alpha hypotheses.)*

---

## What this record is evidence of

A research program is only as trustworthy as its willingness to find nothing. This one tested broadly,
gated every result against permutation nulls / out-of-sample holdouts / realistic costs / crash
survival, audited its own nulls for the possibility that *they* were the bug, and reported the negative
result without inflation. The apparatus that produced these verdicts — the falsification gauntlet — is
in this repository (`gauntlet/`), and it is self-proving: it recovers a planted edge and rejects a
leaked one before it is trusted on real data.

That is the deliverable. Not the edge that wasn't there — the machine that could tell.
