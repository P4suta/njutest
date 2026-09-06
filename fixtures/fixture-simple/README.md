<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-simple

The smallest workspace the engine can open: one library crate with a unit
test module, one integration test target (`tests/parity.rs`), and a helper
module (`src/testutil.rs`) that only the test unit compiles.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `max`: `gt-to-ge`, `negate-condition`, `return-default`; `is_even`: `rem-to-mul`, `eq-to-neq`, `return-true` |
| `src/testutil.rs` | test only | skipped: `test-only-file` |
| `tests/parity.rs` | test target | never mutated |

`FIXTURE_SIMPLE_PAUSE_MS` makes the unit test sleep that many milliseconds, so
a test about interrupting a run can be sure the run is still running. Unset,
it does nothing.

The package denies `unused_qualifications`, which the generated runtime module
and the guards must not trip. `#[allow(warnings)]` does not cover a lint a
project has denied — `warnings` is the lints that are set to warn — so every
such lint has to be named, and this fixture is what says so when one is not.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:8 negate-condition killed
src/lib.rs:11:10 gt-to-ge survived
src/lib.rs:11:16 return-default killed
src/lib.rs:11:27 return-default killed
src/lib.rs:16:5 return-true killed
src/lib.rs:16:7 rem-to-mul killed
src/lib.rs:16:9 int-decrement killed
src/lib.rs:16:9 int-increment killed
src/lib.rs:16:11 eq-to-neq killed
src/lib.rs:16:14 int-increment killed
```
