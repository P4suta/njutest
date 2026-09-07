<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# rust-mutants architecture

**Status: implemented.** Discovery, instrumentation, compiler-validated
acceptance, execution, the probe tree, the branch proof, the public API, and
the command line all work; [the roadmap](../roadmap.md) says which milestone
each part came from.

## Invariants

1. **The source workspace is read-only.** `Workspace::open` copies the tree
   into a disposable snapshot under the temporary root, at a name stable per
   repository root so successive runs share cargo's incremental state. Every
   build, instrumentation, and test happens in the copy. Symbolic links,
   devices, and other irregular files are refused, not skipped: a skipped
   link is a silently absent file.
2. **Instrumentation happens once.** Every compilable mutant of a file lives
   dormant behind a guard in the snapshot; the test binaries are built once;
   `RUST_MUTANTS_ACTIVE=<64 hex id>` activates one mutant per test process.
3. **Bytes are spliced, never pretty-printed.** Comments, whitespace, and CRLF
   are preserved, and every splice keeps its line count, so coverage regions
   and mutant positions agree line for line with the pristine file.
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
   [probe tree: second snapshot, probe runtime, infection log]
   [witness tree: third snapshot, checked only, branch proofs]
        │
   Session: exec(mutant, target, args) / probe(target, args) / changes()
