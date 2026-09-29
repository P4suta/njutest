<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0046 — A verdict is what a sealed run observed

## Status

Accepted, 2026-09-29 (user decision).
Specified by [the sealed execution page](../engine/sealed.md), whose Status says which parts exist.
Implemented so far by `rust_mutants_decision::evidence::standing`, the one place a mutant's standing is decided, and its laws: the exhaustive comparison in `crates/rust-mutants-decision/src/evidence/tests.rs` and the Kani harnesses beside it.
njutest's mutation phase decides each mutation from its sealed executions first, and every row of its report carries what it rests on, which `report::RowVerdict` alone reads ([report v1](../report-v1.md#what-a-decision-rests-on)).
Decision 7's re-run is `rust_mutants::session::Rerunnable`, a preparation with no native baseline that runs recorded executions again through `Bench::put`, and `njutest::assure::rerun`, which `verify` asks before it reissues a stored report; a sealed verdict kept for one mutant, in the engine's outcome store, njutest's store of mutation answers or its checkpoint, is believed only once `rust_mutants::run::sealed_again` has put the mutant again on the run's own bench and its executions came out the same; a sealed result is not yet cached by its digest.
Amends [ADR 0007](0007-survived-evidence-is-universal.md), [ADR 0014](0014-the-guards-are-the-measurement.md), [ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md), [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md), [ADR 0026](0026-a-bound-measures-quiet-not-duration.md), [ADR 0034](0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md) and [ADR 0035](0035-a-crash-is-a-stop-the-next-run-has-to-survive.md).

## Context

A native execution is a process that reports on itself.
Libtest prints what passed and what failed, and the process exits with a status; the engine reads both.
Everything the engine concludes rests on that report, and the process under test writes it.
A test that calls `std::process::exit(0)` before its assertions ends with the status of a pass.
A child process a test starts can print a libtest line into the stream the engine reads.
A test that fails without the mutant fails with it too, and the failure reads as a kill.
A bound on the wall clock depends on how busy the machine was.

On 2026-09-27 the user decided that no affirmative verdict rests on that report until a control channel proves the run completed, that a kill read from a non-zero exit is unproven for the same reason, and that a state in which a run is not sealed should not exist at all.
The user added that the engine should go all the way into WebAssembly if that is the way, and that nothing needs to be kept for compatibility, since nothing has been released.

A first attempt, never landed, sealed a closed integer program on `wasm32-unknown-unknown`.
It could prove its verdicts because it measured nothing a user has.
On that target `exit(0)` and a panic reach the parent as the same trap, so no failing test could ever count as a detection, and a detection had to be a different return value.

`wasm32-wasip1` separates exactly what that target could not.
wasi-libc's `_start` returns only when `main` returned zero, and libtest's `main` returns zero only when every test it ran passed.
`exit(n)` is a call to the host, `proc_exit(n)`, which the host sees.
A panic under `panic = "abort"`, the only strategy the target has, is an `unreachable` trap, which the host sees.
Libtest on a wasm target runs each test on the calling thread and reports a `#[should_panic]` test as ignored.

## Decision

1. **A verdict is what a sealed run observed.** `Verdict` can only be made by `standing`, and `standing` makes one only from sealed executions.
   A native execution yields `Unproven`, and what it observed is reported as a lead: a kill or a survival a person may look into, never one the run concludes.
2. **A sealed run** builds the instrumented snapshot with `cargo test --no-run --target wasm32-wasip1` and runs each test in a fresh wasmtime instance of its test module, as `--exact <name> --test-threads 1 --nocapture`, on the engine's own WASI preview1 host.
   The host is deterministic by construction: clocks advance with the fuel spent and with the sleeps the guest asks for, randomness comes from a seed, the filesystem is a read-only snapshot with a write overlay per instance, directories list in name order, metadata is fixed, sockets are refused, and output is captured.
   A run is bounded by fuel and by a memory limit, never by the wall clock.
