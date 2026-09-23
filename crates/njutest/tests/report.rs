// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report model and the invariants a durable report must satisfy.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::report::audit::{Violation, validate_for_persistence};
use njutest::report::{
    BuildReport, Finding, FindingKind, Git, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Outcome, Position, Report, RunKind, SCHEMA, SeamRecord, TargetAccounting,
    TargetRecord, TargetStatus, UNAVAILABLE, Verdict,
};

/// One field of a report, the words its refusal names it by, and how to leave it saying nothing.
type Blank = (&'static str, &'static str, fn(&mut BuildReport));

/// The namespace the one evidence source ran under, distinct from the final answer's.
const SOURCE_RUN: &str = "20260905t081500z-000000";

/// The namespace the completed answer is issued under.
const FINAL_RUN: &str = "20260905t081500z-abcdef";

/// Exactly what the report derives from rows, spelled here because the model's own counter is private to it.
fn counted(rows: &[MutantRecord]) -> MutantAccounting {
    let mut counts = MutantAccounting {
        cataloged: u32::try_from(rows.len()).expect("a fixture's rows are countable"),
        ..MutantAccounting::default()
    };
    for row in rows {
        let outcome = row.outcome.outcome();
        counts
            .observers
            .counted(outcome.decision())
            .expect("a fixture's rows are countable");
        if row.accepted {
            counts.accepted += 1;
        }
        match outcome {
            Outcome::CompileRejected => counts.rejected += 1,
            Outcome::Killed => {
                counts.executed += 1;
                counts.killed += 1;
                if row.reuse.0.read_back().is_some() {
                    counts.reused_killed += 1;
                }
            }
            Outcome::Survived => {
                counts.executed += 1;
                counts.survived += 1;
                if row.reuse.0.read_back().is_some() {
                    counts.reused_survived += 1;
                }
            }
            Outcome::StepLimitReached => {
                counts.executed += 1;
                counts.step_limit_reached += 1;
            }
            Outcome::Waited => {
                counts.executed += 1;
                counts.waited += 1;
            }
            Outcome::Unreached => counts.unreached += 1,
            Outcome::Equivalent => counts.equivalent += 1,
            Outcome::ModelNoticed => {
                counts.executed += 1;
                counts.model_noticed += 1;
            }
            Outcome::ModelProved => {
                counts.executed += 1;
                counts.model_proved += 1;
            }
            Outcome::Unconfirmed | Outcome::Errored => counts.executed += 1,
        }
    }
    counts
}

/// A report that satisfies every invariant, as the mutable draft a run measures into.
fn sound_draft() -> BuildReport {
    let mut source = BuildReport::new(
        SOURCE_RUN,
        RunKind::Full,
        njutest::config::Contract::StandardV1,
    );
    source.repository.git = Git::Said(njutest::report::Said {
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        branch: "main".to_owned(),
        dirty: false,
        against: None,
    });
    source.toolchain.rustc = "rustc 1.98.0".to_owned();
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    source.timing.started = "2026-09-05T08:15:00Z".to_owned();
    source.timing.finished = "2026-09-05T08:15:30Z".to_owned();
    source.timing.duration_ms = 30_000;
    source.targets = vec![
        target("a1", "core/lib/core one", TargetStatus::Passed, 30),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        target("c3", "core/test/it three", TargetStatus::Skipped, 10),
    ];
    source.count_targets().expect("one exact target accounting");
    source.mutants = vec![mutant(&"a".repeat(64))];
    source.accounting.mutants = counted(&source.mutants);
    source.verdict = source.concluded();
    source
}

