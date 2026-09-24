<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-durable

A count kept on disk, asked about by crashes rather than mutants ([ADR 0035](../../docs/adr/0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)).
Each test reads the count its previous run left under the temporary directory, adds one, keeps it, and reads it back.

- `save_in_pieces` truncates the file, writes `count=`, then the number.
  A stop after the truncation or after `count=` leaves a file `load` cannot read, so the next run fails: both sites are `corrupt`.
  A stop after the number leaves a whole count, and the next run passes over it: `restarted`.
- `save_whole` writes the count beside the file and renames it into place.
  A stop after either leaves the old count or the new one, and the next run passes over it: both are `restarted`.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `crash-after-write` at each of the five writing calls |

```fates --operator crash-after-write
src/lib.rs:31:20 crash-after-write killed
src/lib.rs:32:5 crash-after-write killed
src/lib.rs:33:5 crash-after-write killed
src/lib.rs:43:5 crash-after-write killed
src/lib.rs:44:5 crash-after-write killed
```
