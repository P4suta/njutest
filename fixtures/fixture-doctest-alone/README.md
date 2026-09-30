<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-doctest-alone

A library on an edition before 2024, whose doctests rustdoc compiles one binary each rather than merged into one.
A sealed run builds them once with `--list` baked in, which makes rustdoc list each doctest itself rather than running it, then again without it, handing each binary rustdoc would run to a capture, and runs each alone.

| Function | Documented | Fate |
| --- | --- | --- |
| `add` | with an example that returns, one rustdoc ignores, and one that must not compile | killed by the example that returns |
| `divide` | with an example that should panic | a mutation that stops the panic is killed, because the example returned where it had to fail; one that panics for every divisor survives, because the example asks only that it panics; the division after the panic is unreached |
| `next` | with an example rustdoc compiles and never runs | unreached: no test runs it |

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:20:5 return-default killed
src/lib.rs:20:7 add-to-sub killed
src/lib.rs:29:8 condition-to-false killed
src/lib.rs:29:8 condition-to-true survived
src/lib.rs:29:8 negate-condition killed
src/lib.rs:29:10 eq-to-neq killed
src/lib.rs:29:13 int-increment killed
src/lib.rs:32:5 return-default unreached
src/lib.rs:41:5 return-default unreached
src/lib.rs:41:7 add-to-sub unreached
src/lib.rs:41:9 int-decrement unreached
src/lib.rs:41:9 int-increment unreached
```