/// The draft with one thing broken in it, closed through the checked lattice, which is the only way a report exists now.
#[derive(Debug, thiserror::Error)]
enum FixtureError {
    #[error(transparent)]
    Configured(#[from] njutest::report::across::ConfiguredError),
    #[error(transparent)]
    Completion(#[from] njutest::report::CompletionError),
    #[error("the whole-catalog fixture produced a shard")]
    UnexpectedShard,
    #[error("the fixture run identity was refused")]
    RunId(#[from] rust_mutants::id::RunIdError),
}

#[test]
fn a_new_report_names_the_schema_the_run_and_what_it_ran_on() {
    let report = sound();
    assert_eq!(SCHEMA, "njutest-assurance-report-v1");
    let document = serde_json::to_value(&report).expect("a completed report is a document");
    assert_eq!(document["schema"], SCHEMA);
    assert_eq!(report.run_id(), "20260905t081500z-abcdef");
    assert_eq!(report.run_kind(), RunKind::Full);
    assert_eq!(document["tool"]["njutest"], njutest::VERSION);
    assert_eq!(document["tool"]["rust_mutants"], rust_mutants::VERSION);
    let conclusion = report
        .conclusion()
        .expect("the checked report has a representable conclusion");
    assert!(conclusion.limitations.is_empty());
    assert_eq!(conclusion.mutants.len(), 1);
    let fresh = BuildReport::new("r", RunKind::Full, njutest::config::Contract::StandardV1);
    assert_eq!(
        fresh.repository.workspace_digest, UNAVAILABLE,
        "a report says the tree it is about is unknown until somebody reads the tree: \
         every phase that learns it overwrites this, and none of them has to remember to \
         say so when it cannot"
    );
    assert_eq!(fresh.provenance.identity, UNAVAILABLE);
}

/// The draft with one thing broken in it, closed through the checked lattice, which is the only way a report exists now.
fn completed(vary: impl FnOnce(&mut BuildReport)) -> Result<Report, FixtureError> {
    let mut source = sound_draft();
    vary(&mut source);
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .map_err(|refused| {
        FixtureError::Configured(njutest::report::across::ConfiguredError::Measurements(
            refused,
        ))
    })?;
    let run = rust_mutants::id::RunId::try_from(FINAL_RUN)?;
    let latticed = njutest::report::across::configured(&run, &measurements)?;
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        return Err(FixtureError::UnexpectedShard);
    };
    Ok(latticed.complete_without_models()?)
}

/// A report that satisfies every invariant, for a test to read.
fn sound() -> Report {
    completed(|_| {}).expect("a report that satisfies every invariant")
}

/// One judged row at its canonical catalog position, which the dense whole catalog requires.
fn shelved(index: u32, id: &str) -> MutantRecord {
    let mut record = mutant(id);
    record.catalog_index = njutest::report::CatalogIndex::new(index);
    record
}

fn target(id: &str, name: &str, status: TargetStatus, duration_ms: u64) -> TargetRecord {
    TargetRecord {
        id: id.to_owned(),
        name: name.to_owned(),
        package: "core".to_owned(),
        status,
        duration_ms,
        message: None,
    }
}

#[test]
fn a_position_carries_both_columns_because_one_toolchain_uses_both() {
    let line = "    let γ = 1;";
    let at = line.find('γ').expect("the identifier");
    let position = Position::of(line, 12, at).expect("a column a diagnostic can name");
    assert_eq!(position.line, 12);
    assert_eq!(
        position.column, 9,
        "bytes, which is what a coverage region uses"
    );
    assert_eq!(
        position.character_column, 9,
        "characters, which is what a rustc diagnostic uses"
    );

    let wide = "    let 日本語 = 1; let γ = 2;";
    let at = wide.rfind('γ').expect("the second identifier");
    let position = Position::of(wide, 3, at).expect("a column a diagnostic can name");
    assert_ne!(
        position.column, position.character_column,
        "the two differ wherever it matters"
    );
    assert_eq!(position.column, 28, "27 bytes precede it");
    assert_eq!(position.character_column, 22, "21 characters precede it");
}

#[test]
fn git_is_either_available_with_its_facts_or_explicitly_not() {
    let unavailable = Git::Unavailable;
    assert!(unavailable.said().is_none());
    assert_eq!(unavailable.commit(), "unavailable");
    assert_eq!(unavailable.branch(), "unavailable");
    assert!(!unavailable.dirty());
    assert!(
        unavailable.against().is_none(),
        "the word a reader sees for a tree nobody could ask about is made where it is \
         read rather than stored, so nothing can hold a commit and say it was never \
         asked for one"
    );

    let available = Git::Said(njutest::report::Said {
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        branch: "main".to_owned(),
        dirty: true,
        against: Some(njutest::report::Against {
            merge_base: "fedcba98".to_owned(),
            changed_files: vec!["src/lib.rs".to_owned()],
        }),
    });
    assert!(available.said().is_some());
    assert_eq!(
        available.against().map(|taken| taken.merge_base.as_str()),
        Some("fedcba98"),
        "and a base travels with the files taken against it, because either alone is \
         half a fact a reader cannot act on"
    );
}

#[test]
fn a_sound_report_has_nothing_to_report() {
    assert_eq!(validate_for_persistence(&sound()), Vec::<Violation>::new());
}

#[test]
fn a_report_says_who_decided_every_mutation_it_catalogued() {
    let nobody =
        completed(|source| source.accounting.mutants.observers = ObserverAccounting::default())
            .expect_err(
                "a report that catalogued a mutation and names nobody who decided it is a \
             verdict with nothing behind it, and there is no way for a reader to tell \
             that from a run where everything was decided",
            );
    assert!(
        nobody
            .to_string()
            .contains("mutation accounting that disagrees with its rows"),
        "the decisions are re-derived from the rows before anything else can read them: {nobody}"
    );

    let twice = completed(|source| source.accounting.mutants.observers.types = 1).expect_err(
        "and one counted in two columns is one counted twice by whichever reader \
         trusts the wrong column",
    );
    assert!(
        twice
            .to_string()
            .contains("mutation accounting that disagrees with its rows"),
        "{twice}"
    );
}

#[test]
fn the_target_accounting_must_add_up_and_match_the_records() {
    let summed_wrong = completed(|source| source.accounting.targets.passed = 3)
        .expect_err("the terminal states must sum to the number selected");
    assert!(
        summed_wrong
            .to_string()
            .contains("target accounting that disagrees with its rows"),
        "{summed_wrong}"
    );

    let lost = completed(|source| {
        source.targets.pop();
    })
    .expect_err("a row the accounting still counts");
    assert!(
        lost.to_string()
            .contains("target accounting that disagrees with its rows"),
        "{lost}"
    );
}

#[test]
fn a_verdict_must_be_the_one_the_accounting_supports() {
    let failing = completed(|source| {
        source.targets[0].status = TargetStatus::Failed;
        source.accounting.targets.passed = 1;
        source.accounting.targets.failed = 1;
    })
    .expect_err("a failing target and an assurance cannot both be true");
    assert!(
        failing
            .to_string()
            .contains("of its targets failed; a failing test is not an assurance"),
        "the accounting adds up, the records agree, and the verdict is still derived from \
         what ran: {failing}"
    );

    let missing = completed(|source| {
        source.targets[2].status = TargetStatus::Missing;
        source.accounting.targets.skipped = 0;
        source.accounting.targets.missing = 1;
    })
    .expect_err("a target that could not be found is not an assurance");
    assert!(
        missing
            .to_string()
            .contains("of its targets could not be found"),
        "{missing}"
    );
}

#[test]
fn targets_are_ordered_slowest_first_and_then_by_name() {
    let backwards = completed(|source| source.targets.reverse())
        .expect_err("targets that are not in the canonical order");
    assert!(
        backwards.to_string().contains("out of canonical order"),
        "{backwards}"
    );
    let sorted = completed(|source| {
        source.targets.reverse();
        source.sort_targets();
    })
    .expect("and sorting is what the ledger asks for, so the two agree by construction");
    assert_eq!(validate_for_persistence(&sorted), Vec::<Violation>::new());
    let concluded = sorted
        .conclusion()
        .expect("the checked report has a representable conclusion");
    let order: Vec<&str> = concluded
        .targets
        .iter()
        .map(|one| one.id.as_str())
        .collect();
    assert_eq!(order, ["a1", "b2", "c3"]);
}

#[test]
fn a_report_that_says_nothing_ran_cannot_say_it_is_assured() {
    let nothing = completed(|source| {
        source.accounting.targets = TargetAccounting::default();
        source.targets.clear();
    })
    .expect(
        "a run that observed nothing still completes, because a reader has to be able to read why",
    );
    assert_eq!(
        nothing.verdict(),
        Verdict::Insufficient,
        "a report that says nothing ran cannot say it is assured: the verdict is derived \
         from what ran, so an empty run is INSUFFICIENT by construction and no audit has \
         to catch a claim nothing let it make"
    );
    assert_eq!(
        nothing
            .conclusion()
            .expect("the checked report has a representable conclusion")
            .accounting
            .targets
            .selected,
        0
    );
    assert_eq!(validate_for_persistence(&nothing), Vec::<Violation>::new());
}

#[test]
fn a_report_that_put_no_mutation_to_a_test_cannot_say_it_is_assured() {
    let unasked = completed(|source| {
        source.accounting.mutants = MutantAccounting::default();
        source.mutants.clear();
    })
    .expect(
        "a catalog of nothing completes, because what it cannot do is claim anything about a suite",
    );
    assert_eq!(
        unasked.verdict(),
        Verdict::Insufficient,
        "an assurance is the claim that every mutation was noticed; with none put to a \
         test it is a claim about nothing, and the emptiness reads exactly like a suite \
         that noticed everything: the derived verdict is the boundary now"
    );
    assert_eq!(validate_for_persistence(&unasked), Vec::<Violation>::new());
}

#[test]
fn an_unavailable_fact_is_a_sentinel_and_never_an_empty_string() {
    let empty = completed(|source| {
        source.repository.git = Git::Said(njutest::report::Said {
            commit: String::new(),
            branch: "main".to_owned(),
            dirty: false,
            against: None,
        });
    })
    .expect_err("an empty commit is not an unavailable one");
    assert!(
        empty.to_string().contains("repository.git.commit is empty"),
        "an empty value reads as nothing to say, where the {UNAVAILABLE:?} sentinel reads \
         as a fact that could not be established: {empty}"
    );

    let unasked_git = completed(|source| source.repository.git = Git::Unavailable)
        .expect_err("an unavailable git is a stated limitation");
    assert!(
        unasked_git.to_string().contains("git-metadata-unavailable"),
        "{unasked_git}"
    );
    let stated = completed(|source| {
        source.repository.git = Git::Unavailable;
        source.limitations.push(Limitation::new(
            "git-metadata-unavailable",
            "git is not available, so the run cannot name what it verified",
        ));
    })
    .expect("a run that says git was unavailable and states it");
    assert_eq!(validate_for_persistence(&stated), Vec::<Violation>::new());
}

#[test]
fn a_violation_says_what_is_wrong_in_words_a_reader_can_act_on() {
    let said = completed(|source| source.accounting.targets.passed = 99)
        .expect_err("counts that cannot be derived from the rows they claim")
        .to_string();
    assert!(said.len() > 20, "{said}");
    assert!(!said.contains("Violation"), "{said}");
}

#[test]
fn an_assurance_that_names_a_finding_is_two_claims_at_once() {
    let found = completed(|source| {
        source.findings = vec![Finding::new(
            FindingKind::SurvivingMutant,
            "cccccccc",
            "nothing noticed it",
        )];
    })
    .expect("a report that found something still completes, and the finding is the claim");
    assert_eq!(
        found.verdict(),
        Verdict::Insufficient,
        "an assurance is the claim that nothing was found: a report that names a finding \
         cannot make both claims, and the derived verdict is what cannot make them at once"
    );
    assert_eq!(
        found
            .conclusion()
            .expect("the checked report has a representable conclusion")
            .findings
            .len(),
        1
    );
}

#[test]
fn a_defect_a_reader_cannot_see_named_is_not_one_they_can_act_on() {
    let clean = sound();
    assert_eq!(
        clean.verdict(),
        Verdict::Assured,
        "a run whose every row was answered and that found nothing cannot claim DEFECT: \
         the verdict is derived from the findings, so a defect nobody can see named is \
         one the report cannot say"
    );
    let named = completed(|source| {
        source.findings = vec![Finding::new(
            FindingKind::FailingTest,
            "a1",
            "assertion failed",
        )];
    })
    .expect("a defect that names what it found");
    assert_eq!(named.verdict(), Verdict::Defect);
}

#[test]
fn a_defect_that_names_what_it_found_is_sound() {
    let defect = completed(|source| {
        source.findings = vec![Finding::new(
            FindingKind::FailingTest,
            "a1",
            "assertion failed",
        )];
    })
    .expect("a defect that names what it found");
    assert_eq!(defect.verdict(), Verdict::Defect);
    assert_eq!(validate_for_persistence(&defect), Vec::<Violation>::new());
}

fn mutant(id: &str) -> MutantRecord {
    MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: id.to_owned(),
        display_id: id.get(..20).unwrap_or(id).to_owned(),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 1,
            column: 1,
            character_column: 1,
        },
        rule: "gt-to-ge@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: njutest::report::Decided::Killed {
            by: "core/lib/core one".to_owned(),
        },
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    }
}

