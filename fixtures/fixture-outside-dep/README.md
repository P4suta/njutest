<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-outside-dep

A tree that reads code from beside itself.

`Cargo.toml` names `../fixture-outside-dep-lib`, which is outside this
directory. A run measures a copy of the tree, and the copy does not hold the
sibling, so cargo inside the copy would fail over a manifest that is not
there. The engine says so first, with `RM1017`, before anything is copied:
the refusal names the dependency, the manifest that declares it, and the flag
that allows it.

`--allow-outside ../fixture-outside-dep-lib` is that flag. It copies the
sibling beside the tree under the same name, so the same relative path
resolves in the copy, and the run measures a tree that is the one on disk plus
a directory somebody named.

This is the one fixture whose dependency climbs out of itself, which is why
`cargo xtask fixtures` allows a sibling fixture by name and nothing else.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked --allow-outside ../fixture-outside-dep-lib`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates --allow-outside ../fixture-outside-dep-lib
src/lib.rs:9:5 return-default killed
src/lib.rs:9:41 add-to-sub killed
src/lib.rs:9:43 int-decrement killed
src/lib.rs:9:43 int-increment killed
```
