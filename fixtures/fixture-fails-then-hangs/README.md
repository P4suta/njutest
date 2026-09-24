<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-fails-then-hangs

One function and one test target holding two tests.
`a_says_the_answer_is_ready` asserts what `ready` answers, so a mutation that changes the answer fails it at once.
`b_waits_until_the_answer_is_ready` parks forever when the answer is wrong, without running any of the library again, so nothing counts its steps and only the clock ends the process.

Under such a mutation the harness prints `a_says_the_answer_is_ready ... FAILED` and then never exits.
That is the shape an adopter met: a failure the harness had already named, followed by a hang, which a run once concluded `waited` while naming the failed test in `killed_by`.
A test that finished and failed noticed the mutation whatever the clock did afterwards, so the run concludes `killed`, and the row records that it `lingered` ([ADR 0023](../../docs/adr/0023-a-run-may-not-conclude-from-how-it-measured.md)).

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `ready`: the comparison and its return |

```fates
src/lib.rs:9:5 return-true not_run
src/lib.rs:9:11 gt-to-ge survived
src/lib.rs:9:13 int-increment killed
```
