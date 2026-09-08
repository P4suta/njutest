<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Upgrading rust-mutants

**Status: implemented.** Each section says what changed for somebody who was
already running the release before it, and what to do about it. A change that
needs nothing is not listed.

## Upgrading to E11

**`never-infected` no longer needs the probe tree for a mutation inside an
inert condition.** A guard holds both branches at its site, and where the
compiler has vouched that evaluating either runs none of the program's code —
an operator the connectives of an `if` or `while` condition reach — the
baseline evaluates both and records every time they parted. A target that
never saw them part is discharged, and a kept target is asked only for the
tests that did. It is the same run that already records reach, so the layer
costs nothing: no second tree, no second build, and the answer is per test
rather than per target. `--probe` still builds the probe tree, which is what
answers for everything the syntax cannot call inert.

**`touched-v1.json` gained `narrowing`.** It says which mutants the tree that
ran could record anything about: which guards compare their two branches, and
which markers the instrumenter actually wrote. Without it an absence in
`bodies` or `infected` is silence rather than evidence, and
`cargo xtask engine-audit` now re-derives both kinds of discharge from the
record instead of asking for a coverage build or a probe log. A reader of the
file from an earlier release sees one more key; the version stayed 1.

**The compiler vouches for a comparison of text.** The sealed trait the
witness pass puts to it named the primitives alone; it now names every type
whose comparison the standard library defines — `str`, `String`, `OsStr`,
`OsString`, `Path`, `PathBuf` — and an array, slice, `Vec` or `Option` of
something it already covers. Comparing two of those runs none of the program's
code, cannot panic, allocates nothing, and terminates, which is the whole of
what a claim needs, so `if name <= "m"` earns the same branch proof as
`if a <= b` on two integers. The two operands are asked about separately as
well, so `if label == "target" && rank <= 3` no longer loses the `<=` claim to
the `String` beside the `&str`. A type of your own is refused as before, on
either side.

**A witness check that failed and named nothing now vouches for nothing.** An
error inside a condition's witnesses refused that claim and an error inside a
body's marker refused only the marker; an error in neither was passed over,
which read a tree that would not compile as a tree with nothing wrong in it
and granted every claim. `prove::refusal` is the rule, and a failure it cannot
account for costs the run every proof. Runs made before this release could
have discharged a mutant on a check that never happened.

**A guard between `==` and `!=` no longer records anything.** The two part on
every evaluation, so the record could only say that they did. Such a mutation
gets no witness statement and no comparison call, which is one less thing in
the tree the witness pass checks and one less call on the baseline.

**A body inside a guard's own site is no longer discharged by silence.** The
instrumenter cannot splice a marker into a body a guard writes twice, and the
run used to rest `branch-never-taken` on a marker that was never in the tree.
Such a body now falls back to the coverage region, which is what `--coverage`
is for; with no coverage measured, its targets run. Runs made before this
release may have reported a survivor as discharged in that one case.

## Upgrading to E10

**A mutation goes to the tests that reached it.** The guards of the
instrumented tree record which of a target's tests reached them, on the run
that already verified the baseline, and libtest names each test's thread after
the test. A target no test of which reached a mutation is not run at all, and
a target some of whose tests did runs exactly those in one process. Nothing
about the outcomes changed: `--no-touch` asks for the answer a run with
nothing removed gives, and the differential harness holds every combination of
the two measurements to it. See
[ADR 0014](../adr/0014-the-guards-are-the-measurement.md).

**`[mutation] coverage` is off.** The guards measure reach on a run the engine
was making anyway, so the coverage build — which rebuilds every crate in the
dependency graph, because the flags reach it through `RUSTFLAGS` — is no
longer what a run makes to find out. Pass `--coverage` for the second opinion,
for the branch proofs of the bodies no marker could be written into, and for
the differential harness. A run that was passing `--no-coverage` can stop.

**`RUST_MUTANTS_TOUCH` is reserved.** A run composes it for every test process
it starts and refuses to inherit it, exactly as it does the other three. A
process that finds it already set ends with `RM0006`.

**The `WORK` line counts tests as well as pairs.** A pair is a process; a test
is what the process was asked for, and the two fall separately. The dry run
says the same, and `xtask/work_ceiling.txt` has two more columns.

**`touched-v1.json` goes beside the report.** It is what the guards recorded,
and `cargo xtask engine-audit` re-decides every route from it without the
engine.

