<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0048 — Const items are mutated by a build per mutant

**Status: implemented.**

## Context

A const item is evaluated by the compiler, and its value can be embedded in every caller.
A runtime guard cannot change it, and an absent runtime reach marker cannot establish that no test observes it.
ADR 0047 changes a `const fn` only where the compiler does not evaluate it early; it leaves const item initializers out entirely.
The defect class is a compile-time edit treated as a runtime edit, either omitted from discovery or judged from a binary that still contains its original value.

## Decision

`--tier compiled` selects the `all` operators and expression sites in free const items, associated const items and trait const defaults.
`balanced`, `strong` and `all` retain their runtime cost and discovery scope.
An explicit operator selection under the compiled tier retains its choice of operators and enables these initializer sites.
Static initializers, const blocks, array lengths and enum discriminants retain their `const-context` skip.
Statement sites and match patterns that the expression selector cannot hold remain `unsupported-site`.
Test code, cfg attributes and skip annotations retain their ordinary suppression.

The closed guard form `B` composes a constant selector rather than calling `active`, a probe or a branch witness.
Both generated runtimes share the same `const fn baked` selector, which reads `RUST_MUTANTS_COMPILED_ACTIVE` with `option_env!` at compilation.
Every control build overrides that variable with `none`; each mutant build overrides it with exactly one dense catalog index.
Cargo records the compile-time environment dependency and rebuilds the affected units when the selector changes.
The snapshot is immutable during these builds, and nested guards select only their original branches unless they hold that one index.

Ordinary guard validation first removes alternatives that the compiler refuses even in an inactive branch.
Validation then compiles each remaining initializer mutant alone, records the compiler's refusal against that exact index, and removes refused selectors from the final tree.
Only accepted selectors enter the build owner, so asking about a refusal cannot run the original value as that mutant.
Overflow and division by zero are refusals, not killed mutants.
The final native and sealed control binaries contain the original values.
An accepted initializer is compiled again into separate native and sealed output directories before it is run.
A mutex guard owns these reused directories until the execution finishes, so parallel mutant workers cannot replace one another's executable or module.
Only one extra native build and one extra sealed build are retained; storage does not grow with the catalog.

The closed routing fallback `compile-time` asks every test of every native target.
An initializer has no branch, infection or runtime reach proof, so none may narrow that set.
The mutant's sealed modules are listed and held to the original bench's tests, and retain the original controls, fuel measurements and refusal baselines.
The mutated binary is never run as its own control.
Every controlled test is asked, even though no runtime guard recorded reaching the constant.
A missing module or test leaves the mutation unproven under ADR 0046.
Native fallback executions use the separately compiled native targets and remain leads.

`compiled-mutant` trace notes identify each validation result and each native and sealed build by index.
Validation rounds attribute compile-time refusals exactly as ordinary compiler refusals are attributed.
Outcome-cache checks and whole-report reruns compile the recorded initializer again and compare it against the original controls.

## Evidence

`fixture-const-items` holds free and associated constants, an unobserved value, an equivalent arithmetic edit, an overflow and division by zero.
Its fates are nine sealed kills, three sealed survivors and two compiler refusals.
The toolchain test runs with two workers, checks those fates, and reproduces a sealed detection on a fresh original bench.
It also checks a native lead, reproduces every recorded sealed execution through a new session, and forbids a sealed verdict on a refused selector.
The syntax test holds the opt-in boundary and refuses runtime proof metadata on compiled sites.
The independent tree oracle includes form B and removes its guards back to the original initializer.
