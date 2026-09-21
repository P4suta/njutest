<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0024 — A copy is one prefix substitution

## Status

Accepted, 2026-09-21.
Bounds where `snapshot::create` may put anything it copies.

## Context

A run measures a copy of the tree, and the claim the whole product rests on is that nothing about how it measured reaches the conclusion (ADR 0023).
The copy is therefore not merely a place to build in: it is the thing the measurement is of, and every property of the original the measurement depends on has to survive the move.

Relative paths are one of those properties, and until now they survived by coincidence.

`snapshot::create` placed the measured tree at `<snapdir>/tree`, and `beside()` placed each `--allow-outside` directory at `<snapdir>/<basename>`.
Two independent rules.
A `path = "…"` declaration written from inside the tree resolves, after the move, only where the two rules happen to agree on depth — which is exactly when the allowed directory is a sibling of the root.
Three ordinary layouts did not agree:

- a root nested below what it reads (`trees/nested/proj` naming `../../lib`) resolved outside the snapshot entirely;
- an allowed directory that is not a child of the root's parent (`../shared/lib`) resolved at a path nothing created;
- two allowed directories sharing a basename collided on one destination.

Each failed inside cargo, as a manifest that is not there, three layers from the cause.

An attempt to patch it by rewriting the copied manifests — replacing `path = "…"` textually — was written and abandoned half-built, and it is worth recording why it must not be revived.
Text cannot see the single-quoted spelling, the inline table, the entry under `[patch]`, the one a member inherits from `[workspace.dependencies]`, the `paths` key in `.cargo/config.toml`, or a `build.rs` reading `../`; and rewriting the bytes after they are on disk makes the workspace digest describe a tree that differs from the one on disk.
A geometry problem is not repaired by editing text.

## Decision

**Everything a copy holds is placed by substituting one prefix.**

Let `C` be the longest path the measured root and every allowed directory begin with.
The copy's stage is `<snapdir>/tree`, and every copied directory `D` goes to `stage / (D relative to C)`.

A single prefix substitution preserves every relative path between the things it moves, because a relative path is a count of components and the substitution changes none of them.
The property is arithmetic, not an invariant anybody maintains: `crates/rust-mutants/tests/snapshot_layout.rs` states it as a law and proves it over generated component vectors.

`snapshot::Layout` is the only way to name a placement, and `snapshot::Placement` the only way to name a destination.
Neither has a constructor that takes a destination, so no code can put a copied directory anywhere the substitution did not.
`snapshot::create` takes the layout instead of a source root, so the root a copy is of and the root it was told about cannot disagree.

With no `--allow-outside`, `C` is the root itself, the relative part is empty, and the tree is the stage — byte for byte the layout every ordinary run has always had.

Four refusals remain, and they are refusals because they are facts about two paths rather than a shape a type can hold: a path that is not absolute, one that still climbs, a directory that is the tree or holds it or lies inside it, and one on another filesystem root.
They share `RM1019`.

## Consequences

- A declaration climbing any number of levels resolves inside the copy.
  `fixtures/nested/fixture-climbs-dep` is the case, and the reason `fixtures/` grew the idea of a group: a tree whose root sits below what it reads cannot be expressed by a flat directory.
- The copy's shape now depends on its input.
  Someone reading a kept snapshot has to look rather than know — mitigated by the common case not moving.
- Copied paths are longer when the shared ancestor is high.
  On Windows, where `canonical::plainly` strips the `\\?\` prefix on purpose, a deep ancestor under a deep tree can cross 260 characters and stop the copy with `RM1008`.
  That is recorded in `docs/limitations.md` rather than guarded, because the exclusive `create_dir` already reports it and no bound anybody could write would be right for every filesystem.
- The engine now makes directories the source did not have: the empty ones between the stage and what it holds.
  `Placement::scaffolding` names each exactly once, so they are still created with an exclusive `create_dir` and a directory already there is still a fact to react to.
- `beside()`'s silent skip of a directory whose path ended in `..` is gone, because a destination is computed rather than derived from a file name.
