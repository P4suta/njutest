<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Configuration v1

**Status: implemented** (`njutest::config`).
The defaults, the strictness, and the two rules about what a report may contain are fixed by tests; `njutest init` writes the skeleton below, and a test loads the untouched skeleton and asserts it is exactly the defaults.

`.njutest.toml` is optional and strict.
Missing configuration uses `standard-v1`, the whole workspace, a ten-minute measurement timeout, and a cache capped at 5 GiB and 30 days.
Unknown keys, malformed values, and any `version` other than `1` are errors.

`njutest init` writes an annotated skeleton: the two active defaults, and every section below as commented guidance.
Loading the untouched skeleton yields exactly the defaults.

```toml
version = 1
contract = "standard-v1"        # "standard-v1" | "deep-v1" | "verified-v1" | "whole-v1"

[project]
packages = []                   # cargo package names; empty = every workspace member
include = []                    # workspace-relative globs a file must match to be mutated
exclude = ["**/generated/**"]   # workspace-relative globs; the files are not mutated

[execution]
features = []                   # cargo features
all_features = false
no_default_features = false
test_binary_args = []           # allowed: --test-threads=N, --include-ignored, --nocapture, --show-output
environment = []                # variable names only, never values; RUST_TEST_* is refused
timeout = "10m"                 # upper bound for one measurement; Go duration syntax
build_timeout = ""              # upper bound for one build; empty = no bound
jobs = 0                        # mutation workers; 0 = logical CPUs capped at four
skip_targets = []               # stable target ids never to start; every one is reported
coverage = false                # also make the coverage build, as a second opinion (ADR 0014)

[[configuration]]               # a further build to measure; none by default
name = "all-features"           # what the report calls it; not "default", and unique
features = []
all_features = true
no_default_features = false
profile = ""                    # cargo profile; empty = the command's own default
target = ""                     # target triple; empty = the host

[mutation]
equivalence = false             # ask the compiler whether it renders each survivor identically

[faults]
inject = false                  # fail each call a `?` asks about and ask what noticed; --faults sets it

[verification]                  # verified-v1 only; both keys are mandatory and nonzero
# unwind = 8                    # maximum loop unwind for every proof harness
# timeout = "2m"               # wall-clock ceiling for one checker process

[cache]
max_bytes = 5368709120          # 5 GiB
ttl = "720h"                    # 30 days

[reports]
keep = 20                       # run directories kept
directory = "reports"           # where every run writes, one directory each under <directory>/runs

[fuzz]
run = false                    # drive the fuzz targets, not only find them
max_total_time = "60s"         # per target
targets = []                   # empty = every target the tree holds

[repeatable]
knobs = []                     # e.g. ["timezone", "locale", "temp-directory", "home", "umask", "columns", "threads"]

[soundness]                     # deep-v1 only
miri_flags = []
sanitizers = []                 # e.g. ["thread"] on nightly

[resources.postgres]
command = ["./tools/postgres-provider"]
timeout = "30s"
shared = true                   # or exclusive = true (forces jobs = 1)
environment = ["POSTGRES_IMAGE"]
interpose = ""                  # the variable of the provider's answer naming where the tests dial
wire = "raw"                    # how much of what goes past that seam is read: raw | http
hold = "30s"                    # how long delay-response holds an answer up

[generation]
command = ["./tools/test-generator"]
allowed_paths = ["**/tests/**/*.rs", "**/fuzz/corpus/**"]
environment = ["GENERATOR_TOKEN"]

[[acceptance]]
path = "src/lib.rs"
item = "clamp"
rule = "le-to-lt"
original = "<="
line = 42
reason = "reviewed equivalent boundary"
expires = "2026-12-31T00:00:00Z"
owner = "quality-team"
ticket = "QA-123"
```

`[repeatable] knobs` asks for one more control of every target whose baseline passed per knob, each started with one thing the contract lets differ between machines set differently: `timezone` (TZ=Australia/Lord_Howe), `locale` (LC_ALL=tr_TR.UTF-8), `temp-directory` (an empty temporary directory whose path holds a space), `home` (an empty home directory, with cargo's and rustup's kept), `umask` (077), `columns` (COLUMNS=37 LINES=11), and `threads` (`--test-threads=1`).
Empty by default, because each knob is one more run of every target.
A knob this machine cannot put is stated as `knob-not-put` rather than skipped silently; most CI images install no Turkish locale, so there `locale` is not put.

`[verification]` belongs only to `contract = "verified-v1"`.

`contract = "whole-v1"` asks every dimension a run can measure: it puts every fault and every knob whatever `[faults]` and `[repeatable]` say, runs the soundness phase as `deep-v1` does, and is not `ASSURED` while any dimension is a hole ([ADR 0033](adr/0033-every-dimension-or-a-hole.md)).
Schedules and durability are not measured in this release, so every `whole-v1` run concludes `INSUFFICIENT` until they are; the default stays `standard-v1` until then.
Both `unwind` (the nonzero loop-unwind bound) and `timeout` (the verifier process ceiling, representable as at least one whole millisecond) are mandatory there; the other contracts reject the section instead of silently ignoring proof settings.

