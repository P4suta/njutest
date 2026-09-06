<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
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
exclude = []                   # workspace-relative globs that remove a file
allow_outside = []             # directories outside the root the build may read

[build]
features = []                  # cargo features to turn on
all_features = false           # every feature of every selected package
no_default_features = false    # leave the default features off
target = ""                    # target triple; empty = the host
profile = ""                   # cargo profile; empty = each command's default
jobs = 0                       # cargo compilation jobs; 0 = cargo decides

[mutation]
tier = "balanced"              # balanced | strong | all
operators = []                 # exactly these rules; empty = the tier
timeout = "auto"               # auto = 5x the target's own baseline, never below 30s
build_timeout = ""             # empty = no bound
verify = true                  # run the instrumented baseline first
coverage = true                # measure reach once, then run a mutant only where it was reached

[execution]
offline = false
locked = false
doctests = true                # run a library's documented examples as a target
skip_targets = []              # target ids never to start, as pkg/kind/name
jobs = 0                       # mutants measured at once; 0 = the machine, capped at 4
test_binary_args = []          # --test-threads, --include-ignored, --nocapture, --show-output

[reports]
directory = "reports/mutation" # workspace-relative
keep = 20                      # run directories kept
```

## Flags win

Every value above also has a flag. A flag given on the command line overrides
the file; a list given on the command line *replaces* the file's list rather
than adding to it, so `--package a` means exactly `a`. `--no-config` reads no
file at all and `--config FILE` reads one elsewhere.

`[mutation] coverage` is on. It is the one shipped proof layer, it fails open
— a measurement that could not be made routes every mutant to every target,
exactly as if the layer were not there — and it is what lets a run say
`unreached` about a mutation nothing executes instead of `survived`.
`--no-coverage` turns it off, which measures nothing and runs everything.

With coverage on, the engine also asks the compiler which mutations change
nothing outside the branch they sit in. A target the measurement placed at the
mutation and whose run never entered that branch is *discharged*: it cannot
have noticed the mutation, so running it proves nothing and costs a process.
The report says `discharged` where it would have said `survived`, and names
the proof. A proof without a measurement removes nothing: the lemma is the
compiler's and the premise is the coverage layer's.

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

`[execution] jobs` is how many mutants a run measures at once, and `--jobs`
or `-j` says the same on the command line. Zero is as many as the machine has,
capped at four: each test binary already runs its own tests on as many threads
as the machine has, so a run that started one process per core would have
every process contending with every other and would measure the contention. A
suite that sets `test_binary_args = ["--test-threads=1"]` has already given
that up, and can afford more.

An expired budget buys one more measurement, taken with nothing else the run
started running beside it: a duration measured while three other test
processes were running is a fact about the load rather than about the
mutation. What that measurement observes is what stands — a second timeout is
a timeout, and anything else leaves the run undecided.

Every `[build]` key is what a person would have typed at cargo, passed on
unchanged to every command a run compiles with: the pristine check, each
validation round, the test build, and the coverage, probe, and witness builds.
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
outcome = "survived"           # survived | killed | timed_out
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

A run composes `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG`, and
`RUST_MUTANTS_PROBE` for every test process it starts. Finding any of them
already set in its own environment ends the command with `RM0006`: nothing a
test process said under an inherited activation would be about this run.
