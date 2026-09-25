<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# rust-mutants architecture

**Status: implemented.** Discovery, instrumentation, compiler-validated acceptance, execution, the branch proof, the infection proof, the public API, and the command line all work; [the roadmap](../roadmap.md) says which milestone each part came from.

## Invariants

1. **The source workspace is read-only.** `Workspace::open` copies the tree into a disposable snapshot under the temporary root, at a name stable per repository root so successive runs share cargo's incremental state.
   Every build, instrumentation, and test happens in the copy.
   Symbolic links,
   devices, and other irregular files are refused, not skipped: a skipped link is a silently absent file.
2. **Instrumentation happens once.** Every compilable mutant of a file lives dormant behind a guard in the snapshot; the test binaries are built once;
   `RUST_MUTANTS_ACTIVE=<64 hex id>` activates one mutant per test process.
3. **Bytes are spliced, never pretty-printed.** Comments, whitespace, and CRLF are preserved, and every splice keeps its line count, so coverage regions and mutant positions agree line for line with the pristine file.
4. **Phases are types.** `Workspace::open → Workspace::prepare(self) → Session`;
   a session executes any number of (mutant, target) pairs without rebuilding.

## Pipeline

```text
cargo metadata (in the snapshot) ─→ pristine cargo check + dep-info
        │
   syntactic discovery (syn): candidates, site hints, skips with reasons
        │
   instrument all candidates ─→ cargo check ─→ attribute errors to branches
        │                              └─ re-instrument without them, repeat; bisect what does not settle
   verify (cargo test, no mutant active)
        │
   cargo test --no-run ─→ test binaries, outside the snapshot
        │
   [witness tree: second snapshot, checked only, branch proofs and probes]
        │
   Session: exec(mutant, target, args) / judge(mutant) / changes()
```

## What a scoped run compiles

`PrepareOptions.packages` names the members a run is about.
It narrows what is mutated, and it narrows what is built: the test binaries a run starts are the ones of the packages it is about, so `cargo test --no-run` is given those packages and nothing else.
The instrumented baseline — the tests run with no mutant active — is then those binaries too.

The type check is not narrowed.
A mutation of one package can stop being a program only where another instantiates it, so `cargo check` is always about the whole workspace: narrowing it would let a mutant that does not compile downstream reach the build instead of the rejection it belongs in.

Scoping a run is the one thing a person can do to make it shorter, and until this was so it did not shorten the longest part of one.

The gate `prepare` stands on is that check of the whole workspace, and nothing else.
The check is not only a gate: it is also what says which files each target compiles **outside** a test build, and a file no non-test unit compiled is one only the tests see.
Without it, a library whose only compilation is its own test harness would have every file read as test-only and nothing in it worth mutating.

Whether the tree *links* is a second question, and it used to be a second compilation of the whole workspace on every run.
It is not one any more: the first validation round links the instrumented tree, and an instrumented tree that links is one whose pristine form links too, because the guards only add code.
A round that fails with nothing attributable to a mutation is a round that compiles with nothing live, and that is `RM4001` — the tree, in the compiler's own words.
The answer is the same one build later, and only when the answer is bad.

`preview` — what `list` and `why-skipped` stand on — still only type-checks:
it rules on nothing, so it is answerable for a tree that does not link.

Each round instruments every mutable file, because attribution needs each file's branch spans whatever it condemns, and writes back only the files whose text changed.
`validate-round` says how many that was.

## Snapshot

`snapshot::create` copies the tree byte for byte into `<dest>/rust-mutants-snap-<16 hex of sha256(absolute root)>/tree`.
The name is stable so cargo's fingerprints survive from one run to the next, and the directory beside `tree` carries the `tempowner` lock and marker.
`tree` is the stage: the copy places everything it holds by substituting one prefix, the ancestor the measured root and every `--allow-outside` directory share becoming the stage, so the relative path between any two of them is the path they had on disk.
With no `--allow-outside` the ancestor is the root itself and the tree is the stage, which is where it has always been and is every ordinary run; with one, the tree sits at its own depth under the stage and each allowed directory at its own, and the only entries the engine makes rather than copies are the empty directories between them, which `Placement::scaffolding` names once each.
A directory found under the stable name is swept and copied into again, never adopted; a live, kept, or young unowned one makes the run fall back to a fresh name (reported through `Snapshot::stable_dir`).
`.git` at any depth is always excluded, the caller's own report directory is excluded when it names one, and so is everything `[project] exclude` names — which is why excluding a file the crate declares as a module leaves a tree that does not compile, and why a pattern that is meant to keep code out of the mutations rather than out of the tree belongs in `include`.
A symbolic link, reparse point, device, or backslash-named entry is refused with the first offending path in sorted order.

