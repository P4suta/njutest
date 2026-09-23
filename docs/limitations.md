<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Current boundaries and evidence

**Status: implemented.** Every name here is one a report can carry; the list grows with each release, and every limitation below is stated fail-closed.

- A tree whose identity could not be computed reuses nothing (`workspace-digest-not-computed`) and is reused by nothing: a run that cannot say what it looked at cannot answer for another run's inputs.
- A mutant every measured target was asked about and none of them reaches is reported as surviving, with a detail that says nothing executes it and a route that names the targets that were in a position to notice and did not.
  Where that evidence is not there — nothing was recorded at all, the position is outside every instrumented region, the catalog could not place it, a target that was measured carries no profile, or a target's guards recorded nothing this run can route by — the route widens to every test of every target instead (`not-measured`, `outside-blocks`, `position-unknown`,
  `coverage-incomplete`, `touch-incomplete`).
  Both readings are one finding,
  because they are one gap in the suite.
- A target is a test binary, and a route names which of its tests a mutation is put to.
  A target whose every test is `#[ignore]`d is explicitly `Skipped` with its ignored count.
  A target that printed no result and reported no ignored tests is `Missing`: nothing was learned about it, which is a finding rather than a pass.
- A test that writes into the tree while it is being measured makes every later mutation a measurement of what it wrote (`tree-written-during-measurement`).
  One instrumented snapshot cannot isolate that the way a per-mutant build would; the run says so rather than reporting the later results as if it had.

## Decided in advance

### Durable report authority is Unix-only today

Publishing or reopening evidence is an authority decision, not a display-path operation.
njutest currently makes that decision only through its Unix handle-relative, no-follow backend, which retains the workspace and configured report-root identities through publication, index updates, reading, and retention.
Windows and other non-Unix builds therefore refuse durable report publication and authority-bearing reads with `REPORT_NOT_KEPT`; they do not silently substitute a weaker pathname implementation.
Pure computation that does not open or mutate a report store remains available.

- Doctests are run as one target per library, switched off with `[execution] doctests = false` or `--no-doctests`, and mutations are routed to them at file granularity (`doctests-routed-by-file`): a documented example reaches every mutation in the files its library is made of and narrows none of them.
  A kill one finds names the library's documentation and not the example, because rustdoc merges a file's examples into one compilation whose harness cannot be asked for one of them.
- A `harness = false` test target says what it found by exiting rather than by printing a summary, so how many of its tests ran is unknown and the whole binary is measured (`custom-harness`).
- A library that documents no example has nothing to run and is not a target (`doctests-none`): a target that ran nothing would raise a finding about documentation nobody wrote.
- **A test that starts this engine in a process of its own is measurable only as the instrumented binary the outer run built.** The binary embeds its compiled catalog digest.
  It may inherit exactly one activation or touch mode only beside the same nonempty catalog; every partial, stale, ordinary-build,
  or double-mode environment is still refused with `RM0006`.
  The subprocess keeps the outer identity so its guards can observe the mutation, while any run it starts composes a new test environment with all three mutation variables removed.
  Tests of the actual refusal remove the inherited activation and catalog and deliberately add an incomplete touch mode, so they still exercise the composition root's fail-closed boundary under measurement without being stopped first by the generated runtime's stale-catalog check.
- **A test whose subject is the tree it was compiled from cannot be measured by copying the tree.** A run works in a snapshot, and a test that reaches its own repository through `env!("CARGO_MANIFEST_DIR")` reaches that snapshot rather than the repository: the same tree as the engine rewrote it, guards and generated runtime and all.
  This workspace has one such target,
  `xtask/test/gates`, whose gates refuse exactly what instrumentation adds —
  each private generated support module carries the exact `#[allow(dead_code, unused_qualifications)]` needed by that generated boundary, and `cargo xtask lints` exists to refuse an `#[allow]` in repository source.
  It fails on the instrumentation and would fail on it identically under every mutation, so it is left out in `.rust-mutants.toml` with the reason written beside it.
  The gates are not unmeasured for it: `cargo xtask` applies them to the repository on every run of the check task, which is where an assertion about a tree belongs.
- **A guard against a failure that reappears downstream is a survivor no deterministic suite can decide.** A `?` on an I/O call whose failure is reported again by a later call on the same resource - a write that is put back by a restore of the same path, a lock taken twice, a file opened again - hands its caller the same error whether the guard is there or not, and leaves the same state behind.
  The two versions part only when the condition is transient, and a test that can arrange a transient condition is a test that has stopped being deterministic.
  Such a mutation is not equivalent: it is a fail-open the suite cannot reach.
  Recording it as an expectation is right,
  and the reason has to say which of the two it is, because a later reader deciding whether to delete the guard needs to know that the run never disagreed with it rather than that the run agreed.
- A target whose own tests do not pass with nothing active is dropped from the ones a mutation may be put to (`baseline-not-passing`), and every one of them is named.
  Every mutation put to such a target comes back killed and not one of those kills is about a mutation.
  The run still reports the table, because the moment a reader most needs it is the moment the answer is "all of them".
