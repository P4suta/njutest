# Contributing

## Ground rules

- **Host toolchain.** Everything runs on host `cargo`. The toolchain is pinned
  by `rust-toolchain.toml`; the tools by `mise.toml`.
- **Test-driven, infrastructure first.** Write the test, watch it fail for the
  stated reason, make it pass, remove the duplication. Every change carries
  its developer-facing deliverables — tests, traces, gates, diagnostics,
  documentation — as completion criteria, not as follow-ups. The protocol is
  in [docs/development.md](docs/development.md).
- **No warning suppressions.** An `#[allow(...)]` needs a `reason = "…"`; CI
  runs clippy with `-D warnings` and the workspace lints deny `unwrap`,
  `expect`, `panic`, indexing, and arithmetic side effects in production code.
- **Conventional Commits**, enforced by the `commit-msg` hook (`committed`).
- **Every `.rs`, `.toml`, and `.yml` file starts with the SPDX header:**

  ```rust
  // SPDX-FileCopyrightText: 2026 mjutest contributors
  // SPDX-License-Identifier: MIT OR Apache-2.0
  ```

## Setup

```console
./bootstrap.sh
```

`mise` is the only prerequisite. The script installs the pinned toolchain and
tools, fetches the locked dependency graph, and installs the git hooks.
`mise run doctor` reports what is present and what is missing.

## Development loop

| Task | What it runs |
| --- | --- |
| `mise run check` | every local gate in the order CI runs them: formatting, build, tests, clippy, rustdoc, repository gates, spelling, TOML, workflows, cargo-deny |
| `mise run test` | every target through `cargo nextest`, then the doctests |
| `mise run test:fast` | the inner loop: every suite that starts no toolchain, in seconds |
| `mise run test:slow` | the suites that drive a real cargo against `fixtures/` |
| `mise run gates` | `cargo xtask all`: the seam ratchet, the lint gate, dependency direction, fixture conventions, release consistency |
| `mise run coverage` | region coverage with the ratchet CI enforces |
| `mise run mutants` | cargo-mutants against this workspace |
| `bacon` | the watch loop (`bacon clippy`, `bacon test` = the inner loop, `bacon test-all`, `bacon gates`, `bacon doc`) |

Golden files are rewritten with `UPDATE_GOLDEN=1 cargo test …`; the diff is
the review. Compile-error goldens of the attribute macros are rewritten with
`TRYBUILD=overwrite`.

## Pull requests

- Open a pull request against `main`; direct pushes are rejected.
- Every CI job must pass, including the macOS and Windows test matrix. Paths
  from `tempfile` may pass through symbolic links or short names on those
  runners, so canonicalize before comparing.
- Pull requests are squash-merged, so the PR title becomes the commit message
  and must follow Conventional Commits: `feat:`, `fix:`, `perf:`, `refactor:`,
  `test:`, `docs:`, `chore:`, `ci:`, `build:`. The changelog lists `feat`,
  `fix`, `perf`, and `revert`.
- Behaviour changes need tests, and the pull request template asks for the
  failing output of the first test you wrote.
- `xtask/seam_allowlist.txt` may shrink and never grow. Adding a line is a
  reviewed exception recorded in [ADR 0001](docs/adr/0001-seam-policy.md).

## Troubleshooting

- *clippy passes locally but fails in CI.* CI runs with `--all-features` and
  `RUSTFLAGS=-D warnings`; so does `mise run clippy`.
- *typos flags a word that is right.* Add it to `_typos.toml` with a comment
  saying what it is.
- *a golden test fails on a fresh checkout.* The file is missing or the
  contract changed; `UPDATE_GOLDEN=1` records it and the diff goes in the
  review.
- *`cargo test` cannot reach a registry.* The suites run offline against
  `fixtures/`; `cargo fetch --locked` once, with network, is enough.

## Dependencies

- Cargo dependencies and GitHub Actions are updated weekly by Dependabot.
  Actions are pinned to commit SHAs with a version comment; keep that format.
- Tool versions in `mise.toml` are not managed by Dependabot. Bump them by
  hand and update the matching `taiki-e/install-action` pins in
  `.github/workflows` in the same change.

## License

mjutest is dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE). By contributing you agree that your
contributions are licensed under the same terms.