The manifest is sorted by path and hashed under the domain `rust-mutants-workspace-v1` as `enc(domain) ‖ enc(path) ‖ enc(sha256hex) …` (4-byte big-endian length prefixes).
`Snapshot::redigest` re-walks the copy with no exclusions and reports every added, removed, or changed path: the gate that catches a test writing into its own package.
Cleanup releases the lock, checks a guard (absolute, prefixed name, expected parent), and retries the removal on a 20/40/80/160 ms ladder; `keep` records the decision in the marker so the next sweep obeys it.

### The public API

`Workspace::open` sweeps the temporary area, copies the tree, and locates the toolchain inside the copy.
`Workspace::prepare` consumes the workspace and returns a `Session`: the phases are types, so a mutant cannot be executed against a tree that was never prepared.
A session is `Send + Sync` and every execution takes `&self`, so a consumer runs mutants in parallel across the targets one build produced.

The engine reads no environment variable of its own.
The environment every command and test process runs with, and the temporary directory everything is created in, are arguments ([ADR 0001](../adr/0001-seam-policy.md)); the composition root is the only place that names the process environment, and `cargo xtask devgates` refuses any other.

### The command line

`rust-mutants list` and `why-skipped` copy and type-check the workspace and stop there: fast, and honest that nothing has been ruled on yet.
`catalog` and `explain` prepare the complete catalog.
A filtered `run` keeps that same catalog and digest but instruments, proves, and compiler-validates only its selection; every other candidate remains as `not_run/unselected`, which makes no compiler-acceptance claim.
`catalog --json` prints one `rust-mutants/catalog` document.
`run` exits 0 when the tests noticed the mutant, 1 when they did not, and 2 when nothing was established, so a script can act on the answer.

`--changed` and `--changed-from <REV>` mutate only the files that differ from a revision, committed and not.
They narrow `--include` rather than widening it, and a change set that names no Rust file selects nothing rather than everything.
A change that touches no Rust file the configuration includes is `git::Within::Nothing`, which carries the files that did change; the command says so, names them, and exits 0 before a snapshot is taken, so a pull-request gate can tell an empty change from a failure and a configuration that measures too little is visible rather than silently green.
A tree git cannot be asked about, or a revision it does not know,
ends the command with `RM0010`: a run that could not see what changed must never look like a run that saw nothing change.

`--rule`, `--family`, `--skip-rule`, `--skip-family`, `--file`, `--id`, and `--from-report` are resolved once before preparation.
Discovery remains whole so identities and prefix ambiguity do not depend on the filter, while witness questions, guard placement, and compiler validation scale with the selected candidates.
The same resolved filter is then used for execution; in particular, `--from-report newest` cannot change between preparation and run.

`--features`, `--all-features`, `--no-default-features`, `--build-target`,
`--profile`, and `--build-jobs` are what the `[build]` section spells, and they reach every command a run compiles with: the pristine check, each validation round, the test build, and the coverage and witness builds.
Cargo compiles a different program for a different feature set, triple, or profile, so the report's `selection.build` says which one was measured and a stored outcome is only reused for a run compiled the same way.
The build target is `--build-target` and not `--target`, which `run` already uses to name one test target.

A killed mutant's row says which tests failed with it active (`killed_by`) and the signal the process died from, when it died from one.
Both come from the harness's own per-test lines, which is also what `mutant-exec` records.

Reach is measured by the guards, on the run that already verifies the baseline.
`RUST_MUTANTS_TOUCH` names a log; every guard records the index it was asked about against the thread that reached it, and libtest names each test's thread after the test, so the answer is per test and the measurement costs the run nothing it was not already spending.
A mutation then goes to the tests that reached it: a target no test of which reached it is not started,
and a target some of whose tests did runs exactly those in one process.
What nothing can attribute to a test — the main thread, a benchmark, a thread a test spawned — reaches every test of its target, and a set of tests that does not pass on its own takes its target off test routing altogether.
`--no-touch` asks for the answer a run with nothing removed gives.
See [ADR 0014](../adr/0014-the-guards-are-the-measurement.md).