**`branch-never-taken` no longer needs a coverage build.** The instrumenter
writes a marker at the first statement of every body a claim names, and the
one `cargo check` that already sifts the type witnesses sifts the markers too.
A target nothing of which ran the marker is discharged, and a kept target is
asked only for the tests that entered the body. A body no marker could go
into keeps the coverage region as its premise, which is what `--coverage` is
still for.

**Two limitations are new**, both of which run more rather than less:
`touch-not-recorded` for a target the engine does not start itself, and
`touch-log-unreadable` for a record that did not read back. A set of tests
that only passes beside its neighbours is noted `test-routing-unsound` and its
target runs every test it has.

## Upgrading to E9

**A run says how much work it did.** The `WORK` line counts pairs — one mutant
asked of one target, which is one test process — and says what removed the
rest. `--dry-run` leads with the same count and ends with the duration, marked
as the guess about your machine that it is. Nothing about the exit code or the
report's other columns changed.

**The outcome cache is keyed on what could change the answer, not on the
tree.** It used to be keyed on the digest of the whole workspace and of the
whole catalog, so a note beside the code, a workflow file, a crate the run
never compiled, or one more mutant anywhere threw away every remembered
answer. It is now keyed on the pristine sources every unit of the build
actually compiled, the manifests and lock file that chose its dependencies and
flags, and **the toolchain** — which the old key did not cover at all, so an
outcome established under one compiler could be reused under another. Every
record written before this release stops answering once, and after that a
partial edit costs a partial cache rather than all of it.

**A run says what it is preparing while it prepares it.** The phase lines used
to be collected and printed once preparing was over, which on a real workspace
is minutes of silence — and preparing is where a long run spends most of its
time. They are written as each phase ends now, for `--ui plain` and for
`--json` alike.

**A run builds without debug information.** Nothing here reads a backtrace —
a verdict comes from what the test harness printed — and the debug information
is most of what a build writes: **6.6 GB against 1.1 GB** for this
repository's own engine, every byte of it generated, linked, and thrown away
with the temporary directory. It is also most of what a build holds in memory
while it links, which is what an engine run runs out of first. `[build] debug
= true` puts it back, for attaching a debugger to a snapshot `--keep-temp`
preserved. A profile you named with `--profile` is one you meant, and nothing
overrides it.

**A tree that does not link is `RM4001` rather than `RM5001`.** The gate used
to be a check of the whole workspace and then a second compilation of it to
find out whether it links — a whole build, on every run, for a question the
first validation round answers anyway. The round links the instrumented tree,
and a round that fails with nothing attributable to a mutation is a round that
compiled with nothing live. Same words from the compiler, same remedy, one
build fewer on every run that works.

**A measurement is remembered between runs.** Coverage is a function of the
sources, the manifests and the toolchain, and a mutation changes none of them,
so a run of a tree nothing has touched reads back what the last run measured
instead of instrumenting and rebuilding the whole dependency graph. That is
the largest single piece of work this release removes. `cache` names the
store; `--no-cache` turns it off along with the outcome store.

**A build whose dep-info cannot be read remembers nothing** rather than filing
every mutant under one name. That is slower and it is the only honest answer.

**The run report gains `targets`**, the list of test targets the run built. It
is what says how many pairs a whole run would have started; without it a
reader cannot say what was removed.

## Upgrading to E8

**A run says what it is doing while it does it.** `--ui plain` writes one line
per phase, one per mutant in completion order, and a tally; `--ui quiet` is
the old behaviour. `auto`, which a run without the flag gets, is `plain`.
`--color auto|always|never` decides colour, and `NO_COLOR` or a pipe both mean
never. If you were parsing standard output, parse `--json` instead: one
`rust-mutants-run-stream-v1` object per line, with a reader shipped as
`rust_mutants::report::stream::read_line`.

**`--fail-fast` and the filters.** `--rule`, `--family`, `--skip-rule`,
`--skip-family`, `--file PATH[:LINE[-LINE]]`, `--id PREFIX` and
`--from-report [RUN] --outcome survived` narrow a run after the catalog, so
the identities and the catalog digest do not change. A mutant a filter dropped
is `not_run` with reason `unselected`, and one `--fail-fast` never reached is
`stopped-early`; neither is a finding. `--dry-run` prepares, verifies, and
prints the estimate without executing a mutant.

**`cache --gc` keeps the build caches.** It used to remove them. `--gc --all`
removes every build cache no live run has locked, and `--gc --kept` removes
what `--keep-temp` was asked to preserve. If a script relied on `--gc`
reclaiming everything, it wants `--gc --all` now.

