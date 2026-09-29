<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-faulted

Calls that can fail, asked about by faults rather than mutants ([ADR 0032](../../docs/adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
A fault replaces the call a `?` asks about with its failure, and only a run that names the rule `inject-error` makes any.

- `load` reads a file with `read_to_string(path)?`, and `the_manifest_loads` checks it worked: a failed read is noticed.
- `number` parses with `parse::<u8>()?`, and `seven_is_a_number` checks the answer: a failed parse is noticed.
- `measured` reads a file the same way, but `length` throws its answer away and nothing checks it: a failed read goes unnoticed.
- `ours` propagates an error type only this crate can make, and `maybe` propagates an `Option`.
  Neither can be failed without guessing, so the compiler refuses both faults, and they are not put.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `inject-error` at each of the five `?` |

```fates --operator inject-error
src/lib.rs:0:0 inject-error refused
src/lib.rs:0:0 inject-error refused
src/lib.rs:13:16 inject-error killed
src/lib.rs:22:18 inject-error killed
src/lib.rs:33:16 inject-error survived
```
