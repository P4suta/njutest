<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Current limitations and deferred work

**Status: implemented.** Every name here is one a report can carry; the list grows with each release, and every limitation
below is stated fail-closed.

- A tree whose identity could not be computed reuses nothing
  (`workspace-digest-not-computed`) and is reused by nothing: a run that
  cannot say what it looked at cannot answer for another run's inputs.
- A mutant every measured target was asked about and none of them reaches is
  reported as surviving, with a detail that says nothing executes it and a
  route that names the targets that were in a position to notice and did not.
  Where that evidence is not there — nothing was recorded at all, the position
  is outside every instrumented region, the catalog could not place it, a
  target that was measured carries no profile, or a target's guards recorded
  nothing this run can route by — the route widens to every test of every
  target instead (`not-measured`, `outside-blocks`, `position-unknown`,
  `coverage-incomplete`, `touch-incomplete`). Both readings are one finding,
  because they are one gap in the suite.
- A target is a test binary, and a route names which of its tests a mutation is
  put to. A target whose every test is `#[ignore]`d and one that printed no
  result at all are both a target that executed nothing, and this release
  reports both as missing: which of the two it was does not cross the boundary
  between the engine and this runner yet, and a target nothing is known about
  is a finding rather than a pass.
- A test that writes into the tree while it is being measured makes every
  later mutation a measurement of what it wrote
  (`tree-written-during-measurement`). One instrumented snapshot cannot
  isolate that the way a per-mutant build would; the run says so rather than
  reporting the later results as if it had.

## Decided in advance

- Doctests are run as one target per library, switched off with
  `[execution] doctests = false` or `--no-doctests`, and mutations are routed
  to them at file granularity (`doctests-routed-by-file`): a documented example
  reaches every mutation in the files its library is made of and narrows none
  of them. A kill one finds names the library's documentation and not the
  example, because rustdoc merges a file's examples into one compilation whose
  harness cannot be asked for one of them.
- A `harness = false` test target says what it found by exiting rather than by
  printing a summary, so how many of its tests ran is unknown and the whole
  binary is measured (`custom-harness`).
- A library that documents no example has nothing to run and is not a target
  (`doctests-none`): a target that ran nothing would raise a finding about
  documentation nobody wrote.
- **A test that starts this engine cannot be measured from inside a run of it.**
  A run composes an activation of its own and refuses to inherit one
  (`RM0006`), which is what stops a nested process from answering about the
  wrong catalog. A test that spawns `rust-mutants` therefore fails
  verification under measurement, and the run refuses to judge against it. Such
  a target is left out with `--skip-target` or `[execution] skip_targets`, and
  `target-skipped-by-configuration` says so. What that target's tests would
  have killed is a survivor for as long as it is left out, so a reader working
  through survivors of a run that skipped targets has to hold that in mind: the
  survivor may be a gap in the suite, or it may be the target that was not
  asked. [The roadmap](roadmap.md) argues that this one is worth revisiting:
  where the catalog a nested process inherits is its own, the binary is the
  mutant the outer run activated, and the guards already refuse every other
  case.
- **A guard against a failure that reappears downstream is a survivor no
  deterministic suite can decide.** A `?` on an I/O call whose failure is
  reported again by a later call on the same resource - a write that is put
  back by a restore of the same path, a lock taken twice, a file opened again -
  hands its caller the same error whether the guard is there or not, and leaves
  the same state behind. The two versions part only when the condition is
  transient, and a test that can arrange a transient condition is a test that
  has stopped being deterministic. Such a mutation is not equivalent: it is a
  fail-open the suite cannot reach. Recording it as an expectation is right,
  and the reason has to say which of the two it is, because a later reader
  deciding whether to delete the guard needs to know that the run never
  disagreed with it rather than that the run agreed.
- A target whose own tests do not pass with nothing active is dropped from the
  ones a mutation may be put to (`baseline-not-passing`), and every one of them
  is named. Every mutation put to such a target comes back killed and not one
  of those kills is about a mutation. The run still reports the table, because
  the moment a reader most needs it is the moment the answer is "all of them".
- A target whose guards recorded nothing this run can route by keeps every test
  of it in every route (`touch-not-recorded`), and one whose record did not
  read back is believed about nothing (`touch-log-unreadable`).
