<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Operators

**Status: implemented** (`rust_mutants::syntax`, `rust_mutants::instrument`,
`rust_mutants::validate`).
The v1 table: seventeen families, seventy-four rules,
named `family` / `rule@version`.
The version enters the mutant identity, so changing a rule's output is a new version and every old identity lapses with it.
Adding a rule does not: what enters an identity is the rule's own name and version rather than the table around it, so an acceptance and a reused verdict both survive the table growing.
What would rename a mutant is reordering the table, because two rules that can write the same replacement at the same span are separated by which comes first — so a new family goes at the end of its tier's run and a new rule at the end of its family's block, and `adding_a_rule_never_reorders_the_ones_that_were_there` is the guard.
The golden `crates/rust-mutants/tests/testdata/syntax/families.golden` shows every rule's candidate on one input, with its guard form and site.

Discovery is syntax-first: a rule fires on a token shape (`a + b`, `x?`,
`0..n`, `return e`), and the compiler settles later whether the edit type-checks.
Return replacements read the signature — `-> bool` offers `return-true`, `-> Result<..>` `return-ok-default`, `-> Option<..>` both `return-some-default` and `return-default`, anything else `return-default` —
and never propose a value the code already spells (`0`, `false`, `""`, `()`,
`None`, `[]`, `&[]`, `vec![]`, `Ok(())`, `Default::default()`, `T::new()`).
A signature the syntax cannot say has a default is `unstated-return-type` rather than a candidate the compiler will refuse: an `impl Trait`, a raw pointer, a function type, a type a macro writes, a generic parameter nothing bound to `Default`, and a `&mut T` — a reference that is read is defaultable where the syntax says so (`&str`, `&[T]`, and the arguments of an `Option` or a `Result` that spell one), and a reference that is written is not.

A return site is the whole returned expression and, where that expression is an `if` or a `match` whose arms return, each branch of it as well.
`fn sign` whose body is `if n > 0 { "positive" } else if n < 0 { "negative" } else { "zero" }` therefore carries four return replacements: one that answers for the function and one for each of the three answers it chooses between.
The whole-expression mutant keeps the id it always had; the branch mutants are new ones beside it.

A range swap changes the expression's type, so its guard sits at the enclosing statement or `let` initializer, where the types meet again.
A `&&`/`||` with a `let` operand and an `if let`/`while let` condition are left alone: they cannot be negated or swapped and compile.