/// The tampered document a reader is handed: a completed report whose retained source claims one more finding.
fn document_with(vary: impl FnOnce(&mut BuildReport), finding: Finding) -> String {
    let report = completed(vary).expect("a document to tamper with");
    let mut document = serde_json::to_value(njutest::report::ReportDocument::Complete(report))
        .expect("a report is a document");
    document["report"]["builds"][0]["parts"][0]["findings"]
        .as_array_mut()
        .expect("every retained source carries findings")
        .push(serde_json::to_value(finding).expect("a finding is a document"));
    serde_json::to_string(&document).expect("a tampered document is still JSON")
}

/// An acceptance a retained source claims, spelled as the wire spells it, which only a reader re-derives.
fn acceptance(subject: &str) -> Finding {
    Finding {
        kind: FindingKind::UnmatchedAcceptance,
        subject: subject.to_owned(),
        detail: "the acceptance did not resolve".to_owned(),
        origin: njutest::report::FindingOrigin::Source {
            build: njutest::report::BuildName::try_from(
                njutest::config::DEFAULT_CONFIGURATION.to_owned(),
            )
            .expect("the default build name"),
            run_id: rust_mutants::id::RunId::try_from(SOURCE_RUN)
                .expect("a canonical source namespace"),
            part: njutest::report::CatalogPart::Whole,
        },
        path: None,
        position: None,
    }
}

