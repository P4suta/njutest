# Fixture projects

Each directory here is an independent cargo project the integration suites of
`rust-mutants` and `mjutest-cli` drive with a real `cargo`. The conventions,
enforced by `cargo xtask fixtures`:

- `Cargo.toml` carries an empty `[workspace]` table, so cargo never looks
  upwards and a fixture that fails on purpose cannot fail this workspace.
- `Cargo.lock` is committed. Fixtures build with `--locked --offline`.
- The only dependencies are paths inside the fixture itself, such as a
  proc-macro member: anything else would need a registry, and the suites run
  offline.
- Every `.rs` and `Cargo.toml` starts with the SPDX header used across the
  repository.
- Fast: baselines are measured, and derived timeouts scale with the slowest
  run.
- A fixture's `README.md` states what it is for and, where the fixture exists
  to have a known fate under mutation, a table of every mutant and its
  expected outcome. That table is documentation and test data at once.
