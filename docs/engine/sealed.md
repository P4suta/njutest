<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Sealed execution

**Status: implemented, with the gaps below.** This page is the contract [ADR 0046](../adr/0046-a-verdict-is-what-a-sealed-run-observed.md) decided.
A run seals by default: it builds the instrumented tree for `wasm32-wasip1`, lists each module's tests and runs each one's control on the host, puts every mutant to the tests whose controls reached it, and writes the verdict they establish, with its `evidence`, into the report.
[The standing of a mutant](#the-standing-of-a-mutant) is `rust_mutants_decision::evidence::standing` and [the judgement of one execution](#judging-one-execution) is `rust_mutants_decision::judgement::judged`, both held to their rules by exhaustive comparisons and Kani.
`--no-seal`, or `[mutation] seal = false`, builds nothing for the sealed target, and then every answer is a lead.
A report `verify` stored is reissued only once every sealed execution it rests on has run again and come out the same, as [Reproducing a sealed verdict](#reproducing-a-sealed-verdict) says.
Not yet: a sealed verdict kept for one mutant, in the outcome store, in njutest's mutation evidence or among the answers carried across an edit, is still read back under its key without its executions running again.
Nor is a sealed result cached by its digest: a report records what each sealed execution came to rather than the digest of its transcript, so running one again compares what it came to.

A verdict is what a sealed run observed.
A sealed run is the instrumented snapshot, built for `wasm32-wasip1`, with each test run alone in a fresh WebAssembly instance on a host that answers every question the same way every time.
Everything a native run observes is a lead: something a person may look into, never something the run concludes.

## A sealed run

The engine builds the instrumented snapshot a second time, with `cargo test --no-run --target wasm32-wasip1 --keep-going`, into its own target directory.
Each test binary becomes a WebAssembly command module.
Its tests are the ones `--list` names inside the host, given the harness arguments the run was configured with.

Each test of each module runs in its own instance, as `<module> --exact <name> --test-threads=1 --nocapture` and then the harness arguments the run was configured with, less a thread count or a capture of their own, which libtest refuses to be given twice.
The instance is new, the memory is the module's initial memory, the filesystem is the snapshot with nothing written, and the clock reads zero.
Its working directory is its package's directory in the snapshot, where cargo runs a test and rustdoc a doctest, so a relative path reads what it reads natively.
A path the build baked in, such as `env!("CARGO_MANIFEST_DIR")`, reaches the snapshot as the build spelled it, a Windows build's `C:\…` included, as [the sealed host](sealed-host.md#the-working-directory) says.
`--nocapture` keeps a panic's message on the stream the host records, where libtest's capture would hold it in memory the abort discards.

A library's doctests seal too, as [Doctests](#doctests) says.

A target's sealed tests are exactly the ones its native baseline ran.
A test the native run does not run is no test of the suite's, so the sealed build's copy of it is left out, and a kill by it would be no kill of the suite's.
A test the native run ran that the sealed build does not hold, or holds only as one it ignores, is uncontrolled, and a route to it meets a test the sealed build cannot answer for.
A target whose baseline did not run, or whose harness's account does not name every test it ran, has no sealed tests at all.

The first execution of every test is its control: the test with no mutant active.
The control records what the test did — how it ended, the fuel it spent, the memory it held at most, the guards it touched, and every refusal the host made — and a mutant's execution of the same test is judged against it.

## The host

The host implements `wasi_snapshot_preview1` and nothing else.
A module that imports anything outside this table does not seal.
Every call charges fuel: a fixed cost, and a cost per byte it moves.
A refusal returns its error to the guest and is recorded in the transcript, so a failure that follows one can be told apart from a failure of the test.

| Import | What it does in a sealed run |
| --- | --- |
| `args_get`, `args_sizes_get` | The invocation's arguments, exactly. |
| `environ_get`, `environ_sizes_get` | The invocation's environment, exactly; nothing of the parent's. |
| `clock_res_get` | One nanosecond, for every clock. |
| `clock_time_get` | Virtual time: the fuel spent so far at a fixed rate, plus every sleep asked for. The realtime clock adds a fixed epoch. |
| `poll_oneoff` | A clock subscription advances virtual time to its deadline at once; a descriptor subscription is ready at once. |
| `sched_yield` | Succeeds. There is one thread. |
| `random_get` | The next bytes of a generator seeded by the invocation. |
| `proc_exit` | Ends the instance with that status. |
| `proc_raise` | Refused, `ENOSYS`. |
| `fd_write` | Standard output and error are captured up to a cap; bytes past it are counted, never dropped silently. Files write to the overlay. |
| `fd_read`, `fd_pread` | Standard input is empty. Files read the overlay over the snapshot. |
| `fd_pwrite`, `fd_seek`, `fd_tell` | On the overlay's copy of the file. |
| `fd_close`, `fd_renumber` | On the instance's descriptor table. |
| `fd_advise`, `fd_datasync`, `fd_sync` | Succeed; there is no disk. |
| `fd_allocate`, `fd_filestat_set_size` | Grow or shrink the overlay's copy. |
| `fd_fdstat_get`, `fd_fdstat_set_flags`, `fd_fdstat_set_rights` | On the instance's descriptor table. |
| `fd_filestat_get`, `path_filestat_get` | Fixed metadata: every timestamp one constant, the inode derived from the path, the size the overlay's. |
| `fd_filestat_set_times`, `path_filestat_set_times` | Recorded in the overlay and read back; a time never set reads as the constant. |
| `fd_prestat_get`, `fd_prestat_dir_name` | The snapshot, at the path the build knew it by; the directory the runtime's records land in; and `.`, the test's working directory in the snapshot. |
| `fd_readdir` | Entries in name order, with cookies that are their positions. |
| `path_open`, `path_create_directory`, `path_remove_directory`, `path_rename`, `path_unlink_file` | Inside the snapshot, on the overlay: a relative path from the working directory, an absolute one below the snapshot's root as the build spelled it. A path outside it is refused, `ENOTCAPABLE`. |
| `path_readlink` | A link the snapshot holds. |
| `path_link`, `path_symlink` | Refused, `ENOTSUP`. |
| `sock_accept`, `sock_recv`, `sock_send`, `sock_shutdown` | Refused, `ENOTSUP`. |

The engine starts instances from one compiled module, which stays in memory; a run compiles each module once.
A wall-clock watchdog stands behind every instance in case the host itself stops, and what it stops is the run, with an error, never a test with a verdict.
An interrupted run stops the instance it is in at the next epoch or host call, and every mutant it had not decided is `not_run` for `interrupted`, as a native run's are.

## Judging one execution

The host observes how the instance ended; the harness says what it thought happened.
Both are read, and they must agree.
`rust_mutants_decision::judgement::judged` is this table, held to every row by an exhaustive comparison and by Kani.

| The instance | The harness | The control | It is |
| --- | --- | --- | --- |
| `_start` returned | accounted for exactly the one test, passed | * | passed |
| trapped `unreachable` after a panic's message | * | * | detected: panicked |
| exited with libtest's failure status | named the one test failed | * | detected: failed |
| trapped deterministically for another reason, not a stack overflow | * | * | detected: trapped |
| spent its fuel | * | stayed within a bound it would not reach | detected: fuel exceeded |
| was refused memory | * | stayed within the limit | detected: memory exceeded |
| exited with status zero before the harness finished | * | * | doubted: exited early |
| overflowed its stack | * | * | doubted: stack overflow |
| failed after a refusal, or after the standard library's message for an unsupported operation or a failed allocation | * | met no such refusal | doubted: refused |
| anything else, or the harness disagreeing with what the host saw | * | * | doubted: unaccounted |

A panic's trap with the standard library's message for a failed allocation is a memory bound, not a panic.
A test that declines to measure ([ADR 0043](../adr/0043-a-test-may-decline-to-measure.md)) under the mutant, where its control measured, is detected: declined, whatever its ending, since the mutant changed what the test did.
A test whose control declined is still put every mutant its control reached, as a native run asks it: where it declines again in its control's words it measured nothing and is `set-aside`; in other words, it is detected: declined; where the mutant makes it measure instead, what it measured is the answer, but for a fuel or memory bound it runs past, which no control of its own set, so that is doubted: `unmatched`.

A doctest has no libtest account to ask: rustdoc's `main` runs it, and returns only where it returned, so the ending alone decides.
Its failure status is `ExitCode::FAILURE`, which `main` reports a doctest's error with.

| The instance | The doctest | It is |
| --- | --- | --- |
| `_start` returned | passes by returning | passed |
| exited with the failure status | passes by returning | detected: failed |
| trapped `unreachable` after a panic's message | passes by returning | detected: panicked |
| trapped deterministically for another reason, or was refused memory | passes by returning | detected, as the table above says |
| exited with the failure status, panicked, or trapped | should panic | passed |
| `_start` returned | should panic | detected: failed |
| was refused memory | should panic | doubted: refused, since the failure rests on the sandbox |
| spent its fuel | either | detected: fuel exceeded |
| exited with status zero, overflowed its stack, or exited with any other status | either | doubted, as the table above says |

## Doctests

rustdoc builds a library's doctests for the sealed target with `cargo test --doc --target wasm32-wasip1 -- --test-threads=1`, and runs every binary it built through the target's runner, which the engine sets to a capture of its own.
The capture keeps each binary under the next free claim, prints the claim, and fails, so that rustdoc's own report says which doctest each claim holds:

- A doctest compiled alone that fails with a claim printed is one that passes by returning, and the claim is its binary.
- A doctest compiled alone that passes although the capture failed it is one that should panic; its claim is the next in the order rustdoc ran them in, which the printed claims around it confirm.
- A doctest that fails with no claim did not build for the sealed target: it runs natively and has no sealed execution.
- A claim printed before rustdoc announces its own tests is a merged binary, which edition 2024 compiles every mergeable doctest into.
- A doctest rustdoc only compiles, `no_run` or `compile_fail`, or ignores, is not run natively, and has no binary.

A merged binary names its doctests when it runs them all in one instance, sorted by name as rustdoc indexes them, and runs one alone when `RUSTDOC_DOCTEST_RUN_NB_TEST` gives its index.
A doctest that fails on the sealed target aborts that instance inside itself, so the listing then names each doctest the binary finished and the one it stopped in, and nothing past it.
Before any of them counts, the index the binary announced as its count must be refused as naming no doctest, so a listing that left one out, or a binary built to leave some out, is caught.
The doctest the listing stopped in is then a test like the others, whose control fails, and a doctest past it is not held.
Each doctest is then a test like any other: its control records what it reached and spent, and a mutant is put to it alone.
A report that does not account for every claim the capture gave out, or a merged binary that does not name its doctests, leaves the documentation target unsealed.

## The standing of a mutant

`standing` reads whether the sealed build can answer for the mutant and the one execution of each test that reaches it, and decides:

1. If the sealed build does not hold the mutant's guard, it is unproven.
2. If any sealed execution detected it, it is killed, by the first that did.
3. Otherwise, every reason there is none is collected: a test that ran only natively, a test that reaches the mutant natively and is not in the sealed build, a test that reaches it natively whose sealed control does not reach its guard, and every doubted execution.
   If there is any, it is unproven, and it says all of them.
4. Otherwise, with no test reaching it, it is unreached; with every test that reaches it passed or set aside, and at least one passed, it survived; with every one set aside, nothing measured it, and it is unproven for `declined`.

A survival is universal and a kill existential, as [ADR 0007](../adr/0007-survived-evidence-is-universal.md) says of every survival; a test set aside for declining as its control did is left out of both, as [ADR 0043](../adr/0043-a-test-may-decline-to-measure.md) leaves it out of a native run's.
A native execution never contributes to a verdict: a mutant whose only executions are native is unproven, and says so.

## What cannot be sealed, and how to seal it

Each is found, not listed: from the build, from the module's imports, and from the control.

| Reason | Found by | To seal it |
| --- | --- | --- |
| The target is not installed | `rustc --print target-libdir --target wasm32-wasip1` names no directory | `rustup target add wasm32-wasip1` |
| The crate does not build for the target | the build's own error, per target with `--keep-going` | build its dependencies for `wasm32-wasip1`, or put what cannot build behind `cfg(not(target_family = "wasm"))` |
| The module imports what the host does not provide | the import section | the named import, which is not part of `wasi_snapshot_preview1` |
| A control does not pass sealed | the control | the control's own failure, which names the thread, process, socket or file it needed |
| The target has no libtest harness | `harness = false` in the build's own record of the target | give it libtest's harness, so that each of its tests says how it ended |
| The module's harness does not list its tests | `--list` does not close with libtest's count, or the count and the names disagree | the listing's own output, run on wasmtime |
| A `#[should_panic]` test | libtest reports it ignored | nothing seals it; its mutants rest on the other tests |
| A test the native run ran that the sealed build does not hold | the native baseline names it and the sealed listing does not, or lists it as ignored | build it for the target, or put the difference behind the same `cfg` on both sides |
| A target whose baseline did not run | `--no-verify` | let the run verify its baselines |
| A target whose baseline did not name every test it ran | its passed tests do not come to its summary's count | keep libtest's own report whole: a test that prints over it leaves its tests unnamed |
| A doctest that does not build for the target | rustdoc's report fails it with no claim printed | put what cannot build behind `cfg(not(target_family = "wasm"))`, or mark the example `ignore-wasm32` |
| rustdoc's report does not account for the capture | a claim the report does not name, one out of order, or a merged binary that does not name its doctests | run `cargo test --doc --target wasm32-wasip1` to see what rustdoc reported |

A mutant that no sealed test can answer for is unproven, and its native lead is still reported.
The report says of every target which of these it is, as `targets[].sealed`, and names each test the native baseline ran that has no sealed control and why; the run's own output lists them under `SEALED`, each reason once with what to do about it.

## Exit codes

| Code | When |
| --- | --- |
| 0 | Every mutant has a sealed detection or an accepted claim. |
| 1 | There is a finding, and nothing is unproven. |
| 2 | Something is unproven, whatever else was found. A native lead is always here, and so is a part of a divided catalog. |

A run that failed, or a command used wrongly, ends with a code of its own, which the command line's `--help` names.

## Reproducing a sealed verdict

A sealed execution's digest covers the module, the host's version, wasmtime's version, the arguments, the environment, the snapshot, the seed, the clock policy, the fuel and the limits.
The same digest gives the same transcript on any machine: fuel counts WebAssembly operators, not the host's instructions.
So a stored verdict is run again rather than trusted, and never replayed ([ADR 0003](../adr/0003-no-replay-engine.md)).

Before `njutest verify` reissues a report an earlier run of the same inputs stored, it runs every sealed execution the report rests on again, build by build, in the order they first ran.
Each configured build is prepared as a run prepares it, the sealed build included, by `Workspace::prepare_to_rerun`, whose `Rerunnable` does nothing but run recorded executions again.
It starts no test natively and measures no coverage: a recorded execution names its test, so no native baseline has to say which tests are the suite's, and no route has to choose them.
The module of each target the executions name is listed, the control of each test they name runs first, and each execution is then put and judged against that control, by `Bench::put` and `judged` as every sealed execution is.

The report is reissued only when every execution comes to what it recorded.
The first that does not, or that cannot be made again because its mutant, its guard, its target's module, its test's control or its control's reach of the mutant is not there now, is named with `NJ8006` — the mutant, the target, the test, what the report recorded and what it came to now — and the run establishes everything again.
A report that rests on no sealed execution, because every row is a lead or rests on no execution at all, affirms nothing sealed and is reissued as it is.
The trace records the running again as a `rerun` phase with a `sealed-exec` for each execution that ran, and a `rerun-unmade` note for one that could not be made.
What it costs is the preparation's builds, which the engine's build cache makes incremental, and one instance for each module listed, each control and each recorded execution; no test runs natively, no sentinel is planted, and nothing is routed.

## What sealing cannot see

- Memory corrupted through `unsafe`, which can write anything, including what the harness prints.
- Code that runs before `main`.
- A test that reads the environment that activates a mutant, and behaves differently because of it.
