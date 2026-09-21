# njutest

An assurance runner for Rust, and `rust-mutants`, the mutation-testing engine underneath it.
Either is usable on its own.

`njutest` reports a verdict rather than a score: `ASSURED` means a recorded full-project scope completed its configured fault model with no missing evidence.
A run is made faster only by proofs — evidence it already holds that an execution could not observe a mutant — never by a time budget, a sample, or an exclusion list.

`rust-mutants` compiles every mutant of a file into one instrumented snapshot, each dormant behind a guard, and selects one per test process.
Mutant identities are content-addressed, and the source workspace is read-only.

**Status: 0.1.0.** Both work end to end.
The public API and the document schemas are versioned and may still change before 1.0.

## Install

```console
cargo install --path crates/njutest-cli      # njutest, cargo-njutest
cargo install --path crates/rust-mutants-cli # rust-mutants, cargo-rust-mutants
```

## Use

```console
njutest verify                     # the assurance run, and a verdict
njutest review                     # go through the gaps, deciding as you read
rust-mutants run                   # every mutant, and an exit code that says what happened
rust-mutants explain <id>          # one mutation: where it is, what it did, how to reproduce it
rust-mutants report --format html  # one page, fetching nothing
```

Tests stay ordinary Cargo tests.
njutest adds no assertion, mock, property, or container framework; the optional `njutest` crate only attaches resource metadata, and a comment directive does the same with no dependency:

```rust
#[njutest::integration("postgres")]
#[test]
fn repository_round_trips() {}

//njutest:resources postgres
#[test]
fn repository_round_trips_without_the_crate() {}
```

## Documentation

- [Architecture](docs/architecture.md) and the [decisions](docs/adr/) behind it
- Engine: [getting started](docs/engine/getting-started.md), [command line](docs/engine/command-line.md), [reports](docs/engine/reports.md), [troubleshooting](docs/engine/troubleshooting.md)
- [How it differs from cargo-mutants](docs/engine/comparison-with-cargo-mutants.md)
- Contracts: [assurance](docs/assurance-contract.md), [report](docs/report-v2.md), [trace](docs/trace-v1.md), [errors](docs/errors.md)

## Development

```console
./bootstrap.sh     # the pinned toolchain and tools, then the git hooks
mise run check     # every local gate, in the order CI runs them
mise run doctor    # which tools are present and whether their versions match
```

[CONTRIBUTING.md](CONTRIBUTING.md) is the workflow and [docs/development.md](docs/development.md) the test harness, the design rules this repository enforces as gates, and the developer tooling.
[docs/release.md](docs/release.md) says what a tag sets off.

## Licence

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
