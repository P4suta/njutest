# mjutest

`mjutest` (μtest) is an audit-oriented assurance runner for Rust and Cargo. It
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
- **`mjutest`** — the assurance runner on top of it, the Rust counterpart of
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

For `standard-v1`, mjutest:

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
rust-mutants run --coverage        # only against the targets that reached each mutant
rust-mutants run --shard 1/4       # one part of the catalog, and `merge` puts them together
rust-mutants report --format html  # one page that fetches nothing
rust-mutants report --tui          # read it at the terminal
```

[`docs/engine/comparison-with-cargo-mutants.md`](docs/engine/comparison-with-cargo-mutants.md)
says how it differs from the tool most Rust projects reach for, and why.

## Try it

```console
cargo install --path crates/mjutest-cli      # binary: mjutest
cargo install --path crates/rust-mutants-cli # binary: rust-mutants
mjutest --help
rust-mutants --help
```

## Existing tests stay ordinary Cargo tests

mjutest provides no assertion, mock, property, or container framework. The
optional `mjutest` crate only attaches resource metadata, and a comment
directive does the same without a dependency:

```rust
#[mjutest::integration("postgres", "redis")]
#[test]
fn repository_round_trips() {}

//mjutest:resources postgres redis
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