#[test]
fn a_whole_report_cannot_call_a_uniquely_resolved_acceptance_unmatched() {
    let refused = njutest::report::json::parse(&document_with(|_| {}, acceptance("aaaaaaaa")))
        .expect_err("a document a reader cannot check against the catalog it holds");
    assert!(
        refused.to_string().contains("uniquely resolves to"),
        "the full catalog proves the finding false, and the read is where a reader is \
         protected now that a writer cannot make the document: {refused}"
    );
}

#[test]
fn an_invalid_absent_or_ambiguous_acceptance_is_unmatched() {
    for subject in ["A", "cccc", "aaaa"] {
        let read = njutest::report::json::parse(&document_with(
            |source| {
                let rows = vec![
                    shelved(0, &"a".repeat(64)),
                    shelved(1, &format!("aaaa{}", "b".repeat(60))),
                ];
                source.accounting.mutants = counted(&rows);
                source.mutants = rows;
            },
            acceptance(subject),
        ));
        match read {
            Ok(_document) => {}
            Err(why) => {
                panic!("{subject:?} does not resolve to exactly one catalog entry: {why:?}")
            }
        }
    }
}

#[test]
fn a_shard_leaves_acceptance_resolution_for_the_merged_catalog_to_audit() {
    let mut source = sound_draft();
    source.scope.shard = Some("1/2".to_owned());
    source.findings.push(Finding::new(
        FindingKind::UnmatchedAcceptance,
        "aaaaaaaa",
        "the acceptance did not resolve",
    ));
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let run = rust_mutants::id::RunId::try_from(FINAL_RUN).expect("a canonical final run id");
    let latticed = njutest::report::across::configured(&run, &measurements)
        .expect("one shard does not carry the catalog needed to re-resolve the prefix, and does not pretend to");
    assert!(
        matches!(latticed, njutest::report::LatticedDocument::Shard(_)),
        "{latticed:?}"
    );
}

#[test]
fn a_run_that_looked_at_part_of_a_workspace_does_not_assure_all_of_it() {
    let scoped = completed(|source| source.run_kind = RunKind::Scoped)
        .expect("a scoped run completes; what it may claim is scoped");
    assert_eq!(
        scoped.verdict(),
        Verdict::ScopeAssured,
        "a scoped run assures only what it looked at, which is SCOPE_ASSURED and never \
         plain ASSURED: the verdict is derived from the run kind, so the two claims are \
         no longer two fields a writer had to keep together"
    );
    assert_eq!(validate_for_persistence(&scoped), Vec::<Violation>::new());
}

#[test]
fn one_survivor_nobody_accepted_is_one_too_many() {
    let accepted = completed(|source| {
        source.accounting.mutants = MutantAccounting {
            cataloged: 1,
            executed: 1,
            survived: 1,
            accepted: 1,
            observers: ObserverAccounting {
                unnoticed: 1,
                ..ObserverAccounting::default()
            },
            ..MutantAccounting::default()
        };
        source.mutants[0].outcome = njutest::report::Decided::Survived;
        source.mutants[0].accepted = true;
    })
    .expect("a survivor a reviewer accepted with a reason is one the run may still assure around");
    assert_eq!(accepted.verdict(), Verdict::Assured);

    let unaccepted = completed(|source| {
        source.accounting.mutants = MutantAccounting {
            cataloged: 1,
            executed: 1,
            survived: 1,
            observers: ObserverAccounting {
                unnoticed: 1,
                ..ObserverAccounting::default()
            },
            ..MutantAccounting::default()
        };
        source.mutants[0].outcome = njutest::report::Decided::Survived;
        let displayed = source.mutants[0].display_id.clone();
        source.findings = vec![Finding::new(
            FindingKind::SurvivingMutant,
            &displayed,
            "nothing noticed it",
        )];
    })
    .expect("and the one after it completes too, because what it cannot do is claim assurance");
    assert_eq!(
        unaccepted.verdict(),
        Verdict::Insufficient,
        "this is the boundary the whole contract turns on, so it is the row itself that \
         has to be answered and not the count plus room for one: the acceptance belongs \
         to the row, and the verdict follows the row"
    );
    assert_eq!(
        unaccepted
            .conclusion()
            .expect("the checked report has a representable conclusion")
            .findings
            .len(),
        1
    );
}

