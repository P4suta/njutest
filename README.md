# njutest

`njutest`, short for "new test", is an audit-oriented assurance runner for Rust and Cargo. It
connects ordinary Cargo tests with coverage routing, mutation testing through
its own engine `rust-mutants`, paired kill confirmation, a soundness phase,
targeted fuzzing, explicit integration resources, and reviewable repair
candidates — and it reports a **verdict**, never a percentage.

This repository is two products in one Cargo workspace:

- **`rust-mutants`** — a mutation testing engine for Rust. One instrumented
  snapshot, every compilable mutant of a file dormant behind a guard, one
  environment variable per test process, stable content-addressed mutant IDs,
  a read-only source workspace. The fourth engine in a family —
  [ocaml-mutants], [gleam-mutants], [go-mutants] — that shares this
  architecture.
- **`njutest`** — the assurance runner on top of it, the Rust counterpart of
  [goatest]. `ASSURED` means that a recorded full-project scope completed its
  configured fault model without missing evidence. A run is made faster only
  by *proofs* — evidence the run already holds that an execution could not
  observe a mutant — and never by a time budget, a sample, or an exclusion
  list.

**Status: 0.1.0, and pre-1.0 in every sense the number implies.** Both
products work end to end on a real workspace, and the contracts under
[`docs/`](docs/) describe what they do rather than what they will do; each page
carries a status line, and [`docs/roadmap.md`](docs/roadmap.md) says which
milestones are done. What is not settled is the shape of the public API and of
the documents: they are versioned, they are validated by their own schemas,
and they may still change before 1.0.

[ocaml-mutants]: https://github.com/P4suta/ocaml-mutants
[gleam-mutants]: https://github.com/P4suta/gleam-mutants
[go-mutants]: https://github.com/P4suta/go-mutants
[goatest]: https://github.com/P4suta/goatest

## What a verification will do

For `standard-v1`, njutest:

1. freezes an exact input identity covering source, tests, fuzz corpus,
   dependencies (`Cargo.lock`), toolchain, platform, declared environment,
   configuration, and tool versions;
2. runs every `#[test]` target in its own process under coverage
   instrumentation and classifies it, skips and setup failures included;
3. routes tests to mutants by coverage regions and mutant positions, and
   discharges the reaching tests a branch proof or a probe measurement shows
   cannot observe a mutant;
4. inventories the `unsafe` surface of every crate (the `soundness` phase;
   `deep-v1` runs Miri on it);
5. evaluates every selected `rust-mutants` mutant, with a passing original
   control immediately before a repeated kill confirmation;
6. records survivors, inconclusive outcomes, compile rejections, acceptances,
   and out-of-scope mutants as a complete ID-level inventory; and
7. stores any killing fuzz input or generated test as a candidate. `verify`
   never changes source or corpus; only `fix --apply` does.

The design and the milestones are in [`docs/architecture.md`](docs/architecture.md)
and the decisions behind them in [`docs/adr/`](docs/adr/).

## The engine on its own

`rust-mutants` is a product in its own right, and does not need the runner:

```console
rust-mutants list                  # what the rules propose, before the compiler has ruled
rust-mutants catalog --json        # what it accepts, and every refusal in its own words
rust-mutants run                   # every mutant, and an exit code that says what happened
rust-mutants run --changed         # only the files that differ from HEAD
rust-mutants run --shard 1/4       # one part of the catalog, and `merge` puts them together
rust-mutants report --format html  # one page that fetches nothing, showing every survivor in place
rust-mutants report --tui          # read it at the terminal
rust-mutants rules                 # every operator, with the tier and version that pin it
```

A session against one of this repository's own fixtures, recorded from the
binary by a test so the page cannot drift from the tool:

