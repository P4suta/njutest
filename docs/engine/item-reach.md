<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Item reach

**Status: implemented** as a measurement; `njutest select`, which will route a change by it, is not.
The run that verifies the baseline records which items each test entered, beside which mutation sites it reached, and keeps both in `touched-v1.json`.
Why the markers are placed where they are is [ADR 0026](../adr/0026-an-item-is-entered-where-its-body-starts.md).

## Why sites are not enough

The guards of [ADR 0014](../adr/0014-the-guards-are-the-measurement.md) record a test at a mutation site when the test evaluates the guard.
A test can enter a function and leave it before the first site: an `unwrap` that panics, a `?` that returns early, a loop that never ends.
A function can have no site at all.
Neither is a fact the site record holds, and a change to such a function is one `select` has to route to exactly the tests that entered it.

`fixtures/fixture-item-reach` is that case: `a_word_is_refused` calls `halve("four")`, which panics on its first statement,
so the guards of `halve` never see the test and the entry marker of `halve` does.

## What is measured

Every function, method, and trait default method the instrumenter rewrites takes one call at the first statement of its body, beside the step checkpoint that is already there:

```rust
pub fn halve(text: &str) -> u32 {
    __rm::item(0); __rm::checkpoint(); let n: u32 = text.parse().unwrap();
    …
}
```

Every closure and every `async` block takes the same call with the index of the item it is written in,
because its body runs when it is called or polled, which can be after the item returned and on another test's thread.
So "a test entered item *X*" means "a test ran code written inside *X*'s body", which is what a change of those bytes needs.
A loop takes no entry marker: its body is inside an item that was already entered.

`item(index)` does nothing unless the run asked the guards what they reached (`RUST_MUTANTS_TOUCH`, for this catalog).
Asked, it records the item against the thread that entered it, the way `active(index)` records a site, and each thread appends an `e` record on its way out:

```text
e	tests::a_word_is_refused	0
e	-	4,5
```

`-` is a thread no test answers for, and what it entered every test of its target entered.
An item entered while its thread's own record is being torn down — a `Drop` of a thread-local — is written as `-` at once rather than refused, which only widens what the record says.

## The item catalog

Items are numbered apart from the mutants: densely, file by file in path order, and within a file in the order the instrumenter meets them.
`touched-v1.json` keeps the catalog as `items`, and each target's record keeps what each test entered as `entered`:

| Field | Meaning |
| --- | --- |
| `index` | the index an entry marker names, which is the item's position in `items` |
| `package`, `path` | the package that compiled the file, and the workspace-relative path |
| `name` | the item as a reader writes it, which is exactly the `item` a mutant inside it names |
| `span` | every byte of the item in the pristine file, attributes and signature included |
| `body` | the bytes of its body |
| `measurable` | whether entering it is recorded |

A change belongs to the **innermost item whose body holds all of it** (`Touched::item_holding`).
A change to a nested function's signature lies in the body of the function around it; a change to a top-level signature lies in no body, and `select` treats it as a signature change rather than as a body edit.

`TargetTouches::entering(item)` answers which tests entered an item, with the same reading of `-` as `reaching` has for a site, and `entered_by(test)` and `entered_by_any()` give one test's items and the target's union.
The `touch` trace event counts that union as `entered`.

## What is not measurable

A `const fn`, a `const`, and a `static` are in the catalog with `measurable: false`.
A call the compiler cannot evaluate cannot be written into a body it may evaluate at compile time, and a value computed at compile time is not entered by any test at all.
An absence from `entered` says nothing about such an item, so a change to one routes to every test.

Some code is in no catalog:

- A file the run does not mutate — excluded, test-only, fragment-included, `#![no_std]` without `std` to lend — has no items, so a change in it is one nothing was measured about.
- A closure written inside a macro invocation takes no marker, because the instrumenter does not parse macro tokens.
  It is still inside its item's body, so the only gap is such a closure escaping the item and being called by a test that never entered it.
  No mutation is ever made inside a macro invocation, so the soundness layer below cannot see this gap either.
- An `async` body that is resumed after an `.await` on a different test's thread is recorded on the thread that first polled it.

A global allocator written in a mutated file is as unmeasurable as it is unbounded: its first allocation on a thread re-enters the runtime that is allocating, which the step checkpoints already could not survive.

## What holds it honest

`cargo xtask engine-audit`'s `entry` layer re-derives, from `touched-v1.json` and the rows alone, the property `select` stands on:

- every site a test reached is inside an item that test entered;
- every test that noticed a mutation entered the item it is in;
- every mutation sits in a measurable item whose name is the row's `item`.

Its planted defects are a kill by a test that never entered the item and a site reached inside an unmeasurable item; a gate that does not find either refuses with exit code 2 before it reads the run.
`toolchain_differential` holds every run of its fixtures, in every measuring mode, to the same property through the engine's own API.

## What it costs

No process, no build, and no pass: one call per body and per closure, on the line that already holds the step checkpoint.
Off touch mode the call is one load and a branch; on the baseline run it is one bit per thread per item and one line per thread on its way out.
On `fixture-families` the instrumented build, the baseline, and the whole run moved by less than their own run-to-run spread;
[ADR 0026](../adr/0026-an-item-is-entered-where-its-body-starts.md#consequences) has the numbers and the load they were taken under.
