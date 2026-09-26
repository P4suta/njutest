<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Deciding an execution

**Status: implemented.** One execution of one test process against one mutant comes to one verdict.
This page is the specification of that decision.
The engine's `outcome_of` is one implementation of it, held to every row by a test that asks it every combination of what it reads.
`cargo xtask engine-audit` is another: its `verdict` layer decides every execution a recording holds again, from what the recording says the engine decided it from, and refuses a verdict this table does not give.

## What the decision reads

- `stopped`: how the process came to an end, which is exactly one of `exited`, `timed-out`, `stalled`, `answered`, `cancelled`, `not-started`, `wait-failed`, `step-protocol-failed` and `step-limit-reached`.
  `answered` is the engine ending the process at the first test its harness said failed.
- `exit`, for a process that `exited`: `code-zero`, `code-other`, `self-signal`, `outside-signal` or `unknown`.
  A signal is `self-signal` when it is one a process raises by itself through what it does, which a mutation can make it do, and `outside-signal` when anything else sent it.
  Any other stop has no exit, which only a `*` matches.
- `harness`: `yes` when the target speaks libtest, which names every test's result and closes with a summary, and `no` when it answers by its exit code alone.
- `named`: `yes` when the harness named at least one test that failed.
- `summary`, the harness's closing line:
  `ran-nothing` when it counts no test passed and none failed;
  `clean` when it says `ok`, counts none failed, and something ran;
  `failing` for any other line;
  `none` when there was no line.
- `stale`: `yes` when the process refused, with the runtime's own exit code and its own words, to run against a catalog other than the one it was built with.

## The table

The first row that matches decides.
A cell matches the word it holds, any of several words separated by `, `, or anything at all when it holds `*`.

| stopped | exit | harness | named | summary | stale | verdict |
| --- | --- | --- | --- | --- | --- | --- |
| not-started, wait-failed, step-protocol-failed | * | * | * | * | * | errored |
| step-limit-reached | * | * | * | * | * | step_limit_reached |
| timed-out, stalled | * | yes | yes | * | * | killed |
| timed-out, stalled | * | yes | * | clean | * | survived |
| timed-out, stalled | * | * | * | * | * | waited |
| cancelled | * | * | * | * | * | not_run |
| answered | * | yes | yes | * | * | killed |
| answered | * | * | * | * | * | inconclusive |
| exited | * | * | * | * | yes | errored |
| exited | self-signal | * | * | * | * | killed |
| exited | outside-signal | yes | yes | * | * | killed |
| exited | outside-signal | * | * | * | * | inconclusive |
| exited | unknown | * | * | * | * | not_run |
| exited | code-other | * | * | * | * | killed |
| exited | code-zero | no | * | * | * | survived |
| exited | code-zero | yes | yes | * | * | killed |
| exited | code-zero | yes | * | clean | * | survived |
| exited | code-zero | * | * | * | * | inconclusive |

A test the harness named as failed noticed the mutation, so it is a kill wherever the harness could be heard, whatever the clock, a stop or the exit status did afterwards.
A survivor rests only on a clean summary, or on a zero exit from a harness that answers by its exit code.
A signal sent from outside, a clock, or a cancellation establishes something about the process and nothing about the tests.

## Before and after the table

- Before it: a libtest target whose output is not UTF-8 said nothing this reader can hold to its protocol, so the verdict is `errored` without consulting the table.
- After it: a `survived` verdict from a process that ran while its target was not held to its reach, or that may have run without the environment the run gave it, is `inconclusive`, because a pass nothing could see is no survivor.

## Signals a process raises by itself

<!-- self-raised-signals -->
- `SIGABRT`
- `SIGSEGV`
- `SIGBUS`
- `SIGILL`
- `SIGFPE`
- `SIGTRAP`
- `SIGSYS`
<!-- /self-raised-signals -->

Every one of them is what a process does to itself by aborting, faulting, trapping or making a call it may not.
Windows has no signals: a process there that faulted exits with a status code, which the table reads as `code-other`.