#[test]
fn non_verdict_rows_can_never_be_hidden_behind_assurance_accounting() {
    let cases = [
        njutest::report::Decided::StepLimitReached {
            on: "core/lib/core one".to_owned(),
            boundary: njutest::report::StepBoundary::new(10, 11)
                .expect("the first count beyond the allowance"),
        },
        njutest::report::Decided::Waited {
            on: "core/lib/core one".to_owned(),
        },
        njutest::report::Decided::Unconfirmed {
            on: "core/lib/core one".to_owned(),
        },
        njutest::report::Decided::Errored {
            on: "core/lib/core one".to_owned(),
        },
    ];
    for outcome in cases {
        let name = outcome.name();
        let refused = completed(move |source| {
            source.mutants[0].outcome = outcome;
            let rows = source.mutants.clone();
            source.accounting.mutants = counted(&rows);
        })
        .expect_err("a non-verdict row cannot be turned into an answer by any accounting");
        assert!(
            refused.to_string().contains("that row is not an answer"),
            "aggregate counters copied from a kill cannot turn a {name} row into an \
             answer: {refused}"
        );
        assert!(
            refused.to_string().contains("requires exactly one"),
            "the row's required actionable finding cannot disappear: {refused}"
        );
    }
}

#[test]
fn an_acceptance_answers_only_the_row_that_carries_it() {
    let unanswered = completed(|source| {
        let mut accepted_equivalent = mutant(&"b".repeat(64));
        accepted_equivalent.catalog_index = njutest::report::CatalogIndex::new(1);
        accepted_equivalent.outcome = njutest::report::Decided::Equivalent;
        accepted_equivalent.accepted = true;
        source.mutants[0].outcome = njutest::report::Decided::Survived;
        let rows = vec![source.mutants[0].clone(), accepted_equivalent];
        source.accounting.mutants = counted(&rows);
        source.mutants = rows;
    })
    .expect_err("an acceptance on another row cannot supply the finding this one requires");
    assert!(
        unanswered
            .to_string()
            .contains("requires exactly one surviving-mutant finding"),
        "the acceptance count is equal to the survivor count, but it belongs to another \
         row, and the finding is per row: {unanswered}"
    );

    let answered = completed(|source| {
        let mut accepted_equivalent = mutant(&"b".repeat(64));
        accepted_equivalent.catalog_index = njutest::report::CatalogIndex::new(1);
        accepted_equivalent.outcome = njutest::report::Decided::Equivalent;
        accepted_equivalent.accepted = true;
        source.mutants[0].outcome = njutest::report::Decided::Survived;
        let rows = vec![source.mutants[0].clone(), accepted_equivalent];
        source.accounting.mutants = counted(&rows);
        source.mutants = rows;
        let displayed = source.mutants[0].display_id.clone();
        source.findings = vec![Finding::new(
            FindingKind::SurvivingMutant,
            &displayed,
            "nothing noticed it",
        )];
    })
    .expect(
        "with its own finding the survivor is honestly unanswered rather than silently excused",
    );
    let counts = answered
        .conclusion()
        .expect("the checked report has a representable conclusion")
        .accounting
        .mutants;
    assert_eq!(
        counts.accepted, 1,
        "an acceptance is a fact about one row, and only the row that carries it is \
         counted as answered"
    );
    assert_eq!(
        answered.verdict(),
        Verdict::Insufficient,
        "and the survivor the acceptance does not answer is what the verdict follows"
    );
}

#[test]
fn a_target_record_that_says_it_failed_is_not_answered_by_an_accounting_that_says_none_did() {
    let mismatched = completed(|source| source.targets[2].status = TargetStatus::Failed)
        .expect_err("a record the accounting contradicts");
    assert!(
        mismatched
            .to_string()
            .contains("target accounting that disagrees with its rows"),
        "{mismatched}"
    );

    let agreed = completed(|source| {
        source.targets[2].status = TargetStatus::Failed;
        source.accounting.targets.skipped = 0;
        source.accounting.targets.failed = 1;
    })
    .expect_err(
        "the accounting adds up and the records are all there, and one of them still \
         says it failed",
    );
    assert!(
        agreed
            .to_string()
            .contains("of its targets failed; a failing test is not an assurance"),
        "a reader who trusts the counts and a reader who reads the rows would come to \
         two different answers about the same run: {agreed}"
    );
}

#[test]
fn a_field_a_report_leaves_empty_is_named_by_the_violation_that_refuses_it() {
    let blank: [Blank; 9] = [
        ("schema", "schema is empty", |source: &mut BuildReport| {
            source.schema.clear();
        }),
        (
            "run_id",
            "canonical writable run id",
            |source: &mut BuildReport| source.run_id.clear(),
        ),
        (
            "repository.root_name",
            "repository.root_name is empty",
            |source: &mut BuildReport| source.repository.root_name.clear(),
        ),
        (
            "repository.workspace_digest",
            "repository.workspace_digest is empty",
            |source: &mut BuildReport| source.repository.workspace_digest.clear(),
        ),
        (
            "repository.configuration_digest",
            "repository.configuration_digest is empty",
            |source: &mut BuildReport| {
                source.repository.configuration_digest.clear();
            },
        ),
        (
            "repository.git.commit",
            "repository.git.commit is empty",
            |source: &mut BuildReport| {
                if let Git::Said(said) = &mut source.repository.git {
                    said.commit.clear();
                }
            },
        ),
        (
            "repository.git.branch",
            "repository.git.branch is empty",
            |source: &mut BuildReport| {
                if let Git::Said(said) = &mut source.repository.git {
                    said.branch.clear();
                }
            },
        ),
        (
            "toolchain.rustc",
            "toolchain.rustc is empty",
            |source: &mut BuildReport| source.toolchain.rustc.clear(),
        ),
        (
            "provenance.identity",
            "provenance.identity is empty",
            |source: &mut BuildReport| source.provenance.identity.clear(),
        ),
    ];

    for (field, words, empty) in blank {
        let refused = completed(empty).expect_err(&format!(
            "a report that says nothing where {field} goes is one a reader cannot check"
        ));
        assert!(
            refused.to_string().contains(words),
            "and a refusal that does not name the field leaves them to find it: {refused}"
        );
    }
}