Coverage is the second opinion, and `--coverage` asks for it.
It builds the tree once more with `-C instrument-coverage`, runs every test target once with nothing active, and reads back which target executed which regions.
A mutant is then only run against the targets that reached it, and one no measured target reaches is reported as `unreached-mutant` rather than executed against everything to find that out again.
Every export names every binary the build produced, so a test that spawns one of the workspace's own binaries has that binary's coverage attributed to it; a place no export instrumented is a place the measurement says nothing about, and its mutant is run everywhere.
What the tree's cargo configuration puts in `build.rustflags` is read and put back into that build,
because the variable it passes flags in replaces them rather than adding to them.
A tree that configures `rustflags` for a target, a configuration file nobody can parse, a build that will not instrument, and tools that are not installed each leave the measurement empty, which routes every mutant to every target exactly as if the layer were not there.

With a measurement in hand, a second proof layer removes more.
The compiler is asked which mutations change nothing outside the branch they sit in; a target the measurement placed at such a mutation and whose run never entered that branch is *discharged*, because it cannot have noticed it.
The lemma is the compiler's and the premise is the measurement's, and `prove::discharges` is the pure function of the two, so a caller with its own coverage discharges with its own evidence and an audit re-derives the decision without the engine.
Coverage regions nest: what says a body ran is a region that *begins* inside it, not one that contains it, and the region at the body's closing brace is the one the compiler emits for what follows the branch.

A third layer needs no measurement of its own beyond the one already made.
A guard holds both branches at its site, and where the compiler has vouched that the condition around it is inert — the same witnesses a branch proof rests on — the baseline evaluates both and records every time they parted.
A target that never saw them part is discharged `never-infected`, and a kept target is asked only for the tests that did.
It is the same run and the same log, so the layer is free.
A return replacement is answered the same way for a different reason: it writes a constant, so the guard compares the value the branch that keeps the original produced against that constant, with the compiler vouching for the type in the same witness tree.
See [ADR 0015](../adr/0015-the-guard-is-the-infection-probe.md) and [ADR 0016](../adr/0016-the-probe-tree-is-a-tree-nobody-needs.md).

`Session::judge` runs what the route reaches and `Session::exec` runs what the measurement placed, discharges included: a discharge is a proof a caller may not share, and `exec` is the question "what do the tests say", asked by a caller with its own evidence.

`report --format` writes a stored run for somebody else: `json` is the document verbatim, `markdown` the summary a person puts in a pull request,
`junit` one test case per mutant for a CI server's test view, `sarif` one result per finding for a code scanning view, `html` one page that fetches nothing and shows every measured file whole with its mutants on their lines,
and `stryker` the mutation testing report every Stryker reader understands,
with columns in UTF-16 as that schema counts them.
A file the report names and the root does not hold is `RM0012` rather than a file quietly left out.
`report --tui` reads it at the terminal instead: the rows on the left, and on the right the mutant, the file it is in, the findings, or the keys; `/` searches, `1`–`5` narrow, and `y` names a mutant that quitting prints on standard output, so a person can pick one and hand it to the next command.
What each projection is and who reads it is in [reports](reports.md).

`doctor` answers about this environment rather than about any code: one check per thing a run needs — `cargo`, `rustc`, `host`, `workspace`, `config`,
`temp`, `git`, `targets`, `environment`, `cache`, `disk`, `snapshots`,
`llvm-tools` — each standing `ok`, `warn` or `fail`, and each carrying the next step when there is one.
A warning is a run that costs more or measures less, never a reason not to run, so only a failure changes the exit code.
`doctor --json` answers with a `rust-mutants/doctor` document.

`diagnostics [RUN]` gathers one run into one directory: the report, the catalog, the measurement, the probe logs, the recording, the configuration, a fresh doctor document, what the toolchain says about itself, and the **names** of the variables that were set — never a value, because a bundle travels and a value that travels with it is one nobody chose to publish.
`bundle.json` says what is held and what the run did not leave, so a reader can tell a run that had nothing to say from a file that never arrived.