**`doctor` answers about more, and grades what it finds.** Every check carries
a `status` of `ok`, `warn` or `fail` and a `remedy` when there is something to
do; `ok` on the document stays true through a warning, and the exit code
follows it. The new checks are `git`, `targets`, `environment`, `cache`,
`disk` and `snapshots`. A consumer reading only `ok` per check is unaffected.

**`doctor` and `diagnostics` no longer refuse to start under a reserved
variable.** Every other command still does. These two report it instead,
because they are what a person runs to find out that `RUST_MUTANTS_ACTIVE` is
set.

**Three more report formats, and a page that shows the code.** `--format
markdown`, `junit` and `sarif` join `lines`, `json`, `html` and `stryker`.
The HTML page now shows every measured file whole with its mutants on their
lines, and carries one inline script that searches, filters and sorts. A file
the report names and `--root` does not hold is now `RM0012` rather than a file
quietly dropped from the projection — if you generate a Stryker or HTML report
from a checkout that does not hold the sources, pass `--root` at the tree the
run measured.

**Stryker's `killedBy` names the tests, not the binary.** It carried the test
target's id; it now carries the tests that failed with the mutation live, and
falls back to the target only when the harness named none. `[reports.stryker]
high` and `low` set the thresholds that projection declares. Nothing in this
engine reads them back.

**The terminal browser has a source pane and a search.** `p` shows the file,
`/` searches, `1`–`5` narrow, `F` shows the findings, `?` shows the keys, and
`y` names a mutant that quitting prints on standard output. `Esc` now puts a
pane away before it leaves.

**New commands.** `rules` lists every operator with the tier and version that
pin it, `diagnostics` gathers a run into one directory for a bug report, and
`replay` puts a recorded kill back in front of the tests. `run --run-id NAME`
names a run and its report directory, and `merge --runs` finds the parts by
name or by glob.

**`[reports.stryker]` is the one new configuration key.** Everything else on
this list is a flag. `rust-mutants init` writes a skeleton that now covers
every key the reader accepts.

## Upgrading to E7

**Eighteen more operators, in three more families.** The table went from
fifty-one rules in twelve families to sixty-nine in fifteen. `balanced` gained
`negate-bool-method`, `return-err-default`, `match-arm` and `control-flow`;
`strong` gained eight iterator and slice swaps; `all` gained
`delete-else-branch` and the `literal` family. A run of the same tree will
find survivors it did not find before — that is what the operators are for —
and `[mutation] operators = [...]` pins exactly the rules you had if you want
the old set back. The catalog digest changed, so stored outcomes are cold once.

**A return type the syntax cannot default no longer produces a candidate the
compiler refuses.** `-> impl Trait`, `-> &mut T`, a raw pointer, a function
type, a type a macro writes, a generic parameter nothing bound to `Default`,
and `Box`/`Rc`/`Arc` around a trait object are stated as
`unstated-return-type` instead. Mutants you had accepted at those places are
gone, and their expectations will read as `unmatched` until you take them out.
A `&str`, a `&[T]`, and the arguments of an `Option` or a `Result` that spell
a default keep their return replacement.

**Every branch of a returned `if` or `match` is a return site.** A function
that chooses between three answers carries four return replacements now rather
than one. The whole-expression mutant keeps the identity it had.

**A method swap is guarded at the end of the chain its call is a receiver
in.** `skip` and `take` do not produce the same type, so the two branches of a
guard at the call have no type to be. Nothing about the mutations changes;
where the guard sits does.

**`rust-mutants: skip <reason>` in the source, and `[[mutation.skip]]` in the
configuration.** Both hide places, both require the reason, and both report a
claim that hid nothing as an `unmatched-skip` finding, which is a finding your
run did not have before. A file whose markers all still match is unaffected.

**An expectation can be addressed by a locator.** `path`, `item`, `rule` and
`original`, with `line` as a hint, name a mutation by where it is rather than
by an identity the next edit to the file will change. Existing `id` claims
keep working; never write both forms in one entry.

**Reports and recordings carry more.** An expectation row gained `locator`, a
site record gained `note`, the trace vocabulary gained `skip-claim`, and the
findings enum gained `unmatched-skip`. All are optional additions at schema
version 1; a reader from this release or later accepts them.

## Upgrading to E6