```

## What a scoped run compiles

`PrepareOptions.packages` names the members a run is about. It narrows what is
mutated, and it narrows what is built: the test binaries a run starts are the
ones of the packages it is about, so `cargo test --no-run` is given those
packages and nothing else. The instrumented baseline — the tests run with no
mutant active — is then those binaries too.

The type check is not narrowed. A mutation of one package can stop being a
program only where another instantiates it, so `cargo check` is always about
the whole workspace: narrowing it would let a mutant that does not compile
downstream reach the build instead of the rejection it belongs in.

Scoping a run is the one thing a person can do to make it shorter, and until
this was so it did not shorten the longest part of one.

The gate `prepare` stands on is that check of the whole workspace and then a
test build of the packages the run is about. A check answers "is this a
program"; it does not answer "does this link", and a tree that fails only at
link time used to pass the gate and then fail every round, where the failure
reads as a mutation the compiler refused. `preview` — what `list` and
`why-skipped` stand on — still only type-checks: it rules on nothing, so it is
answerable for a tree that does not link.

Each round instruments every mutable file, because attribution needs each
file's branch spans whatever it condemns, and writes back only the files whose
text changed. `validate-round` says how many that was.

## Snapshot

`snapshot::create` copies the tree byte for byte into
`<dest>/rust-mutants-snap-<16 hex of sha256(absolute root)>/tree`. The
name is stable so cargo's fingerprints survive from one run to the next;
the directory beside `tree` carries the `tempowner` lock and marker, so
every byte under `tree` came from the source. A directory found under the
stable name is swept and copied into again, never adopted; a live, kept, or
young unowned one makes the run fall back to a fresh name (reported through
`Snapshot::stable_dir`). `.git` at any depth and `reports/mutation` are
always excluded; a symbolic link, reparse point, device, or backslash-named
entry is refused with the first offending path in sorted order.

The manifest is sorted by path and hashed under the domain
`rust-mutants-workspace-v1` as `enc(domain) ‖ enc(path) ‖ enc(sha256hex) …`
(4-byte big-endian length prefixes). `Snapshot::redigest` re-walks the copy
with no exclusions and reports every added, removed, or changed path: the
gate that catches a test writing into its own package. Cleanup releases the
lock, checks a guard (absolute, prefixed name, expected parent), and retries
the removal on a 20/40/80/160 ms ladder; `keep` records the decision in the
marker so the next sweep obeys it.

### The public API

`Workspace::open` sweeps the temporary area, copies the tree, and locates
the toolchain inside the copy. `Workspace::prepare` consumes the workspace
and returns a `Session`: the phases are types, so a mutant cannot be
executed against a tree that was never prepared. A session is `Send + Sync`
and every execution takes `&self`, so a consumer runs mutants in parallel
across the targets one build produced.

The engine reads no environment variable of its own. The environment every
command and test process runs with, and the temporary directory everything
is created in, are arguments ([ADR 0001](../adr/0001-seam-policy.md)); the
composition root is the only place that names the process environment, and
`cargo xtask devgates` refuses any other.

### The command line

`rust-mutants list` and `why-skipped` copy and type-check the workspace and
stop there: fast, and honest that nothing has been ruled on yet. `catalog`,
`explain`, and `run` prepare it, so what they report is what the compiler
accepted. `catalog --json` prints one `rust-mutants/catalog` document.
`run` exits 0 when the tests noticed the mutant, 1 when they did not, and 2
when nothing was established, so a script can act on the answer.

`--changed` and `--changed-from <REV>` mutate only the files that differ from
a revision, committed and not. They narrow `--include` rather than widening
it, and a change set that names no Rust file selects nothing rather than
everything. A tree git cannot be asked about, or a revision it does not know,
ends the command with `RM0010`: a run that could not see what changed must
never look like a run that saw nothing change.

`--features`, `--all-features`, `--no-default-features`, `--build-target`,
`--profile`, and `--build-jobs` are what the `[build]` section spells, and
they reach every command a run compiles with: the pristine check, each
validation round, the test build, and the coverage, probe, and witness builds.
Cargo compiles a different program for a different feature set, triple, or
profile, so the report's `selection.build` says which one was measured and a
stored outcome is only reused for a run compiled the same way. The build
target is `--build-target` and not `--target`, which `run` already uses to
name one test target.

A killed mutant's row says which tests failed with it active (`killed_by`) and
the signal the process died from, when it died from one. Both come from the
harness's own per-test lines, which is also what `mutant-exec` records.

Coverage is measured unless `--no-coverage` says otherwise. It builds the tree once more with `-C instrument-coverage`, runs
every test target once with nothing active, and reads back which target
executed which regions. A mutant is then only run against the targets that
reached it, and one no measured target reaches is reported as
`unreached-mutant` rather than executed against everything to find that out
again. Every export names every binary the build produced, so a test that
spawns one of the workspace's own binaries has that binary's coverage
attributed to it; a place no export instrumented is a place the measurement
says nothing about, and its mutant is run everywhere. What the tree's cargo
configuration puts in `build.rustflags` is read and put back into that build,
because the variable it passes flags in replaces them rather than adding to
them. A tree that configures `rustflags` for a target, a configuration file
nobody can parse, a build that will not instrument, and tools that are not
installed each leave the measurement empty, which routes every mutant to every
target exactly as if the layer were not there.

With a measurement in hand, a second proof layer removes more. The compiler is
asked which mutations change nothing outside the branch they sit in; a target
the measurement placed at such a mutation and whose run never entered that
branch is *discharged*, because it cannot have noticed it. The lemma is the
compiler's and the premise is the measurement's, and `prove::discharges` is
the pure function of the two, so a caller with its own coverage discharges
with its own evidence and an audit re-derives the decision without the engine.
Coverage regions nest: what says a body ran is a region that *begins* inside
it, not one that contains it, and the region at the body's closing brace is
the one the compiler emits for what follows the branch.

`Session::judge` runs what the route reaches and `Session::exec` runs what the
measurement placed, discharges included: a discharge is a proof a caller may
not share, and `exec` is the question "what do the tests say", asked by a
caller with its own evidence.

`report --format` writes a stored run for somebody else: `json` is the
document verbatim, `markdown` the summary a person puts in a pull request,
`junit` one test case per mutant for a CI server's test view, `sarif` one
result per finding for a code scanning view, `html` one page that fetches
nothing and shows every measured file whole with its mutants on their lines,
and `stryker` the mutation testing report every Stryker reader understands,
with columns in UTF-16 as that schema counts them. A file the report names and
the root does not hold is `RM0012` rather than a file quietly left out.
`report --tui` reads it at the terminal instead. What each projection is and
who reads it is in [reports](reports.md).

`doctor` answers about this environment rather than about any code: one check
per thing a run needs — `cargo`, `rustc`, `host`, `workspace`, `config`,
`temp`, `git`, `targets`, `environment`, `cache`, `disk`, `snapshots`,
`llvm-tools` — each standing `ok`, `warn` or `fail`, and each carrying the
next step when there is one. A warning is a run that costs more or measures
less, never a reason not to run, so only a failure changes the exit code.
`doctor --json` answers with a `rust-mutants/doctor` document.

`diagnostics [RUN]` gathers one run into one directory: the report, the
catalog, the measurement, the probe logs, the recording, the configuration, a
fresh doctor document, what the toolchain says about itself, and the **names**
of the variables that were set — never a value, because a bundle travels and a
value that travels with it is one nobody chose to publish. `bundle.json` says
what is held and what the run did not leave, so a reader can tell a run that
had nothing to say from a file that never arrived.

These two are the only commands that do not refuse to start under a reserved
variable. Every other command refuses, because nothing a test process said
under an inherited activation would be about this run; these report it,
because they are what a person runs to find out that it is set.

`run --shard K/N` runs one part of the catalog, cut by index, and `merge`
reassembles the parts into the report the whole would have written. A run
reads back what an earlier run of this exact tree established unless
`--no-cache` is given; `cache` says what is stored and `cache --gc` removes
what no run still owns.

`--trace[=DIR]` records what the command did, as JSON Lines. A run records
beside its report and every other command under `<reports>/traces/`; `trace
summary`, `trace check`, and `trace diff` read one back. A recording is
diagnostic exhaust and never evidence, so a directory that cannot be created
costs one line on standard error and never the command.

### What a run says while it happens

`--ui plain` writes one line per phase as preparing finishes it, one line per
mutant as the run judges it, and the tally every ten and at the end.
`--ui quiet` writes none of them and the summary all the same. `--ui auto`
is what a run without the flag gets, and is `plain` until there is a renderer
that overwrites in place.

Preparing is not a run, so nothing observes it: the phases come from the
recording, which a `Sink::Channel` tees to the command's own thread. A
recording is diagnostic exhaust ([ADR
0002](../adr/0002-trace-is-diagnostic-exhaust.md)), so a display that has
gone away costs the event and never the run.

`--color auto|always|never` paints the outcome word and nothing else: what a
reader is scanning for is the one line that is not a kill, and a page of
colours is a page nobody scans. `auto` paints a terminal that has not set
`NO_COLOR`.

### Narrowing a run

`--rule`, `--family`, `--skip-rule`, `--skip-family`, `--file PATH[:FROM-TO]`
and `--id PREFIX` say which of the catalog's mutants a run is about, and
`--from-report [RUN] --outcome survived` says the ones a stored run left that
way. A filter changes nothing about the catalog: the digest is the catalog's,
a stored outcome is still the same tree's, and what a filter took out keeps
its row with `unselected` as its reason, so a report of a narrowed run still
accounts for the whole of the catalog it was cut from. A mutant nobody
selected is not a finding.

`--fail-fast` stops at the first thing a reader has to act on — a mutation
nothing noticed, one nothing could decide, one nothing reached — and not at a
kill, which is the run working. What it did not reach is `stopped-early`,
which is the run doing what it was asked to rather than a run that was killed:
the exit code is the finding's, never `130`.

`--dry-run` prepares, verifies, and then says what a run would cost: every
mutant it would measure with the targets that could notice it, and the size
of the job from what the verification measured. Nothing is executed.

### What a run leaves, and what finds it again

`--run-id NAME` names a run, which is what its report directory is called;
without one the name is the moment it started. A name is one to sixty-four of
letters, digits, `.`, `_` and `-`, because it is a directory.

`--keep-temp` keeps the snapshot and the build cache a run would otherwise
remove, and writes them into `kept-v1.json` under the report directory, with
the run that kept them. `cache` lists what is there: the snapshots, the build
caches, the outcome store with its size, and every kept directory with the run
that kept it. `cache --gc` removes what is abandoned and leaves the build
caches, so the next run is still fast; `--gc --all` takes them too; `--gc
--kept` takes what was kept on purpose. `--clear-outcomes` empties the store
and says how much was in it, and `--cache-dir` says where the store is. A
ledger this release cannot read authorises nothing: it is read as empty, so a
sweep never removes a directory on the strength of a document it did not
understand.

`merge --root DIR --runs a,b` finds the parts of one catalog under a report
directory rather than being handed their paths, and a `--runs` value may be a
glob.

`replay <prefix>` puts one finding back to the tests as the run that found it
did: the target and the test come from the stored report rather than from a
guess, so a replay asks the question the run asked rather than a wider one,
and says whether the answer is still the same.

## Guards

Four forms. **Form C** for a position that is syntactically boolean (an
`if` or `while` condition, an operand of `&&`/`||`, a match guard):
`__rm::active(3) && (a >= b) || !(__rm::active(3)) && (a > b)`. **Form E**
for any expression in value position, in parentheses:
`(if __rm::active(5) { a - b } else { a + b })` — both branches unify to one
type, so `Default::default()` is inferred from the original. **Form S** for
a statement: `if __rm::active(7) { x -= step; } else { x += step; }`, the
original bytes in the `else` so lines are kept. **Form M** for a match arm
that has no guard, which is the one shape that adds syntax rather than
replacing it: the site is the pattern, kept verbatim, and the guard is
written after it — `0 if (__rm::active(9) && (false) || !(__rm::active(9))
&& (true)) =>`, where the branch that keeps the arm is the guard it did
without. A position where wrapping would move a value out of place — an
assignment target, a borrow operand, a method receiver, a scrutinee —
escalates to its parent expression, then to the statement.

The runtime is a private `mod __rm` appended after the last line of each
instrumented file ([ADR 0011](../adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)).
A guarded function gets `#[allow(warnings)]` on the same line as its first
token, so the warnings guards provoke never trip a `#![deny(warnings)]`.