#[test]
fn a_run_that_could_not_ask_git_says_so_and_claims_none_of_its_facts() {
    let unasked = completed(|source| source.repository.git = Git::Unavailable)
        .expect_err("a run that could not name the commit it verified has to state that");
    assert!(
        unasked.to_string().contains("git-metadata-unavailable"),
        "or a reader comparing two reports has no way to know which tree either was \
         about: {unasked}"
    );

    for (what, available, commit, branch, dirty, merge_base, changed) in [
        (
            "a commit",
            "false",
            r#""0123456789abcdef0123456789abcdef01234567""#,
            r#""unavailable""#,
            "false",
            "null",
            "[]",
        ),
        (
            "a branch",
            "false",
            r#""unavailable""#,
            r#""main""#,
            "false",
            "null",
            "[]",
        ),
        (
            "uncommitted changes",
            "false",
            r#""unavailable""#,
            r#""unavailable""#,
            "true",
            "null",
            "[]",
        ),
        (
            "a base it was taken against",
            "false",
            r#""unavailable""#,
            r#""unavailable""#,
            "false",
            r#""fedcba98""#,
            "[]",
        ),
        (
            "a list of files that differ from a base it does not name",
            "true",
            r#""0123456789abcdef0123456789abcdef01234567""#,
            r#""main""#,
            "false",
            "null",
            r#"["src/lib.rs"]"#,
        ),
    ] {
        let written = format!(
            r#"{{"available":{available},"commit":{commit},"branch":{branch},"dirty":{dirty},"merge_base":{merge_base},"changed_files":{changed}}}"#
        );
        let read: Result<Git, serde_json::Error> = njutest_devkit::strictjson::decode_str(&written);
        assert!(
            read.is_err(),
            "a tree nobody could ask git about that nonetheless has {what} is a document \
             where one of the two halves is untrue and a reader cannot tell which. This \
             used to be five refusals a run made when it wrote a report, which left a \
             reader of an already-written one to notice for themselves"
        );
    }
}

#[test]
fn targets_that_took_the_same_time_are_ordered_by_name_and_the_place_is_named() {
    let tied_wrongly = completed(|source| {
        source.targets = vec![
            target("a1", "core/lib/core one", TargetStatus::Passed, 30),
            target("c3", "core/test/it three", TargetStatus::Passed, 20),
            target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        ];
        source.count_targets().expect("one exact target accounting");
    })
    .expect_err("two targets that took the same time are ordered by identity");
    assert!(
        tied_wrongly.to_string().contains("out of canonical order"),
        "and the pair that breaks the order is the second one here, which the source \
         namespace points a reader at: {tied_wrongly}"
    );
    let sorted = completed(|source| {
        source.targets = vec![
            target("a1", "core/lib/core one", TargetStatus::Passed, 30),
            target("c3", "core/test/it three", TargetStatus::Passed, 20),
            target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        ];
        source.count_targets().expect("one exact target accounting");
        source.sort_targets();
    })
    .expect("and sorting is what the ledger asks for, so the two agree by construction");
    assert_eq!(validate_for_persistence(&sorted), Vec::<Violation>::new());

    let backwards = completed(|source| {
        source.targets = vec![
            target("a1", "core/lib/core one", TargetStatus::Passed, 10),
            target("b2", "core/lib/core two", TargetStatus::Passed, 20),
            target("c3", "core/test/it three", TargetStatus::Passed, 30),
        ];
        source.count_targets().expect("one exact target accounting");
    })
    .expect_err("every list in the wrong order is refused");
    assert_eq!(
        backwards
            .to_string()
            .matches("out of canonical order")
            .count(),
        1,
        "every pair of a list in the wrong order is in the wrong order, and saying so \
         once for each would bury the one thing a reader has to do under a count of how \
         long the list is"
    );
}