- A target that does not pass the first time is run once more before the session refuses, and one that passes the second time is measured against that second answer with `baseline-passed-on-retry` against its name.
- A libtest target whose baseline was read as passing tests that do not come to the count its own summary gives carries `baseline-passed-unparsed`: a line the suite wrote past libtest's capture can read as a result, or split one, so which tests passed is the parser's answer and its reach is not compared with a control's.
  Three things a reader has to tell apart used to arrive as one refusal: a target that is broken, a target that lost a race with something outside the code, and a target that passed.
  The middle one is a finding about the run's footing, not a reason to throw away the work already done, and the run says which target it was so that a later reader knows a single result against it rests on a measurement that once came out differently.
- A target whose guards recorded nothing this run can route by keeps every test of it in every route (`touch-not-recorded`), and one whose record did not read back is believed about nothing (`touch-log-unreadable`).
- A target whose tree started a process without the environment the run gave it — a test that calls `env_clear()` on a child — is `uncontrolled-child`: no mutant can be active in that process and nothing records what it entered, so the target stays in every route and a survival it reports is inconclusive ([ADR 0028](adr/0028-a-process-that-loses-the-environment-says-so.md)).
  Only the baseline is watched this way; a process that lost the environment under one mutant only and not on the baseline is not yet noticed.
- **Which items a test entered is measured where a body starts, and not everywhere code runs.** A `const fn`, a `const`, and a `static` are cataloged as items nothing records entering.
  A closure written inside a macro invocation takes no entry marker, and an `async` body resumed on another test's thread is recorded on the thread that first polled it.
  A change to any of these has to be routed to every test; [item reach](engine/item-reach.md) says which is which.
- **A subprocess is measured, but its main-thread touches have no libtest test name.** It inherits the outer touch log and catalog, so its guards are not lost.
  The engine records those touches as unattributed and widens the route to every test of the target.
  An in-process command test keeps test-level attribution and costs less work; a subprocess test remains sound, with the fallback costing precision rather than evidence.
- Fuzz targets are found always and driven only when `[fuzz] run` says so;
  a tree that holds targets nobody asked to drive carries `fuzz-not-executed`.
  A target the fuzzer could not drive — no cargo-fuzz on a nightly toolchain, a fuzz crate that will not build, a sanitizer the toolchain has no runtime for — carries `cargo-fuzz-unavailable` and a `not-measured` finding instead, and is never counted among the targets that were driven.
  libFuzzer exits non-zero when it finds something, and something is an input it keeps, so a status with nothing kept is a target that never started.
  A run stopped by its own bound *was* driven, for less time than it was asked, and says that instead.
- `standard-v1` does not execute anything about `unsafe` code; it inventories it and says so (`soundness-not-executed`).
  `deep-v1` interprets the suite under Miri and refuses to run at all without it (`NJ7001`), and what Miri will not interpret is `miri-unsupported` rather than a pass.
  A sanitizer the configuration asks for and the toolchain will not run is `sanitizer-unavailable`; every sanitizer run also carries `sanitizer-standard-library-not-instrumented`.
- A question about a seam that hands the caller the bytes it was handed already is `proved` and never run.
  Cutting an answer with no body short keeps everything up to and including the blank line that ends the head, so what comes back is identical; asking for the status the upstream already gave,
  worded the way a run words it, writes the line that is already there.
  Nothing that reads bytes can tell either pair apart, which is a stronger answer than a run — not that no test noticed, but that no observer could.
  Both checks are asked of the code that would have done the injecting, so a proof is a check on the injection rather than a second opinion about it.
  A 500 whose reason phrase differs from the one a run writes is a different answer and is still put: a proof that held only most of the time would not be a proof.
  Every other question changes what the caller is handed or when, so every other question is put.
- `replay-request` delivers the same request to the dependency a second time and hands the caller the first answer, which is what a retry after a lost answer leaves behind.
  What could notice is whatever holds the dependency's state, so a suite that notices nothing here is one that would not notice a double charge.
  The run does not undo the second delivery: a resource the configuration leases is the run's to spend, and a seam nobody named is never watched in the first place.
- `stale-response` answers one exchange with what the exchange before it on the same seam got, which is what reading a replica the writer has outrun looks like from inside the program.
  The first exchange on a seam licenses no such question, because nothing came before it to be answered with.
- A run told to measure only what changed re-measures every seam question it licenses.
  An exchange says which target caused it — the catalogue run drives the targets one at a time and names each as it goes — and the behaviour key that says whether a target changed is already minted per target for the mutation layer.
  So the two halves meet at a map lookup rather than at a mechanism that has to be built.

  What that buys is the decision and not the catalogue, and the difference is
  the whole of why this is still a limitation. A seam question asks *if the
  dependency answered differently, would a test notice* — which depends on
  the target's behaviour, and the key covers that. **Which exchanges exist at
  all depends on what the dependency does, and the key covers none of it.** A
  target whose key is unchanged can produce a different catalogue tomorrow
  because the upstream it talks to changed: a new endpoint, another status,
  one round trip where there were two.

  So a run may reuse an answer and may not reuse the question. Skipping on
  the second would report a question as still answered about an exchange that
  no longer happens, which is a run saying something about a program it did
  not observe — `wire-fault-not-put` inverted, and worse for being quiet.
  Re-deriving the catalogue costs one run of the suite and re-deciding costs
  one per question, so keeping the derivation and reusing the decisions gives
  up almost none of the saving. It is the expensive half that is reusable,
  which is the opposite of what this entry assumed before it was measured.

  `None` remains the honest answer wherever the run cannot tell which target
  caused an exchange, because naming the wrong one would route a question to
  tests that were not there, and a changed-run skipping a question on that
  premise would be worse than asking it again.
