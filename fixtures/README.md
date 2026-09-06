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
- A fixture's `README.md` states what it is for, in prose, and ends with a
  ```` ```fates ```` block: one line per mutation and per refused candidate,
  as `path:line:column rule outcome`. The rest of the fence line is the
  arguments the run takes beyond `--tier all --offline --locked`, so a fixture
  that exists for a proof layer says which layer. The block is documentation
  and test data at once: `cargo test -p rust-mutants-cli --test
  toolchain_fates` runs every fixture and refuses a difference, and
  `UPDATE_FATES=1` rewrites the blocks so the diff is the review.
- Every fixture is named by at least one test. A fixture nothing drives is one
  nothing keeps honest, and the same suite refuses it.