`timeout` and `steps` can both stop a mutation that stops a program ending,
and they are not interchangeable.
`timeout` is a clock, and what a clock measures is partly the machine: two runs of one catalogue on one commit can disagree about the same mutation because one was on a busy laptop.
A mutation a bound expires on is `waited`, which establishes nothing — neither that the tests noticed nor that they did not — so it is not counted in the score.
`steps` is how many times the mutation's own guard may be taken; the guard sits where the mutation is, so a loop whose condition was mutated takes it once an iteration and the number is the same everywhere.
Crossing the allowance is `step-limit-reached`: a nonce-correlated fact that this execution reached the first count outside the configured bound.
It is **not** a detection.
A finite computation on the original code can cross the same count, so without a matched control the run has established neither that the mutation diverges nor that it survives.
It is therefore non-verdict, non-score, non-cacheable and cannot be accepted.
Where the count is on, `timeout` stops bounding the whole execution and bounds how long it may go without raising the count: a mutation that keeps moving is left to the allowance, and one that goes quiet for a whole window is `waited`.
Ten windows is a ceiling over the whole of it, for a process that keeps moving more slowly than the allowance can end.
`0` counts nothing and leaves the clock as the only bound.
Lowering the count can surface a deterministic place to investigate sooner;
it cannot turn the bound itself into evidence about the mutation.

`build_max_bytes` and `build_dir` were removed from `[cache]`.
Remove those keys when upgrading: configuration is strict, so keeping either one is an unknown-field error.
Compiled artifacts belong to rust-mutants' stable target directory; `[cache]` now controls only the outcome store described here.

`[project] packages` is what a run is about, and `--package` on the command line takes its place rather than adding to it.
A package no member answers to is refused, the way `njutest plan` has always refused one: a run narrowed to a name nobody wrote would measure the whole workspace and report the narrow verdict `SCOPE_ASSURED` over it.

`[execution] test_binary_args` is how this project's suite runs, and every test process of a run is started with those arguments, the baseline included:
a baseline taken one way and mutations measured another compares two suites.
Arguments given after `--` take the place of the ones the file holds rather than adding to them.

`[execution] coverage` asks for the coverage build, which is off.

ADR 0014 is why.
The guards the instrumenter places are the measurement, so a run needs no coverage build and no `llvm-profdata`, and asking for one adds a whole compile of the workspace and one run of every target to decide something the guards already decided.
It stays available because an independent second opinion is worth having when the two disagree, and a disagreement between them is a defect in one of them.

`njutest` had this on and unconfigurable, which was neither what the decision says nor something a caller could turn off: two reachability measurements ran and the page describing the run named only one.
`rust-mutants` has always had it off with `--coverage` and `--no-coverage` beside it.

`[execution] skip_targets` is the narrow escape hatch for a process whose tests inspect the instrumented tree itself, or otherwise fail for the same known reason under every mutation.
Each entry is the stable target id a report and `njutest plan` name, such as `pkg/test/ui`.
An id the workspace does not declare is an error, and every id that is left out is recorded as `target-skipped-by-configuration`; it is never a silent pass.

`[faults] inject` fails, one at a time, every call a `?` in a measured file asks about, and asks the suite whether it noticed ([ADR 0032](adr/0032-a-fault-is-a-failed-call-the-suite-is-asked-about.md)).
It is off by default because it costs a second instrumented build and baseline of the tree, and one execution for every `?` a test reaches.
`njutest verify --faults` turns it on for one run whatever the file says, and the run's identity carries the answer, so a run with faults never reads back one without.

`[execution] features`, `all_features` and `no_default_features` are the words cargo would have been given, and every command of a run is given them: the baseline, the mutation phase, the equivalence layer, `plan`, `replay` and `fix`.
Cargo compiles a different program for a different feature set, so a run that measured the default build while the project ships another would put a verdict on a program nobody runs.

`[project] include` and `[project] exclude` say which files are mutated and nothing else, and they are each other's pair: a project verifying four files out of a hundred writes four patterns rather than ninety-six.
Every file they name is still copied into the tree, still compiled, and still run, so the patterns never turn a workspace that builds into one that does not, and a run is a function of their bytes whether or not a mutation was put to them: the evidence a run leaves behind is keyed on the whole tree, and `njutest watch` starts a round when one of them changes.
What the exclusion removes is findings, which is why the report carries the patterns in `scope.excluded` and why a run left with no mutation to put to a test concludes `INSUFFICIENT` rather than assuring what it did not ask.
A pattern that is not a pattern —
a leading or trailing `/`, an empty string — is refused when the file is read,
so a typo narrows nothing silently.
`rust-mutants` spells both keys the same way and means the same thing by them.
A file the copy should not carry at all is `[snapshot] omit`, which only the engine has, because a runner that did not copy a file could not compile the workspace it is verifying.

There is no `profile` key — `cargo test`'s `test` profile is the one under verification — and no `toolchain` key: `rust-toolchain.toml` is the idiomatic pin and `rustc -vV` is recorded.
Environment entries are names, never `KEY=value`; values are not written to reports.
`njutest accept` appends an `[[acceptance]]` table while preserving the comments of the file, and writes every field one can carry.
It writes the locator — `path`, `item`, `rule`,
`original` and the line as a hint — rather than the identity, because an identity is a function of the whole file and the edit that closes a survivor is an edit to that file: a recorded identity stops naming anything the moment somebody does the thing the acceptance was written about.
An `id` is still read for a record written before this, and still resolves by prefix.
An acceptance whose `expires` has passed answers for nothing and the findings it was hiding are raised again; one that names no date never lapses.

njutest owns the libtest flags that alter routing, repetition, selection,
output protocol, or completeness — positional filters, `--exact`, `--list`,
`--ignored`, `--skip`, `--format`, `--logfile`, `--test`, `--bench`, `-q`,
`--color`, `--report-time`, `--shuffle*`, `-Z*` — and rejects them after `--`.
