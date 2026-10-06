<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0050 — A run adopts every process it starts

## Status

Accepted, 2026-10-06.
Implemented by `rust_mutants::escaped::{adopt, Adoption, reap_adopted}`, the census in `escaped::working_under` and `escaped::end_working_under`, `rust_mutants_decision::descent::{descent, could_have_started}` and their Kani laws, `njutest_process::{adopt, reap_adopted}` and `njutest_process::procfs::parsed`, and the reaping at the end of every `GroupChild` settlement.
Held by `snapshot::a_hidden_process_this_one_did_not_start_does_not_refuse_the_cleanup`, `snapshot::a_hidden_process_this_one_started_still_refuses_the_cleanup`, the tests of `njutest_process`'s `adopted` module, and the unreaped processes `toolchain_cli_contract` looks for after a run of `fixture-escapes`.

## Context

A run ends every process still working in its copy of the tree or its scratch before it removes them: `Workspace::close` and the cleanup of a `Snapshot` take a census of the processes of this user by the directory each works in.
On Linux the census reads `/proc/<pid>/cwd`, and `/proc` refuses that to the process's own user where the process is not dumpable: an `sshd-session` for a new login, a process that changed its credentials through `sudo -u`, a setgid program.

The census answered such a refusal by when the process started.
One that started before this process could not have been started by it and was left out; one that started after was refused as a process the run may have started, and the cleanup failed with RM1011, which a `Snapshot`'s drop turns into an abort.
A login that arrives while a run is under way is such a process.
It failed a coverage job on GitHub's ubuntu-24.04 runner, whose own processes logged in during the run, and it failed one toolchain test after another on a shared Linux machine that takes new SSH logins all the time ([issue 257](https://github.com/P4suta/njutest/issues/257)).
A person running njutest on a server while opening another SSH or editor connection meets the same refusal.

When a process started says nothing about who started it.
Its parents do, but only if every process the run started stays below the run: by default a process whose parent ends is handed to init, or to the nearest reaper above, and leaves the run's subtree, so that a process outside the subtree could still be one the run started.

## Decision

**The engine adopts every process it starts.** `snapshot::create` makes this process a child subreaper, `PR_SET_CHILD_SUBREAPER` through `rustix`, before the copy exists.
From then on a process whose parent ends is handed to the engine rather than to anybody above it, so every process the run starts stays among the engine's descendants until it ends.
A producer is a process working in the copy, which nothing can be before the copy exists, and the scratch is claimed after the copy, so no producer can be handed elsewhere before the flag is set.
The flag belongs to the process and nothing clears it: the engine asks for it once per snapshot, which repeats a setting the kernel already holds, rather than once per process at the composition root, where a library caller or a test that makes a snapshot in its own process would go without it.
Nothing reads the environment to decide it, since adopting is not a setting ([ADR 0001](0001-seam-policy.md)).
The census takes the proof `escaped::Adoption`, which only `escaped::adopt` makes and the snapshot keeps, so no census runs where adoption was not established; a kernel that refuses the flag fails the snapshot with RM1025 before anything is copied.

**A process whose working directory is hidden is decided by its parents.** `/proc/<pid>/stat` answers for every process, dumpable or not, and names its parent, so `descent` reads parents one at a time from the hidden process up.
One whose parents reach this process descends from it and may be a process the run started: the census refuses the cleanup as before, naming the process, its command and the parents it descends through.
One whose parents reach a process with no parent without passing this one was not started by this run, and is left out of the census.
A process that has ended is no producer.
A parent that is not in the table, or that started after its child, which is a parent's id reused by a younger process, or parents that run past the number of processes listed, are a table that changed under the reading: the census reads them again, up to `DESCENT_READINGS` (8) times, and then refuses, naming the process.
The Kani law `a_descent_is_only_what_the_parents_read_say` proves, for every table of four processes and every process and ancestor among five ids, that a descent is reached only through held parents each no younger than its child, that one is apart only past a parentless process and never past the ancestor, that only an absent process has ended, and that a table whose parents are all held and older is always decided.
A process whose working directory can be read is decided by that directory, as before.

**What the engine adopts, it reaps.** An adopted process that ends is a zombie until the engine reaps it, and no other code waits for it; but a child the engine started and somebody waits for, a `GroupChild` or a `std::process::Child`, is a child too, and reaping it would take its status from its owner, which a `GroupChild` answers by aborting.
The kernel does not say whether a child was adopted, and a ledger of the children this process started would be process-wide state every spawn writes to, which ADR 0001 refuses.
What the kernel does say is a child's process group and session, and every child a process starts leads its own process group or stays in the starter's, and leads its own session or stays in the starter's.
So `could_have_started` keeps every ended child in one of those shapes, and the engine reaps only an ended child in a process group or a session none of them is in; the Kani law `a_child_this_process_started_is_never_reaped_as_handed_over` proves that every shape a started child has is kept and every other is reaped.
The reaper opens a pidfd on the child, checks that the process it holds is still the one it read, and waits on that pidfd with `WEXITED | WNOHANG`, so a reused id is never reaped and a child another thread reaped first is no failure.
`njutest_process` reaps after every group settlement, when the members an ended leader left behind are the engine's, in the leader's group; the census reaps after it has ended what worked in the copy, whose daemons sit in a group or session their own ended parent led.
A process that adopts nothing has nothing to reap, and is not looked at.

## Consequences

- A login, a `sudo -u` or a setgid program of the same user running beside a run no longer fails its cleanup; a hidden process the run started still does, and says which.
- An adopted process that leads its own process group in the engine's session, or its own session, looks exactly like a child the engine started: a process a test started with `process_group(0)`, or one that called `setsid` itself, whose parent ended before it did.
  It stays a zombie until the engine's process ends, which for the command line is the end of the run; every member an execution's group leaves behind, and every daemon that forked twice, is reaped.
- A child the engine starts that then moves itself into the process group of a third process would be reaped from under its owner; nothing the engine starts does that, and a process that joins an execution's group is already that execution's member under the supervision contract.
- A process the run orphans names the engine as its parent rather than init; the orphan attribution of [ADR 0029](0029-a-process-that-loses-the-environment-says-so.md) reads a parent that is still running as somebody else's either way.
- Each settlement on Linux reads the process table once more while the process adopts.
- macOS has no subreaper and keeps its `lsof` census: `lsof` leaves out a process of this user it cannot inspect, a setuid `login`, rather than refusing, so a hidden process there is out of the census whoever started it, as [limitations](../limitations.md) says.
  Windows ends every process of an execution with its Job Object and takes no census.