- Two answers arriving in the other order is not a question this release can put, and will not become one while the catalogue names exchanges by their place in the order.
  An interposer that served connections concurrently could hold one answer back past another, but then which exchange is the second one would depend on how the threads were scheduled, and every fault identity would move between runs of the same suite.
  Serving one connection at a time is what makes the recording reproducible, and reproducible identities are what let a fault be named, stored, compared and re-derived at all.
  The question a sequential caller can still be asked — an answer belonging to a moment that has passed — is `stale-response`.
- A clock the program reads is not a seam an interposer sits in front of, so skew and TTL boundaries are not asked here.
  What the seam can ask is how long the caller waits, which is `delay-response`, and for how long is the `hold` of the resource rather than a number this tool picked.
  How slow is too slow is a property of the system under test: a service with a one-second budget and a nightly batch job put different questions to the same seam, and a run that chose for them would report an answer to a question nobody asked.
- The catalogue is derived from one run of the suite that the seam phase makes for the purpose, which costs one run more than reading what the phases before it happened to do.
  It is not free and it is not optional.
  A fault names an exchange by its place in the order and is put by running the suite once, so a catalogue has to come from one run of the suite to hold questions one run can reach.
  A phase that builds and verifies runs the suite more than once and the repeats are the same exchanges counted again; the mutation phase runs it once per mutation.
  Questions derived from either are unreachable by construction and would be stated as questions nobody put —
  holes a run invented by measuring.
  The seams record only during that one run and nothing is recorded after it.
- An answer of a different shape — a field gone, a value of another type, an enum from a later release — is not proposed, because the recording keeps no bodies to derive one from.
  Inventing a shape would be guessing at a program this run never saw, and the catalogue is derived from what went past or it is not derived at all.
- A rule this release has no injection for is `not-measured` and never a survivor.
  A catalogue that grew a name before the interposer did would otherwise pass the answer along quietly and come back as a gap where no question was ever asked.
- A question the exchange it names never came past is `not-measured`, whatever the tests did.
  A suite that took a different path this time and passed throughout was asked nothing, and reading that as nothing noticing would report a gap the tests could close where nobody was asked at all.
- A seam the configuration names with `interpose` is recorded, and every fault the recording licenses is put back to the suite with nothing mutated.
  A run that recorded a seam and could not put its questions — the workspace did not build, or the run was cancelled — says `wire-fault-not-put` and counts none of them as survivors: a question nobody was asked establishes nothing, and reporting it as a gap would put something in the report no test could close.
- Repository reads are not observed at run time.
  A package that names a directory-reading crate keys the whole snapshot for evidence reuse; nothing is excluded from testing.
  The names searched for are the paths those crates are reached through (`ignore::`, `glob::`) rather than their bare words,
  because `ignore` is how every `#[ignore]` spells itself and `glob` is inside `global`: a marker that matches every package keys every package on the whole tree, which is not a widening that costs a little but one that throws every stored answer away whenever anybody edits a file beside the code.
- A generation provider that could not be asked, or that said something this release cannot read, is `generation-provider-unavailable`.
  A candidate that held up under the tests and could not be stored is `generation-candidate-not-kept`, which is a different thing to do about:
  `fix --apply` writes what was checked rather than asking again, so a candidate whose content is gone is an offer nothing can take up.
- Mutation inside a macro invocation the engine has no allowlist entry for, a `const` context, and `#[cfg]`-guarded code is skipped with a stated reason.
  The leading arguments of the six assertion macros are mutated; the rest of every invocation is `macro-invocation`.
- A `#![no_std]` crate is measured: the runtime borrows `std` under a name of its own.
  A crate the host cannot lend `std` to — one supplying a `#[panic_handler]`, a `#[global_allocator]`, or `#![no_main]`, or written in edition 2015 — is `no-std-crate`.
- A file another file pastes in with `include!` where an expression goes is `included-expression`: it is a fragment rather than a program.
- The equivalence layer is off by default and proves almost nothing on a project that leaves `[profile.test] opt-level` at cargo's default of zero,
  where two mutations the compiler would render identically at any optimisation level are still two different sets of instructions.
  It is a fact about the profile the tests run under rather than about the mutation,
  and the layer answers the question the tests ask rather than an easier one.
- A procedural macro's own unit tests are measured like any others; what it expands to is not (`proc-macro-expansion-not-measured`).
  A macro decides that during the build and a mutation is activated for a test process, so the two never meet, and cargo does not rebuild for an environment variable.
  A mutation only the expansion would change is reported as surviving.
- Coverage routing says nothing about a place its export never instrumented,
  and such a mutant is run everywhere.
  A tree that configures `rustflags` for a target — `[target.<triple>]` or `[target.cfg(…)]` in a cargo configuration file — is not routed by coverage at all (`coverage-refused-configured-rustflags`), because which of those tables apply is cargo's decision about the target being built and a guess would compile something other than the project's own binaries.
  A `[build] rustflags` is read instead and put back into the coverage build, so a tree that configures one is measured.
  A configuration file that cannot be parsed leaves what the tree compiles with unknown (`cargo-configuration-unreadable`), and is not routed by coverage either;
  cargo itself normally refuses such a tree first, with `RM1014` naming the file.
  Three other things leave the measurement empty and route every mutant to every target, exactly as if the layer were not there:
  a tree that will not build with instrumentation (`coverage-build-failed`), a toolchain whose LLVM tools are not installed (`coverage-tools-missing`), and tools that ran and produced nothing a run could route by (`coverage-not-measured`).
  The last of these is also stated per target, as `coverage-not-measured:<target id>`, when one target's profile could not be read: that target is kept in every route rather than dropped, because what the measurement says nothing about is run rather than assumed.
