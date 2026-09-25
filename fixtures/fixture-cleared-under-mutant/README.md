<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-cleared-under-mutant

`tests/threshold.rs` asks `delegated(7)` and, only when it says yes, starts the `child` binary with a cleared environment.
On the tree as written it says no, so the baseline starts no child and the target is not `uncontrolled-child`.

`return-true` makes `delegated` say yes, and the test starts a child no mutant can be active in.
The test still passes, and that pass is not a survival: the run sees the child say it lost the environment while the execution ran, and records the mutant inconclusive.
`gt-to-ge` starts no child for 7 and survives as it always did.
`doubled` runs only in the child, which the baseline never starts, so its mutants are unreached.

The fates are read with `--jobs 1`.
On Windows an orphan cannot name its parent, so while executions overlap the child `return-true` starts is charged to every one of them, and a neighbour's survival reads as inconclusive.
One execution at a time makes the attribution exact on every platform; the flag goes when [#145](https://github.com/P4suta/njutest/issues/145) attributes a Windows orphan by the job it ran in.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `delegated`: its comparison; `doubled`: its arithmetic |

```fates --jobs 1
src/bin/child.rs:8:14 int-decrement unreached
src/bin/child.rs:8:14 int-increment unreached
src/lib.rs:9:5 return-true inconclusive
src/lib.rs:9:7 gt-to-ge survived
src/lib.rs:9:9 int-decrement survived
src/lib.rs:9:9 int-increment survived
src/lib.rs:15:5 return-default unreached
src/lib.rs:15:7 mul-to-div unreached
src/lib.rs:15:9 int-decrement unreached
src/lib.rs:15:9 int-increment unreached
```
