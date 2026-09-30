<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-faulted-ignore

A `?` in statement position whose deletion no test notices, so the survivor gains its evidence only from the call failing beside it ([ADR 0032](../../docs/adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).

- `leave` writes a note with `std::fs::write(path, "left")?;` and returns `Ok(())`; `a_note_is_left` checks only the answer, never whether the note landed.
  Deleting the `?` leaves the failure unread and the answer `Ok`, so `ignore-question-statement` survives.
  The fault alone fails the write and the `?` answers the failure, so the test fails and the fault is noticed; beside the deletion the failure is read by nobody, so the survivor is told apart from an equivalence by the call failing: `observable-under-fault`, `failed: alone`.
- `the_manifest_reads` gives the suite a second question, so it is not one assertion.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `inject-error` at the `?` of `leave` |

```fates --operator inject-error
src/lib.rs:13:5 inject-error killed
```
