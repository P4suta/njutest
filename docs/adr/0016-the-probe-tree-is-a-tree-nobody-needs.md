<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0016 — The probe tree is a tree nobody needs

## Status

Accepted, 2026-09-08.
Implemented by `instrument::observable`, the probe witness of `instrument::witness`, and the four `un…` entries of the runtime of [ADR 0011](0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md) (E12).

## Context

[ADR 0015](0015-the-guard-is-the-infection-probe.md) folded the infection question into the guard wherever the compiler had vouched that a condition is inert, and left the probe tree standing "for everything the syntax cannot call inert: a return replacement, an arithmetic edit, a call swapped for another."

Two thirds of that sentence were wrong.
The probe tree answers four rules and no others — `return-default`, `return-ok-default`, `return-some-default`,
`return-true` — and it has never had anything to say about an arithmetic edit or a swapped call.
What it actually covers is exactly the return replacements,
and only where the returned expression is one evaluating a second time is not an event.

For that it costs a `cargo check` loop of up to `max_rounds` rounds, a build of the whole workspace into a target directory of its own, and a run of every target.
Measured on this engine's own workspace on a cold cache: **168 seconds of the 575 a preparation takes**, and it discharged nothing at all.
The same preparation on a warm cache is 99 seconds, so what the probe tree cost was a cold-cache cost paid once per tree — but paid every time the tree changed enough to matter, which for a workspace under development is often.

Meanwhile a return replacement is the easiest infection question there is.
The mutation writes a *constant* — the default, `true`, `Ok(default)`,
`Some(default)` — so answering "could this test have seen the replacement" needs no second evaluation of anything.
It needs the value the program already computed, compared against a constant.
The instrumented tree has that value:
it is what the branch that keeps the original produces.

## Decision

The guard asks the question, and the probe tree goes.

The branch that keeps the original is wrapped in one call —
`undefaulted(i, (original))`, or `untrue`, `unokdefault`, `unsomedefault` —
which answers with the value it was given and records `i` when the value and the constant differ.
It is a call rather than a block for the reason the rest of Form E is an expression, and it takes the value by move so the program computes it exactly once.

What it may compare is a sealed `Observable` trait: the primitives, `str` and `String`, `Option` and `Vec` of one of those, and a reference to any of them.
Equality on those is the whole of what a program can tell apart, which is what the discharge rests on.
Floats are outside it — `-0.0 == 0.0` holds and `-0.0` is not what the default writes, so a probe there would call a mutation that changed the sign of a zero no change at all.
A type of your own is outside it,
because a `PartialEq` that answers about one field while a test reads another would say nothing happened while a test watched the difference.

Being in the trait is not enough: the value must also *have* a `Default`, and a reference usually does not.
A function returning `&str` whose body borrows a `String` field returns a `&String`, which the trait covers and `Default` does not, so the compiler refuses it however plainly it coerces at the return.
The trait says which equalities may be trusted; `Default` says what there is to compare against.

The type question goes to the **witness tree**, which [ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md) already builds once, in the shape the guard will hold:
`({ let v = <value>; w_default(&v); v })`.
So what the compiler vouched for is literally what gets written, and the instrumented tree cannot fail to build over a probe.
A value the compiler refuses costs the probe and never the mutant: `Placed::Probe` says so, and the mutation is measured by running it,
which is what a run does with everything it cannot prove.

Both generated modules — the witness tree's and the runtime's — render the trait from one constant.
A witness tree that vouched for more than the runtime implements is an instrumented tree that does not build, and that is a thing to make unwriteable rather than to remember.

## Consequences

- A default preparation makes **three builds instead of four** and runs every target **once instead of twice**. On this engine's own workspace that is 168 seconds of 575 on a cold cache; the same preparation warm is 99, of which 77 are the one run of every target that is the baseline and the measurement at once.
- `never-infected` now covers return replacements without a flag, and covers more of them than the probe tree did: the probe tree's `Observable` named the primitives only, and this one adds `str`, `String`, `Option` and `Vec`.
- The answer is per test rather than per target, because it rides on the record of ADR 0014 like every other guard does.
- `--probe`, `[mutation] probe`, `probe/`, `probe-tree-not-built` and `probe-log-unreadable` go.
  So does the probe log format, and the exit code a probe process used when it could not write one.
- A caller that reported on the probe reads `touched.narrowing.compared` for which mutants the tree can speak about and `TargetTouches::infected` for what each test infected.
  Both already existed; neither is new surface.
- The layer still says nothing about an arithmetic edit, a swapped call, or a deleted statement.
  It never did.

## Alternatives

- **Keep the probe tree for the types `Observable` refuses.** It refuses them for a reason that has nothing to do with which tree asks: a `PartialEq` that answers about less than a test can see is unsound in either.
- **Evaluate the replacement rather than comparing against a constant.** The four rules write constants; evaluating `Default::default()` for a type of the program's own is running the program's code, which is the thing the measurement may not do.
- **Ask the type question in the instrumented tree and drop what fails.** The instrumented tree's compilation decides which mutants exist ([ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md)),
  so a probe the compiler refused there would cost the mutant rather than the probe.
  The witness tree is where a question may be refused for free.
