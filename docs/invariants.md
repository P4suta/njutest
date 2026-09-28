<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Invariants of the critical decisions

**Status: the registry is implemented, but the campaign remains incomplete while the gap ledger is non-empty.**

`cargo xtask invariants` holds this page to the tree, and the ledger of holes is the work still owed.

A critical decision is one whose mistake changes a verdict, a score, or what a reader is told was measured.
Each has one row here, and the row says what holds it at every layer: a cell names the items of the tree that do, or says `none`.
A `none` is a hole somebody owns: `xtask/invariant_gaps.txt` lists it as `<decision> <layer> <owner>`, and the two agree both ways.
`cargo xtask invariants`, part of `cargo xtask all`, refuses a cell naming an item the tree does not define, a `none` the ledger gives nobody, a hole the table does not have, and an oracle named with nothing said in Blind about what it cannot see.
`cargo xtask ratchets`, also part of `all`, reads this page where the change meets `origin/main` and refuses a layer held there and `none` here, and a decision there that is gone here.
A decision new to that base enters with whatever it owes, owned in the ledger; a renamed one writes its cell as `new-name (was old-name)`, which carries the base row over to it, and the marker can go once the rename is on `main`.
A bound kept in the same tree as what it bounds is one the same change can raise, so the comparison is with the base, and every line the gate writes names the commit it compared with.

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
| answered-stop | a stop at the first failing test reads the same whichever ending arrives first | `Termination` | `checked_answered_termination` | `a_should_panic_test_that_stops_panicking_is_a_kill` | `a_planted_exit_after_a_named_failure_is_refused`, `a_planted_answer_without_failure_evidence_is_refused` | none | `a_process_that_named_its_failure_is_answered_whichever_ending_arrives_first` | the order of the two endings is forced only in the unit law |
| group-stop | the stop classifier distinguishes whole-group reach from leader-only fallback; a refusal with surviving or unseen members is never classified as whole-group reach | `Stopped`, `StopDecision` | `checked_decide_stop` | `every_answer_a_group_stop_can_meet_comes_to_what_the_table_says` | `a_planted_whole_group_claim_for_a_refused_group_is_rejected`, `every_planted_wrong_group_stop_decision_is_rejected` | none | `a_gentle_stop_of_a_group_whose_leader_has_already_exited_is_no_failure` | the supervisor accepts leader-only reach for its owned child, and a protected descendant can outlive it; leader states other than an exited, unreaped one |
| stall-watch | a counted process that raises a boundary at least once a quiet window is never stalled | `Progress` | `configured_beat`, `confirms_stall` | `a_test_slower_than_the_bound_is_waited_for_while_it_keeps_moving` | `a_planted_stall_cannot_stop_a_child_that_keeps_beating` | none | `a_child_is_told_to_beat_well_inside_every_window` | a process that blocks inside one boundary for longer than a window |
| step-transition | the allowance is spent exactly, across every crate and process of an execution | `step_transition` | `read_step_state` | `a_mutation_outside_a_loop_in_a_file_nothing_mutates_is_counted_at_the_boundary` | `a_planted_activation_that_spends_two_steps_is_rejected` | none | `kani_laws` | a process that dies between reserving and spending |
| step-stop-said | every stop the generated runtime makes says which check failed before the process ends | `stop` | none | `a_protocol_stop_is_named_by_the_last_line_the_runtime_wrote_for_it` | `a_planted_missing_or_misrecorded_runtime_stop_is_refused` | none | `the_runtime_ends_a_process_in_one_place_and_says_why_there_first` | a process killed from outside says nothing |
| operator-swap | a swap writes the file's own tree with exactly one operator replaced | `Grouping` | `keeps` | `every_operator_swap_keeps_the_operands_and_their_grouping` | `a_planted_wrong_grouping_is_caught_by_the_tree_oracle` | none | `Binding` | unary operators and method calls |
| instrument-shape | instrumenting keeps the file's tree, every guard standing where its site stood | `opens_a_block` | `read_through` | `instrumenting_keeps_the_tree_of_every_file` | `a_guard_that_breaks_inside_an_identity_macro_is_seen_where_a_plain_parse_is_blind` | none | `every_guard_form_is_observed_with_the_independent_tree_oracle` | what a user's own macros hold |
| execution-confinement | a test process writes its home only inside its own execution, or the run names the target whose process does not | `Scratch` | `confinement_held` | `a_write_a_test_makes_under_its_home_lands_in_its_execution` | `a_home_that_escaped_its_execution_is_refused_before_the_process_starts` | none | `a_confined_home_replaces_a_home_the_base_spells_another_way` | `CARGO_HOME`, an `unconfined-target`, a variable the engine does not confine, and a Windows known folder |
| repository-claims | every repository claim names what it says on every push | `Resolution` | `claims` | `repository_claims_are_rederived_from_source_and_candidate_json` | `ROTTEN`, `MOVED` | none | `every_claim_resolution_is_observed_across_real_build_inputs` | candidate discovery's rule and item attribution, claims in skipped targets, and whether a claim's reason is true or adequate |
| test-decline | a test's decline sets its execution aside only where the process ran whole and the baseline declined in the same words, and any other decline under a mutation is a kill | `Declines`, `Held`, `held` | `checked_decision`, `declines_agree`, `storable` | `baseline_declines`, `declined`, `a_decline_the_row_misstates_is_refused_from_the_recording_alone` | `a_planted_wrong_execution_decline_decision_is_refused` | none | `every_reading_decides_one_well_formed_notice_by_its_one_rule`, `every_refusal_of_a_notice_is_one_a_notice_can_reach` | the recording itself, which the audit re-derives from, so a notice the engine never recorded or recorded wrongly is invisible to it; a test that skips without writing the notice, which is a pass the engine cannot tell from a measurement; and a test whose words change from run to run, which reads as a kill |
| sealed-verdict | a verdict comes only from sealed executions: one sealed detection kills, a survival needs every reaching test's sealed pass, and anything native, doubted or outside the sealed build leaves the mutant unproven with every reason | `standing`, `Verdict` | none | `every_standing_up_to_three_executions_is_the_one_the_rules_give` | `a_standing_that_reads_a_native_pass_as_sealed_is_caught`, `a_standing_that_ignores_a_doubt_is_caught` | none | `native_executions_alone_never_establish_a_verdict`, `the_order_of_the_executions_does_not_change_the_class` | which executions the engine hands it and whether they came from a sealed run, which the decision takes as given; until the sealed runner exists every execution is native |
