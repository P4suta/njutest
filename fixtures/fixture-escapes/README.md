<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-escapes

One library whose integration test, when `FIXTURE_ESCAPES_RECORD` names a file, starts a `sleep` in a process group of its own and appends its process id to that file, as a test that starts a daemon does.
On unix a process that leaves the execution's process group is out of reach of the kill that ends the execution, so it outlives the test, the execution and the run.
It keeps working in the directory the test did, which lies in the run's copy of the tree or its scratch, so the run ends every process still working there when it closes, and a copy is never removed while one does.
`toolchain_cli_contract`'s `a_process_a_test_left_running_ends_with_the_run` sets the variable, runs this fixture, and finds every recorded process gone and the trace saying which it ended.
With `FIXTURE_ESCAPES_HOLDS_OUTPUT` set the daemon keeps the test's output open as well, so the execution cannot read to its end and the target's baseline is `errored` (`wait-failed`); `a_process_that_holds_a_refused_runs_output_ends_with_it` finds that run's daemons gone too.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:7 mul-to-div killed
src/lib.rs:9:9 int-decrement killed
src/lib.rs:9:9 int-increment killed
```