### What instrumentation writes

`rust_mutants::instrument` composes the guards and appends a `mod __rm` to
each rewritten file. Alternatives of one site share one chain, nested sites
become nested guards composed children first, and only the branch that keeps
the original carries the guards inside it. Each alternative is folded onto
one line and the original branch keeps its bytes, so a guard holds exactly
as many line breaks as the bytes it replaced and every byte stays on its
line. `#[allow(warnings)]` goes on the signature line of the innermost
function that holds a guard, so a crate's deny policy is untouched
elsewhere. The runtime reads `RUST_MUTANTS_ACTIVE` once per process; a
`RUST_MUTANTS_CATALOG` that is not the one the tree was built from ends the
process with exit 97, and an identity this file does not know activates
nothing here because it belongs to another file.

### How a candidate becomes a mutant

`rust_mutants::validate` compiles the instrumented tree and reads the
diagnostics. Every alternative occupies a known byte range, so an error
whose primary span falls inside one is about exactly that mutant: a whole
round's refusals are condemned at once, and the loop costs one
recompilation per round rather than one per candidate. An error that falls
outside every branch is isolated by bisection instead, and a tree that does
not compile with nothing live stops the run (`RM4001`) rather than blaming
candidates until the error goes away. What comes back is always a tree that
compiled, plus a rejection per refused candidate carrying the compiler's own
words.

