<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# `.rust-mutants.toml`

**Status: implemented.** The file is optional, strict, and defaulted.
`rust-mutants init` writes an annotated skeleton whose every value is already
the default, so the file a person starts from changes nothing.

Unknown keys, malformed values, and any `version` other than `1` end the read
rather than being ignored. Every check the file can fail happens when it is
read, not half-way through a run: a pattern that is not a pattern, an operator
no rule answers to, a harness flag the engine owns, a report directory that
leaves the workspace.

```toml
version = 1

[project]
packages = []                  # cargo package names; empty = every member
include = []                   # workspace-relative globs a file must match
exclude = []                   # globs naming files nothing is mutated in
allow_outside = []             # directories outside the root the build may read

[build]
features = []                  # cargo features to turn on
all_features = false           # every feature of every selected package
no_default_features = false    # leave the default features off
target = ""                    # target triple; empty = the host
profile = ""                   # cargo profile; empty = each command's default
jobs = 0                       # cargo compilation jobs; 0 = cargo decides
debug = false                  # write debug information; off, because nothing here reads a backtrace

[mutation]
tier = "balanced"              # balanced | strong | all
operators = []                 # exactly these rules; empty = the tier
timeout = "auto"               # auto = 5x the target's own baseline, never below 30s
steps = 50_000_000             # guard takes one mutant may spend; 0 = the bound only
build_timeout = ""             # empty = no bound
verify = true                  # run the instrumented baseline first
coverage = false               # build once with LLVM coverage and route by its regions as well
touch = true                   # ask the guards which tests reached them, and run only those

[execution]
offline = false
locked = false
doctests = true                # run a library's documented examples as a target
skip_targets = []              # target ids never to start, as pkg/kind/name; a name no target has is refused
jobs = 0                       # mutants measured at once; 0 = the machine, capped at 4
test_binary_args = []          # --test-threads, --include-ignored, --nocapture, --show-output
scratch_working_directory = false # start each test process in a directory of its own

[reports]
directory = "reports/mutation" # workspace-relative
keep = 20                      # run directories kept

[reports.stryker]
high = 80                      # at or above this, a Stryker reader shows green
low = 60                       # below this, it shows red

[snapshot]
omit = []                      # globs the copy does not carry at all
```

`[reports.stryker]` is read by nobody but a Stryker reader. **Nothing in this
engine decides anything by a percentage** — a threshold is not a claim anybody
can check, and the gate is an expectation with a reason
([ADR 0004](../adr/0004-proof-layers-not-budgets.md)). The two numbers are
carried into the projection because that schema requires them, and `low` above
`high` is refused rather than passed on.

## What ends a mutation that does not end

A mutation can stop a program ending. Something has to stop it, and the two
things that can are not interchangeable.

`timeout` is a clock. What it measures is partly the machine: the same mutation
on a loaded machine and a quiet one is two answers, so a run that concluded
from it would report a different score depending on what else was running. A
mutation a bound expires on is `waited`, and `waited` establishes nothing —
neither that the tests noticed nor that they did not.

`steps` is a count of how many times the active mutant's guard is taken. The
guard sits where the mutation does, so a loop whose condition was mutated takes
it once an iteration, and the number is the same on every machine at every job
count under every load. A mutation that spends the allowance is `runaway`, and
that **is** an answer: the mutation stopped the program terminating, which is a
behaviour change any observer would meet. It counts as detected, and a run does
not put it a second time, because a count cannot disagree with itself.

The default of fifty million spends in roughly a second and a half for a loop
that cannot terminate, which sits well inside the thirty-second floor a derived
`timeout` never goes below. `0` counts nothing and leaves the clock as the only
thing that can end a runaway.

The reason to lower it is a project whose own bound is short: **a bound the
count cannot beat turns a `runaway` into a `waited`**, and the answer stops
being about the code. Where the count does not reach at all — a mutation
outside the loop it stopped ending — is in
[limitations](../limitations.md).


## Flags win

Every value above also has a flag. A flag given on the command line overrides
the file; a list given on the command line *replaces* the file's list rather
than adding to it, so `--package a` means exactly `a`. `--no-config` reads no
file at all and `--config FILE` reads one elsewhere.