Type-directed splits are impossible without a type checker ([ADR 0008](../adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)),
so families split by syntax and the compiler rejects what does not type-check.
Replacements derive from the token, never from a string.
The table has eighteen families and seventy-five rules.
One of them, `fault`, is not a mutation: it fails the call a `?` asks about, no tier chooses it, and a run asks for it by name with `--operator inject-error` ([ADR 0032](../adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).

| Family | Rules | Tier |
| --- | --- | --- |
| `boolean-literal` | `true-to-false`, `false-to-true` | balanced |
| `condition-negation` | `negate-condition`, `negate-loop-condition`, `remove-not`, `negate-bool-method` | balanced |
| `boolean-connective` | `and-to-or`, `or-to-and` | balanced |
| `comparison` | `eq-to-neq`, `neq-to-eq`, `lt-to-le`, `le-to-lt`, `gt-to-ge`, `ge-to-gt` | balanced |
| `range` | `range-to-inclusive`, `inclusive-to-range` | balanced |
| `arithmetic` | `add-to-sub`, `sub-to-add`, `mul-to-div`, `div-to-mul`, `rem-to-mul`, `remove-unary-minus` | balanced |
| `return-replacement` | `return-default`, `return-ok-default`, `return-some-default`, `return-true`, `return-err-default` | balanced |
| `error-propagation` | `question-to-unwrap`, `ignore-question-statement` | balanced |
| `match-arm` | `delete-match-arm`, `remove-match-guard` | balanced |
| `control-flow` | `break-to-continue`, `continue-to-break` | balanced |
| `condition-removal` | `condition-to-true`, `condition-to-false` | balanced |
| `bitwise` | `band-to-bor`, `bor-to-band`, `xor-to-band`, `shl-to-shr`, `shr-to-shl` | strong |
| `compound-assignment` | `add-assign-to-sub-assign`, `sub-assign-to-add-assign`, `mul-assign-to-div-assign`, `div-assign-to-mul-assign`, `rem-assign-to-mul-assign`, `band-assign-to-bor-assign`, `bor-assign-to-band-assign`, `xor-assign-to-band-assign`, `shl-assign-to-shr-assign`, `shr-assign-to-shl-assign` | strong |
| `method-swap` | `is-some-to-is-none`, `is-none-to-is-some`, `is-ok-to-is-err`, `is-err-to-is-ok`, `max-to-min`, `min-to-max`, `all-to-any`, `any-to-all`, `first-to-last`, `last-to-first`, `skip-to-take`, `take-to-skip`, `sum-to-product`, `product-to-sum` | strong |
| `statement-deletion` | `delete-call-statement`, `delete-assignment`, `delete-compound-assignment`, `delete-else-branch` | all |
| `literal` | `int-increment`, `int-decrement`, `string-to-empty` | all |
| `saturating-arithmetic` | `saturating-add-to-wrapping-add`, `saturating-sub-to-wrapping-sub`, `saturating-mul-to-wrapping-mul` | all |
| `fault` | `inject-error` | named |

`balanced ⊂ strong ⊂ all`, which the table's order carries: it is non-decreasing in tier, so each profile's rules are a prefix of the next one's, and `tiers_never_decrease_down_the_table` is what holds it there.

`delete-match-arm` asks whether a suite notices an arm going away, by making the arm's guard `false`; `remove-match-guard` asks whether it notices the arm widening, by making the guard `true`.
An arm with no guard is where the guard is written, which is what Form M is for.
Deletion is offered only where the syntax can say the match stays exhaustive without the arm: the arm is not itself a bare `_`, and a bare `_` sits below it.
Every other arm may be the one carrying exhaustiveness, and a mutation the compiler refuses says nothing about the tests.

`break-to-continue` and `continue-to-break` keep the label, because the label says which loop the jump is about.
A `break` that carries a value is left alone: `continue` carries none, and the loop whose value it was would have nothing to be.
A swap that turns the only way out of a loop into a way round it is a mutation the tests notice as a timeout, which is a kill.

`condition-to-true` and `condition-to-false` fix an `if` condition at each answer in turn.
They ask of an `if` exactly what `remove-match-guard` and `delete-match-arm` already ask of an arm, which is why they are two rules rather than one: a suite that kills `negate-condition` has one test whose branch changed, and that test kills exactly one of the pair.
The negation says a branch is checked; the pair says *which* branch is checked and which is not, which is a different question and the one somebody can act on.
A condition somebody already wrote as `true` or `false` is left alone — writing `true` where `true` is written is an equivalent mutant offered by construction — and a loop condition is left alone because `while true` does not answer a question about the tests: it hangs, the run bounds it, and the bound is recorded as a kill nobody learned anything from.

`delete-else-branch` takes the `else` an `if` chain ends with, and only where the chain stands as a statement: an `if` that is a value has to have an `else` and every branch has to produce the same type.

`int-increment` and `int-decrement` respell a literal one step away in the radix it was written in, keeping its suffix.
Rust spells no negative literal — `-1` is a unary minus on `1` — so zero has no predecessor to offer, and a suffix that names a type bounds what the literal may become.
`string-to-empty` empties a string that says something: a message nobody checks is a message nobody would miss.

`negate-bool-method` asks the opposite of a call that answers a question —
`is_*`, `has_*`, `contains`, `contains_key`, `starts_with`, `ends_with` — by wrapping it in a `!`.
It stays out of three places another rule already asks about: the whole of an `if` or `while` condition, which is `negate-condition`'s and `negate-loop-condition`'s; a call directly under a `!`, which is `remove-not`'s; and `is_some`, `is_none`, `is_ok`, `is_err`,
which are the swaps'.
Every one of those places carries the other rule's decision, so leaving it is not silence.

A method swap edits the method's identifier and nothing else.
It reads no type, so `is_none` on a receiver that has no such method is a mutation the compiler refuses, which is where acceptance is settled ([ADR 0008](../adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)):
proposing it costs a `cargo check` diagnostic and never a wrong answer.

An assertion macro's leading arguments are walked as the expressions they are:
`assert!`, `debug_assert!` and `matches!` for their first, `assert_eq!`,
`assert_ne!`, `debug_assert_eq!` and `debug_assert_ne!` for their first two.
`proc_macro2` keeps the byte position of every token, so parsing an argument back into an expression re-reads the same bytes rather than a pretty-printed copy, and an edit inside one is an edit where the reader sees it.
An argument that does not parse as an expression puts the whole invocation back to being a `macro-invocation` skip.

The allowlist is fixed in the code and is not configurable.
What a macro does with its tokens is the macro's business, and a guard spliced into an invocation the engine does not understand is a guess.
`panic!`, `unreachable!`, `write!` and `format!` are not on it: their arguments are a message and a format string,
and mutating those asks nothing about the program.

Mutating a macro's *expansion* is not the answer and will not become one.
`-Zunpretty=expanded` is nightly, which would break the rule that the engine adds nothing to the project under test; the expansion is pretty-printed, which breaks byte splicing and the line-count invariant at the root; and both a mutant's identity and an llvm-cov region are coordinates in the original file,
so there would be nothing to map an expansion's positions onto.


### What the saturating three cost, and when they pay

`saturating_add` and `wrapping_add` are the same function everywhere except at the type's boundary, so whether a mutant of one is observable is a question about the values that arrive rather than about the edit.

On an **unsigned** type the boundary is zero, index arithmetic reaches it in every other line, and the mutant is observable — usually as a loop or an allocation over `usize::MAX`, which a run reports as a timeout rather than a kill.
On a **wide signed** type holding a small domain — a typographic length in 1/720 em sits six orders of magnitude from `i32::MAX` — nothing any test supplies can tell the two apart, and every mutant is a survivor no amount of reading will resolve.

Asked of one crate that saturates by lint policy, the three minted 438 mutants and killed none of them: 426 survived and 6 timed out, all six of the latter on `usize` indices.
That is the reason they are in `all` rather than `strong`.
Nothing in the syntax pass knows a type, so the engine cannot mint them only where they pay.

A rule like this is expensive twice, which is worth saying because the intuition runs the other way.
A killed mutant stops its target at the first failing test; a surviving one runs the suite to the end.
In the run above the three cost 3.5 seconds a mutant against 0.45 for the same crate's whole catalog — eight times the price, for the mutants least likely to tell anybody anything.

## Proofs the engine states

- **Branch proof** (`Mutant.branch`): for `le-to-lt`, `ge-to-gt`, and `or-to-and`, when the edit sits under an `if` or `while` condition reached only through `&&`, `||`, and parentheses, the whole condition is inert (identifiers, literals, `!`, comparisons and casts between primitives — as the witness tree proves), and the body has at least one statement: the body's brace-to-brace span.
- **Probe form** (`Mutant.probed`): for the `return-replacement` family, when every operand of the returned expression is effect-free and cannot panic and the compiler accepts the probe.
  It accepts it only where equality is the whole of what a program can tell apart — the integers, `bool`, `char`, the unit, `str`, `String`, and `Option` or `Vec` of one of those — so a float,
  and a type whose `PartialEq` answers about less than a test can read, leave the mutant unprobed.

A selected mutant without a proof is still cataloged, instrumented, and executed; what it lacks is only the licence to skip a test.
A run filter can leave a different candidate explicitly `unselected` before either proof or compiler validation.

## What `include!` does to a file

A file another file pastes in with `include!` is discovered like any other file in the package, because it is one: cargo compiles it as part of whatever includes it.
What differs is where its bytes end up.

At item position the included file's items — and the runtime module appended to it — land in the includer's module.
The module names carry the digest of their file's path so the two never collide ([ADR 0011](../adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)),
and the guards travel with the module, so a file included inside a nested `mod` resolves exactly as it does at the root.

At expression position the included file is a fragment: `[1, 2, 3]` is not a set of items and nothing parses it as one.
Discovery reads the includes of the files that do parse, and a file only pasted in that way is skipped with the reason `included-expression`.
Refusing the whole run over it would refuse to measure a project the compiler is perfectly happy with.

An `include!` whose argument is not a single string literal — `concat!` and `env!` around an `OUT_DIR` path, say — names a file this run cannot name, and a file it cannot name it says nothing about.

## What a procedural macro tells a run

A proc-macro crate's `--test` build is an ordinary executable: it links the crate as a library and runs its unit tests in a process of its own, so what those tests reach is measured exactly like anything else.
Most of such a crate is helper functions, and they are now measured.

The expansion is not.
A macro decides what it expands to during the build, a mutation is activated for a test process, and the two never meet — and cargo's fingerprint does not include the activation variable, so changing it would not rebuild anything even if the timing worked.
A mutation only the expansion would change is therefore reported as surviving, and `proc-macro-expansion-not-measured` says why rather than letting the reader read it as a gap in the tests.

The test binary is built with `prefer-dynamic`, so starting it directly — which is what the engine does, never through `cargo test` — needs the toolchain's own library directories on the dynamic search path.
The engine puts them there,
from the sysroot rustc names as its own.

## What a documented example is to this engine

A library's documentation is a target cargo runs rather than one the engine starts: rustdoc compiles each example while cargo runs it, so there is no binary in a build's messages to find, and what there is instead is a command.
Such a target carries the arguments cargo needs before the harness's own,
which go after a `--`.

Two consequences follow, and both are about what cannot be asked rather than about what this engine chose.
rustdoc merges a file's examples into one compilation, and that harness does not honour a filter naming one example: a filter that matches nothing filters everything out, and one that matches an example runs every example in its file.
And a documented example's binary is never seen by this run, so it carries no coverage map to read.

The third is about a guard.
A test binary the engine starts itself is recognised as having found a stale catalog by its exit code, and cargo turns that code into its own 101 — the code a failing test has.
So the runtime's own sentence in the output is what recognises it there, and a tree rebuilt behind a run's back cannot be mistaken for a kill.
