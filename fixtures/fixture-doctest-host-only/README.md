<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest-host-only

A library whose only example of one function is marked `ignore-wasm32`: it runs natively and is ignored on the sealed target.

| Function | Documented | Fate |
| --- | --- | --- |
| `twice` | with an example the sealed target ignores | unproven: the example that would notice it runs only natively, so its kill is a lead |
| `thrice` | with an example that runs everywhere | killed by it, sealed |

The first row is what a sealed station has to hold to be honest.
The merged binary built for the sealed target still names the ignored example, and runs it as nothing when asked for it by index, so a station that went by name would put `twice` to an example that never calls it and call its mutants unreached.
A station holds exactly the tests the native baseline ran, and an example the sealed build ignores is one it does not hold, so the route to `twice` meets a test the sealed build cannot answer for.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:12:5 return-default unproven
src/lib.rs:12:7 mul-to-div unproven
src/lib.rs:12:9 int-decrement unproven
src/lib.rs:12:9 int-increment unproven
src/lib.rs:21:5 return-default killed
src/lib.rs:21:7 mul-to-div killed
src/lib.rs:21:9 int-decrement killed
src/lib.rs:21:9 int-increment killed
```
