<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# nested/fixture-climbs-dep

A tree that reads a library through a path climbing more than one level out of it.

`Cargo.toml` names `../../fixture-climbs-dep-lib`, which is outside this directory and outside the group holding it.
That is the case a flat copy cannot hold: placing the tree at one fixed name and the allowed directory at another puts them at equal depth, so a declaration climbing twice resolves past the copy entirely and cargo fails over a manifest that is not there.

A copy places everything it holds by substituting one prefix — the ancestor the tree and what it reads share becomes the directory the copy is made in — so the path between them is the path they had.
This fixture is why the group exists: a tree whose root sits below what it reads cannot be expressed by a flat directory.

## Fates

What one run of this fixture establishes for every mutation of it.
The run is `rust-mutants run --tier all --offline --locked --allow-outside ../../fixture-climbs-dep-lib`; `cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --allow-outside ../../fixture-climbs-dep-lib
src/lib.rs:9:5 return-default killed
src/lib.rs:9:43 add-to-sub killed
src/lib.rs:9:45 int-decrement killed
src/lib.rs:9:45 int-increment killed
```