3. **Completion is observed, not reported.** A test passed when its instance's `_start` returned and its harness accounted for exactly that test as passed.
   It detected the mutant when it panicked, when its harness exited with the failure status, when its instance trapped for another reason the host decides deterministically, or when it exceeded a fuel or memory bound its matched control, the same test without the mutant, stayed within.
   The matched control is the premise [ADR 0023](0023-a-run-may-not-conclude-from-how-it-measured.md) asks of a bound, and determinism supplies it.
   A test that ended its instance with status zero before its harness finished established nothing, and neither did a stack overflow, whose depth the host's native frames decide.
   A failure that followed a refusal of the sandbox the control did not meet, or one of the pinned standard library's messages for an unsupported operation or a failed allocation, established nothing either: it is how the run measured.
4. **The sealed build answers only where it is the same program.** It is another compilation, with other `cfg` values, a 32-bit `usize` and another stack.
   A mutant is judged only when the sealed build holds its guard and every test that reaches it natively; otherwise it is unproven.
5. **Kill is existential and survival universal** ([ADR 0007](0007-survived-evidence-is-universal.md)): one sealed detection kills, and a mutant survives only when every test that reaches it passed sealed.
6. **Exit codes.** 0 when every mutant has a sealed detection or an accepted claim.
   1 when there is a finding and nothing is unproven.
   2 when something is unproven, whatever else was found; a native lead is always here.
   A run that failed, or a command used wrongly, ends with a code of its own.
   A part of a divided catalog concludes `PARTIAL`, which is 2.
7. **A sealed verdict is reproducible, so it is re-run rather than trusted.** Its digest covers the module, the host's and wasmtime's versions, the arguments, the environment, the snapshot, the seed, the clock policy, the fuel and the limits.
   `verify` re-runs a stored report's sealed executions ([ADR 0003](0003-no-replay-engine.md): re-run, never replay), and a stored report is affirmative only when they come out the same.
   A sealed result is cached by its digest, because it is a function of it.
8. **What cannot be sealed is derived, never listed.** A build that fails for the target, a module importing what the host does not provide, a control that does not pass sealed: each is a typed reason, and each reason says how to seal what it names.
9. **The host is the engine's own.** `wasmtime-wasi` sleeps in real time, shows the host's metadata and directory order, writes through to the disk, and has no virtual filesystem; `wasi-common` is deprecated.
   Every `wasi_snapshot_preview1` import is one row of a table in the host crate, with its deterministic meaning.
10. **What sealing cannot see, said once:** memory corrupted through `unsafe`, code that runs before `main`, and a test that reads the environment that activates a mutant and behaves differently because of it.

## Consequences

- Sealing needs the standard library of `wasm32-wasip1`, a target of the toolchain a user already has (`rustup target add wasm32-wasip1`), as coverage needs `llvm-tools`.
  The engine finds it with `rustc --print target-libdir --target wasm32-wasip1` and names the command when it is missing.
- One instance per test replaces one process per target and routed test set.
  A pre-linked instance with copy-on-write memory starts in microseconds, where a process start costs milliseconds, so the arithmetic [ADR 0018](0018-the-assurance-layer-rides-the-standard-interfaces.md) did for the schemata form changes sign: isolating every test is now the cheaper shape.
- A test that uses threads, processes or sockets does not seal; its mutants are unproven with that reason, and their native leads are still reported.
  A `#[should_panic]` test does not seal either, because libtest ignores it on this target.
- A sealed test binary runs one thread by construction, so the schedule dimension ([ADR 0034](0034-a-binary-is-single-threaded-only-where-nothing-says-otherwise.md)) has nothing to explore there.
- A flaky failure read as the mutant's kill, the concern of the draft ADR 0040, cannot arise in a sealed run: one test per instance, and the same instance the same way every time.
- Deterministic crash evidence needs one confirming round, where [ADR 0035](0035-a-crash-is-a-stop-the-next-run-has-to-survive.md) asked for three: the next instance starts from the crashed one's overlay.
- A sealed run's bound is fuel alone; the quiet window of [ADR 0026](0026-a-bound-measures-quiet-not-duration.md) and the thread names [ADR 0014](0014-the-guards-are-the-measurement.md) attributes reach by stay with native runs, where they produce leads.
- The native pipeline stays.
  It is where a mutant that cannot be sealed is still observed, and where a lead comes from.
