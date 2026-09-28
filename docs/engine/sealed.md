<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Sealed execution

**Status: implemented, with the gaps below.** This page is the contract [ADR 0046](../adr/0046-a-verdict-is-what-a-sealed-run-observed.md) decided.
A run seals by default: it builds the instrumented tree for `wasm32-wasip1`, lists each module's tests and runs each one's control on the host, puts every mutant to the tests whose controls reached it, and writes the verdict they establish, with its `evidence`, into the report.
[The standing of a mutant](#the-standing-of-a-mutant) is `rust_mutants_decision::evidence::standing` and [the judgement of one execution](#judging-one-execution) is `rust_mutants_decision::judgement::judged`, both held to their rules by exhaustive comparisons and Kani.
`--no-seal`, or `[mutation] seal = false`, builds nothing for the sealed target, and then every answer is a lead.
Not yet: doctests are not sealed, so what only a doctest reaches is unproven; a sealed verdict is kept in the outcome store and read back under its key, but `verify` does not yet run a stored report's sealed executions again; a mutant that makes a test decline where its control did not is a doubt rather than the detection ADR 0043 makes it; an interrupt waits for the instance it lands in.

A verdict is what a sealed run observed.
A sealed run is the instrumented snapshot, built for `wasm32-wasip1`, with each test run alone in a fresh WebAssembly instance on a host that answers every question the same way every time.
Everything a native run observes is a lead: something a person may look into, never something the run concludes.

## A sealed run

The engine builds the instrumented snapshot a second time, with `cargo test --no-run --target wasm32-wasip1 --keep-going`, into its own target directory.
Each test binary becomes a WebAssembly command module.
Its tests are the ones `--list` names inside the host.

Each test of each module runs in its own instance, as `<module> --exact <name> --test-threads 1 --nocapture`.
The instance is new, the memory is the module's initial memory, the filesystem is the snapshot with nothing written, and the clock reads zero.
`--nocapture` keeps a panic's message on the stream the host records, where libtest's capture would hold it in memory the abort discards.

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
| `fd_prestat_get`, `fd_prestat_dir_name` | The snapshot's one preopened directory, at the path the build knew it by. |
| `fd_readdir` | Entries in name order, with cookies that are their positions. |
| `path_open`, `path_create_directory`, `path_remove_directory`, `path_rename`, `path_unlink_file` | Inside the preopened directory, on the overlay. A path outside it is refused, `ENOTCAPABLE`. |
| `path_readlink` | A link the snapshot holds. |
| `path_link`, `path_symlink` | Refused, `ENOTSUP`. |
| `sock_accept`, `sock_recv`, `sock_send`, `sock_shutdown` | Refused, `ENOTSUP`. |

The engine starts instances from one compiled module, which stays in memory; a run compiles each module once.
A wall-clock watchdog stands behind every instance in case the host itself stops, and what it stops is the run, with an error, never a test with a verdict.

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

## The standing of a mutant

`standing` reads whether the sealed build can answer for the mutant and the one execution of each test that reaches it, and decides:

1. If the sealed build does not hold the mutant's guard, it is unproven.
2. If any sealed execution detected it, it is killed, by the first that did.
3. Otherwise, every reason there is none is collected: a test that ran only natively, a test that reaches the mutant natively and is not in the sealed build, a test that reaches it natively whose sealed control does not reach its guard, and every doubted execution.
   If there is any, it is unproven, and it says all of them.
4. Otherwise, with no test reaching it, it is unreached; with every test that reaches it passed, it survived.

A survival is universal and a kill existential, as [ADR 0007](../adr/0007-survived-evidence-is-universal.md) says of every survival.
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
| A doctest | not a test module | nothing seals it yet |

A mutant that no sealed test can answer for is unproven, and its native lead is still reported.

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
`verify` runs a stored report's sealed executions again and compares; a stored report is affirmative only when they match.
A sealed result is cached by its digest.

## What sealing cannot see

- Memory corrupted through `unsafe`, which can write anything, including what the harness prints.
- Code that runs before `main`.
- A test that reads the environment that activates a mutant, and behaves differently because of it.
