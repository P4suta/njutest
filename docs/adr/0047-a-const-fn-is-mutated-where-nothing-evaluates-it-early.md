<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0047 — A const fn is mutated where nothing evaluates it early

## Status

Accepted, 2026-09-29.
Implemented by the walk, which records with each candidate the `const fn` whose body holds it (`syntax::SiteHint::const_fn`); by the instrumenter, which writes a `const fn` without its `const` where it holds a guard or carries one (`instrument::Deconst`, `instrument::Constant`); and by validation, which reads the compiler's `E0015` about such a function (`validate::Constness`, `validate::Condemnation`).
`fixtures/fixture-const-fn` states the fate of each use, and `a_chain_the_compiler_evaluates_gives_back_one_const_a_round_and_a_caller_only_the_program_calls_carries_the_guard` in `crates/rust-mutants/tests/toolchain_validate.rs` holds the rounds to a real compiler.
Amends [ADR 0008](0008-compiler-validated-acceptance-and-the-type-witness-pass.md), whose validation now also decides where a guard may live, and keeps [ADR 0027](0027-an-item-is-entered-where-its-body-starts.md) as it is: a `const fn` is still an item nothing records entering.

## Context

A guard is a call the program makes while it runs, and a `const fn` is a function the compiler may evaluate before it runs.
So the engine passed over every expression in the body of a `const fn`, as `const-fn-body`.

That was a decision about every `const fn` made for the ones something evaluates early, and those are few.
Clippy's `missing_const_for_fn` asks for `const` wherever the body allows it, so a codebase that follows it writes most of its small functions `const`, and nearly all of them only ever run at run time.
The engine's own decision crate, `rust-mutants-decision`, is the case in point: its judgements are `const fn`s, 87 places in it were passed over, and a run of the crate cataloged 12 candidates, 6 of which the compiler accepted, all in the one function that is not `const`.

Whether the compiler evaluates a function before the program runs is not a fact the syntax holds.
It is a fact about the program's uses of the function — a `const` item, a `static`, an array length, a `const` block, a `const fn` that is itself evaluated — and the compiler states it exactly, as `E0015` at the call, when the function is not `const`.

## Decision

1. **The body of a `const fn` is proposed as any other body.** What the compiler evaluates wherever it is written stays `const-context`: a `const` or `static` initializer, a `const` block, an array length, an enum discriminant, a `const` item inside the function.
   A closure or a function written inside a `const fn` is a body of its own, which the compiler never asks to be `const`.
2. **The instrumented tree writes a `const fn` without its `const` where it holds a guard, and nowhere else but where it carries one.** The keyword becomes as many blanks, so no byte after it moves.
   A `const fn` that holds no guard keeps its `const`, unless the compiler has said it calls one written without its own and nothing says it is evaluated before the program runs: then it carries the guard and goes without its `const` too.
   The `const` goes only where a guard needs it gone.
   Taking it only from a function that holds a guard was the first design, and a real crate refuted it: a `const fn` whose one candidate the compiler refuses — a `return-default` of a type with no `Default`, which is most of the decision crate — kept its `const`, and every mutant of every function it calls was then left out as evaluated before the program runs, which nothing does.
3. **A `const fn` takes no checkpoint and no entry marker, with its `const` or without it.** What the steps count and what an item's reach names are what the pristine file says, so a round that gives a function its `const` back changes neither.
4. **`E0015` about a function written without its `const` is about that function, wherever it points.** A free function is named by a note whose span is its definition, which holds the blanked keyword; an associated function or a method only by its type and its name, which names every function of that name and type the round wrote without its `const`, and every function of that name where no such type is one.
   A refusal that condemns too little is refused again next round, and one that guesses is a build nobody can trust, so where the name is ambiguous every function it could name is taken.
5. **Where the call is in the body of a `const fn` written with its `const`, and nothing has said the compiler evaluates that function, the caller carries the guard; anywhere else the callee is evaluated before the program runs.** A callee evaluated before the program runs keeps its `const` from the next round on, and every mutant it holds is left out as `evaluated-before-run`.
   A round learns a call or a function that keeps its `const`, and never unlearns one, so the rounds end whatever the chain: they are not held to the round limit that bounds ordinary attribution, and none ends in a bisection or a refused build.
   A bisection starts from the tree with nothing live, in which no function goes without its `const`.
   Which of the two a refusal is, is read from where it points, never from its words: the call is in the body of the narrowest `const fn` written with its `const` whose body holds the refusal's primary span, unless a constant inside that body holds it — an initializer, a `const` block, an array length, a discriminant or a const argument or default — which the compiler evaluates on its own, and then the callee is evaluated before the program runs.
   The refusal is recognised by its code, `E0015`, and the callee by a note's span where it has one; only an associated function or a method is still named by the path the message quotes between backquotes.
   `the_pinned_compiler_s_own_spans_tell_a_constant_inside_a_const_fn_from_its_body` in `crates/rust-mutants/tests/toolchain_validate.rs` holds the rule to the pinned compiler's spans.
6. **A candidate left out this way is a place passed over, not a refusal.** Its edit may compile, and a test may notice it through a value the compiler computed.
   What it is, is a place no guard can live.
   A report counts it under `skipped`, as `evaluated-before-run`, one place per candidate, and lists it among the rejections with `reason: "evaluated-before-run"` for its identity and the compiler's words; `refused` counts only `reason: "compiler-refused"`.
   A run document whose skips and rejections disagree about it is refused.
7. **The witness tree asks nothing about a site in a `const fn`.** It is checked before validation says which functions go without their `const`, and a witness written into one would be a call its `const` refuses.

## Consequences

- The decision crate's `const fn` bodies are mutated: 99 candidates, of which the compiler accepts 39 and refuses 60 return replacements of types with no `Default`, and none is left out as evaluated before the program runs, where there were 12 candidates and 6 mutants.
- Two uses of a function are outside the build validation compiles: a documented example, which cargo compiles only by running it, and code behind a `#[cfg(...)]` that build does not enable.
  Where one evaluates a function the tree wrote without its `const`, that build fails with the same `E0015`, and the run refuses or names it as it would any failing baseline or unsealable build; [limitations](../limitations.md#places-a-run-passed-over) says so and names the marker that keeps a function `const`.
- A mutant in a `const fn` carries no branch proof, comparison or probe, so every test that reaches it runs it.
  A proof about such a site would need the witness tree to learn the same `E0015`s validation does, in rounds of its own.
- The trace's `validate-round` gains `carried`, the errors a caller that goes without its `const` from the next round on accounts for, so a round that condemned nothing and still made progress says what it learned.
- njutest makes no row of a candidate left out this way; its report states it as the limitation `skipped-evaluated-before-run`, as it states every skip reason.
- `const-fn-body` is gone from the skip vocabulary, and `evaluated-before-run` is in its place, decided by the compiler rather than by the walk.
