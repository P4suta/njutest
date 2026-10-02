<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest

A library whose documentation runs, and is routed to.

| Function | Documented | Fate |
| --- | --- | --- |
| `double` | with an example, and unit-tested | killed, by whichever of the two tests that reach it runs first sealed |
| `half` | with an example only | **killed by the documentation**, which is the only test that exercises it at all |
| `third` | with an example that must not compile, which rustdoc compiles alone and never runs | unreached: no test's sealed control reaches it |

The middle row is why this fixture exists.
A mutation only a documented example can notice used to be reported as surviving, which is a finding that is not a gap in the tests, and then as unproven, because nothing sealed a doctest.

rustdoc on edition 2024 merges a library's examples into one binary, and a sealed run gets it by handing rustdoc a runner that keeps each binary it is given.
The merged binary is built to list its examples when it runs, and runs one of them alone when it is given that one's index, so each example is a test of its own with a control of its own, as a test of any other target is.
The route still reaches every mutation of the library through the documentation, as `doctests-routed-by-file` says, because a native run cannot tell which example reached what; the sealed controls can, so the third row is unreached rather than survived.
The third row's example is why rustdoc prints two reports here, the merged binary's and its own for the example it compiles alone, and a native run of the documentation is one run that accounts for both.

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
src/lib.rs:30:5 return-default unreached
src/lib.rs:30:7 div-to-mul unreached
src/lib.rs:30:9 int-decrement unreached
src/lib.rs:30:9 int-increment unreached
```
