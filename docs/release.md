<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Releasing

**Status: implemented.** The release train described here is `release-plz.yml` and `release.yml`, and `cargo xtask release-check` is the gate that keeps the versions in step.

A release is a tag, and everything else follows from it.
Nothing here is manual except deciding that a version is ready and approving the environment the publish runs in.

## What makes a version

release-plz runs on every push to `main` and keeps one release pull request open.
It bumps `[workspace.package].version` and writes `CHANGELOG.md` from the Conventional Commits since the last tag; merging that pull request cuts one `vX.Y.Z` tag.

Every crate shares the workspace version, so there is one tag rather than one per crate, and `njutest` owns it because the tag names the product a person installs.
`release-check` holds that shape: the root manifest is the only place a version is written, and a member carrying its own is one the next tag would not name.

It runs as a GitHub App rather than on `GITHUB_TOKEN`, for two reasons that are both about what the token may do.
Actions may not open a pull request in this repository at all, and GitHub suppresses workflow triggers from a ref pushed with `GITHUB_TOKEN` — so a tag pushed that way would start nothing, and `release.yml` is exactly what a tag is for.
The credentials are `RELEASE_PLZ_APP_CLIENT_ID` and `RELEASE_PLZ_APP_PRIVATE_KEY` on the `release-plz` environment, which is more scoped than a repository secret: a workflow on any other branch cannot read them.

## What the tag sets off

`.github/workflows/release.yml` runs on `v*`:

1. **check** — `cargo xtask release-check` agrees the manifests name one version, and the tag names that version.
   The third point, every binary saying it when asked `--version`, is asked where each binary is built, in the next step.
   Three points, one answer, or nothing is published.
2. **artifacts** — `cargo xtask bundle --target <triple> --out dist` on Linux, macOS, and Windows, which writes the archive and its SHA-256 beside it.
   What the archive holds is described under [The archive](#the-archive).
3. **sbom** — `cargo xtask sbom` writes what the release is made of, as CycloneDX, from the locked graph the build resolved.
4. **publish** — gated on the `release` environment, so a person approves it.
   It attests the provenance of every file it is about to publish, creates the release as a draft, and then undrafts it, so a failure between the two leaves a draft rather than a half-published release.

## The archive

`cargo binstall njutest` and `cargo binstall rust-mutants-cli` download the archive `[package.metadata.binstall]` names in each manifest and take each binary from the path its `bin-dir` names.
The archive is made by `cargo xtask bundle`, and nothing else decides what is in it: no list in a workflow, no second spelling of a path.

- Which binaries: every `[[bin]]` of every package cargo would publish, read from `cargo metadata`, so a binary declared tomorrow is bundled tomorrow.
- Where each goes: the package's `bin-dir`, filled the way binstall fills it for the target, `.exe` and all.
  Every shipped package's `pkg-url` must name the one archive, and a key, a format, or a template variable the command does not fill the way binstall does is refused (`XT7003`), because what it cannot predict it cannot promise.
- How each is built: in release, for the target, one package at a time, as `cargo install` builds it, taken from where cargo reports it put it (`XT7004`).
- What each says: every binary is asked `--version`, and one that does not name its package's version is refused before anything is written (`XT7005`).
- What goes beside them: both licences and the README, in the binaries' directory.

The archive is written by the tooling rather than by a platform's `tar`, owned by nobody and stamped with one time, so the same binaries make the same bytes on every platform.
It is read back and compared with its plan, entry by entry, before it takes its name, and its SHA-256 is written beside it as `shasum -a 256` writes one (`XT7006`).

It is the same command on a developer's machine:

```console
cargo xtask bundle --target aarch64-apple-darwin --out dist
```

## Before merging the release pull request

Merging it is the human gate; there is no other.

- `mise run check` is green on `main`.
- `cargo xtask all` is green, `proofaudit` included.
- The dogfood run (`mise run dogfood`) reaches a verdict, and any new survivor is either killed or accepted with a reason.
- `docs/roadmap.md` says what is done, and every contract page's status line is true of the code.
- `CHANGELOG.md` reads as something a user would want to read, which is what the release pull request is for.

## Publishing to crates.io

Not yet.
The workspace publishes as five crates and the API is not export-stable, and nothing here is fixed until it is.
Until then the release is the tag and its artifacts.
