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
timeout = "5m"                 # one mutant execution, before a serial retry
build_timeout = ""             # empty = no bound
verify = true                  # run the instrumented baseline first
coverage = false               # measure reach once, then run a mutant only where it was reached

[execution]
offline = false
locked = false
doctests = true                # run a library's documented examples as a target
skip_targets = []              # target ids never to start, as pkg/kind/name
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

## Reserved environment

A run composes `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG`, and
`RUST_MUTANTS_PROBE` for every test process it starts. Finding any of them
already set in its own environment ends the command with `RM0006`: nothing a
test process said under an inherited activation would be about this run.
