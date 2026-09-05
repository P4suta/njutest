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
