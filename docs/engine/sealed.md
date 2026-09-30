<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Sealed execution

**Status: implemented, with the gap below.** This page is the contract [ADR 0046](../adr/0046-a-verdict-is-what-a-sealed-run-observed.md) decided.
A run seals by default: it builds the instrumented tree for `wasm32-wasip1`, lists each module's tests and runs each one's control on the host, puts every mutant to the tests whose controls reached it, and writes the verdict they establish, with its `evidence`, into the report.
[The standing of a mutant](#the-standing-of-a-mutant) is `rust_mutants_decision::evidence::standing` and [the judgement of one execution](#judging-one-execution) is `rust_mutants_decision::judgement::judged`, both held to their rules by exhaustive comparisons and Kani.
`--no-seal`, or `[mutation] seal = false`, builds nothing for the sealed target, and then every answer is a lead.
A report `verify` stored is reissued only once every sealed execution it rests on has run again and come out the same, and a sealed verdict kept for one mutant is believed only once its executions, put again on the run's own bench, come out the same, as [Reproducing a sealed verdict](#reproducing-a-sealed-verdict) says.
Not yet: a sealed result is cached by its digest nowhere: a report records what each sealed execution came to rather than the digest of its transcript, so running one again compares what it came to.

A verdict is what a sealed run observed.
A sealed run is the instrumented snapshot, built for `wasm32-wasip1`, with each test run alone in a fresh WebAssembly instance on a host that answers every question the same way every time.
Everything a native run observes is a lead: something a person may look into, never something the run concludes.

## A sealed run

The engine builds the instrumented snapshot a second time, with `cargo test --no-run --target wasm32-wasip1 --keep-going`, into its own target directory.
Each test binary becomes a WebAssembly command module, compiled and linked, besides the flags cargo would compile the tree for the target with, with `-C link-arg=--undefined=chdir -C link-arg=--export=chdir -C link-arg=--export=malloc`, so that the host can start it in a directory, and with what answers the standard library's temporary and home directories, as [The platform the standard library does not have](#the-platform-the-standard-library-does-not-have) says.
Those are what the environment's `CARGO_ENCODED_RUSTFLAGS` or `RUSTFLAGS` give, and otherwise what the configuration gives the target, under its triple or a `cfg(…)` predicate that holds for it, and otherwise `build.rustflags`; rustdoc's flags are chosen the same way.
Where which of them cargo would use is not known, because a configuration file could not be read or a predicate names what a target alone does not decide, nothing is sealed, `flags-unmerged`.
Its tests are the ones `--list` names inside the host, given the harness arguments the run was configured with.

Each test of each module runs in its own instance, as `<module> --exact <name> --test-threads=1 --nocapture` and then the harness arguments the run was configured with, less a thread count or a capture of their own, which libtest refuses to be given twice.
The instance is new, the memory is the module's initial memory, the filesystem is the snapshot with nothing written, and the clock reads zero.
It starts in its package's directory in the snapshot, where cargo runs a test and rustdoc a doctest: the host enters it through the guest's own `chdir` before `_start`, so a relative path reads what it reads natively, and `std::env::current_dir()` names it.
What a test writes for itself lands in directories the instance holds, each empty when it starts and written only to its overlay: `CARGO_TARGET_TMPDIR` at the path the build baked in, which for the sealed target is the target's own directory inside the target directory, and a home and a temporary directory, which `HOME` and `TMPDIR` name.
What a target's build script wrote, its `OUT_DIR`, the instance holds as the build left it, at the path the build gave the target, so a test that reads it back at run time, through `std::env::var("OUT_DIR")` or `env!("OUT_DIR")`, reads what it reads natively; what a test writes there only its overlay holds, and the build's own directory is never touched.
Each of these names reaches something sealed on every machine, and the same thing on each: the paths of `HOME`, `TMPDIR` and the scratch are the instance's own, and those of `CARGO_TARGET_TMPDIR` and `OUT_DIR` are the ones the build gave the target, read or made empty when the bench is assembled.
`std::env::temp_dir()` and `std::env::home_dir()` answer what `TMPDIR` and `HOME` name, as they do on a POSIX system, so code that keeps its files in either works sealed, and what it made is gone with the instance.
A panic of the standard library's own platform layer, `library/std/src/sys/`, is a refusal of the sandbox rather than something the test observed: a test whose control fails after it has no control, for `refused`, and an execution that meets it where its control did not is doubted, `refused`, never a detection, since the same code passes natively.
A path the build baked in, such as `env!("CARGO_MANIFEST_DIR")`, reaches the snapshot as the build spelled it, a Windows build's `C:\…` included, as [the sealed host](sealed-host.md#the-root-and-the-start) says.
Any other absolute path names no directory the instance holds, and is refused as an escape and recorded, never read from the package's directory, as `fixtures/fixture-absolute-path` shows: the guest's root directory reaches only what the instance holds.
`--nocapture` keeps a panic's message on the stream the host records, where libtest's capture would hold it in memory the abort discards.

A library's doctests seal too, as [Doctests](#doctests) says.

### The platform the standard library does not have

The standard library of `wasm32-wasip1` has no temporary or home directory of its own: its `std::env::temp_dir` panics in its platform layer whatever `TMPDIR` names, and its `std::env::home_dir` answers no home at all, an answer the compiler copies into each crate that calls it.
So the sealed build gives it one.
It compiles an object of its own with the run's toolchain, whose two functions answer from `TMPDIR` and `HOME` as the standard library of a POSIX system does, and links it into every sealed module and doctest, exporting both.
It compiles every module without optimisation and keeps every function's name, `-C opt-level=0 -C strip=none`, which changes no program's meaning, so that no copy of the two is folded into its caller.
Before an instance starts, the host rewrites every function the module's name section names `std::env::temp_dir` or `std::env::home_dir`, the standard library's own and each copy, to answer by calling the object's; a module that names one and does not export the object's is refused (`RS1007`).
The toolchain is probed first: a program that prints the two directories, compiled and linked as a sealed module is, rewritten and run sealed with `TMPDIR` and `HOME` named, must print them.
Where the object does not build or the probe does not answer, nothing is sealed, `platform-unanswered`, and the trace says why; the object and the probe's answer are kept under the sealed build's directory, under the toolchain's version and everything they are made of.
`fixtures/fixture-temporary` and `fixtures/fixture-home` show both.

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
| `fd_fdstat_get`, `fd_fdstat_set_flags`, `fd_fdstat_set_rights` | On the instance's descriptor table, whose rights every call they govern asks for: a right narrowed away is gone, and opening through a directory does not give it back. |
| `fd_filestat_get`, `path_filestat_get` | Fixed metadata: every timestamp what the realtime clock reads when the instance starts, the inode derived from the path, the size the overlay's. |
| `fd_filestat_set_times`, `path_filestat_set_times` | Recorded in the overlay and read back; a time never set reads as the instance's start. |
| `fd_prestat_get`, `fd_prestat_dir_name` | The snapshot, at the path the build knew it by; the directory the runtime's records land in; a scratch directory of the instance's own, whose empty `home` and `tmp` are what `HOME` and `TMPDIR` name; for a target cargo gives one, an empty directory at the `CARGO_TARGET_TMPDIR` the build baked in; for a target whose package has a build script, what the script wrote, at the `OUT_DIR` the build gave the target; and `/`, the root, which reaches each of these by its own spelling of an absolute path and refuses every other. |
| `fd_readdir` | Entries in name order, with cookies that are their positions. |
| `path_open`, `path_create_directory`, `path_remove_directory`, `path_rename`, `path_unlink_file` | Inside the snapshot, on the overlay: a relative path from the package's directory the test started in, an absolute one below the snapshot's root as the build spelled it. A path outside it is refused, `ENOTCAPABLE`. A name its directory holds only in another case is refused, `ENOTCAPABLE`, since a file system that compares names in either case would have answered it from that name. |
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
| failed after a refusal, or after the standard library's message for an unsupported operation or a failed allocation, or a panic of its platform layer | * | met no such refusal | doubted: refused |
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

rustdoc builds a library's doctests for the sealed target with `cargo test --doc --target wasm32-wasip1 -- --test-threads=1 --list`, and hands every merged binary it builds to the target's runner, which the engine sets to a capture of its own.
A merged binary takes no command line: `--list` is baked into it at compile time, so it lists the doctests it holds when it runs, and rustdoc, which cannot run the doctests it did not merge through a listing, lists those itself instead of running them.

The capture keeps each merged binary under the next free claim, prints the claim, and fails.
A claim printed before rustdoc lists anything of its own is a merged binary, and each name rustdoc lists itself, closed by its count, is a doctest compiled alone.
The library is built again without `--list` where rustdoc named any doctest itself: rustdoc then runs each alone binary through the capture, which claims and fails it, so a doctest that passes although the capture failed it is one that should panic, and one that fails with no claim did not build for the sealed target and runs natively.
Where the first build holds merged binaries, it is built once more with `--list --ignored`: each binary so built, run the same way, lists the doctests it holds that the sealed target ignores.

A merged binary names its doctests when it runs with no index, its harness's own listing, in the order it holds them, whatever any of them does when it runs, and runs one alone when `RUSTDOC_DOCTEST_RUN_NB_TEST` gives its index.
Before any of them counts, the index past the last the binary listed must be refused as naming no doctest, so a listing that left one out, or a binary built to leave some out, is caught.
What each doctest passes by comes from the native run, whose harness names the tests that should panic; its listing says nothing of it.
A doctest the sealed target ignores, which the listing of the `--ignored` build names, runs as nothing when asked for by index, so the station holds none of them: a route to the mutant it alone reaches meets a test the sealed build cannot answer for.
A run again of a stored report has no native run to ask what each doctest passes by, so it judges every merged doctest by returning, and one that should panic fails its control there — a difference the re-run names, and the run establishes afresh.
Each doctest is then a test like any other: its control records what it reached and spent, and a mutant is put to it alone.
A report that does not account for every claim the capture gave out, or a merged binary that does not list its doctests, leaves the documentation target unsealed.

## The standing of a mutant

`standing` reads whether the sealed build can answer for the mutant and the one execution of each test that reaches it, and decides:

1. If the sealed build does not hold the mutant's guard, it is unproven.
2. If any sealed execution detected it, it is killed, by the first that did.
3. Otherwise, every reason there is none is collected: a test that ran only natively, a test that reaches the mutant natively and is not in the sealed build, a test that reaches it natively whose sealed control does not reach its guard, and every doubted execution.
   If there is any, it is unproven, and it says all of them.
4. Otherwise, with no test reaching it, it is unreached; with every test that reaches it passed or set aside, and at least one passed, it survived; with every one set aside, nothing measured it, and it is unproven for `declined`.

A survival is universal and a kill existential, as [ADR 0007](../adr/0007-survived-evidence-is-universal.md) says of every survival; a test set aside for declining as its control did is left out of both, as [ADR 0043](../adr/0043-a-test-may-decline-to-measure.md) leaves it out of a native run's.
A native execution never contributes to a verdict: a mutant whose only executions are native is unproven, and says so.

"No test reaching it" is what the sealed controls say, which the trace records as one `sealed-control` event per test of every station, with every guard it reached.
A target a native route names without naming its tests, as the documentation target is named for every library mutant because rustdoc's processes cannot be attributed natively, is answered by its station's controls: where every one of them is controlled and none reached the guard, the mutant is unreached, whatever the native route's granularity.
The engine audit and proofaudit hold a sealed unreached row to those records: no recorded control reached its guard, and every target its native route names has a station every control of which was controlled.

## What cannot be sealed, and how to seal it

Each is found, not listed: from the build, from the module's imports, and from the control.

| Reason | Found by | To seal it |
| --- | --- | --- |
| The target is not installed | `rustc --print target-libdir --target wasm32-wasip1` names no directory | `rustup target add wasm32-wasip1` |
| The toolchain's standard library does not answer its temporary and home directories from the environment, `platform-unanswered` | the object that answers them does not build with the run's toolchain, or the probe linked with it does not print what `TMPDIR` and `HOME` name | the trace's `sealed-build` note names what the object or the probe said |
| Which flags cargo compiles the sealed target with is not known, `flags-unmerged` | a cargo configuration file that does not read, or a `target.'cfg(…)'` table whose predicate names what a target alone does not decide | configure the flags of `wasm32-wasip1` under its own triple, or in `build.rustflags` |
| The crate does not build for the target | the build's own error, per target with `--keep-going` | build its dependencies for `wasm32-wasip1`, or put what cannot build behind `cfg(not(target_family = "wasm"))` |
| The module imports what the host does not provide | the import section | the named import, which is not part of `wasi_snapshot_preview1` |
| A control does not pass sealed | the control | the control's own failure, which names the thread, process, socket or file it needed |
| A control fails after a refusal of the sandbox, `refused` | the host refused one of its calls, or the standard library printed its message for one, or panicked in its platform layer | what the transcript names it asked for: a socket, a signal, a link, a path outside the tree or named only in another case, or `std::env::temp_dir()`, whose place `TMPDIR` names |
| The target has no libtest harness | `harness = false` in the build's own record of the target | give it libtest's harness, so that each of its tests says how it ended |
| The module's harness does not list its tests | `--list` does not close with libtest's count, or the count and the names disagree | the listing's own output, run on wasmtime |
| A `#[should_panic]` test | libtest reports it ignored | nothing seals it; its mutants rest on the other tests |
| A test the native run ran that the sealed build does not hold | the native baseline names it and the sealed listing does not, or lists it as ignored | build it for the target, or put the difference behind the same `cfg` on both sides |
| A target whose baseline did not run | `--no-verify` | let the run verify its baselines |
| A target whose baseline did not name every test it ran | its passed tests do not come to its summary's count | keep libtest's own report whole: a test that prints over it leaves its tests unnamed |
| A doctest that does not build for the target | rustdoc's report fails it with no claim printed | put what cannot build behind `cfg(not(target_family = "wasm"))`, or mark the example `ignore-wasm32` |
| rustdoc's report does not account for the capture | a claim the report does not name, one out of order, or a merged binary that does not list its doctests | run `cargo test --doc --target wasm32-wasip1 -- --list` to see what rustdoc reported |

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

A sealed verdict kept for one mutant is run again too, on the bench the run assembles anyway.
The engine's outcome store, njutest's store of mutation answers and njutest's checkpoint of an interrupted run each keep, beside a verdict, the sealed executions it rests on, and a run that reads one back puts the mutant again to its own bench, through `Bench::put` and `judged` as every sealed execution goes, by `rust_mutants::run::sealed_again`.
The kept verdict is believed, or the kept kill inherited, only where the executions come to what it recorded, in the order they ran, and establish again the verdict it records: a kept verdict whose own executions, each coming out as kept, establish another is a record that contradicts itself, and is not believed either.
The comparison is over the whole sequence: an execution the record left out, one it names that the bench cannot make again, because its target has no station, its test no control or its control no reach of the mutant, and a survival that a test now reaching the mutant natively leaves unproven all part from it.
Where they part, the mutant is established afresh from those same executions, never read back, and the trace carries an `unreproduced` note naming the first execution that parted: its place, its target and test, what it was kept as and what it came to now, or the verdict they establish where the one kept is another.
A run that seals nothing cannot make a kept execution again, so it establishes the mutant natively, and what it says is a lead.
Believing a kept verdict costs one fresh answer: the executions a fresh answer makes are the ones compared, one instance each, and nothing is built for them that the run does not build anyway.
So a kept sealed verdict no longer saves an instance; it says which run first established the verdict.
On fixture-simple, a `verify` whose whole report is gone, so that each of its ten mutations asks the store, and a `verify --no-cache` of the same tree each assembled the same bench, put the mutations to 10 sealed executions, and ran the same 8 cargo invocations; the first believed the 9 kills it had kept, each after its one execution came out the same, and established the survivor, which it had not kept.
Trusting the 9 kept kills, as a run did before, skipped those 9 executions.
A kept lead is not a verdict, so nothing is run again for it: sealing is tried first, as for a mutant nothing was kept about.
A sealed verdict njutest finds resting on a target whose native reach moved is put again the same way, by `rust_mutants::run::sealed_along`, with that target counted among the ones reaching it natively and nothing recorded a second time, and the verdict that put establishes, or the lead it leaves, replaces the one that rested on the moved record ([ADR 0036](../adr/0036-what-rested-on-a-moved-reach-is-run-again.md)).
An answer carried across an edit ([carrying an answer](carry.md)) rests on native executions, so it is always such a lead.

## Crashes

A run asked for crashes ([ADR 0035](../adr/0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)) seals its crash session as it seals its mutation session, and puts a crash to a test in a sealed instance wherever the bench answers for that test at the crash's guard: its station holds the test, and the test's control passed and reached the guard.
The instance runs with the crash active, and the runtime told to publish its notice at `/rust-mutants-sealed/crash-notice`, a file of the records directory the test never sees, under a nonce issued to that instance alone.
The runtime writes the notice beside that path and renames it into place, and the invocation's `halt` names the path, so the host ends the instance in that rename as `Halted` ([the sealed host](sealed-host.md#how-a-run-ends)).
Nothing the guest would have run after the call runs: no destructor, no exit handler, and no flush of a buffer that still holds bytes.
The engine says the instance stopped at the call only where the host halted it and the notice the overlay holds is exactly the one it issued; `Session::crash_sealed` makes that decision, and `Bench::crash` the instance.

What the stop left is every change of its overlay outside the runtime's records, named as a native stop's are: below the temporary directory relative to it, below the home from `~/`, below the tree from `./`, below `CARGO_TARGET_TMPDIR` from `$CARGO_TARGET_TMPDIR/`, a directory with a trailing `/` and a removal with ` (removed)` after it.
The next instance is the test with nothing active, in a fresh instance whose trees are those changes applied to the instance's own snapshots, by `Preopens::after`, with a records directory of its own; `Bench::after` runs it and judges it against the test's control, as [Judging one execution](#judging-one-execution) says.
What the stop left can be no state an instance of the test starts in, as one is that removed the directory the guest starts in: the next instance is not run, and the crash is `undecided`, recorded as `unstartable`, since what the tree then holds says nothing of whether the program could start over it.
It passing is `restarted`, a detection is `corrupt`, with the test as the one failure, and anything else is `undecided`, since it establishes nothing either way.
One round decides: the control is the fresh run a native round asks for, and the same crash comes out the same every time, so there is no second stop to confirm.

A test with no sealed control, one that starts a process, a thread or a socket, or one the sealed build does not hold, is put the crash natively, as before, in three rounds.
The report's crash record says `sealed: true` exactly where the decision rests on at least one run and every run it rests on was a sealed instance, and the recording's `crash-exec` says of each run whether it was one ([trace v1](../trace-v1.md)).

## What sealing cannot see

- Memory corrupted through `unsafe`, which can write anything, including what the harness prints.
- Code that runs before `main`.
- A test that reads the environment that activates a mutant, and behaves differently because of it.
- A function of the standard library whose platform layer on `wasm32-wasip1` answers another way than a native one without a panic, such as `std::env::current_exe` or `std::thread::available_parallelism`, which return an error of their own: a test that meets the error and goes on takes another path than it does natively, and nothing records it, where the two directories are answered as natively.
