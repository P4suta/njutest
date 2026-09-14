<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0012 — One workspace, two products

## Status

Accepted, 2026-09-05 (user decision). Enforced by `cargo xtask deps`.

## Context

goatest and go-mutants are separate repositories, and
[ADR 0004](0004-proof-layers-not-budgets.md) notes the consequence: a new
proof is a change to two repositories — the engine gains a claim, the runner
gains a rule, a trace vocabulary, documentation, and an audit layer — and a
proof without all four is not finished. Two repositories make that a
sequence of pull requests with a version pin between them.

## Decision

`rust-mutants` and `njutest` live in one Cargo workspace. The engine remains
a product of its own — a library with a stable public API and, from milestone
E2, a standalone command line with its own reports — and the dependency
direction is fixed: the runner depends on the engine and on the public API
crate, the engine depends on nothing of the runner, the public API crate
depends only on its macros, and the devkit is a dev-dependency of everybody.
`cargo xtask deps` refuses every other edge.

## Consequences

- A proof is one pull request with the four parts together.
- The engine's crates are publishable on their own; nothing in them names
  the runner.
- One CI, one set of gates, one dogfood: `mise run dogfood` runs the runner on
  the workspace, and `mise run dogfood:engine` runs the engine on it.