- Fuzz targets are found always and driven only when `[fuzz] run` says so;
  a tree that holds targets nobody asked to drive carries
  `fuzz-not-executed`. Without cargo-fuzz on a nightly toolchain, a run that
  was asked to drive them carries `cargo-fuzz-unavailable` and a
  `not-measured` finding instead.
- `standard-v1` does not execute anything about `unsafe` code; it
  inventories it and says so (`soundness-not-executed`). `deep-v1` interprets
  the suite under Miri and refuses to run at all without it (`MJ7001`), and
  what Miri will not interpret is `miri-unsupported` rather than a pass. A
  sanitizer the configuration asks for and the toolchain will not run is
  `sanitizer-unavailable`; every sanitizer run also carries
  `sanitizer-standard-library-not-instrumented`.
- Repository reads are not observed at run time. A package that uses a
  directory-reading API keys the whole snapshot for evidence reuse; nothing
  is excluded from testing.
- Mutation inside a macro invocation the engine has no allowlist entry for, a
  `const` context, and `#[cfg]`-guarded code is skipped with a stated reason.
  The leading arguments of the six assertion macros are mutated; the rest of
  every invocation is `macro-invocation`.
- A `#![no_std]` crate is measured: the runtime borrows `std` under a name of
  its own. A crate the host cannot lend `std` to — one supplying a
  `#[panic_handler]`, a `#[global_allocator]`, or `#![no_main]`, or written in
  edition 2015 — is `no-std-crate`.
- A file another file pastes in with `include!` where an expression goes is
  `included-expression`: it is a fragment rather than a program.
- The equivalence layer is off by default and proves almost nothing on a
  project that leaves `[profile.test] opt-level` at cargo's default of zero,
  where two mutations the compiler would render identically at any
  optimisation level are still two different sets of instructions. It is a
  fact about the profile the tests run under rather than about the mutation,
  and the layer answers the question the tests ask rather than an easier one.
- A procedural macro's own unit tests are measured like any others; what it
  expands to is not (`proc-macro-expansion-not-measured`). A macro decides that
  during the build and a mutation is activated for a test process, so the two
  never meet, and cargo does not rebuild for an environment variable. A
  mutation only the expansion would change is reported as surviving.
