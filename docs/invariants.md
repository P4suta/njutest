<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Invariants of the critical decisions

**Status: implemented.** `cargo xtask invariants` holds this page to the tree, and the ledger of holes is the work still owed.

A critical decision is one whose mistake changes a verdict, a score, or what a reader is told was measured.
Each has one row here, and the row says what holds it at every layer: a cell names the items of the tree that do, or says `none`.
A `none` is a hole somebody owns: `xtask/invariant_gaps.txt` lists it as `<decision> <layer> <owner>`, the two agree both ways, and the ledger may shrink and never grow, which `xtask/invariant_gap_ceiling.txt` holds.
`cargo xtask invariants`, part of `cargo xtask all`, refuses a cell naming an item the tree does not define, a `none` the ledger gives nobody, and a hole the table does not have.

The layers, and what each one is for:

- **Types**: the decision is made in one place, by a type or a single classifier, and nothing else makes it.
- **Self-check**: the running engine verifies its own output and fails closed, naming what failed.
- **Oracle**: a test that shares no code with the decision derives the answer on its own.
- **Plant**: a defect is planted where the decision is made, and the checks above are shown to catch it.
- **Mutation**: rust-mutants runs over the module that decides, with no survivor left unaccounted for.
- **States**: every state the decision can meet — a platform, an order of events, a process shape — is a row generated from a closed set, so a flake is a missing row rather than a retry.
- **Blind**: what the oracle cannot see, said once, so nobody reads its silence as coverage.

Why this page exists: a defect in a critical decision reached a run through checks that proved a weaker property than the invariant, an oracle that shared its input with what it checked, a fix made to one instance rather than the layer, a state nobody enumerated, and a contract between two mechanisms that nobody wrote down.
A row is where each of those is either answered or owned.

| Decision | Invariant | Types | Self-check | Oracle | Plant | Mutation | States | Blind |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| answered-stop | a stop at the first failing test reads the same whichever ending arrives first | `Termination` | none | `a_should_panic_test_that_stops_panicking_is_a_kill` | none | none | `a_process_that_named_its_failure_is_answered_whichever_ending_arrives_first` | the order of the two endings is forced only in the unit law |
| group-stop | stopping a process group ends every member, whatever state its leader is in | none | none | none | none | none | `a_gentle_stop_of_a_group_whose_leader_has_already_exited_is_no_failure` | leader states other than an exited, unreaped one |
| stall-watch | a counted process that raises a boundary at least once a quiet window is never stalled | none | none | `a_test_slower_than_the_bound_is_waited_for_while_it_keeps_moving` | none | none | none | a process that blocks inside one boundary for longer than a window |
| step-transition | the allowance is spent exactly, across every crate and process of an execution | `step_transition` | `read_step_state` | `a_mutation_outside_a_loop_in_a_file_nothing_mutates_is_counted_at_the_boundary` | none | none | `kani_laws` | a process that dies between reserving and spending |
| step-stop-said | every stop the generated runtime makes says which check failed before the process ends | none | none | none | none | none | none | a process killed from outside says nothing |
| operator-swap | a swap writes the file's own tree with exactly one operator replaced | none | none | none | none | none | none | unary operators and method calls |
| instrument-shape | instrumenting keeps the file's tree, every guard standing where its site stood | none | none | none | none | none | none | what a user's own macros hold |
| execution-confinement | a test process writes its home only inside its own execution, or the run names the target whose process does not | `Scratch` | `confinement_held` | `a_write_a_test_makes_under_its_home_lands_in_its_execution` | `a_home_that_escaped_its_execution_is_refused_before_the_process_starts` | none | `a_confined_home_replaces_a_home_the_base_spells_another_way` | `CARGO_HOME`, an `unconfined-target`, a variable the engine does not confine, and a Windows known folder |