**A run compiles what you tell it to.** The `[build]` section and its flags —
`--features`, `--all-features`, `--no-default-features`, `--build-target`,
`--profile`, `--build-jobs` — are passed to every command a run compiles with.
`--build-target` rather than `--target`, because `run --target` already names
a test target. The report's `selection.build` says what was used.

**Stored outcomes go cold once.** The cache key now covers the build
configuration, because the same tree compiled two ways is two programs and a
record kept for one of them answers nothing about the other. The first run
after this release re-executes everything; the ones after it reuse as before.

**A report says which test noticed a mutation.** A mutant row gained
`killed_by` and `signal`, and a `mutant-exec` recording gained the same two.
Both are optional and the schema version stayed 1.

**`build.rustflags` no longer stops the coverage measurement.** The flags a
cargo configuration file sets are read and put back into the instrumented
build, so a tree that sets one is measured rather than routed everywhere.
Flags set for a *target* (`[target.<triple>]`, `[target.cfg(…)]`) still refuse
the measurement: which of those tables apply is cargo's decision about the
target being built. The order the files are joined in was wrong and is now
cargo's own — the home directory first, the nearest file last.

**Exit code 98 from a test process is an error, not a kill.** It is the code a
probe process ends with when it cannot record what it saw, and reading it as a
failing test credited a test that never ran.

**A cancelled build is a cancellation.** `Ctrl-C` during a compilation used
to be read as a tree that does not compile: the half-finished command printed
nothing a diagnostic could be attributed to, so validation bisected and
condemned mutants nothing had refused. `cargo::CargoErrorKind::Cancelled` and
`ValidateError::Cancelled` both carry `RM0001`, and the run exits 130 having
written nothing.

**Attribution reads every span of a diagnostic, not only the primary one.**
The compiler points at the place it decided, which for a type error is often
the definition rather than the edit; the edit is named by another span of the
same message, or by one of its notes. Runs that used to bisect for such an
error now attribute it directly, which is faster and names the mutant with the
compiler's own words. Nothing that named no branch at all is attributed.

**A refusal says whether the compiler refused it on its own.** Rejection rows
gained `isolated`. Bisection compiles each offender it isolated once more,
alone, so the row carries the compiler's words about *that* mutant rather than
"no diagnostic named it"; a `bisect` recording says how many it could. When
neither half of a suspect set fails, the offence straddles them, and narrowing
by increasing granularity finds the mutants that interact rather than
condemning everything that was live. Those rows have `isolated: false` and say
which other mutants they were refused with.

**Coverage is measured by default.** `[mutation] coverage` is on and
`--no-coverage` turns it off. A small crate's first run is slower by one
instrumented build; every run after it is shorter, because a mutation no test
reaches is reported as `unreached` rather than executed against every target
to find that out again. Reports that used to say `survived` for such a
mutation now say `not_run` with `unreached`, which is the stronger answer: the
tests have a gap where the mutant is.

**A run keeps what an audit re-derives its proofs from.** `reached-v1.json`,
`catalog-v1.json`, and a copy of every probe log are written beside the
report, and an `evidence` recording names each with its digest. `--probe` asks
each test what it would have noticed, so a target that ran a mutation without
its value ever differing is not run against it.

**A mutation the tests run and cannot observe is discharged.** With coverage
on, the engine asks the compiler which mutations change nothing outside the
branch they sit in, and a target whose measured run never entered that branch
is removed from what could have noticed it. A mutant every target is
discharged from is a `discharged-mutant` finding and an accounting column of
its own. `Session::exec` is unchanged: it runs what the measurement placed,
discharges included, because a discharge is a proof a caller may not share.

**A test that writes into the tree during the coverage pass is drift.** The
reseal that absorbs the instrumentation used to absorb that too, so the drift
report said nothing about it.

**A target the coverage measurement could not read is run.** The route always
said so; the execution narrowed by the measurement alone and dropped it, so a
mutation only that target could have noticed was reported as surviving without
anything having measured it. Both now come from `Route::narrowing`. Runs on
trees where every profile was readable are unaffected.

**A run measures four mutants at once.** `[execution] jobs` and `--jobs`/`-j`
say how many; zero, the default, is as many as the machine has capped at four.
Results are delivered as they finish rather than in catalog order, so the
progress lines of a run are no longer in index order; the report still is.

