<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-faulted-writes

A call whose failure a test answers by writing into the tree it is measured in ([ADR 0032](../../docs/adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).

`load` reads a file with `read_to_string(path)?`.
`a_failed_read_leaves_a_note` passes whether the read works or not, and where it fails it writes `failed-read.log` beside the manifest.
With no fault in place nothing is written, so the tree is only ever written while the fault fails the read: njutest reports that as `broken-under-fault`, and the fault itself as unnoticed, since the test passed.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `inject-error` at the `?` |

```fates --operator inject-error
src/lib.rs:13:16 inject-error survived
```