These two report a reserved variable instead of refusing it, because they are what a person runs to find out that it is set.
Every other command refuses an unrelated activation.
The matching instrumented self-measurement pair described below is accepted because it is the binary and catalog the outer run built.

`run --shard K/N` runs one part of the catalog, cut by index, and `merge` reassembles the parts into the report the whole would have written.
A run reads back what an earlier run of this exact tree established unless `--no-cache` is given; `cache` says what is stored and `cache --gc` removes what no run still owns.

That includes the instrumented baseline.
A passing baseline is reused only when the complete copied tree, compiled closure and manifests, engine and toolchain, build and harness arguments, selected guard and proof-marker sets,
test-target commands, working directories, and effective environments all match.
Every directly built executable is then compared byte for byte with the one that passed.
A failure, a process that wrote into the copied tree, an unreadable executable, a changed input, or a malformed or incomplete cache document is never reused.
A remembered result still emits one `verify` and `touch` record per target, marked as remembered, so the audit sees the same premises without claiming that a process ran twice.

`--trace[=DIR]` records what the command did, as JSON Lines.
A run records beside its report and every other command under `<reports>/traces/`; `trace summary`, `trace check`, and `trace diff` read one back.
A recording is diagnostic exhaust and never evidence, so a directory that cannot be created costs one line on standard error and never the command.

### What a run says while it happens

`--ui plain` writes one line per phase **as preparing reaches it**, one line per mutant as the run judges it, and the tally every ten and at the end.
Preparing is most of a long run — the snapshot, the check, the measurement,
the instrumented build — so the work runs on a thread of its own and the calling thread writes what the recorder says while it waits.
A reader shown nothing until preparing is over cannot tell a slow run from a hung one, which is the one thing a progress display is for.
`--json` says the same thing as `phase-start` and `phase-end` lines, at the same moment.
`--ui quiet` writes none of them and the summary all the same.
`--ui auto` is what a run without the flag gets, and is `plain` until there is a renderer that overwrites in place.

Preparing is not a run, so nothing observes it: the phases come from the recording, which a `Sink::Channel` tees to the command's own thread.
A recording is diagnostic exhaust ([ADR 0002](../adr/0002-trace-is-not-evidence.md)), so a display that has gone away costs the event and never the run.

`--color auto|always|never` paints the outcome word and nothing else: what a reader is scanning for is the one line that is not a kill, and a page of colours is a page nobody scans.
`auto` paints a terminal that has not set `NO_COLOR`.

### Narrowing a run

`--rule`, `--family`, `--skip-rule`, `--skip-family`, `--file PATH[:FROM-TO]` and `--id PREFIX` say which of the catalog's mutants a run is about, and `--from-report [RUN] --outcome survived` says the ones a stored run left that way.
A filter changes nothing about the catalog: the digest is the catalog's,
a stored outcome is still the same tree's, and what a filter took out keeps its row with `unselected` as its reason, so a report of a narrowed run still accounts for the whole of the catalog it was cut from.
A mutant nobody selected is not a finding.

`--fail-fast` stops at the first thing a reader has to act on — a mutation nothing noticed, one nothing could decide, one nothing reached — and not at a kill, which is the run working.
What it did not reach is `stopped-early`,
which is the run doing what it was asked to rather than a run that was killed:
the exit code is the finding's, never `130`.

`--dry-run` prepares, verifies, and then says what a run would cost: every mutant it would measure with the targets that could notice it, and the size of the job from what the verification measured.
Nothing is executed.

### What a run leaves, and what finds it again

`--run-id NAME` names a run, which is what its report directory is called;
without one the name is the moment it started.
A name is one to sixty-four of letters, digits, `.`, `_` and `-`, because it is a directory.

`--keep-temp` keeps the snapshot and the build cache a run would otherwise remove, and writes them into `kept-v1.json` under the report directory, with the run that kept them.
`cache` lists what is there: the snapshots, the build caches, the outcome store with its size, and every kept directory with the run that kept it.
`cache --gc` removes what is abandoned and leaves the build caches, so the next run is still fast — except the ones no run can look up again, which every run already sweeps on its way past: a cache is keyed to a source tree, and one whose tree is gone will never make anything fast.
`--gc --all` takes the rest too; `--gc --kept` takes what was kept on purpose.
`--clear-outcomes` empties the store and says how much was in it, and `--cache-dir` says where the store is.
A ledger this release cannot read authorises nothing: it is read as empty, so a sweep never removes a directory on the strength of a document it did not understand.