**The driver is the engine's.** `run`, the outcome store, the run and catalog
documents, and everything a run's policy decides now live in
`rust_mutants::{run, outcomes, report}`; `rust-mutants-cli` re-exports them at
the paths it used before, so a consumer of the library sees them move and a
consumer of the command line sees nothing. A mutant row gained
`not_run_reason` and `route`, both optional.

**`xtask report-diff` reads a mutation run's report.** It compares the
accounting, the score, the findings, and each mutant's outcome, and says
nothing about how long the run took: the same tree measured twice takes two
different amounts of time and establishes one thing.

**The default timeout is derived, not five minutes.** `[mutation] timeout`
now defaults to `auto`: five times what that target's own baseline took, never
below thirty seconds, falling back to five minutes for a target nothing
verified. A duration still pins it, `--timeout auto` asks for the derived one,
and a `mutant-exec` recording says which of the two a run used. Stored
outcomes are keyed on the word rather than on a number of milliseconds, so a
tree configured with `auto` reuses across machines whose baselines differ.

**A tree that does not link is refused before any round.** The gate is a
check of the whole workspace and then a test build of the packages the run is
about. A tree that type-checks and fails at link time used to be accepted,
instrumented, and then fail every round, where the failure reads as a mutation
the compiler refused. `list` and `why-skipped` still only type-check.

**A snapshot directory that is already gone is removed.** Cleanup released the
lock, then retried five times with a backoff against a directory a sweeper had
already taken, and reported `RM1011` for it.
## Upgrading to E5

**A run can now record what it did.** `--trace` takes an optional directory.
Without one, a run records under `<reports.directory>/<run id>/trace/` and
every other command under `<reports.directory>/traces/<run id>-<command>/`.
Both places are pruned by `reports.keep`, counted apart, so a recording never
costs a stored report its place. Nothing records unless `--trace` is given,
and a directory that cannot be created costs one line on standard error and
never the run.

**Reports carry three more fields per mutant and one more per refusal.** A
mutant row gained `start_byte`, `end_byte`, and `source_digest`; a refusal
gained `index`. Both schemas gained them in the same release, and the version
stayed 1: the rule is that an optional field added with its schema entry is
the same version. A reader that refused an unknown field will need the reader
from this release or later; the engine's own readers stopped refusing one.

**The findings enum gained `unreached-mutant`.** The engine has reported it
since coverage routing shipped and the published schema refused it, so a
consumer validating against `rust-mutants-run-report-v1.json` from an earlier
release would have rejected a valid coverage run. Take the schema from this
release.

**The exit code of a merged run changed.** `merge` derived the code from the
accounting columns and returned 2 for any run with mutants that were not run,
which made a coverage run whose only finding is `unreached-mutant` look like a
run that broke. It now derives the code from the findings, as an unmerged run
always did: a mutation nothing reaches is a gap in the tests and exits 1.

**An inconclusive mutant says which of the two things left it undecided.** The
detail said "timed out once and did not do so again" whether or not anything
had timed out. It now says that only after a retry, and otherwise says that no
test ran with the mutation active.

**`prune` no longer counts a directory that holds no run report.** A run
directory is one with a `run-report-v1.json` in it, and `reports.keep` bounds
those, the recordings of runs that wrote no report, and `traces/` as three
separate sets. Pruning also happens after a command that recorded, not only
after one that reported, so `--trace` on its own no longer grows the report
directory without limit. Anything else you kept under the report directory is
left alone.

**A coverage export whose region ends before it starts is refused.** Such a
region describes nothing, so `contains` answers no for every point in it and a
mutation a target really did reach would be routed away from that target. The
whole measurement is now refused instead, which routes every mutation
everywhere: what a measurement cannot say is what a run has to run. No export
`llvm-cov` writes contains one.

**`LLVM_PROFILE_FILE` is composed rather than inherited.** A run sets it to a
path under the temporary directory each execution owns, or to the coverage
pass's own path when it is the one measuring. If you were setting it around
`rust-mutants run` to measure a project's coverage, set it around the build
instead: an instrumented test process that inherited it wrote over the
measurement that started the run, and one with no path at all wrote
`default_*.profraw` into the tree being measured, which the drift report then
blamed on your tests. Running `rust-mutants` under `cargo llvm-cov` is
unaffected and is still allowed on the command line; only the three
`RUST_MUTANTS_*` variables are refused there.

**`RM5002` says what it observed.** It claimed the pristine tree passes, which
a run never establishes: it type-checks the tree rather than running it. It
now names the target that failed with nothing active and says that
`--no-verify` is the way past it.