- A guard compares its two branches only where the compiler vouched that the condition around it is inert — every comparison in it between primitives,
  `str`, or a slice of those — and the guard's own form can hold the call.
  The trait names the types whose comparison the standard library defines and a container of one of those, and asks about the two operands separately, so a comparison between two different ones is vouched for where both are named.
  A type of your own is refused whichever side it is on, and so is a container holding one.
- A swap between `==` and `!=` is never offered the comparison.
  The two are each other's negation, so a guard between them parts on every evaluation and the record could only ever say that it did.
  Everywhere else `never-infected` says nothing about the mutation however many tests ran it, and the record names which mutants it can speak about so that its silence about the rest is never read as evidence.
- A return replacement is asked whether the value it overwrites already held what it would write, and only where two things hold.
  Evaluating the value must not itself be an event, which is an allowlist over the syntax that refuses every call and every arithmetic operator; and its type must be one whose equality is the whole of what a program can tell apart — the primitives, `str`, `String`, and `Option` or `Vec` of one of those.
  A float is refused because `-0.0 == 0.0` holds and `-0.0` is not what the default writes; a type of your own is refused because its `PartialEq` may answer about less than a test reads.
  The value must also *have* a `Default`, which a reference usually does not: a function returning `&str` whose body borrows a `String` field returns a `&String` and is refused, however much it coerces at the return.
  A value the compiler refuses costs the question and never the mutant, which is measured by running it.
- A run refuses a tree whose instrumented baseline does not pass, and it names every target that failed rather than the first: a person reading the refusal is about to fix what it names, and a message that names one of five sends them round the loop five times.
  A caller that would rather measure what it can asks for the other policy, and each target left out carries `baseline-not-passing` so that no score is read as being about a mutation the target was never in a position to notice.
- The guards measure reach on the baseline run, and a target whose guards were not asked carries `touch-not-recorded`.
  They are not asked when the engine does not start the process itself: a documented example, which rustdoc compiles and runs, or a target the project configured a runner for.
  A record that was made and did not read back carries `touch-log-unreadable`, and so does one the filesystem would not hand over: a runtime creates its log the first time it has something to say, so a file that is not there is a process that said nothing, and a file that is there and will not open is neither.
  Either way the target keeps every test of it in every route, which is what a run that measured nothing about it has to do.
- A coverage export whose region ends before it starts is refused outright rather than read as far as it goes.
  Such a region describes nothing, and a reader that kept it would answer "this target did not reach that place" for every place inside it, which removes an execution on evidence that says nothing.
  Refusing the whole measurement routes every mutation everywhere instead.
- A library with no documented examples has a documentation target that answers nothing.
  It carries `doctests-none`, and no mutation is routed to it: paying a `cargo test --doc` for every mutation nothing else noticed, to be told each time that no test ran, is work nobody reads.
- A target with `harness = false` says what it found by exiting, and neither `cargo metadata` nor the build's messages report the flag, so the engine reads it from the manifest.
  How many of its tests ran is something only a harness could have said, and the target carries `custom-harness` to say so.
- A target named in `[execution] skip_targets` is never started, and carries `target-skipped-by-configuration`.
  It is for a suite whose tests are about the text of what the compiler said, which instrumentation changes: leaving it out is a decision somebody made, and the report says so rather than reporting a failure nobody can read.
- A run composes `LLVM_PROFILE_FILE` for every test process it starts, so an inherited one never reaches one and an instrumented binary never falls back to `default_*.profraw` in its working directory.
  Without that, a project measuring its own coverage around a run would have its profiles written into the tree the run is measuring, and the drift report would say the project's tests write into their own tree.
- One workspace per run: `--root` names the workspace, and a member of one is refused with `RM1018` naming the workspace it belongs to.
  A run measures a copy of what it was given, and a member on its own is not a tree cargo can build.