`merge --root DIR --runs a,b` finds the parts of one catalog under a report directory rather than being handed their paths, and a `--runs` value may be a glob.

`replay <prefix>` puts one finding back to the tests as the run that found it did: the target and the test come from the stored report rather than from a guess, so a replay asks the question the run asked rather than a wider one,
and says whether the answer is still the same.

## Guards

Four forms.
**Form C** for a position that is syntactically boolean (an `if` or `while` condition, an operand of `&&`/`||`, a match guard):
`__rm::value!(__rm::active(3) && __rm::value!(a >= b) || !__rm::active(3) && a > b)`.
**Form E** for any expression in value position:
`__rm::value!(if __rm::active(5) { a - b } else { a + b })` — both branches unify to one type, so `Default::default()` is inferred from the original.
`value!` expands to exactly its expression: it gives the parser one grouped expression without a function-call type-inference boundary, a temporary scope, or lint-producing parentheses.
**Form S** for a statement: `if __rm::active(7) { x -= step; } else { x += step; }`, the original bytes in the `else` so lines are kept.
**Form M** for a match arm that has no guard, which is the one shape that adds syntax rather than replacing it: the site is the pattern, kept verbatim, and the guard is written after it — `0 if (__rm::active(9) && (false) || !(__rm::active(9)) && (true)) =>`, where the branch that keeps the arm is the guard it did without.
A position where wrapping would move a value out of place — an assignment target, a borrow operand, a method receiver, a scrutinee —
escalates to its parent expression, then to the statement.

The runtime is a private `mod __rm` appended after the last line of each instrumented file ([ADR 0011](../adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)).
No lint attribute is put on user code.
The private generated support module has one exact `#[allow(dead_code, unused_qualifications)]`: one shared runtime serves files that use different subsets of it, and its collision-proof standard-library paths are deliberately fully qualified.
A crate that `forbid`s either lint (or its `unused`/`warnings` group) is refused before instrumentation because Rust does not permit the module to lower a `forbid`.
Because no lint attribute is put on user code, every call planted into it — a guard, a body marker, a step checkpoint — has to be one that code passes on its own.
Each names the runtime with the fewest `super::` segments that reach it: none where every inline module out to the file root does `use super::*`, since the name is then already in scope and a qualified call is one `unused_qualifications` calls unnecessary, and one per module out to the first that does not.
The runtime's own code uses every value it computes, so a project that denies `unused_results` compiles it too.
`fixtures/fixture-strict-lints` denies every lint this code could trip and is instrumented on every test run.

### What instrumentation writes

`rust_mutants::instrument` composes the guards and appends a `mod __rm` to each rewritten file.
Alternatives of one site share one chain, nested sites become nested guards composed children first, and only the branch that keeps the original carries the guards inside it.
Each alternative is folded onto one line and the original branch keeps its bytes, so a guard holds exactly as many line breaks as the bytes it replaced and every byte stays on its line.
Guards are lint-clean macro expressions and the user's lint policy applies to every alternative exactly as it applies to a standalone edit.
The runtime reads `RUST_MUTANTS_ACTIVE` once per process; a `RUST_MUTANTS_CATALOG` that is not the one the tree was built from ends the process with exit 97, and an identity this file does not know activates nothing here because it belongs to another file.

### How a candidate becomes a mutant

`rust_mutants::validate` compiles the instrumented tree and reads the diagnostics.
Every alternative occupies a known byte range, so an error whose primary span falls inside one is about exactly that mutant: a whole round's refusals are condemned at once, and the loop costs one recompilation per round rather than one per candidate.
An error that falls outside every branch is isolated by bisection instead, and a tree that does not compile with nothing live stops the run (`RM4001`) rather than blaming candidates until the error goes away.
What comes back is always a tree that compiled, plus a rejection per refused candidate carrying the compiler's own words.

Whether an edit is a program is a fact about the toolchain, never assumed:
`fixtures/fixture-rejectable` records that this compiler refuses `String - &str` and a `RangeInclusive` where a `Range` belongs, and accepts `value / 0` for a run-time `value` — a mutant that dies at run time rather than at compile time.

