<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Releasing

**Status: implemented.** The release train described here is `release-please.yml` and `release.yml`, and `cargo xtask release-check` is the gate that keeps the versions in step.

A release is a tag, and everything else follows from it. Nothing here is
manual except deciding that a version is ready and approving the environment
the publish runs in.

## What makes a version

`release-please` is started by hand — `workflow_dispatch` — and opens a release
pull request with the next version and the changelog it derives from the
Conventional Commits since the last tag. Merging that pull request writes the
version into every manifest and pushes the tag.

It is asked for rather than automatic because opening that pull request needs a
permission this repository withholds from Actions, so a run on every push to
`main` could only fail, and a branch that is red for a job nobody wanted is a
branch whose colour says nothing. Granting the permission is part of meaning to
cut a version, not part of merging a change.

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
