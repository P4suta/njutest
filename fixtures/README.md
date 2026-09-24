# Fixture projects

Each directory here is an independent cargo project the integration suites of
`rust-mutants` and `njutest-cli` drive with a real `cargo`. The conventions,
enforced by `cargo xtask fixtures`:

- `Cargo.toml` carries an empty `[workspace]` table, so cargo never looks
  upwards and a fixture that fails on purpose cannot fail this workspace.
- `Cargo.lock` is committed. Fixtures build with `--locked --offline`.
- The only dependencies are paths inside the fixture itself, such as a proc-macro member, or a climb out of it that lands on another fixture and nothing else: anything else would need a registry, and the suites run offline.
  How far the climb goes is how deep the fixture sits, so a fixture in a group reaches its library by climbing twice.
- A directory here holding no `Cargo.toml` is a **group**, and holds fixtures and nothing else.
  A group exists for the one thing a flat directory cannot express — a tree whose root is nested below what it reads — and `cargo xtask fixtures` refuses a group inside a group, so this stays a place to find a fixture rather than a tree to search.
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
- A fixture whose `.njutest.toml` puts an interposer in front of a seam ends
  with a ```` ```seams ```` block as well: one line per question the recording
  licensed, as `capability:seq rule decision [who decided it]`. What a run
  establishes about a seam is a ledger for the same reason its mutation fates
  are one. `cargo test -p njutest-cli --test toolchain_wire_fixture` runs it
  again and refuses a difference, and `UPDATE_FATES=1` rewrites the block.
- Every fixture is named by at least one test. A fixture nothing drives is one
  nothing keeps honest, and the same suite refuses it.
- A fixture that exists to hold a selection to account (`fixture-edits`) carries its edits under `edits/`, one directory per edit holding each file as the edit leaves it, or a `FEATURES` file where the edit is to how the fixture is built.
  It states what each one breaks in an ```` ```edits ```` block: one line per edit, as `<edit> breaks <package>/<kind>/<name>, …` or `<edit> breaks nothing`.
  `cargo test -p njutest --test toolchain_edits` applies each edit to a copy, builds and runs every target, and refuses a difference, without asking any selection anything: the block is what an edit breaks, established apart from whatever is later asked to predict it.
