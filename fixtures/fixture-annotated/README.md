<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-annotated

Four `rust-mutants: skip` markers, one of each shape a marker can take.

| Marker | Shape | What it hides |
| --- | --- | --- |
| `larger`'s body | end of line | every edit that starts on that line |
| above `name` | own line, over an item with attributes | the whole item, attributes and all |
| above `let _said` | own line, over a statement | the statement |
| above a comment | own line, over nothing | nothing, which is an `unmatched-skip` finding |

What is left is `doubled`'s arithmetic, which the fixture's own test kills.
A marker is read out of the gaps between tokens, so the words inside a string
literal are a string; a documentation comment is a `#[doc]` attribute by then
and is never a marker.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:24:5 return-default killed
src/lib.rs:24:7 mul-to-div killed
src/lib.rs:24:9 int-decrement killed
src/lib.rs:24:9 int-increment killed
```