`[mutation] touch` is on and is the shipped proof layer. The guards of the
instrumented tree record which of a target's tests reached them, on the run
that verifies the baseline, so it costs the run nothing it was not already
spending. It fails open — a target whose guards recorded nothing this run can
route by keeps every test of it in every route, exactly as if the layer were
not there — and it is what lets a run say `unreached` about a mutation nothing
executes instead of `survived`, and what puts a mutation to the tests that
reached it rather than to every test of every target that did. `--no-touch`
turns it off, which measures nothing and runs everything
([ADR 0014](../adr/0014-the-guards-are-the-measurement.md)).

`[mutation] coverage` is off. It builds the tree once more with
`-C instrument-coverage`, which changes the fingerprint of every crate in the
dependency graph and is therefore the largest single thing a run could do. It
is kept as an independent second opinion, and for the branch proofs of the
bodies no marker could be written into: `--coverage` asks for it.

The engine also asks the compiler which mutations change nothing outside the
branch they sit in. A target nothing of which entered that branch is
*discharged*: it cannot have noticed the mutation, so running it proves
nothing and costs a process, and a kept target is asked only for the tests
that did enter. The report says `discharged` where it would have said
`survived`, and names the proof. A proof without a measurement removes
nothing: the lemma is the compiler's and the premise is a measurement's.

`[mutation] equivalence` asks, after the run, whether the compiler renders
each survivor's mutation identically to what it mutates. It costs a tree of
its own and one build per survivor, and it never says a mutation is
equivalent: what it can say is that the two binaries are the same bytes, which
is a fact about what the compiler produced. Only survivors are asked, because
a mutation a test noticed is one the compiler plainly rendered.

`[mutation] timeout` is `auto` or a duration. `auto` is five times what that
target's own baseline took when the run verified it, and never below thirty
seconds — a budget shorter than a machine's own noise makes a timeout a
finding about the machine. A target nothing verified has no baseline to be a
multiple of, and the budget falls back to five minutes. A duration pins it,
and the report and the recording say which of the two a run used.

`[execution] test_binary_args` and the arguments after `--` are how this
project's suite runs, and every test process of the run is started with them,
the baseline included. They are the same run either way, so the arguments a
person gives take the place of the ones the file holds rather than adding to
them. A baseline taken one way and mutations measured another compares two
suites: a mutation could be noticed by a test the baseline never ran, which
is a kill nothing vouched for, and a mutation's budget is a multiple of a
duration measured under other flags.

`[execution] jobs` is how many mutants a run measures at once, and `--jobs`
or `-j` says the same on the command line. Zero is as many as the machine has,
capped at four: each test binary already runs its own tests on as many threads
as the machine has, so a run that started one process per core would have
every process contending with every other and would measure the contention. A
suite that sets `test_binary_args = ["--test-threads=1"]` has already given
that up, and can afford more.

An expired bound buys one more measurement, put again with nothing else the
run started running beside it: a duration measured while three other test
processes were running is a fact about the load rather than about the
mutation. What that measurement observes is what stands — a bound that
expires again is `waited`, and anything else leaves the mutation
`inconclusive`.

A `runaway` buys nothing here and is never put twice. The count that ended it
is the same number on a quiet machine as on a busy one, so a second reading
cannot disagree with the first, and asking for one would be spending a
process to be told what the run already knows.

Every `[build]` key is what a person would have typed at cargo, passed on
unchanged to every command a run compiles with: the pristine check, each
validation round, the test build, and the coverage and witness builds.
Cargo compiles a different program for a different feature set, target triple,
or profile, so a run that measures one of them while the project ships another
measures a program nobody runs. The report's `selection.build` says which one
was measured, and a stored outcome is only reused for a run compiled the same
way.

`offline`, `locked`, and `verify` are the exception a reader should know
about: `--offline`, `--locked`, and `--no-verify` can only turn a switch on
(or verification off). A file that says `verify = false` is not overridden
back to true by a flag, because there is no flag that says so.

## Expectations

```toml
[[mutation.expect]]
id = "b8e3f78d"                # identity, or a prefix that names exactly one
reason = "the bound is equivalent under the invariant the type carries"
outcome = "survived"           # survived | killed | runaway | waited
```

An expectation is a claim, not a suppression: the run resolves the identity,
compares the outcome, and says which of three things happened.

