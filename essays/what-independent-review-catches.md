# What Independent Review Actually Catches

A claim I make in this repository is that an independent, adversarial review pass — one whose only
job is to *find what's wrong*, with a standing instruction to verify at source and never
rubber-stamp — catches things the builder cannot. That's easy to assert. Here are four cases where it
actually changed the answer. None of these are hypothetical; all came out of the research program
behind this repo. I've kept the domain details generic on purpose — the lessons are what transfer.

---

## Receipt 1 — The "null" that was really a bug

An automated test concluded a hypothesis was dead: no detectable signal, shut it down. That's the
*comfortable* failure — a null lets you stop looking.

The review didn't accept it. Tracing the computation back to source, it found an **off-by-one
alignment error** in how the event window was constructed: the analysis was reading the bar *before*
the event instead of the event bar. The signal hadn't been absent — it had been mis-windowed into
noise. Correcting the alignment recovered a real, statistically significant effect the first pass had
buried.

And then the important part: the recovered effect was put through a fair out-of-sample test plus a
realistic cost model — and **killed anyway**, because by then it had decayed to nothing tradable.

Two lessons, both load-bearing: **a null can be a bug, so you verify your nulls with the same rigor
as your wins** — and **recovering a signal does not entitle you to keep it.** The fair test is the
fair test regardless of how much work it took to find the thing it's about to reject.

---

## Receipt 2 — The contaminated win

The opposite failure mode. A corporate-event strategy showed a clean, attractive edge. Wins are even
more dangerous than nulls, because you *want* to believe them.

Independent review found that the dataset hadn't correctly handled delisted names — companies that
had failed or been acquired were under-represented, so the sample was quietly tilted toward
survivors. The measured "edge" was substantially an artifact of that survivorship. Repairing the
contamination collapsed the result to null.

The lesson is a rule I now apply reflexively: **contamination almost always flatters you.**
Survivorship bias, look-ahead, and selection effects nearly all push the result in the *same*
direction — too good. So a result that looks great is not a reason to relax; it's the exact situation
that warrants the most aggressive attempt to break it.

---

## Receipt 3 — Green tests, dead binary

A component arrived with dozens of genuinely passing tests over real, correct library code. By the
usual heuristic — "it has tests and they pass" — it was done.

It was not done. The review opened the *deployed binary's* entrypoint and found a stub: a loop that
logged `"active and monitoring"` on a timer while initializing none of the components it claimed to
run. The real initialization was a `// TODO`. The test suite was real, but it exercised the library
the binary never actually called. The tests were green and the system did nothing.

This is the single most useful distinction I learned: **"the tests pass" and "the system works" are
different claims.** A passing suite proves the code it touches is correct. It says nothing about
whether the thing you ship *calls that code*. You only close that gap by checking the runtime — the
actual executed path of the actual deployed artifact — not the suite, and not the README.

---

## Receipt 4 — The deferred check that never ran

A result was recorded as "confirmed robust — pending a final cost gate." Reasonable enough: the
expensive confirmation was deferred to the end. But the work closed out *before* that gate ran, and
the "confirmed" label quietly stuck to a result that had never actually been confirmed.

A later independent pass caught the orphaned caveat and corrected the verdict to "inconclusive." The
lesson: **a verification you can skip is not a verification.** A deferred check that depends on a
future step which may never happen is a comfortable way to bank an unearned conclusion. Either the
check runs, or the claim is downgraded — there is no "confirmed, pending."

---

## Why this is the skill

Every one of these was invisible to the person who built the thing — not from incompetence, but
because the builder's mental model *is* the thing being tested, and you cannot independently audit
your own assumptions. Catching them needed a second pass that was structurally adversarial: separate,
skeptical by mandate, and required to cite source rather than agree.

That is verification work, and it is the discipline this whole repository is built to demonstrate.
