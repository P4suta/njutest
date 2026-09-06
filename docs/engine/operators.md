<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Operators

**Status: implemented** (`rust_mutants::syntax`, `rust_mutants::instrument`,
`rust_mutants::validate`). The v1 table: twelve families, fifty-one rules,
named `family` / `rule@version`. The version enters the mutant identity, so
changing a rule's output is a new version and every old identity lapses with
it. Adding a rule does not: what enters an identity is the rule's own name and
version rather than the table around it, so an acceptance and a reused verdict
both survive the table growing. What would rename a mutant is reordering the
table, because two rules that can write the same replacement at the same span
are separated by which comes first — so a new family goes at the end of its
tier's run and a new rule at the end of its family's block, and
`adding_a_rule_never_reorders_the_ones_that_were_there` is the guard. The golden
`crates/rust-mutants/tests/testdata/syntax/families.golden` shows every
rule's candidate on one input, with its guard form and site.

Discovery is syntax-first: a rule fires on a token shape (`a + b`, `x?`,
`0..n`, `return e`), and the compiler settles later whether the edit
type-checks. Return replacements read the signature — `-> bool` offers
`return-true`, `-> Result<..>` `return-ok-default`, `-> Option<..>` both
`return-some-default` and `return-default`, anything else `return-default` —
and never propose a value the code already spells (`0`, `false`, `""`, `()`,
`None`, `Ok(())`, `Default::default()`, `T::new()`). A range swap changes
the expression's type, so its guard sits at the enclosing statement or `let`
initializer, where the types meet again. A `&&`/`||` with a `let` operand
and an `if let`/`while let` condition are left alone: they cannot be
negated or swapped and compile.

Type-directed splits are impossible without a type checker
([ADR 0008](../adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)),
so families split by syntax and the compiler rejects what does not
type-check. Replacements derive from the token, never from a string.

| Family | Rules | Tier |
| --- | --- | --- |
| `boolean-literal` | `true-to-false`, `false-to-true` | balanced |
| `condition-negation` | `negate-condition`, `negate-loop-condition`, `remove-not` | balanced |
| `boolean-connective` | `and-to-or`, `or-to-and` | balanced |
| `comparison` | `eq-to-neq`, `neq-to-eq`, `lt-to-le`, `le-to-lt`, `gt-to-ge`, `ge-to-gt` | balanced |
| `range` | `range-to-inclusive`, `inclusive-to-range` | balanced |
| `arithmetic` | `add-to-sub`, `sub-to-add`, `mul-to-div`, `div-to-mul`, `rem-to-mul`, `remove-unary-minus` | balanced |
| `return-replacement` | `return-default`, `return-ok-default`, `return-some-default`, `return-true` | balanced |
| `error-propagation` | `question-to-unwrap`, `ignore-question-statement` | balanced |
| `bitwise` | `band-to-bor`, `bor-to-band`, `xor-to-band`, `shl-to-shr`, `shr-to-shl` | strong |
| `compound-assignment` | `add-assign-to-sub-assign`, `sub-assign-to-add-assign`, `mul-assign-to-div-assign`, `div-assign-to-mul-assign`, `rem-assign-to-mul-assign`, `band-assign-to-bor-assign`, `bor-assign-to-band-assign`, `xor-assign-to-band-assign`, `shl-assign-to-shr-assign`, `shr-assign-to-shl-assign` | strong |
| `method-swap` | `is-some-to-is-none`, `is-none-to-is-some`, `is-ok-to-is-err`, `is-err-to-is-ok`, `max-to-min`, `min-to-max` | strong |
| `statement-deletion` | `delete-call-statement`, `delete-assignment`, `delete-compound-assignment` | all |

`balanced ⊂ strong ⊂ all`, which the table's order carries: it is
non-decreasing in tier, so each profile's rules are a prefix of the next
one's.

A method swap edits the method's identifier and nothing else. It reads no
type, so `is_none` on a receiver that has no such method is a mutation the
compiler refuses, which is where acceptance is settled
([ADR 0008](../adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)):
proposing it costs a `cargo check` diagnostic and never a wrong answer.