Whether an edit is a program is a fact about the toolchain, never assumed:
`fixtures/fixture-rejectable` records that this compiler refuses
`String - &str` and a `RangeInclusive` where a `Range` belongs, and accepts
`value / 0` for a run-time `value` — a mutant that dies at run time rather
than at compile time.

## Validation, stated

A round instruments every candidate that is still live, compiles, and reads
what the compiler said. An error is attributed to the mutant whose branch holds
one of its spans — the primary one first, then the others, then the spans of
its notes, because the compiler points at the place it decided and for a type
error that is often the definition rather than the edit. The mutants an error
names are condemned, the file is written again without them, and the next round
follows.

What no span names is what bisection is for. It halves the live set, and a half
that still fails is halved again, so an offender the compiler refuses on its
own is found in a logarithmic number of builds. When neither half fails, the
offence straddles them — a combination only ever seen with mutants from both
sides live — and narrowing by increasing granularity finds the mutants that
interact rather than condemning everything that happened to be live. The search
is bounded; running out of the budget condemns what is left, which is the same
answer halving alone gave.

Every offence bisection names is then compiled once more on its own, so the
report carries the compiler's words about that mutant rather than a sentence
saying there were none. A row says `isolated` when the compiler refused it
alone, and names the mutants it was refused with when it did not.

