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

Every crate shares the workspace version, so there is one tag rather than one per crate, and `njutest-cli` owns it because the tag names the product a person installs.
`release-check` holds that shape: the root manifest is the only place a version is written, and a member carrying its own is one the next tag would not name.

It runs as a GitHub App rather than on `GITHUB_TOKEN`, for two reasons that are both about what the token may do.
Actions may not open a pull request in this repository at all, and GitHub suppresses workflow triggers from a ref pushed with `GITHUB_TOKEN` — so a tag pushed that way would start nothing, and `release.yml` is exactly what a tag is for.
The credentials are `RELEASE_PLZ_APP_CLIENT_ID` and `RELEASE_PLZ_APP_PRIVATE_KEY` on the `release-plz` environment, which is more scoped than a repository secret: a workflow on any other branch cannot read them.

## What the tag sets off

`.github/workflows/release.yml` runs on `v*`:

1. **check** — `cargo xtask release-check` agrees the manifests name one
   version, the tag names that version, and both binaries say it when asked
   `--version`. Three points, one answer, or the release stops here.
2. **artifacts** — a release build on Linux, macOS, and Windows, each bundled
   with both licences and the README, and each with its SHA-256 beside it.
3. **sbom** — `cargo xtask sbom` writes what the release is made of, as
   CycloneDX, from the locked graph the build resolved.
4. **publish** — gated on the `release` environment, so a person approves it.
   It attests the provenance of every file it is about to publish, creates the
   release as a draft, and then undrafts it, so a failure between the two
   leaves a draft rather than a half-published release.

## Before merging the release pull request

Merging it is the human gate; there is no other.

- `mise run check` is green on `main`.
- `cargo xtask all` is green, `proofaudit` included.
- The dogfood run (`mise run dogfood`) reaches a verdict, and any new
  survivor is either killed or accepted with a reason.
- `docs/roadmap.md` says what is done, and every contract page's status line
  is true of the code.
- `CHANGELOG.md` reads as something a user would want to read, which is what
  the release pull request is for.

## Publishing to crates.io

Not yet. The workspace publishes as five crates and the API is not
export-stable, and nothing here is fixed until it is. Until then the release
is the tag and its artifacts.