## Validation, stated

A round instruments every candidate that is still live, compiles, and reads what the compiler said.
An error is attributed to the mutant whose branch holds one of its spans — the primary one first, then the others, then the spans of its notes, because the compiler points at the place it decided and for a type error that is often the definition rather than the edit.
The mutants an error names are condemned, the file is written again without them, and the next round follows.

What no span names is what bisection is for.
It halves the live set, and a half that still fails is halved again, so an offender the compiler refuses on its own is found in a logarithmic number of builds.
When neither half fails, the offence straddles them — a combination only ever seen with mutants from both sides live — and narrowing by increasing granularity finds the mutants that interact rather than condemning everything that happened to be live.
The search is bounded; running out of the budget condemns what is left, which is the same answer halving alone gave.

Every offence bisection names is then compiled once more on its own, so the report carries the compiler's words about that mutant rather than a sentence saying there were none.
A row says `isolated` when the compiler refused it alone, and names the mutants it was refused with when it did not.

A build nobody waited for is a cancellation and not a tree that does not compile: `Ctrl-C` during a round ends validation with `RM0001`, and nothing is condemned on the strength of what a half-finished command printed.

## Skips, stated

`const-context`, `macro-invocation`, `cfg-attribute`, `test-code`,
`unsupported-site`, `excluded`, `test-only-file`, `no-std-crate`,
`included-expression`, `generated-outside-workspace`, `forbidden-lints`,
`const-fn-body`, `let-condition`, `open-range`, `unstated-return-type`,
`loop-value`, `annotated`, `configured`.
Each is counted and named;
`rust-mutants why-skipped` lists them.
A skip is a decision the tool made and says; a rejection (a mutant the compiler refused) is a fact about the program and is reported with the diagnostic.

A `rust-mutants: skip <reason>` comment is the one skip an author writes.
Sharing a line with code it hides what starts on that line; alone on a line it hides what starts on the next one, item, statement, arm or `else` block and all.
The reason is required (`RM2008`), `skip` is the only directive (`RM2009`), and a marker that hid nothing is an `unmatched-skip` finding rather than a comment nobody notices is stale.
Markers are read from the gaps between tokens, so the words inside a string literal are a string and a documentation comment, which is an attribute by then, is never a marker.

`[[mutation.skip]]` says the same thing from the configuration file, where the code cannot be edited or where one entry covers what a hundred markers would: a path glob, a reason, and at most one of a line range or an item.
It is applied where the walk's own decisions are, so every tally says the same thing about the file, and an entry that hid nothing is the same `unmatched-skip` finding a stale marker is.

Every place a rule targets has a decision: a candidate with its guard form, or a skip with its reason.
A rule that passed over a place without saying so is what `crates/rust-mutants/tests/census.rs` refuses, and it is why a `const fn` body, a condition that binds with `let`, and a range with no end are reasons of their own rather than silence.
Two decisions carry a note instead of a reason of their own: `identical-replacement` where a rule's replacement is what is already written, and `text-mismatch` where the bytes at the span are not the operator the rule expects.

The per-file walk (`rust_mutants::syntax`) keeps walking inside a region it will not mutate and counts every candidate it would have produced under the outermost reason, so the tallies say how much code each reason hides.
A macro invocation counts once, since its body is tokens the walker does not parse.
Whole-file reasons (`excluded`, `test-only-file`,
`no-std-crate`, `generated-outside-workspace`, `forbidden-lints`) are decided by the workspace layer from cargo metadata and dep-info, not by the walk.
A file a build script A crate is `forbidden-lints` when its root, or the `[lints]` table cargo builds it with, forbids one of the lints the guards' own attribute turns off:
`forbid` is the one level an `allow` cannot override, so a guard there is a compile error whatever it edits, and every mutant of the crate would otherwise be refused with nothing saying why.
A `deny` is fine, which is what the attribute is carried for.
A file a build script wrote is named `<generated>/<its own name>`: the directory it was written to is different on every machine and every run, and naming it would say where this run put its temporary files rather than which file was passed over.

## Execution