A build nobody waited for is a cancellation and not a tree that does not
compile: `Ctrl-C` during a round ends validation with `RM0001`, and nothing is
condemned on the strength of what a half-finished command printed.

## Skips, stated

`const-context`, `macro-invocation`, `cfg-attribute`, `test-code`,
`unsupported-site`, `excluded`, `test-only-file`, `no-std-crate`,
`included-expression`, `generated-outside-workspace`, `forbidden-lints`,
`const-fn-body`, `let-condition`, `open-range`, `unstated-return-type`,
`loop-value`, `annotated`, `configured`. Each is counted and named;
`rust-mutants why-skipped` lists them. A skip is a decision the tool made and says; a
rejection (a mutant the compiler refused) is a fact about the program and is
reported with the diagnostic.

A `rust-mutants: skip <reason>` comment is the one skip an author writes.
Sharing a line with code it hides what starts on that line; alone on a line it
hides what starts on the next one, item, statement, arm or `else` block and
all. The reason is required (`RM2008`), `skip` is the only directive
(`RM2009`), and a marker that hid nothing is an `unmatched-skip` finding
rather than a comment nobody notices is stale. Markers are read from the gaps
between tokens, so the words inside a string literal are a string and a
documentation comment, which is an attribute by then, is never a marker.

`[[mutation.skip]]` says the same thing from the configuration file, where
the code cannot be edited or where one entry covers what a hundred markers
would: a path glob, a reason, and at most one of a line range or an item.
It is applied where the walk's own decisions are, so every tally says the
same thing about the file, and an entry that hid nothing is the same
`unmatched-skip` finding a stale marker is.

Every place a rule targets has a decision: a candidate with its guard form, or
a skip with its reason. A rule that passed over a place without saying so is
what `crates/rust-mutants/tests/census.rs` refuses, and it is why a `const fn`
body, a condition that binds with `let`, and a range with no end are reasons
of their own rather than silence. Two decisions carry a note instead of a
reason of their own: `identical-replacement` where a rule's replacement is
what is already written, and `text-mismatch` where the bytes at the span are
not the operator the rule expects.

The per-file walk (`rust_mutants::syntax`) keeps walking inside a region it
will not mutate and counts every candidate it would have produced under the
outermost reason, so the tallies say how much code each reason hides. A
macro invocation counts once, since its body is tokens the walker does not
parse. Whole-file reasons (`excluded`, `test-only-file`,
`no-std-crate`, `generated-outside-workspace`, `forbidden-lints`) are decided by the workspace
layer from cargo metadata and dep-info, not by the walk. A file a build script
A crate is `forbidden-lints` when its root, or the `[lints]` table cargo
builds it with, forbids one of the lints the guards' own attribute turns off:
`forbid` is the one level an `allow` cannot override, so a guard there is a
compile error whatever it edits, and every mutant of the crate would otherwise
be refused with nothing saying why. A `deny` is fine, which is what the
attribute is carried for. A file a build script
wrote is named `<generated>/<its own name>`: the directory it was written to
is different on every machine and every run, and naming it would say where
this run put its temporary files rather than which file was passed over.

## Execution

A test binary is started directly, never through `cargo test`, with the
environment cargo would give it, `RUST_MUTANTS_*` stripped and set, a
scratch `TMPDIR`, and the libtest arguments verbatim. The outer supervisor
owns the timeout and the process tree; exit status is read in this order —
start failure → `errored`; timed out → `timed_out`; killed by us → `not_run`;
97 → `errored` (stale catalog, never a kill); non-zero → `killed`; zero →
`survived`. A libtest run that matched no test is green and says so:
`tests_run` carries the count the summary line reported.

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