| Standing | What it means | Effect |
| --- | --- | --- |
| `met` | The run established the declared outcome | The mutant is accounted for and is not a finding |
| `stale` | The run established something else | A `stale-expectation` finding; the run is not clean |
| `unmatched` | No mutant of this catalog answers to the identity | An `unmatched-expectation` finding; the claim verifies nothing |

`reason` is required by the shape itself. An expectation without one is a
suppression, and a report cannot audit a suppression.

The same claim can be addressed by where the mutation is rather than by an
identity, which is minted from the whole file's digest and so changes when
anything in the file does:

```toml
[[mutation.expect]]
path = "src/lib.rs"
item = "clamp"                 # a suffix of the item path is enough
rule = "le-to-lt"
original = "<="                # the bytes the edit replaces
line = 42                      # a hint, when the rest names more than one
reason = "the bound is equivalent under the invariant the type carries"
outcome = "survived"
```

Never both: an identity and a locator are two ways of naming one mutant and
two chances to name different ones. A locator whose line has moved still
holds, and the report says where the mutation is now.

A locator names one mutation. Where the same reason is true of several of
them at once — the same call written at three places in one function, say —
`count` says how many it was written for:

```toml
[[mutation.expect]]
path = "src/prove.rs"
item = "establish"
rule = "delete-call-statement"
original = "phase.end();"
count = 3                      # the reason is written for exactly these three
reason = "Phase ends its phase in Drop, and a_phase_guard_ends_its_phase_once_with_its_duration_and_phases_nest holds it"
outcome = "survived"
```

The count is what keeps that from being a licence. Without it, a locator
that names more than one mutation is `unmatched`, because a reason written
about one mutation says nothing about another that happens to share a path,
an item, a rule and the bytes it replaces. With it, two things have to hold
at once: the catalog holds exactly that many, so a mutation added or removed
at the same place stops the claim instead of joining it, and **every one of
them** came to the declared outcome, so a claim covering three stops holding
the moment a test kills one of the three. What covered that one is the test,
and the claim would otherwise go on exempting the other two on its strength.

## Configured skips

```toml
[[mutation.skip]]
path = "src/scanner/**"        # glob against the workspace-relative path
lines = "40-58"                # inclusive; only with a literal path
item = "Scanner::skip_ws"      # a suffix of the item path; not with lines
reason = "a hand-tuned loop; a mutant here is a timeout, not a finding"
```

A configured skip is the decision a `rust-mutants: skip` comment makes,
written where the code cannot be edited or where one entry covers what a
hundred comments would. `reason` is required for the same reason an
expectation's is. `lines` and `item` are two ways of saying where, so an
entry says it once, and `lines` needs a literal path: line forty of every
file a glob matches is not a place anybody meant.

An entry that hid nothing is an `unmatched-skip` finding, exactly as a
comment that hid nothing is. A skip that quietly stops meaning anything when
the code under it moves is worse than no skip at all.

## Reserved environment

A run composes `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG`,
`RUST_MUTANTS_TOUCH`, and `RUST_MUTANTS_STEPS` for every test process it
starts. Finding any of them already set normally ends the command with
`RM0006`: nothing a test process said under an unrelated activation would be
about this run, and a touch log another run owns is not one this run may
append to.

`RUST_MUTANTS_STEPS` is how many times the active mutant's guard may be taken
before the process stops itself and exits 95. The guard sits where the
mutation does, so a loop whose condition was mutated takes it once an
iteration, and a count is the same number on every machine, at every job
count, under every load — which is why a mutation that will not stop is
`runaway` and counts as detected, where a bound that expired is `waited` and
establishes nothing. Unset, or `0`, counts nothing and leaves the clock as the
only bound.

There is one closed exception for this repository measuring itself. Cargo
compiles every instrumented tree with an internal
RUST_MUTANTS_COMPILED_CATALOG build input, and the two engine composition roots
embed it with `option_env!`. A nonempty inherited catalog is accepted only
when it equals that embedded digest and exactly one of `ACTIVE` or `TOUCH` is
also nonempty. A normal binary, a partial pair, a stale catalog, or both modes
still earns `RM0006`. The internal value is a build input, not a variable a
user sets or a test process inherits.
