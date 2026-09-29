<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-durable-calls

A count kept on disk by every kind of call that writes, asked about by crashes rather than mutants ([ADR 0035](../../docs/adr/0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)).
Each test reads the count its previous run left under the temporary directory `TMPDIR` names, adds one, keeps it through the library, and reads it back, so its tests seal as they run natively ([ADR 0046](../../docs/adr/0046-a-verdict-is-what-a-sealed-run-observed.md)).
The library holds the calls that write and nothing else, and the tests read the count themselves, so a run of it is small enough that `xtask/tests/testdata/crash-run-sealed` and `crash-run-native` keep one each for the audit.

- `save_by_copy` writes the count beside the file, copies it into place, and removes the copy: a stop after any of the three leaves the old count or the new one, and the next run passes over it, `restarted`.
- `save_synced` makes the file afresh, writes the count, and syncs it twice: a stop after `File::create` leaves an empty file the next run cannot read, `corrupt`; after `write_all`, `sync_data` or `sync_all`, `restarted`.
- `save_cut` cuts the file back to nothing with `set_len` before writing again: a stop after `set_len` leaves it empty, `corrupt`; after the write, `restarted`.
- `save_buffered` writes through a `BufWriter`: a stop after `File::create` or after `write_all` into the buffer leaves an empty file, since nothing flushes the buffer after the stop, `corrupt`; after `flush`, `restarted`.
- `save_guarded` writes the count while a guard is alive whose `Drop` writes `<name>.dropped`: a stop after the count's write leaves no `.dropped`, since nothing runs after the stop, and one after the guard's own write leaves it; both are `restarted`.
- `save_from_a_child` is reached only by the test binary run again as a process of its own, which `a_count_kept_by_a_child_goes_up` starts: a stop there is a stop in that process, not in the test's, and the crash is `undecided`.
  A process does not seal, so that test has no sealed control, and its crash is put natively.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `crash-after-write` at each of the fifteen writing calls |

```fates --operator crash-after-write
src/lib.rs:12:5 crash-after-write unproven
src/lib.rs:13:5 crash-after-write unproven
src/lib.rs:14:5 crash-after-write unproven
src/lib.rs:19:20 crash-after-write unproven
src/lib.rs:20:5 crash-after-write unproven
src/lib.rs:21:5 crash-after-write unproven
src/lib.rs:22:5 crash-after-write unproven
src/lib.rs:33:5 crash-after-write unproven
src/lib.rs:34:5 crash-after-write unproven
src/lib.rs:39:46 crash-after-write unproven
src/lib.rs:40:5 crash-after-write unproven
src/lib.rs:41:5 crash-after-write unproven
src/lib.rs:47:5 crash-after-write unproven
src/lib.rs:55:9 crash-after-write unproven
src/lib.rs:61:5 crash-after-write unproven
```