A test process inherits the environment the run was started with, plus what
cargo sets for its target, minus the four variables a run composes for
itself: `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG`, `RUST_MUTANTS_PROBE`,
and `LLVM_PROFILE_FILE`. The first three decide which mutation is active, and
an inherited one would make every answer be about somebody else's run. The
fourth is there because a measurement *of this engine* sets it: an
instrumented test process that inherited it would write over the very
measurement that started the run. Removing it is not enough on its own, since
an instrumented binary with no path writes `default_*.profraw` into its
working directory, which is the snapshot being measured and which the drift
check would then report as the project's own tests writing into their tree. So
a run puts a path of its own in its place, under the temporary directory that
execution owns. The coverage pass puts its own path there instead, per
target.

Only the first three are refused on the command line. A run under
`cargo llvm-cov` is an ordinary thing to want; a run under somebody else's
activation is not.

## The run

The engine drives its own runs. `rust_mutants::run::run` walks the accepted
mutants a shard holds, asks `Session::judge` about each, reuses what an earlier
run of this exact tree established, records a route for every one of them, and
hands back what it decided; `rust_mutants::report::run::document` turns that
into the document, and `merge` puts the parts of a sharded run back together.
The command line renders. Nothing about a run's policy — what a finding is,
what the exit code says, which mutants a shard holds — lives above the engine
any more, so a second consumer of the engine gets the same answers rather than
a second implementation of them.

A caller hears about a run through an `Observer`, whose every method is called
on the calling thread and has a default that does nothing, so an observer
implements only what it draws. `Silent` draws nothing.

A run measures `jobs` mutants at once — as many as the machine has, capped at
four — and delivers each as it finishes rather than in catalog order: one
mutant that runs for its whole budget would otherwise hold back every result
behind it, and a progress line, a stream, and a stop-at-the-first-finding
would all wait on it. The report is put back into catalog order when it is
written, because that is the order a reader compares two runs in. A run that
is cancelled leaves every mutant it never claimed as not run, `interrupted`.

`Route::narrowing` is the one place a narrowing is decided, and both the route
a report shows and the targets an execution runs come from it. An execution
that narrowed by anything else would run fewer targets than the route says,
and a survivor it reported would be one nobody measured — which is what
happened to a target whose profile the measurement could not read: the route
kept it and the execution dropped it.

A mutant that was never executed says why: `unreached`, `discharged`, or
`interrupted`. A process the runner never got a status from is `interrupted`,
not a mutation no test can notice. Its row also carries the route — which targets could have
noticed it, which a proof removed, and which of them ran — so a reader can see
a proof layer remove work without a recording.

## Describing a session

A session owns a snapshot and the processes that run in it, so it cannot be
reopened: what a later command reads is `Session::describe` — the catalog, the
targets, the two digests, and the toolchain, all of it a document — and the
stored outcomes. `Session::source` hands back a mutable file as it was before
instrumentation, because a report names the bytes an edit replaces and showing
somebody the edit needs the file they would open rather than the rewrite the
snapshot holds.

A catalog on the wire names each rule and the version that entered every
identity in it. A name this release does not know, or a version it does not
agree with, is a catalog it refuses to read: every identity in it was minted
from the version it names.

## Identity

```text
id = SHA-256( enc("rust-mutants-id-v1") ‖ enc(path) ‖ enc(rule) ‖ enc(rule version)
              ‖ enc(start byte) ‖ enc(end byte) ‖ enc(source sha256) ‖ enc(original sha256)
              ‖ enc(replacement sha256) )
enc(s) = 4-byte big-endian length ‖ UTF-8 bytes
```

The same recipe as go-mutants with this domain string, so an identity is
reproducible from the catalog fields alone in any language. `display_id` is
the first twenty hex digits; a prefix of four or more resolves a mutant.

## Trace

Every decision the engine takes — every site's form and skip reason, every
validation round and the diagnostic that condemned each mutant, every bisect
step, every build, verification and execution, and the route every judged
mutant took — is recorded to the sink `OpenOptions` names. See
[trace](trace.md).