- Coverage routing says nothing about a place its export never instrumented,
  and such a mutant is run everywhere. A tree that configures `rustflags` for
  a target — `[target.<triple>]` or `[target.cfg(…)]` in a cargo configuration
  file — is not routed by coverage at all
  (`coverage-refused-configured-rustflags`), because which of those tables
  apply is cargo's decision about the target being built and a guess would
  compile something other than the project's own binaries. A `[build]
  rustflags` is read instead and put back into the coverage build, so a tree
  that configures one is measured. A configuration file that cannot be parsed
  leaves what the tree compiles with unknown
  (`cargo-configuration-unreadable`), and is not routed by coverage either;
  cargo itself normally refuses such a tree first, with `RM1014` naming the
  file. Three other things leave the measurement empty and
  route every mutant to every target, exactly as if the layer were not there:
  a tree that will not build with instrumentation
  (`coverage-build-failed`), a toolchain whose LLVM tools are not installed
  (`coverage-tools-missing`), and tools that ran and produced nothing a run
  could route by (`coverage-not-measured`). The last of these is also stated
  per target, as `coverage-not-measured:<target id>`, when one target's
  profile could not be read: that target is kept in every route rather than
  dropped, because what the measurement says nothing about is run rather than
  assumed.
- A guard compares its two branches only where the compiler vouched that the
  condition around it is inert — every comparison in it between primitives,
  `str`, or a slice of those — and the guard's own form can hold the call. The
  trait names the types whose comparison the standard library defines and a
  container of one of those, and asks about the two operands separately, so a
  comparison between two different ones is vouched for where both are named. A
  type of your own is refused whichever side it is on, and so is a container
  holding one.
- A swap between `==` and `!=` is never offered the comparison. The two are
  each other's negation, so a guard between them parts on every evaluation and
  the record could only ever say that it did.
  Everywhere else `never-infected` says nothing about the mutation however
  many tests ran it, and the record names which mutants it can speak about so
  that its silence about the rest is never read as evidence.
- A return replacement is asked whether the value it overwrites already held
  what it would write, and only where two things hold. Evaluating the value
  must not itself be an event, which is an allowlist over the syntax that
  refuses every call and every arithmetic operator; and its type must be one
  whose equality is the whole of what a program can tell apart — the
  primitives, `str`, `String`, and `Option` or `Vec` of one of those. A float
  is refused because `-0.0 == 0.0` holds and `-0.0` is not what the default
  writes; a type of your own is refused because its `PartialEq` may answer
  about less than a test reads. The value must also *have* a `Default`, which
  a reference usually does not: a function returning `&str` whose body borrows
  a `String` field returns a `&String` and is refused, however much it coerces
  at the return. A value the compiler refuses costs the question and never the
  mutant, which is measured by running it.
- A run refuses a tree whose instrumented baseline does not pass, and it names
  every target that failed rather than the first: a person reading the refusal
  is about to fix what it names, and a message that names one of five sends
  them round the loop five times. A caller that would rather measure what it
  can asks for the other policy, and each target left out carries
  `baseline-not-passing` so that no score is read as being about a mutation the
  target was never in a position to notice.
- The guards measure reach on the baseline run, and a target whose guards were
  not asked carries `touch-not-recorded`. They are not asked when the engine
  does not start the process itself: a documented example, which rustdoc
  compiles and runs, or a target the project configured a runner for. A record
  that was made and did not read back carries `touch-log-unreadable`, and so
  does one the filesystem would not hand over: a runtime creates its log the
  first time it has something to say, so a file that is not there is a process
  that said nothing, and a file that is there and will not open is neither.
  Either way the target keeps every test of it in every route, which is what a
  run that measured nothing about it has to do.
- A coverage export whose region ends before it starts is refused outright
  rather than read as far as it goes. Such a region describes nothing, and a
  reader that kept it would answer "this target did not reach that place" for
  every place inside it, which removes an execution on evidence that says
  nothing. Refusing the whole measurement routes every mutation everywhere
  instead.
- A library with no documented examples has a documentation target that
  answers nothing. It carries `doctests-none`, and no mutation is routed to
  it: paying a `cargo test --doc` for every mutation nothing else noticed, to
  be told each time that no test ran, is work nobody reads.
- A target with `harness = false` says what it found by exiting, and neither
  `cargo metadata` nor the build's messages report the flag, so the engine
  reads it from the manifest. How many of its tests ran is something only a
  harness could have said, and the target carries `custom-harness` to say so.
- A target named in `[execution] skip_targets` is never started, and carries
  `target-skipped-by-configuration`. It is for a suite whose tests are about
  the text of what the compiler said, which instrumentation changes: leaving
  it out is a decision somebody made, and the report says so rather than
  reporting a failure nobody can read.
- A run composes `LLVM_PROFILE_FILE` for every test process it starts, so an
  inherited one never reaches one and an instrumented binary never falls back
  to `default_*.profraw` in its working directory. Without that, a project
  measuring its own coverage around a run would have its profiles written into
  the tree the run is measuring, and the drift report would say the project's
  tests write into their own tree.
- One workspace per run: `--root` names the workspace, and a member of one is
  refused with `RM1018` naming the workspace it belongs to. A run measures a
  copy of what it was given, and a member on its own is not a tree cargo can
  build.
- A path dependency or a `[patch]` entry that reads from outside the root is
  refused with `RM1017` before anything is copied, naming the dependency, the
  manifest that declares it, and `--allow-outside`. That flag copies the named
  directory beside the tree under its own name, so the same relative path
  resolves in the copy. What is copied beside the tree is read and never
  mutated, and is not part of the workspace digest: a run says what it
  measured, and it measured the tree.
- Symbolic links in the evidence tree are rejected.
- The coverage build sets `CARGO_ENCODED_RUSTFLAGS`, which replaces
  `build.rustflags` rather than adding to it, so mjutest reads the project's
  `.cargo/config.toml` files and puts those flags back. What it does not put
  back is `target.<triple>` and `target.cfg(…)` flags — which of them apply
  is cargo's decision about the target being built, and guessing wrong would
  compile something other than the project's binaries. A project that
  configures them gets `target-rustflags-not-merged`, and a configuration
  file that cannot be parsed gets `cargo-configuration-unreadable`.
