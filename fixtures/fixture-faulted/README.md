<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-faulted

Calls that can fail, asked about by faults rather than mutants ([ADR 0032](../../docs/adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
A fault replaces the call a `?` asks about with its failure, and only a run that names the rule `inject-error` makes any.

- `load` reads a file with `read_to_string(path)?`, and tests in two targets check it worked: a failed read is noticed.
- `number` parses with `parse::<u8>()?`, and `seven_is_a_number` checks the answer: a failed parse is noticed.
- `measured` reads a file the same way, but `length` throws its answer away in two targets: a failed read is absorbed by both.
- `ours` propagates an error type only this crate can make, and `maybe` propagates an `Option`.
  Neither can be failed without guessing, so the compiler refuses both faults, and they are not put.
- `spare` reads a file no test asks for: its fault is unreached.
- `linger` sleeps for sixty seconds when its read fails: its native fault waits out the two-second measurement bound.
  The sealed host advances virtual time through the sleep, so the engine's sealed fault survives.
- `refused` is reached only by a target whose one test declines in the same words with and without a fault: nobody measures it, and the native fault is undecided.
  Its sealed fault is unproven because every reaching test is set aside for declining.

The njutest toolchain test runs this fixture with native faults and re-decides its report and trace with proofaudit.
The engine fates below use sealed executions and are also kept as `engine-run-faulted` beside the engine audit.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `inject-error` at each of the eight `?` |

```fates --operator inject-error
src/lib.rs:0:0 inject-error refused
src/lib.rs:0:0 inject-error refused
src/lib.rs:13:16 inject-error killed
src/lib.rs:22:18 inject-error killed
src/lib.rs:33:16 inject-error survived
src/lib.rs:66:16 inject-error unreached
src/lib.rs:75:16 inject-error survived
src/lib.rs:84:16 inject-error unproven
```