#[test]
fn a_document_that_pairs_the_flag_with_a_name_it_cannot_go_with_is_not_read() {
    let sound = serde_json::to_string(&sound()).expect("a report is a document");
    for (what, cached, source) in [
        (
            "a run that established its own facts and also names another",
            "false",
            r#""20260905t081500z-000000""#,
        ),
        (
            "a report read back from an earlier run that names no run",
            "true",
            "null",
        ),
        ("a source run with no name", "true", r#""""#),
    ] {
        let written = sound.replace(
            r#""cached":false,"source_run_id":null"#,
            &format!(r#""cached":{cached},"source_run_id":{source}"#),
        );
        assert_ne!(written, sound, "the document under test was composed");
        let read: Result<Report, serde_json::Error> =
            njutest_devkit::strictjson::decode_str(&written);
        assert!(
            read.is_err(),
            "{what} is a document a reader cannot check against the run it points at, \
             which is the whole of what provenance is for. It used to be refused when a \
             report was written, which left every reader of an already-written one to \
             notice for themselves"
        );
    }
}

#[test]
fn a_run_that_read_its_own_answer_back_is_refused_when_it_is_written() {
    let itself = completed(|source| {
        source.provenance.facts = njutest::report::Established::ReadBackFrom(SOURCE_RUN.to_owned());
    })
    .expect_err("a namespace cannot have read its own answer back");
    assert!(
        itself.to_string().contains("read its own answer back"),
        "this is the one a type cannot refuse, because telling it apart needs the run's \
         own identity and the value holds only the source's: {itself}"
    );
}

/// Every completion this lattice refuses, one for each way it refuses one.
fn refused() -> Vec<String> {
    vec![
        completed(|source| source.accounting.targets.passed = 99)
            .expect_err("counts that do not add up")
            .to_string(),
        completed(|source| source.schema.clear())
            .expect_err("a schema that says nothing")
            .to_string(),
        completed(|source| {
            source.targets[0].status = TargetStatus::Failed;
            source.accounting.targets.passed = 1;
            source.accounting.targets.failed = 1;
        })
        .expect_err("a failing target")
        .to_string(),
        completed(|source| {
            source.accounting.mutants = MutantAccounting {
                cataloged: 1,
                executed: 1,
                survived: 1,
                observers: ObserverAccounting {
                    unnoticed: 1,
                    ..ObserverAccounting::default()
                },
                ..MutantAccounting::default()
            };
            source.mutants[0].outcome = njutest::report::Decided::Survived;
        })
        .expect_err("a survivor without its finding")
        .to_string(),
        completed(|source| source.timing.finished = "2026-09-05T08:14:59Z".to_owned())
            .expect_err("a source that finished before it started")
            .to_string(),
        completed(|source| {
            source.provenance.facts =
                njutest::report::Established::ReadBackFrom(SOURCE_RUN.to_owned());
        })
        .expect_err("a source that read its own answer back")
        .to_string(),
        completed(|source| {
            source.provenance.facts = njutest::report::Established::ReadBackFrom(String::new());
        })
        .expect_err("a source with no name")
        .to_string(),
        completed(|source| source.repository.git = Git::Unavailable)
            .expect_err("git unasked and unstated")
            .to_string(),
        completed(|source| source.targets.reverse())
            .expect_err("targets backwards")
            .to_string(),
        completed(|source| {
            let rows = vec![mutant(&"a".repeat(64)), shelved(1, &"a".repeat(64))];
            source.accounting.mutants = counted(&rows);
            source.mutants = rows;
        })
        .expect_err("one identity counted twice")
        .to_string(),
    ]
}

#[test]
fn a_report_that_says_it_was_read_back_from_a_run_that_is_not_itself_is_coherent() {
    let cached = completed(|source| {
        source.provenance.facts =
            njutest::report::Established::ReadBackFrom("20260905t081500z-999999".to_owned());
    })
    .expect(
        "reusing what an earlier run of the same inputs established is the whole of what \
         evidence is for, and an audit that refused it would make every second run write \
         a report nobody may keep",
    );
    assert_eq!(validate_for_persistence(&cached), Vec::<Violation>::new());
}

#[test]
fn two_records_that_share_an_identity_are_not_blamed_for_being_out_of_order() {
    let twice = completed(|source| {
        source.targets = vec![
            target("a1", "core/lib/core one", TargetStatus::Passed, 30),
            target("b2", "core/lib/core two", TargetStatus::Passed, 20),
            target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        ];
        source.count_targets().expect("one exact target accounting");
    })
    .expect_err("the same identity twice");
    assert!(
        twice.to_string().contains("duplicate"),
        "the same identity twice is a report with a problem, and the problem is not the \
         order: the refusal names duplication, so it does not send a reader to sort a \
         list that is already sorted: {twice}"
    );
}

#[test]
fn every_refusal_is_a_finished_sentence() {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for refused in refused() {
        assert!(!refused.is_empty(), "a report this lattice refuses");
        assert!(
            !refused.contains("  "),
            "two spaces where a word was: a sentence with a hole in it is one \
             somebody wrote and nobody read: {refused:?}"
        );
        assert!(
            !refused.trim_end().ends_with([';', ':', ',']),
            "a sentence that stops at its own semicolon promised a reason and gave \
             none: {refused:?}"
        );
        assert!(
            !refused.ends_with(char::is_whitespace),
            "and one that stops at the space before the reason is the same hole with \
             nothing to see: whatever the words around it were, they were written to \
             be followed by something: {refused:?}"
        );
        assert!(
            refused.len() > 20 && !refused.contains("Violation"),
            "what is wrong, in words a reader can act on, and not the name of a \
             variant: {refused:?}"
        );
        seen.insert(refused);
    }
    assert!(
        seen.len() >= 10,
        "one report for each way this refuses one, and every way says something \
         different: {distinct} of them distinct. This number went down \
         when `report::Established` and `report::Git` made seven of the refusals \
         unwritable, and down again when the derived verdict and the checked build \
         ledger took over more, and down is the direction to want it: a refusal a \
         type has taken over is one no reader has to be told about. What it must not \
         do is go down because a refusal stopped being made",
        distinct = seen.len()
    );
}

#[test]
fn a_seam_finding_that_names_a_question_the_report_does_not_hold_is_refused() {
    let refused = completed(|source| {
        source.findings.push(Finding::new(
            FindingKind::WireUnnoticed,
            &"c".repeat(64),
            "nothing noticed when the run was told to answer 500",
        ));
    })
    .expect_err("a seam finding that names a question the report does not hold");
    assert!(
        refused.to_string().contains("holds no such question"),
        "a reader handed a sixty-four character name with nothing in the report to \
         look it up in has been told nothing they can act on, and ADR 0002 keeps the \
         thing it stands for off the recording: {refused}"
    );

    let allowed = completed(|source| {
        source.findings.push(Finding::new(
            FindingKind::WireUnnoticed,
            &"c".repeat(64),
            "nothing noticed when the run was told to answer 500",
        ));
        source.seams.push(SeamRecord {
            id: "c".repeat(64),
            capability: "api".to_owned(),
            seq: 3,
            asked: "GET /orders".to_owned(),
            answered: Some(200),
            rule: njutest::wire::rule::Rule::StatusServerError,
            decision: njutest::report::SeamDecision::Unnoticed,
        });
    })
    .expect("and once the report holds the question, the finding resolves");
    assert_eq!(
        allowed
            .conclusion()
            .expect("the checked report has a representable conclusion")
            .seams
            .len(),
        1
    );
    assert_eq!(validate_for_persistence(&allowed), Vec::<Violation>::new());
}

/// The draft's one row decided as `outcome` by this run and routed by `routing`, with the counts and finding the row requires.
fn answering(
    outcome: njutest::report::Decided,
    routing: Option<njutest::report::Routing>,
) -> impl FnOnce(&mut BuildReport) {
    move |source| {
        source.mutants[0].outcome = outcome;
        source.mutants[0].routing = routing;
        source.accounting.mutants = counted(&source.mutants);
        source.findings = source
            .mutants
            .iter()
            .filter_map(|row| {
                row.outcome
                    .outcome()
                    .required_finding(row.accepted)
                    .map(|kind| Finding::new(kind, &row.display_id, "the finding the row requires"))
            })
            .collect();
        source.verdict = source.concluded();
    }
}

/// A route this run asked by, keeping `reaching` and recording `answered` in the order it asked.
fn asked_by(reaching: &[&str], answered: &[(&str, Outcome)]) -> njutest::report::Routing {
    njutest::report::Routing {
        granularity: rust_mutants::session::Granularity::Block,
        reaching: reaching.iter().map(|one| (*one).to_owned()).collect(),
        discharged: Vec::new(),
        fallback: None,
        answered: answered
            .iter()
            .map(|(target, outcome)| njutest::report::Answered {
                target: (*target).to_owned(),
                outcome: *outcome,
            })
            .collect(),
    }
}

const ONE: &str = "core/lib/core one";
const TWO: &str = "core/lib/core two";

#[test]
fn a_kill_this_run_established_is_the_last_answer_its_own_route_recorded() {
    let killed = || njutest::report::Decided::Killed { by: ONE.to_owned() };
    for (earlier, why) in [
        (Outcome::Survived, "a target that ran it and did not notice"),
        (
            Outcome::Unconfirmed,
            "a kill that did not reproduce, which the run goes on past",
        ),
        (Outcome::Waited, "a target this machine stopped waiting for"),
        (
            Outcome::StepLimitReached,
            "a target that crossed its step allowance",
        ),
        (Outcome::Errored, "a target nothing could be measured on"),
    ] {
        completed(answering(
            killed(),
            Some(asked_by(
                &[TWO, ONE],
                &[(TWO, earlier), (ONE, Outcome::Killed)],
            )),
        ))
        .unwrap_or_else(|refused| {
            panic!("the run goes on past {why}, so a kill after it stands: {refused}")
        });
    }
    completed(answering(
        killed(),
        Some(asked_by(
            &[TWO, ONE],
            &[(TWO, Outcome::Survived), (ONE, Outcome::Killed)],
        )),
    ))
    .expect(
        "a kill after a target that ran it and did not notice is the run stopping where it must",
    );
    for (answered, why) in [
        (
            vec![(ONE, Outcome::Survived)],
            "the target named as the killer answered that it survived",
        ),
        (
            vec![(ONE, Outcome::Killed), (TWO, Outcome::Survived)],
            "the run asked another target after the one that noticed",
        ),
        (
            vec![(TWO, Outcome::Killed), (ONE, Outcome::Killed)],
            "an earlier target already noticed, so the run would have stopped there",
        ),
        (Vec::new(), "the route recorded no answer at all"),
        (
            vec![(TWO, Outcome::Unreached), (ONE, Outcome::Killed)],
            "a target does not answer what only a whole mutation can be",
        ),
    ] {
        let refused = completed(answering(killed(), Some(asked_by(&[ONE, TWO], &answered))))
            .expect_err(why)
            .to_string();
        assert!(
            refused.contains("was killed by core/lib/core one in this run")
                && refused.contains("stops at the first target that notices"),
            "{why}, and the report is refused for saying two things about one mutation: {refused}"
        );
    }
    let mut read_back = sound_draft();
    answering(killed(), Some(asked_by(&[ONE], &[])))(&mut read_back);
    read_back.mutants[0].reuse = njutest::report::Reuse(
        njutest::report::Established::ReadBackFrom("20260904t000000z-000000".to_owned()),
    );
    read_back.accounting.mutants = counted(&read_back.mutants);
    completed(|source| *source = read_back).expect(
        "a kill read back from an earlier run was asked there, so this run's route records no answer for it",
    );
}

#[test]
fn a_survivor_this_run_established_was_asked_of_every_target_its_route_kept_and_each_survived() {
    let mut fallback = asked_by(
        &[ONE, TWO],
        &[(ONE, Outcome::Survived), (TWO, Outcome::Survived)],
    );
    fallback.granularity = rust_mutants::session::Granularity::All;
    fallback.fallback = Some(rust_mutants::session::Fallback::NotMeasured);
    completed(answering(
        njutest::report::Decided::Survived,
        Some(fallback.clone()),
    ))
    .expect("a route widened to every target asks every target, and each survived");
    fallback.answered.pop();
    completed(answering(
        njutest::report::Decided::Survived,
        Some(fallback),
    ))
    .expect_err("a widened route that left a target unasked is not a survivor's");
    completed(answering(
        njutest::report::Decided::Survived,
        Some(asked_by(
            &[ONE, TWO],
            &[(TWO, Outcome::Survived), (ONE, Outcome::Survived)],
        )),
    ))
    .expect("the answers need not come in the route's order, only one from each target it kept");
    for (answered, why) in [
        (
            vec![(ONE, Outcome::Survived)],
            "a target the route kept was never asked",
        ),
        (
            vec![(ONE, Outcome::Survived), (TWO, Outcome::Killed)],
            "a target that noticed is not one that survived",
        ),
        (
            vec![
                (ONE, Outcome::Survived),
                (TWO, Outcome::Survived),
                (TWO, Outcome::Survived),
            ],
            "a target asked twice is not a route asked once",
        ),
    ] {
        let refused = completed(answering(
            njutest::report::Decided::Survived,
            Some(asked_by(&[ONE, TWO], &answered)),
        ))
        .expect_err(why)
        .to_string();
        assert!(
            refused.contains("survived in this run")
                && refused.contains("every target that could notice ran it and did not"),
            "{why}, and the report is refused for saying two things about one mutation: {refused}"
        );
    }
}
