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

**Status: pre-alpha, under construction.** The engine works end to end:
`rust-mutants list`, `catalog`, `instrument`, `explain`, and `run` do what
they say on a real workspace. The assurance runner is next. The contracts under [`docs/`](docs/)
describe the intended product; each page carries a status line saying how much
of it exists. Nothing here should be read as a description of working software
until its status says so.

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

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
