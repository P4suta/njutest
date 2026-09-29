<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest-refused

A library whose examples rustdoc merges into one binary, one of which the sealed host refuses: it starts a thread.

| Function | Documented | Fate |
| --- | --- | --- |
| `after` | with an example the merged binary runs before the refused one | killed by it, sealed |
| `double` | with an example that starts a thread | unproven: the example that reaches it has no sealed control, so its kill is a lead |
| `half` | with an example the merged binary runs after the refused one | unproven: the merged binary never names it, so the sealed build does not hold it |

The merged binary runs its examples in the order of their names, all in one instance, and a panic on the sealed target aborts the instance.
So the example that starts a thread stops the listing inside itself, and the binary names only the examples before it and the one it stopped in.
Each of those is a test with a control of its own, run alone by its index, so the refused one is one test without a control rather than a library the sealed build cannot answer for.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:12:5 return-default killed
src/lib.rs:12:7 add-to-sub killed
src/lib.rs:12:9 int-decrement killed
src/lib.rs:12:9 int-increment killed
src/lib.rs:22:5 return-default unproven
src/lib.rs:22:7 mul-to-div unproven
src/lib.rs:22:9 int-decrement unproven
src/lib.rs:22:9 int-increment unproven
src/lib.rs:31:5 return-default unproven
src/lib.rs:31:7 div-to-mul unproven
src/lib.rs:31:9 int-decrement unproven
src/lib.rs:31:9 int-increment unproven
```
