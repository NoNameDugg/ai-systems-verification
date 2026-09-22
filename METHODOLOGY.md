# Methodology — How I Verify That Systems Actually Work

This repository is organized around a single conviction: **the hard part of building a system
is not making it run — it is proving whether it does what you claim.** Most of the engineering
here exists to answer that question honestly, including when the honest answer is "no."

What follows is the methodology I developed running a multi-year solo research-and-execution
program. It has four pillars: a **three-role review protocol**, **test-driven implementation**,
a discipline I call **"green tests are not a working system,"** and **falsification over
confirmation**. The last one is why the centerpiece of this repo is a gauntlet that tries to
*kill* signals rather than bless them.

---

## 1. Three independent roles: build, attack, decide

Every non-trivial change moved through three separated roles. Critically, these are *adversarial*
to each other by design — not a rubber stamp.

- **Builder** drafts the plan and writes the code. Produces a charter: scope, the exact files
  touched, the acceptance gate, and a rollback path.
- **Reviewer** is an independent pass whose only job is to *find what breaks* — what would leak a
  secret, fail on a clean checkout, or read as overclaim to a skeptical expert. The reviewer's
  standing instruction is **verify at source, never rubber-stamp**: every "this is fine" must cite
  the file and line that makes it fine; every "I can't verify this" is stated as such.
- **Arbiter** (the decision-maker) rules on the reviewer's findings and authorizes execution.

The execution rule is **halt → route → ratify → execute**, never *execute-then-flag*. When the
builder hits something the charter did not anticipate, work *stops* and routes back for a decision
before any irreversible step. In practice, multiple charter-gap catches per change is a healthy
signal, not a failure — it means the review is doing its job before the cost is paid, not after.

This is the same shape as good evals work: the system that produces an answer cannot also be the
system that certifies it.

---

## 2. Test-Driven Implementation (TDI)

No production code is written before its tests exist. The contract is defined by the tests; the
implementation merely satisfies it. The standard for a component is:

- **Tests first, always.** If implementation code exists before the tests, the component is
  restarted. Tests define the contract; code fulfills it.
- **Four test classes, not one.** Unit (logic in isolation), integration (real collaborators, real
  data flow), performance (explicit budgets, measured — not asserted), and **failure** (missing
  data, corruption, timeouts, resource exhaustion). A suite that only tests the happy path is not a
  suite.
- **Coverage is verified, not estimated** — measured by tooling, with any genuinely untestable line
  explicitly justified, not silently skipped.
- **Zero regression.** New code is held against a captured baseline; existing behavior must not
  move outside a declared tolerance.

The components shipped here carry their real suites: the Rust flight-recorder and order-book
adapter, the Python risk gate (456 tests), and the falsification gauntlet all build and test from a
clean checkout. **Test counts in this repo are emitted by CI, never typed into a README** — a
hand-written badge is exactly the kind of unverified claim this methodology exists to forbid.

---

## 3. "Green tests are not a working system"

The most important lesson of the whole program is the gap between *the tests pass* and *the thing
works*. They are different claims, and conflating them is how impressive-looking projects turn out
to be hollow.

A suite can be entirely real — hundreds of genuinely passing tests exercising genuinely correct
library code — while the deployed entrypoint that is supposed to *use* that library is a stub: a
loop that logs "active" while doing nothing. The tests are green. The system does nothing. Only by
checking the **runtime path** — what the binary actually calls, what the live process actually does
— does the gap surface.

So the discipline is: **verify against the live runtime, not the config or the README.** Faithfulness
claims are checked against logs and the actual executed code path, never against the text that
describes them. When this repo says a component works, it means the extracted component — only its
own files, a clean dependency install, nothing else on the machine — builds and tests green. That
"clean-clone self-containment" check is load-bearing: a component that secretly imports a sibling,
the parent tree, or a licensed data loader passes in place and fails the moment someone clones it.

---

## 4. Falsification over confirmation

The research half of the program tested many hypotheses for a tradable edge. Almost all of them
returned **null** — no edge. That is not a failure of the method; it *is* the method. The job of a
verification framework is to make it as hard as possible to fool yourself, and the strongest form
of that is to attack your own conclusions before anyone else can.

- **Understand before you test.** Never test what you can't explain mechanistically; never trust an
  edge with no causal story. A pattern that "just works" with no reason is a curve-fit until proven
  otherwise.
- **Steel-man the case against.** For every promising result, the strongest argument *against* it is
  constructed and honestly evaluated — look-ahead bias, survivorship, p-hacking, overfitting, and
  multiple-testing inflation are assumed present until ruled out.
- **The gauntlet proves the apparatus, not just the signal.** The falsification gauntlet in this
  repo runs on synthetic fixtures with a *known* answer: it must **recover a deliberately planted
  edge** and **reject a deliberately leaked one.** A test harness that can't catch a bug you planted
  on purpose cannot be trusted to catch one you didn't. Self-proving fixtures come before any real
  data.
- **A null is a result, and you state it plainly.** The hardest and most valuable discipline was
  building a real system, proving it had no edge, and shutting it down — rather than letting hope
  launder a weak signal into a deployment.

---

## 5. Verification primitives I hold to

Small rules, learned the expensive way, that prevent whole classes of error:

- **Numbers come from source, never from memory or stdout.** Any metric, hash, or count that lands
  in a verdict, a record, or a README is read back from the artifact that produced it. Transcription
  is how false numbers enter the record.
- **Verify the claim at the exact write site.** "This binding is in scope" / "this string is absent"
  is confirmed at the precise line, in the precise scope — not inferred from a nearby read.
- **Absent-claims are verified at source.** Automated audits over-report things as missing; "it's not
  there" is the claim most likely to be wrong, so it gets checked directly.
- **No deferred gate that can silently never run.** A "we'll check this at the final gate" caveat
  evaporates if the gate is skipped; either the check runs or the conclusion is downgraded to
  "unconfirmed."
- **No silent truncation.** If coverage is bounded — top-N, sampled, no-retry — that bound is logged.
  Silent truncation reads as "covered everything" when it didn't.

---

## Why this is the portfolio

I build the part of a system that most people skip: the apparatus that decides whether the rest of
it is real. That skill — adversarial verification, test-first construction, honest null-reporting,
and provable self-containment — is the skill that matters wherever a system's value has to be
demonstrated before it is trusted. The components and the research record in this repository are
the evidence that I actually work this way, not just describe it.
