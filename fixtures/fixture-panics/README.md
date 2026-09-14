<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-panics

Two ways a test process ends other than by failing an assertion.

`in_range` is guarded by an assertion and one of its tests is
`#[should_panic]`. A mutation that negates the condition makes the function
stop panicking, and a `#[should_panic]` test that stops panicking fails: a
kill through the absence of a panic rather than the presence of one. Nothing
else in the suite has that shape.

`capacity` calls `std::process::abort` where there is nothing to return. A
mutation that inverts the comparison aborts on a value the test does pass, so
the process dies from a signal rather than exiting. On unix that is `128 + 6`
and on Windows an exit code of its own; both are non-zero, and a run reads a
non-zero exit as a kill on either. The point of the fixture is that the two
platforms reach the same verdict by different routes.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:17:8 negate-condition killed
src/lib.rs:17:10 ge-to-gt killed
src/lib.rs:17:13 int-decrement killed
src/lib.rs:17:13 int-increment killed
src/lib.rs:20:5 return-default killed
src/lib.rs:31:8 negate-condition killed
src/lib.rs:31:10 eq-to-neq killed
src/lib.rs:31:13 int-increment killed
src/lib.rs:34:5 return-default killed
```
