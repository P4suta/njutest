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

Deferred to a later version: mutation inside `assert!`-family macros.

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