- A path dependency or a `[patch]` entry that reads from outside the root is refused with `RM1017` before anything is copied, naming the dependency, the manifest that declares it, and `--allow-outside`.
  That flag copies the named directory into the snapshot at the position it has relative to the tree, so every path that reaches it on disk reaches it in the copy however far the declaration climbs.
  A directory the copy cannot place that way is refused with `RM1019`: one that is the tree, holds it, is inside it, or lies on another filesystem root.
  What is copied beside the tree is read and never mutated, and is not part of the workspace digest: a run says what it measured, and it measured the tree.
  The cost is path length — a high shared ancestor makes the copied paths longer, and on Windows, where the engine strips the `\\?\` prefix from every canonical path on purpose, a deep ancestor under a deep tree can cross 260 characters and stop the copy with `RM1008`.
- Symbolic links in the evidence tree are rejected.
- The coverage build sets `CARGO_ENCODED_RUSTFLAGS`, which replaces `build.rustflags` rather than adding to it, so njutest reads the project's `.cargo/config.toml` files and puts those flags back.
  What it does not put back is `target.<triple>` and `target.cfg(…)` flags — which of them apply is cargo's decision about the target being built, and guessing wrong would compile something other than the project's binaries.
  A project that configures them gets `target-rustflags-not-merged`, and a configuration file that cannot be parsed gets `cargo-configuration-unreadable`.
- The witness tree the branch proofs check is compiled with every lint capped at a warning.
  That tree is not the project's code: it is the project's code with a statement written in front of each condition, put there to ask the compiler one question about the types, and whether the project's own lints are satisfied is not that question.
  A caller who denies warnings for their own build — which is what a continuous integration job does — is therefore not asking for every proof of their tree to go unmade.
  The project's own `.cargo/config.toml` flags are still put back in front of the cap, because a tree that does not compile without them would not compile here either.

## What a run cannot do on Windows

A report is published through a capability rooted at the store's own directory, so that what a reader opens is the file this run wrote and not one a name was pointed at afterwards.
That rooting is implemented with the POSIX directory-capability calls and has no Windows equivalent here, so `Store::keep` answers `NJ6004` on Windows rather than publishing:
`this platform has no supported capability-rooted report publication backend`.

A Windows run therefore measures, decides and prints, and cannot leave a durable report where the next run or a reader will find it.
Everything downstream of publication — `njutest report`,
`accept`, `bundle`, `why` against a stored run — has nothing to read.

This is a deliberate refusal rather than a defect: the alternative is publishing through a path that can be replaced between the check and the write, which is the thing the capability exists to prevent.
What it is not is documented anywhere a person would look before installing on Windows, which is why it is here.

## What a run says about itself

Six of the names a report can carry are not about the code under test at all.
They are what the run says about its own footing, and each is stated fail-closed:

- Git could not be asked what the tree is (`git-metadata-unavailable`), so the report carries `unavailable` rather than a guess.
  [The report contract](report-v1.md) says what that sentinel means.
- The run continued one that was interrupted (`resumed-from-checkpoint`), so part of what it reports another run established.
  [The checkpoint contract](checkpoint-v1.md) says what may be inherited and what is judged again.
- The run worked in a directory it does not own (`temp-directory-unclaimed`),
  so what it left there is not its to remove.
  A run that cleaned up somebody else's directory would be a run that deleted work nobody asked it to.
- A resource the run started would not stop (`resource-not-stopped`).
  Something it started is still running, and the run says so rather than exiting as though the world were as it found it.
- The configuration named a seam to watch and this run could not put an interposer in front of it (`seam-not-watched`), naming the capability and which of the five ways it could not: the lease carries no such variable, the value names no authority, the authority names a host that did not resolve, the interposer would not take a port, or the value could not be rewritten to name it.
  A run that met any of these and said nothing recorded no seam, raised no finding and stated no limitation, so a reader read the wire dimension as covered when nothing about it had been measured.
- The interpreter ran out of the time it was given (`miri-timed-out`).
  This is not a claim that it found nothing: a budget that expired is a question nobody answered, which is why it is a limitation and never a pass.
- A file the soundness inventory walked could not be read as Rust this release understands (`soundness-source-unreadable`), so what it holds is not in the count.
  A count taken over part of a tree and reported as a count over the tree is the one number a reader cannot check.

## What a run asks of a suite whose reach moves

Every proof layer reads one baseline run of each target: what its tests reached, which bodies they entered, which sites they saw infected.
That is sound only where what a target reaches is a function of the target, and a suite that reads a clock, a hash seed, the order its threads were scheduled in, or state an earlier process left behind can reach something different on the next run of the same tests.
A run checks it by running every target again: the original-code control that confirms a kill records what it reached too, a target no kill was confirmed on is run whole once more after the mutation phase for the comparison alone, and a union that moved over the same passing tests is a counterexample, raised as `unstable-baseline` about the target ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)).

Three things follow and are not hidden.
A target whose second run cannot be compared — it failed, passed other tests than its baseline, or could not record — is `drift-not-measured`, and every proof read off its baseline rests on one run.
The comparison sees what the guards see, so a suite whose behaviour moves where no mutant sits moves without this noticing.
And this release reports a moved target without running again what rested on it: the `unreached` claims and the executions a proof removed are counted in the finding, and re-executing them without those proofs is the next change.

## What a run asks of a suite that talks about time

A run does something to a suite that `cargo test` never does: it runs every target's baseline before it measures anything, and then measures mutants against a machine that is already busy.
Every assumption a suite holds about how long something takes is put to that, and the first thing a run finds is usually not a mutant at all — it is which of those assumptions was never proof against a machine doing something else.

Four shapes of assumption, and the fourth is the one that hides:

- an assertion on elapsed time, `assert!(started.elapsed() < THREE_SECONDS)`;
- a deadline a fake server takes once and reuses across requests, where what sits between two requests is a whole command;
- a read or accept timeout;
- **a timeout a fixture is passed**, `required("policy", &hook, 5)`, repeated at every call site.
  This one is not an assertion about time and does not read as a clock: it reads as configuration, and nothing greps for it.

None of these is a defect this tool found in the code under test, and none is a wrong answer — they arrive as a refusal, at the end of the most expensive phase.
Stating it in advance is the useful thing: *if your suite talks to sockets, spawns processes, or asserts on elapsed time, the first thing a run will tell you is which of those assumptions was never load-proof.*

The remedy is almost never a larger constant.
A bound of that kind exists so that a wedged process cannot hang a suite, not to decide how patient a machine is entitled to be; a number tuned against one busy afternoon is one nobody can defend six months later.

### A suite that makes something outside its own process makes it once per mutant

A run executes the tests once with nothing active and then once per mutant it puts to them.
A test that creates a resource the operating system owns rather than the process — a mount, a loopback device, a container, a listening port, a record in an external service — creates it that many times.
Three under `cargo test` is five hundred here.

Two things then differ from an ordinary test run, and the second is the one that costs:

- **The count.** What a suite creates and destroys three times, a measurement creates and destroys as many times as there are mutants reaching it.
- **The interruption.** A run can be stopped, and a `Drop` does not run in a process that was killed.
  Cleanup that lives in a destructor is cleanup that does not happen.
  One caller stopped a run and found 183 mounts and 442 attached devices left behind, and the mounts held directories that then could not be removed at all.

`[execution] skip_targets` is the right answer for such a target, and it is reported as a limitation, so leaving it out is a thing the run says rather than a thing it hides.
The unit is the test binary, which is the unit a resource of this kind belongs to.

### Taking something back has to be faster than making it was

This is advice about cleanup code in general, and it applies to this tool's own `cache --gc` as much as to a suite's.

Removing a directory is usually instant.
A directory something else is holding can take minutes to refuse, and a loop that walks a few hundred of those runs for a day.
The caller above wrote recovery code for the leak in the section above; it asked the system to release the devices one at a time, and the system was already wedged, so what should have taken seconds was on course to take fifteen hours.
The measurement that found it was a test that had gone from 135 seconds to 10 once the recovery had a budget.

Three rules, which `rust-mutants cache --gc` now follows:

- **Give the recovery a budget.** Failing means "some of it is still there",
  never "this does not finish".
- **Let the first failure tell you about the rest.** Where taking one back and making one go through the same thing, a recovery that failed says the next creation will too, and paying that timeout twice is paying it for nothing.
- **Say what was left, with what to do about it.** A count of what it did not reach is worth more than a longer wait, because a directory that will not go is being held by something, and that something is what a person has to deal with.

### On macOS, measure what an execution costs before measuring anything else

macOS evaluates an executable before it may run, once per file, and where that evaluation has a backlog the first execution of a newly written file costs seconds or minutes.
Measured on one developer's machine: a two-line shell script took **395 seconds** to run the first time and **0.22 seconds** the second.
A run builds a file per target and runs each of them, so it pays that once per target.

Three things about it are counter-intuitive enough to be worth writing down,
because each was believed here and each was wrong:

- **Load average is not the signal.** The 395-second execution happened at a load average of 4.93.
  An earlier 23-second one happened at 22.
- **The backlog outlives what made it.** The builds that produced the files had finished; the evaluation kept running for over two hours afterwards.
  "Wait for the build to finish" is not the advice.
- **Reducing parallelism cannot help**, and the evaluation is not single-threaded, so there is no queue to shorten by sending it less at once.

Nothing a caller can see reports this — not load, not free processors, not free memory.
`rust-mutants doctor` therefore runs one newly written file twice and prints both numbers, because the pair is the evidence and neither number means anything alone.
A first execution in the hundreds of seconds beside a second in hundredths says a run started now would measure the evaluation and not the tests.

## Where a count reaches, and where the clock is still the only bound

A mutation whose active guard crosses the configured count is `step_limit_reached`.
The count and the exact `N + 1` boundary are stable across machines, but they do not prove the program would never have ended: a long finite computation crosses a finite count too.
The outcome is therefore unresolved and excluded from both score and cache.
The guard can reach that boundary only where it is *inside* the part that keeps running.

The guard is not only where the mutation is.
Every mutable file is given control-flow checkpoints — function entries, loop bodies, async blocks, and closure invocations — including a file with no mutant of its own, and a checkpoint charges the allowance once the selected mutation has been reached.
So a mutation *outside* a loop that makes the loop non-terminating is caught by the count as surely as one inside it:

```rust
let step = 1;              // mutated to 0
while i < n { i += step; } // and this never ends — the loop body is a checkpoint
```

Run against a workspace of that shape, the mutation of the literal reports `step_limit_reached` at `N + 1` and never `waited`, and it does so with the loop in a second file the run is not mutating.
This paragraph said the opposite until that was measured; what it described was the instrumenter before it placed checkpoints across a whole workspace.
No fixture in this tree pins it yet, which is the next thing this claim wants.

How long the allowance takes to reach belongs to the platform, not to the count.
The durable step protocol pays one locked state-file round trip per take, and that round trip is measured here at about 7.4ms on Windows: a hundred takes cost 789ms, where the machines this figure was first chosen on spend a fraction of that.
Where an allowance and a bound are sized against each other, that difference is a race, and the clock winning it reports a proved non-termination as a mere `waited` with no notice — the safe direction again, and again a worse answer than the one available.
The lever is the allowance rather than the bound.
Lengthening the bound wins the race by moving its cost to the moment the count breaks, when every non-terminating mutant waits the whole bound out and is then asked again: the suite that has just started failing is also the suite that has just become slow to read, which is the worst moment for it.
A bound is also what says a hang is a hang, so what a short bound buys is not worth spending.
Lowering the allowance costs nothing anywhere and widens the same difference, down to the floor of what an ordinary execution of the same tests spends — and that floor is a count, not a duration, so it is the same number on every platform and can be measured once.
Spend the headroom there rather than above the floor: crossing the floor fails the same way on every machine and says so, where losing to the clock arrives as a test that passed until the machine was busy.
What remains is that both stoppers are timers, and no allowance makes that difference infinite.
So the assertion says what a loss means when it loses: that the machine was slow, that the protocol is not the thing to go looking at, and which number to change.

Where the clock is still the only bound is where the instrumenter could not put a checkpoint.
Macro expansions and dependency crates are not rewritten, so a computation that stays inside either — a loop in a crate the workspace only calls, an expansion the source never spells — reaches no boundary and raises no count.
Nothing counts there, the allowance is never reached, and the bound on the execution is what stops the process.

The run reports `waited`, which says nothing was established, and for such a mutation that is true of the run and misleading about the code, because something *could* have noticed: the program hangs.
A reader who sees `waited` is told the machine ran out of time, and the honest reading of that is *ask again*, which is what they would do.
This is the safe direction and it is still a gap, now a narrower one than the workspace-wide gap this section used to describe.

Neither `waited` nor `step_limit_reached` counts as a detection, so no mutation is called caught on the strength of a clock or a finite prefix.
Raising `[mutation] timeout` does not close it when the count is not rising: the clock is the only thing that can stop that process, and lowering it would only make the run give up sooner.

A positive divergence proof needs more than the notice: the original control must complete under the same target, test and arguments, and the mutant must reproduce the same step boundary.
Until that comparison is recorded, the report stays unresolved.

Closing what is left means a checkpoint in code this workspace does not own, which is a different undertaking from placing one in code it does.
It is not closed here, and a report that says `waited` is saying exactly what it knows.

## Places a run passed over

Every place a rule targets gets a decision: a candidate, or a skip with the reason for it.
A run reports its skips as one limitation per reason —
`skipped-<reason>`, with how many places it covers — because a place nothing was put to is a place the suite was never asked about, and a tally of them is the difference between "the tests noticed every mutation" and "the tests noticed every mutation somebody proposed".
The engine's [architecture page](engine/architecture.md) says how each decision is reached; what follows is what the name in a report means.

Five are about a whole file, decided from cargo's metadata rather than by reading it:

- `skipped-excluded` — `[project] exclude` names the file.
  The only one of these a reader chose, and [the configuration page](configuration.md) says what it does and does not narrow.
- `skipped-test-only-file` — only a test unit compiles the file, so a mutation of it would be a mutation of the tests.
- `skipped-no-std-crate` — the crate is `#![no_std]`, and this release's guards need the standard library.
- `skipped-generated-outside-workspace` — a build script wrote the file outside the tree, which the run does not hold and cannot rewrite.
- `skipped-forbidden-lints` — the crate `forbid`s a lint the guards' own attribute turns off, which `forbid` does not let an `allow` override, so every mutant of it would be refused with nothing saying why.

