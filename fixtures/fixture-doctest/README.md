<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest

A library whose documentation runs, and is routed to.

| Function | Documented | Fate |
| --- | --- | --- |
| `double` | with an example, and unit-tested | killed by `doubling_two_is_four`, which is the cheaper of the two tests that reach it and therefore the one that runs first |
| `half` | with an example only | **killed by the documentation**, which is the only test that exercises it at all |
| `third` | without an example | survived: the documentation reaches the file it is in and does not notice this mutation |

The middle row is why this milestone exists. A mutation only a documented
example can notice used to be reported as surviving, which is a finding that
is not a gap in the tests.

The coarseness is in the third row and is stated as
`doctests-routed-by-file`. rustdoc compiles a documented example into a binary
this run never sees, so there is no coverage map to read and no profile to
merge: what is known is which files the library is made of, so the
documentation reaches every mutation in them and narrows none of them.

The documentation is one target for the whole library rather than one per
example, and that is not a simplification. On an edition where rustdoc merges
a file's examples into one compilation, asking the harness for one of them by
name runs every example in that file — a filter that matches nothing filters
everything out, and a filter that matches one example runs all of them. So a
kill the documentation finds names the documentation, and `--test-runtool`
is what would name the example.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:12:5 return-default killed
src/lib.rs:12:7 mul-to-div killed
src/lib.rs:12:9 int-decrement killed
src/lib.rs:12:9 int-increment killed
src/lib.rs:21:5 return-default killed
src/lib.rs:21:7 div-to-mul killed
src/lib.rs:21:9 int-decrement killed
src/lib.rs:21:9 int-increment killed
src/lib.rs:26:5 return-default survived
src/lib.rs:26:7 div-to-mul survived
src/lib.rs:26:9 int-decrement survived
src/lib.rs:26:9 int-increment survived
```