A test binary is started directly, never through `cargo test`, with the environment cargo would give it, `RUST_MUTANTS_*` stripped and set, a scratch `TMPDIR`, and the libtest arguments verbatim.
The outer supervisor owns the timeout and the platform's declared process set; exit status is read in this order —
start or wait failure → `errored`; cancellation → `not_run`; a bound expired → `waited`; a verified monitor notice → `step_limit_reached`; a signal or non-zero status → `killed`; zero → `survived`.
The runner represents those as one closed `Termination`, so a result cannot be both timed out and exited.
A libtest run that matched no test is inconclusive and says so:
`tests_run` carries the count the summary line reported.

The step runtime does not claim an exit status.
The selected guard activates one process-wide state, and subsequent non-const function entries, loop bodies,
async blocks and closure invocations in every rewritten workspace file advance it.
At the first boundary past the allowance it writes an atomic side-channel record containing a fresh 128-bit nonce, the catalog and mutant identities, the allowance and `N + 1`, then parks.
The supervisor kills its declared process set only after the final file appears; the execution layer validates every field before reporting the step fact.
A stale, partial or mismatched record becomes `errored`, and the reserved protocol-failure status follows that typed failure path rather than a mutation verdict.

`StepBoundaryScope::InstrumentedWorkspaceSource` states the exact limitation:
macro expansions and dependencies are not rewritten.
Expression-body closures are wrapped so an external iterator calling back into workspace source still advances the state.
A loop that remains wholly inside an expansion or dependency has no such boundary and may reach only the clock (`waited`), never a fabricated step fact.
The fresh nonce is a correlation token passed to the subject process,
not authentication material; exclusive scratch, no-follow state access, exact structural binding and fail-closed parsing provide the protocol boundary.

`SupervisionBoundary` states the platform containment limit separately.
Windows uses an inescapable Job Object (`ContainedTree`).
POSIX uses an inherited process group (`InheritedProcessGroup`): descendants remain supervised unless they deliberately call `setsid` or `setpgid` to leave it.
The leader remains waitable until the group has been forcefully signalled, so its numeric process-group id cannot be recycled into an unrelated process before supervision is released.
This POSIX boundary establishes forceful signalling, not kernel quiescence: a member in an uninterruptible kernel wait may remain until the kernel can finish it.
Windows Job Objects provide the stronger contained-tree lifetime boundary.

### The scratch a test process is given

`TMPDIR`, `TMP` and `TEMP` all name a directory of that execution's own, so two test processes running at once cannot meet in one another's files.
The name is short on purpose: a Unix socket bound under `TMPDIR` has to fit in `sun_path`, which is 104 bytes on macOS and 108 on Linux, and the temporary root alone spends about forty of them.
A test that binds one would otherwise pass on its own and fail under a run, which is a finding about this engine and not about the suite.

So the directory is `rm-scratch-<n>/<execution>` beside the run's target directory rather than inside it, where `<n>` is the lowest number no other run holds.
The long descriptive name stays on the target directory, which is a build cache somebody may find in a temporary root and has to be able to identify; a scratch directory is worth nothing once its run is over, so it is claimed like a run's own directory and the next sweep collects it.

### What cargo tells a test process

A test process the engine starts is told what cargo would have told it:

| Variables | From |
| --- | --- |
| `CARGO_MANIFEST_DIR`, `CARGO_MANIFEST_PATH` | the package's manifest |
| `CARGO_PKG_NAME`, `CARGO_PKG_VERSION`, and `VERSION_MAJOR`, `VERSION_MINOR`, `VERSION_PATCH`, `VERSION_PRE` | the version, cut the way cargo cuts it |
| `CARGO_PKG_AUTHORS`, `DESCRIPTION`, `HOMEPAGE`, `REPOSITORY`, `LICENSE`, `LICENSE_FILE`, `RUST_VERSION`, `README` | the manifest, as the empty string where it says nothing, which is what cargo does |
| `CARGO_BIN_EXE_<name>` | the file the build produced, for an integration test or a tested example. Composing it from a profile name and a target name guesses at both and on Windows guesses wrong |
| `CARGO_TARGET_TMPDIR` | the build directory, for an integration test or a tested example |
| `OUT_DIR` and every `cargo::rustc-env` value | the package's own build script, from the `build-script-executed` message |

### The environment a test process gets