Eleven are about a place inside a file, decided by the walk:

- `skipped-const-context` and `skipped-const-fn-body` — the compiler may evaluate the code before the program runs, where a runtime guard cannot live.
- `skipped-macro-invocation` — the body is tokens the walker does not parse,
  counted once for the whole invocation.
- `skipped-cfg-attribute` — the place is behind a `#[cfg(...)]`, so what the build compiles is not what the walk read.
- `skipped-test-code` — a `#[test]` function or anything behind `#[cfg(test)]`.
- `skipped-unsupported-site` — no guard form can express the two versions at that position.
- `skipped-included-expression` — another file pastes this one in at expression position, which is a fragment rather than a program.
- `skipped-let-condition` — the condition binds with `let`, and a guard cannot rearrange it without moving the binding out of scope.
- `skipped-open-range` — a range with no end has no other form to become.
- `skipped-unstated-return-type` — the syntax cannot say the return type has a default, so there is no value to return instead.
- `skipped-loop-value` — the loop decides the jump's value by what it breaks with, so the other jump has no value to carry.

Two are what a person wrote, and are the two to read first:

- `skipped-annotated` — a `rust-mutants: skip <reason>` marker in the source.
- `skipped-configured` — a `[[mutation.skip]]` entry in the engine's configuration.
  A marker or an entry that hid nothing is an `unmatched-skip` finding rather than a line nobody notices.