```console
$ rust-mutants run

run       <run>
workspace <workspace digest>
catalog   <catalog digest>

discharged-mutant      every target that could have noticed 16b0cd40508fc0785477 was removed by a proof, so no test could have: the mutation is in code the tests run and never observe

MUTANTS   10 mutants were cataloged: 9 executed, 0 refused by the compiler, 2 places that produced no candidate.
OUTCOMES  killed=9 survived=0 runaway=0 waited=0 inconclusive=0 errored=0 not_run=1
OF THOSE  Those 7 add to the 10 cataloged. Within them, not run is 0 unreached, not run is 1 discharged, survived is 0 expected.
SCORE     100.0%  (9 detected of 9 decided)
WORK      started=9 of 30 pairs across 3 targets; 70.0% removed (unreached=20 never-infected=1)
          tests=9 of 30; 70.0% removed
REPORT    ./reports/mutation/<run>/run-report-v1.json

$ rust-mutants explain 16b0
NAME      src/lib.rs:max:gt-to-ge@11
MUTANT    16b0cd40508fc0785477d495daa14695fcff4d61ada9eb35a8c2c995eb1f881f
SHORT     16b0cd40508fc0785477
RULE      gt-to-ge@1 (comparison)
WHERE     src/lib.rs:11:10
EDIT      ">" => ">="
RUN       <run>
OUTCOME   not_run
TIMING    <duration>
ROUTE     discharged reaching [] executed []
PROVED    fixture-simple/lib/fixture_simple: never-infected
REPRODUCE rust-mutants run --mutant src/lib.rs:max:gt-to-ge@11
ACCEPT    [[mutation.expect]]
          path = "src/lib.rs"
          item = "max"
          rule = "gt-to-ge"
          original = ">"
          line = 11
          reason = ""  # why this is not a gap in the tests

--- a/src/lib.rs
+++ b/src/lib.rs
@@ -8,7 +8,7 @@
 
 /// The larger of two numbers, spelled with a comparison a mutant can flip.
 pub fn max(a: i32, b: i32) -> i32 {
-    if a > b { a } else { b }
+    if a >= b { a } else { b }
 }
 
 /// Whether `n` is even.
```

That mutation is never run. The guards of the instrumented tree hold both
branches at `a > b`, and on the one baseline run they never answered
differently, so the engine reports what running it would have established
rather than spending a process on it
([ADR 0015](docs/adr/0015-the-guard-is-the-infection-probe.md)). It is a
finding all the same, and the exit code is 1: a mutation the tests run and
cannot notice is the same gap as one they run and do not notice. The score is
over what a run *decided*, and this one was decided by a proof rather than by
a test.

A gap is a gap in the tests or a claim to write down; `explain` says which,
`replay` puts it back to the tests — and for a discharged mutation that is
what puts the proof itself to them — and `[[mutation.expect]]` accepts one
with a reason. There is no threshold flag and no percentage to pass:
[ADR 0004](docs/adr/0004-proof-layers-not-budgets.md) says why.

[getting started](docs/engine/getting-started.md) is the first hour,
[the command line](docs/engine/command-line.md) every flag,
[reports](docs/engine/reports.md) what a run becomes for other readers, and
[troubleshooting](docs/engine/troubleshooting.md) what to do when something
is wrong.
[`docs/engine/comparison-with-cargo-mutants.md`](docs/engine/comparison-with-cargo-mutants.md)
says how it differs from the tool most Rust projects reach for, and why.

## Try it

```console
cargo install --path crates/njutest-cli      # njutest, cargo-njutest
cargo install --path crates/rust-mutants-cli # rust-mutants, cargo-rust-mutants
njutest --help
rust-mutants --help
```

## Existing tests stay ordinary Cargo tests

njutest provides no assertion, mock, property, or container framework. The
optional `njutest` crate only attaches resource metadata, and a comment
directive does the same without a dependency:

```rust
#[njutest::integration("postgres", "redis")]
#[test]
fn repository_round_trips() {}

//njutest:resources postgres redis
#[test]
fn repository_round_trips_without_the_crate() {}
```

## Development

Development is test-driven and developer infrastructure comes first: every
milestone carries its tests, traces, gates, and diagnostics as completion
criteria. See [CONTRIBUTING.md](CONTRIBUTING.md) for the `mise`-based
workflow and [`docs/development.md`](docs/development.md) for the test harness,
the TDD protocol, and the catalog of developer tooling.

```console
./bootstrap.sh     # mise installs the pinned toolchain and tools, then the git hooks
mise run check     # every local gate, in the order CI runs them
mise run doctor    # which tools are present and whether their versions match
```

Releases are tags: [`docs/release.md`](docs/release.md) says what a tag sets
off, what is checked before anything is published, and what each release
carries with it.

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
