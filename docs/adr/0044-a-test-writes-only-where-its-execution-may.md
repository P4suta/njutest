<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0044 — A test writes only where its execution may

## Status

Accepted, 2026-09-26, after review by njutest-bottleneck-optimization.
Implemented by `execute::Home` and `confine` in rust-mutants, the baseline's `given_home` fallback, and the `unconfined-target` limitation.

## Context

A mutation changes what the code under test does, and the code under test may be the code that decides where a test writes.
`xtask/tests/slot.rs` isolates its lanes by setting `NJUTEST_SLOT_DIR` to a temporary directory.
A mutant of `xtask/src/lanes.rs` that drops that branch falls through to `XDG_STATE_HOME`, then `HOME`.
Under measurement those are the real ones, so a test under that mutant wrote `~/.local/state/njutest/slots/heavy.holder` and took the machine's heavy lane, and every session's push waited on it for ten minutes.
A disk cleaner such as storage-scout, whose tests confine it to a temporary root, is one mutation away from pruning the real home.

The engine already gives each execution a scratch of its own for `TMPDIR`, `TMP` and `TEMP` ([architecture](../engine/architecture.md#the-scratch-a-test-process-is-given)), and a control may already run a test with another `HOME` ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)), because the contract lets `HOME` differ between machines.
What it hands every test unchanged is the real `HOME` and the XDG directories under it.

## Decision

1. **Every execution gets a home inside its scratch.** `HOME` names `<scratch>/home`, created empty for the execution, and `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `XDG_DATA_HOME` name directories under it.
   On Windows `USERPROFILE`, `HOMEDRIVE` with `HOMEPATH`, `APPDATA` and `LOCALAPPDATA` do the same.
   A write a test makes through any of them lands where the execution's scratch is emptied, whatever a mutant did to the code choosing the path.
   The variables are a closed list the engine owns; a variable it does not name keeps the value the run was given.
2. **What a build needs is pinned to where it is.** `CARGO_HOME` and `RUSTUP_HOME` are set to the directories they name for the engine, the defaults under the real home spelled out, so a test that runs cargo finds the registry and the toolchains, and the run's own toolchain comes first on the search path where a bare `cargo` would not find it.
   `CARGO_HOME` stays writable, since cargo writes its registry cache there: a mutation that redirects an install into it is a hole this record names and accepts.
3. **What a test reads from the home is a copy.** The engine copies `.gitconfig` and `$XDG_CONFIG_HOME/git/config` into the execution's home where they exist, so a test that commits in a temporary repository keeps the identity it had.
   It is a copy and never `GIT_CONFIG_GLOBAL`, so `git config --global` in a test writes the copy.
   Nothing else is copied.
4. **A target that passes only with the real home runs with it, and every run says so.** A baseline runs under the execution's home first.
   Only where it fails is it run again with the real home, and a target that passes only there keeps the real home for every execution of the run.
   The run reports it with the finding kind `unconfined-target`, naming the target, every run, as `skip_targets` is reported, so the one place a mutation can still write outside is never silent.
   The second baseline is paid only by a target that failed the first.

## Consequences

- A mutation of confined code can no longer redirect a write into the user's home, their XDG directories, or a machine-wide lock kept there.
- `CARGO_HOME` is the named exception, and an `unconfined-target` the stated one.
- A tool a test runs through a shim that keeps its state under the home, as mise does, finds nothing in the execution's home; the run's toolchain is pinned first on the search path, and anything else a target needs from the real home makes it an `unconfined-target` rather than a failure.
- The registry gains a row, `execution-confinement`: the invariant, the closed list of variables as its states, the fixture that writes under `$HOME` as its oracle, and `CARGO_HOME` and `unconfined-target` as owned holes.