## Survivors a suite cannot close

A survivor is a test to write, and three kinds of them are not.

Two are about the mutation.
One is a mutation whose two versions no deterministic suite can tell apart, because the guard it removes is there for a failure that happens once and not again: a `?` on a write that a later restore of the same path repeats, a lock taken twice, a file opened again.
The caller is handed the same error either way and the tree is left in the same state, and a test that could make the failure transient is not a deterministic test.
Such a mutation is not equivalent — it is a fail-open the suite cannot reach — and the difference matters to whoever reads the acceptance later: a mutation this run agreed was equivalent may have its guard removed, and one it merely could not reach may not.
An acceptance says which.

The third is not about the mutation at all.
**The instrument for observing it is more fragile than the thing observed.** The call site of a function whose every rule is already tested is one example: killing it means running the whole composition, and a test that runs a whole verification inside another one is one a measurement times out rather than answers.
This classification is made case by case, never from inconvenience alone.
Optional-tool discovery is not in it: the doctor suite supplies an isolated `PATH` with fake executables and observes missing entries, later entries, non-files, spawn failures, exit statuses, the working directory, and the exact environment without depending on the developer's machine.
A fragile observation is recorded here rather than accepted, because it is not a claim that the mutation changes nothing.

## Speed regression closed by this change

Speed is a release criterion, not a limitation deferred to another milestone.
The focused runs below all asked the same two-file question with `--jobs 1 --build-jobs 2 --locked --offline --trace`.
They are about `doctor.rs` and `plan.rs`, not a score for the whole workspace.
Wall time is `finished_at - started_at`, so it includes preparation, baseline verification,
and mutation execution rather than quoting only the mutation loop.

