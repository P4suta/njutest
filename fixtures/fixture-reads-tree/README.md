<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-reads-tree

Two integration targets.
`enters` calls `loud`.
`reads` never calls anything: it reads `src/quiet.rs` as text and checks it still says `"hush"`.

A selection skips a target only where its tests entered none of the items a change is to.
`reads` enters nothing, yet an edit to the body of `quiet` fails it, because what it depends on is the file's text and not its code.
`njutest measure` finds it by running each target once more with every source file that holds an item taken out of the copy; `reads` answers differently, so it is `reads-tree` and a selection never skips it.
`enters` answers the same and is still skipped for an edit to `quiet`.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `loud`: its arithmetic |
| `src/quiet.rs` | lib | `quiet`: its string |

```fates
src/lib.rs:10:5 return-default killed
src/lib.rs:10:7 mul-to-div killed
src/lib.rs:10:9 int-decrement killed
src/lib.rs:10:9 int-increment killed
src/quiet.rs:8:5 return-default unreached
src/quiet.rs:8:5 string-to-empty unreached
```
