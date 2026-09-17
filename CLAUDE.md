# Working in this repository

njutest is two products in one workspace: `rust-mutants` (a mutation testing
engine, `crates/rust-mutants`) and `njutest` (an assurance runner on top of
it, `crates/njutest-cli`). Read `docs/architecture.md` first, then
`docs/adr/0004-proof-layers-not-budgets.md` and `docs/adr/0001-seam-policy.md`;
they explain most of what looks unusual here.

## The protocol

1. **Red.** Write the test against the behaviour, not the implementation, and
   watch it fail for the stated reason. Paste that output in the pull request.
   A test that passes before the change is not evidence.
2. **Green.** The smallest change that is honest about the contract.
   Fail-closed is part of the contract, not an error path to add later.
3. **Refactor.** Remove the duplication with the suite green throughout.
4. **Infrastructure is a deliverable.** Every milestone in the plan carries
   tests, traces, gates, diagnostics, and documentation as completion
   criteria. Speed never wins over them.

## Gates you must keep green

```console
mise run check          # everything CI asks that this machine can answer, and what pre-push runs
cargo xtask all         # seam ratchet, lints, dependency direction, fixtures, release consistency
cargo xtask report-diff A B   # what changed between two stored reports
```

- `xtask/seam_allowlist.txt` may shrink and never grow. If the seam ratchet
  fails, move the behaviour into an argument (an options field, a hooks
  value, a trait object); only `main.rs` reads the process environment.
- Every error variant has a code in `docs/errors.md`; the tests keep both in
  sync.
- Golden files: `UPDATE_GOLDEN=1` rewrites, and the diff is the review.
  `TRYBUILD=overwrite` for the macro compile-error goldens.
- Fixtures under `fixtures/` are independent cargo projects with a committed
  `Cargo.lock` and no dependencies; their README states each mutant's fate.

## Conventions

- English in code, documentation, and commits. Conventional Commits. SPDX
  header on every `.rs`, `.toml`, `.yml`.
- **No comments, and one line of documentation.** `cargo xtask lints` refuses a
  comment anywhere in the repository's own code. What stays is the SPDX header,
  a `rust-mutants:` annotation, which is an instruction the engine reads rather
  than an account of the code, and one line per item saying what it is — plus
  the `# Errors` and `# Panics` sections clippy asks for. Nothing else: a name
  that needs a comment is a name to change, and an assertion that needs one
  wants a better message. A rationale long enough to want paragraphs belongs in
  `docs/` or an ADR, where somebody deciding goes looking for it, rather than
  beside the code, where it is read by everybody working past it.
- Workspace lints are strict (`pedantic`, `nursery`, `unwrap_used`, …).
  **`#[allow]` is never written** — `cargo xtask lints` refuses it anywhere,
  tests included. A waiver is `#[expect(…, reason = "…")]`, which the
  compiler retires once the lint stops firing, so every waiver in the tree
  is one somebody still needs. The only `allow` in the repository is inside
  the string literals the engine generates into somebody else's tree.
- **No `Box<dyn Trait>`**, and the same gate refuses it. A closed set of
  implementations is an enum the compiler checks exhaustively; an open one
  is a generic parameter. A boxed trait object is a decision to argue for,
  not one to accumulate.
- Errors are `thiserror` enums with named fields, `#[non_exhaustive]`, a
  `code()`; names end in `Error` but are never bare `Error`.
- No `unsafe` outside the two Windows FFI modules of the engine.
- Dependencies are minimal; the engine adds nothing to the project under test.

## Where things are

| | |
| --- | --- |
| Plan and milestones | the approved plan lives outside the repository; `docs/architecture.md` states the current shape |
| Contracts | `docs/assurance-contract.md`, `docs/report-v1.md`, `docs/trace-v1.md`, `docs/engine/` |
| Decisions | `docs/adr/` |
| Developer tooling | `docs/development.md`, `xtask/`, `crates/njutest-devkit/`, `scripts/doctor.sh` |
| Error codes | `docs/errors.md` |