| Field | Before, 2026-09-11 | Whole-catalog validation defect, 2026-09-12--13 | Selection-aware final, 2026-09-13 |
| --- | --- | --- | --- |
| Cataloged | 12,720 | 12,743 | 13,764 |
| Executed | 88 | 94 | 89 |
| Killed | 50 | 94 | 89 |
| Survived | 38 | 0 | 0 |
| Unreached | 12 | 0 | 0 |
| Expected | 0 | 0 | 0 |
| Configured target skips | `toolchain_errors`, `toolchain_run_report`, `xtask/test/gates` | `xtask/test/gates` | `xtask/test/gates` |
| Compiler validation | not recorded separately | 5:22:56.073 | 1:04.557 |
| Wall time | 1:58:34.441 | 5:40:14.094 | 19:31.263 |
| Git HEAD at start | `6ef5a09c1e62c6311bbc3ae8b073dcd4f2302382` | `75a1145092aeb9bc26f2b059d89d3ab046d697a6` | `75a1145092aeb9bc26f2b059d89d3ab046d697a6` |
| Workspace digest | `31ce3ec1d60952979b6a13bdb7e3af3eefc94fb0ba3a1b6b02e9d4254ea39dd0` | `d2b735f9f90a53782e55958d6a8599608b1ad5d0f3a44c19724b84d775c3e444` | `e583e25f534ebf5db23e33db7130aa408ad91d5eb57fda5c711e9cd8116e429f` |
| Catalog digest | `8c000670a35b71fdcb1575ad8b77d64a8f13a2255136847996b282efb3fdad09` | `ce095b9bf5b2c15d897ddbf92b9bf2e3d2c6366a7d09796fb764d4867d4b5262` | `9e11aebe06102cf328fd4c1c1e891519e4240b5b7d7af65a82d99f3f5062ecd5` |
| `.rust-mutants.toml` SHA-256 | `3da1e45a0f631bab208e4db2509affbad5f343fd824a6d0ef1eb72d75b93bab6` | `8e4b3dd906b4d10d776a9644b2ab641e8f00b22df523a725bda838f4cd95a591` | `8e4b3dd906b4d10d776a9644b2ab641e8f00b22df523a725bda838f4cd95a591` |
| Recording | `reports/mutation/measure-changed` | `reports/mutation/20260912T202113425Z` | `reports/mutation/scope-speed-final-fresh` |

The defective run compiled all 12,743 candidates although only 94 were in scope.
Selection now reaches witness generation, guard placement, and compiler validation before any of them compile.
An intermediate run of 97 selected mutants reduced validation from 19,376 to 56 seconds and total wall time from 5:40:14 to 17:59: a 346-fold reduction in the defective phase and 18.9-fold end to end.
The final run then killed all 89 current selected mutants with no new expectation or acceptance.
Its exit 1 comes only from checking the whole workspace expectation ledger against a two-file report; the mutation rows the run selected contain no survivor or unreached result.

## Repeated baseline cost

The next repeated cost was the passing instrumented baseline.
On the current self-hosted tree a dry run executed 209 baseline targets in 670.417 seconds;
the complete preparation took 711.329 seconds.
The result passed and reached cache serialization, but this sandbox refused the final write to its read-only user cache directory.
That run is `/tmp/rust-mutants-cache-fixed-fresh-trace`; the `baseline-not-remembered` record names the failed path and OS error rather than pretending it was cached.

The same release binary was then tested through its CLI against `fixture-simple` with a writable cache.
The fresh run took 2.48 seconds and started three baseline target processes.
The identical run took 2.11 seconds,
started none of those three processes, emitted all three `verify` records with `remembered = true`, and passed `trace check`.
The absolute difference is small because this fixture's three targets take less than a second; the structural assertion is the performance contract.
A regression test holds the same rule in-process: the second exact preparation must emit the complete verify/touch evidence and start fewer processes.
The 670 seconds measured on the self-hosted suite are therefore removable on an exact repeat, but that is an inference from the process-elision proof, not a fabricated warm wall-time measurement.

## Controlled comparison with cargo-mutants

One controlled slice used the same source, three common mutations, passing baseline policy, one mutation job, two build jobs, locked offline dependencies,
and two libtest threads.
rust-mutants killed all three in 7:40.09;
cargo-mutants 27.1.0 caught all three in 7:52.63.
Maximum RSS was 653,956 KiB and 652,392 KiB respectively.
The 12.54-second, 2.65% total-time lead is real,
but it is not an overwhelming general win and this page does not present it as one.
The shipped speed claims are the measured selection regression fix and the exact-input work elision above; a universal lead over different languages and test suites is not inferred from one Rust slice.

The literal workspace and configuration digests describe the inputs to these two recordings.
This page is itself part of the workspace digest, so no page can contain its own post-edit digest as a fixed point.
Any later confirmation is identified by its recording path; the digests inside that report are the authoritative identity of the tree it measured.
