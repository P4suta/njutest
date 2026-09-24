<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-faulted-failure-writes

A call whose failure a test notices, and records by writing into the tree as it fails ([ADR 0032](../../docs/adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).

`load` reads a file with `read_to_string(path)?`.
`a_failed_read_is_recorded_and_fails_the_test` fails when the read fails, and first writes `regressions.txt` beside the manifest, the way a property test keeps the input that broke it.
The write belongs to the test's own failure, not to the program's answer to the failed call: run alone with the fault, the test fails, so njutest names the path in a `not-measured` finding about `fault-write-unattributed` rather than calling it `broken-under-fault`, and reports the fault as noticed.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `inject-error` at the `?` |

```fates --operator inject-error
src/lib.rs:13:16 inject-error killed
```
