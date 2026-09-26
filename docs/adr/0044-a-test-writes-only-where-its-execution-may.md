<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0044 — A test writes only where its execution may

## Status

Accepted, 2026-09-26, after review by njutest-bottleneck-optimization; amended the same day after review by phase-0a-0b-baseline-controls.
Implemented by `execute::Scratch`, which lays out and makes an execution's directories, `confine`, the check `confinement_held`, the baseline's `given_home` fallback, and the `unconfined-target` limitation.

## Context

A mutation changes what the code under test does, and the code under test may be the code that decides where a test writes.
`xtask/tests/slot.rs` isolates its lanes by setting `NJUTEST_SLOT_DIR` to a temporary directory.
A mutant of `xtask/src/lanes.rs` that drops that branch falls through to `XDG_STATE_HOME`, then `HOME`.
Under measurement those are the real ones, so a test under that mutant wrote `~/.local/state/njutest/slots/heavy.holder` and took the machine's heavy lane, and every session's push waited on it for ten minutes.
A disk cleaner such as storage-scout, whose tests confine it to a temporary root, is one mutation away from pruning the real home.

The engine already gives each execution a scratch of its own for `TMPDIR`, `TMP` and `TEMP` ([architecture](../engine/architecture.md#the-scratch-a-test-process-is-given)), and a control may already run a test with another `HOME` ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)), because the contract lets `HOME` differ between machines.
What it hands every test unchanged is the real `HOME` and the XDG directories under it.

## Decision

1. **Every execution gets a home of its own, beside its temporary directory.** `execute::Scratch` lays an execution's directory out as `tmp`, `engine` and `home`, none inside another.
   `HOME` names `home`, created empty for the execution, and `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME` and `XDG_RUNTIME_DIR` name directories under it, the last one only its owner may enter.
   On Windows `USERPROFILE`, `HOMEDRIVE` with `HOMEPATH`, `APPDATA` and `LOCALAPPDATA` do the same, each set in place of every spelling Windows reads as its name.
   A write a test makes through any of them lands where the execution's scratch is emptied, whatever a mutant did to the code choosing the path.
   The home is never inside `tmp`, because what a run reads in `tmp` is the process's own — a crash's leftovers are ([ADR 0035](0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)) — and a test that empties its `TMPDIR` must empty nothing else.
   The variables are a closed list the engine owns; a variable it does not name keeps the value the run was given, even where it names a place under the given home.
2. **What a build needs is pinned to where it is.** `CARGO_HOME` and `RUSTUP_HOME` are set to the directories they name for the engine, the defaults under the real home spelled out, so a test that runs cargo finds the registry and the toolchains.
   Where the run was given no home, only a variable that is set is pinned.
   `CARGO_HOME` stays writable, since cargo writes its registry cache there: a mutation that redirects an install into it is a hole this record names and accepts.
   A bare `cargo` is asked from the copy both as a test with the given home asks it and as one in a home of its own does, and the run's own toolchain comes first on the search path where either would not find it.
3. **What a test reads from the home is a copy.** The engine copies git's identity from where git reads it — `GIT_CONFIG_GLOBAL` where it is set and `~/.gitconfig` otherwise, and `$XDG_CONFIG_HOME/git/config`, or `~/.config/git/config` where that is unset — into the execution's home, so a test that commits in a temporary repository keeps the identity it had.
   `GIT_CONFIG_GLOBAL` is removed from the process's environment, so `git config --global` in a test writes the copy.
   Nothing else is copied.
   A source that is absent is nothing to copy; a source that cannot be read, or a home that cannot be made, refuses the run with `RM5012`, since it is the engine's failure and never a fact about the target.
4. **A target whose tests fail in its own home and pass with the real one runs with the real one, and every run says so.** A baseline runs in a home of its own first.
   Only where its tests ran and failed there, twice, is it run again with the real home; a process that did not start or did not finish says nothing about a home.
   A target that passes only there keeps the real home for every execution of the run, and the run reports it as `unconfined-target`, every run, including one that recalls the remembered baseline, so the one place a mutation can still write outside is never silent.
   The remembered baseline is keyed on the environment of both homes, so a change to what the real home's variables name is a new baseline.
   Each run of a baseline has a directory of its own, so the retry and the run with the real home never start over what an earlier one left.
   The reach a target showed in a home of its own is not read for a target that runs with the real one: every mutant routes to it as to a target nothing measured.
5. **The engine checks what it composed before it starts a process.** Before a confined process starts, `confinement_held` checks that every variable the engine confines names the execution's home, under one spelling, and that no global git configuration of the real home is named; a process that fails the check is not started, and its execution is an apparatus error the trace names `execution-confinement`.
   A control that asks for another home on purpose, as the `home` knob does with a directory of njutest's scratch ([ADR 0025](0025-a-reach-that-moves-is-not-a-measurement.md)), sets it over what the check held, from the closed set of variables a control may change.

## Consequences

- A mutation of confined code can no longer redirect a write into the user's home, their XDG directories, or a machine-wide lock kept there.
- `CARGO_HOME` is the named exception, and an `unconfined-target` the stated one; a variable the engine does not confine that names a place under the home, as `MISE_DATA_DIR` may, is a hole this record names: which variables are places a program writes, rather than programs or search paths, is not something the engine can tell from a value.
- On Windows a program that asks the shell for a known folder, rather than reading `USERPROFILE`, `APPDATA` or `LOCALAPPDATA`, is told the real one, so the claim there covers what the variables name.
- A tool a test runs through a shim that keeps its state under the home, as mise does, finds nothing in the execution's home; the run's toolchain is pinned first on the search path, and anything else a target needs from the real home makes it an `unconfined-target` rather than a failure.
- The registry has a row, `execution-confinement`: `Scratch` as its type, `confinement_held` as its self-check, the fixture that writes under `$HOME` as its oracle, three planted escapes as its plant, and the Windows spellings as its states.