An assertion macro's leading arguments are walked as the expressions they are:
`assert!`, `debug_assert!` and `matches!` for their first, `assert_eq!`,
`assert_ne!`, `debug_assert_eq!` and `debug_assert_ne!` for their first two.
`proc_macro2` keeps the byte position of every token, so parsing an argument
back into an expression re-reads the same bytes rather than a pretty-printed
copy, and an edit inside one is an edit where the reader sees it. An argument
that does not parse as an expression puts the whole invocation back to being a
`macro-invocation` skip.

The allowlist is fixed in the code and is not configurable. What a macro does
with its tokens is the macro's business, and a guard spliced into an invocation
the engine does not understand is a guess. `panic!`, `unreachable!`, `write!`
and `format!` are not on it: their arguments are a message and a format string,
and mutating those asks nothing about the program.

Mutating a macro's *expansion* is not the answer and will not become one.
`-Zunpretty=expanded` is nightly, which would break the rule that the engine
adds nothing to the project under test; the expansion is pretty-printed, which
breaks byte splicing and the line-count invariant at the root; and both a
mutant's identity and an llvm-cov region are coordinates in the original file,
so there would be nothing to map an expansion's positions onto.

## Proofs the engine states

- **Branch proof** (`Mutant.branch`): for `le-to-lt`, `ge-to-gt`, and
  `or-to-and`, when the edit sits under an `if` or `while` condition reached
  only through `&&`, `||`, and parentheses, the whole condition is inert
  (identifiers, literals, `!`, comparisons and casts between primitives — as
  the witness tree proves), and the body has at least one statement: the
  body's brace-to-brace span.
- **Probe form** (`Mutant.probed`): for the `return-replacement` family, when
  every operand of the statement is effect-free and cannot panic and the
  compiler accepts the probe. It accepts it only where equality is the whole of
  what a program can tell apart — the integers, `bool`, `char`, the unit — so a
  float, and a type whose `PartialEq` answers about less than a test can read,
  leave the mutant unprobed.

A mutant without a proof is still cataloged, instrumented, and executed; what
it lacks is only the licence to skip a test.

## What `include!` does to a file

A file another file pastes in with `include!` is discovered like any other file
in the package, because it is one: cargo compiles it as part of whatever
includes it. What differs is where its bytes end up.

At item position the included file's items — and the runtime module appended
to it — land in the includer's module. The module names carry the digest of
their file's path so the two never collide
([ADR 0011](../adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)),
and the guards travel with the module, so a file included inside a nested `mod`
resolves exactly as it does at the root.

At expression position the included file is a fragment: `[1, 2, 3]` is not a
set of items and nothing parses it as one. Discovery reads the includes of the
files that do parse, and a file only pasted in that way is skipped with the
reason `included-expression`. Refusing the whole run over it would refuse to
measure a project the compiler is perfectly happy with.

An `include!` whose argument is not a single string literal — `concat!` and
`env!` around an `OUT_DIR` path, say — names a file this run cannot name, and a
file it cannot name it says nothing about.

## What a procedural macro tells a run

A proc-macro crate's `--test` build is an ordinary executable: it links the
crate as a library and runs its unit tests in a process of its own, so what
those tests reach is measured exactly like anything else. Most of such a crate
is helper functions, and they are now measured.

The expansion is not. A macro decides what it expands to during the build, a
mutation is activated for a test process, and the two never meet — and cargo's
fingerprint does not include the activation variable, so changing it would not
rebuild anything even if the timing worked. A mutation only the expansion would
change is therefore reported as surviving, and
`proc-macro-expansion-not-measured` says why rather than letting the reader
read it as a gap in the tests.

The test binary is built with `prefer-dynamic`, so starting it directly — which
is what the engine does, never through `cargo test` — needs the toolchain's own
library directories on the dynamic search path. The engine puts them there,
from the sysroot rustc names as its own.