A test process inherits the environment the run was started with, plus what cargo sets for its target, minus the four variables a run composes for itself: `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG`, `RUST_MUTANTS_TOUCH`,
and `LLVM_PROFILE_FILE`.
The first three decide which mutation is active and what the guards are to record, and an inherited one would make every answer be about somebody else's run — or append this run's touches to a file another run is about to read.
The fourth is there because a measurement *of this engine* sets it: an instrumented test process that inherited it would write over the very measurement that started the run.
Removing it is not enough on its own, since an instrumented binary with no path writes `default_*.profraw` into its working directory, which is the snapshot being measured and which the drift check would then report as the project's own tests writing into their tree.
So a run puts a path of its own in its place, under the temporary directory that execution owns.
The coverage pass puts its own path there instead, per target.

Only the first three are refused on the command line.
An instrumented engine binary is the narrow exception: its composition root embeds the catalog digest Cargo supplied as `RUST_MUTANTS_COMPILED_CATALOG`, and accepts an inherited `ACTIVE+CATALOG` or `TOUCH+CATALOG` pair only when that digest matches and only one mode is present.
This makes a child process part of the outer measurement without licensing a normal binary or a stale environment.
A run under `cargo llvm-cov` is an ordinary thing to want; a run under somebody else's activation is not.

## The run

The engine drives its own runs.
`rust_mutants::run::run` walks the accepted mutants a shard holds, asks `Session::judge` about each, reuses what an earlier run of this exact tree established, records a route for every one of them, and hands back what it decided; `rust_mutants::report::run::document` turns that into the document, and `merge` puts the parts of a sharded run back together.
The command line renders.
Nothing about a run's policy — what a finding is,
what the exit code says, which mutants a shard holds — lives above the engine any more, so a second consumer of the engine gets the same answers rather than a second implementation of them.

A caller hears about a run through an `Observer`, whose every method is called on the calling thread and has a default that does nothing, so an observer implements only what it draws.
`Silent` draws nothing.

A run measures `jobs` mutants at once — `auto`, as many as the machine has capped at four; `all`, every processor, for a runner doing nothing else; or a count — and delivers each as it finishes rather than in catalog order: one mutant that runs for its whole budget would otherwise hold back every result behind it, and a progress line, a stream, and a stop-at-the-first-finding would all wait on it.
The report is put back into catalog order when it is written, because that is the order a reader compares two runs in.
A run that is cancelled leaves every mutant it never claimed as not run, `interrupted`.

`Route::narrowing` is the one place a narrowing is decided, and both the route a report shows and the targets an execution runs come from it.
An execution that narrowed by anything else would run fewer targets than the route says,
and a survivor it reported would be one nobody measured — which is what happened to a target whose profile the measurement could not read: the route kept it and the execution dropped it.

A mutant that was never executed says why: `unreached`, `discharged`, or `interrupted`.
A process the runner never got a status from is `interrupted`,
not a mutation no test can notice.
Its row also carries the route — which targets could have noticed it, which a proof removed, and which of them ran — so a reader can see a proof layer remove work without a recording.

## Describing a session

A session owns a snapshot and the processes that run in it, so it cannot be reopened: what a later command reads is `Session::describe` — the catalog, the targets, the two digests, and the toolchain, all of it a document — and the stored outcomes.
`Session::source` hands back a mutable file as it was before instrumentation, because a report names the bytes an edit replaces and showing somebody the edit needs the file they would open rather than the rewrite the snapshot holds.

A catalog on the wire names each rule and the version that entered every identity in it.
A name this release does not know, or a version it does not agree with, is a catalog it refuses to read: every identity in it was minted from the version it names.

## Identity

```text
id = SHA-256( enc("rust-mutants-id-v1") ‖ enc(path) ‖ enc(rule) ‖ enc(rule version)
              ‖ enc(start byte) ‖ enc(end byte) ‖ enc(source sha256) ‖ enc(original sha256)
              ‖ enc(replacement sha256) )
enc(s) = 4-byte big-endian length ‖ UTF-8 bytes
```

The same recipe as go-mutants with this domain string, so an identity is reproducible from the catalog fields alone in any language.
`display_id` is the first twenty hex digits; a prefix of four or more resolves a mutant.

## Trace

Every decision the engine takes — every site's form and skip reason, every validation round and the diagnostic that condemned each mutant, every bisect step, every build, verification and execution, and the route every judged mutant took — is recorded to the sink `OpenOptions` names.
See [trace](trace.md).
