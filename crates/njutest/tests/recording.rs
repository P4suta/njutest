// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run writes into its report once a phase has answered.

use std::collections::{BTreeMap, BTreeSet};

use njutest::assure::baseline::Baseline;
use njutest::assure::mutation::{Disposition, Judged, Mutation};
use njutest::assure::route::{BRANCH_NEVER_TAKEN, Discharge, Route};
use njutest::assure::run::{about, absorb, record};
use njutest::config::Contract;
use njutest::report::{BuildReport, RunKind};

fn judged(display_id: &str, disposition: Disposition, reused: bool) -> Judged {
    Judged {
        catalog_index: 0,
        id: format!("{display_id}{}", "0".repeat(60)),
        display_id: format!("{display_id}{}", "0".repeat(16)),
        path: "src/lib.rs".to_owned(),
        rule: "add-to-sub@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: String::new(),
        position: None,
        disposition,
        source_run_id: reused.then(|| "20260905T081500Z-000000".to_owned()),
        observed: Vec::new(),
        routing: None,
    }
}

fn blank() -> BuildReport {
    BuildReport::new(
        "20260909t000000z-000001",
        RunKind::Full,
        Contract::StandardV1,
    )
}

#[test]
fn one_limitation_about_five_targets_is_one_row_and_not_five() {
    let folded = about(&[
        "custom-harness:pkg/test/one".to_owned(),
        "doctests-none".to_owned(),
        "custom-harness:pkg/test/two".to_owned(),
    ]);

    assert_eq!(
        folded,
        vec![
            (
                "custom-harness".to_owned(),
                vec!["pkg/test/one".to_owned(), "pkg/test/two".to_owned()]
            ),
            ("doctests-none".to_owned(), Vec::new()),
        ],
        "the name is what the ledger of limitations is keyed by, so it is what reaches \
         the report once, and which targets it was about goes into the sentence: five \
         rows saying one thing is a reader counting rows instead of reading them"
    );
}

#[test]
fn what_the_baseline_could_not_do_reaches_the_report_with_the_targets_it_was_about() {
    let mut report = blank();
    let baseline = Baseline {
        targets: Vec::new(),
        failure: None,
        limitations: vec![
            "custom-harness:pkg/test/one".to_owned(),
            "custom-harness:pkg/test/two".to_owned(),
        ],
    };

    absorb(&mut report, &baseline).expect("the baseline ledger fits the report counters");

    assert_eq!(report.limitations.len(), 1, "{:?}", report.limitations);
    let stated = report.limitations.first().expect("one limitation");
    assert_eq!(stated.name, "custom-harness");
    assert!(
        stated.detail.contains("pkg/test/one") && stated.detail.contains("pkg/test/two"),
        "which targets it was stated about is what a reader acts on: {stated:?}"
    );
}

#[test]
fn a_workspace_that_did_not_build_is_a_finding_and_the_compiler_speaks_first() {
    let mut report = blank();
    "demo".clone_into(&mut report.repository.root_name);
    let baseline = Baseline {
        targets: Vec::new(),
        failure: Some("error[E0432]: unresolved import\n  --> src/lib.rs:1:5\n".to_owned()),
        limitations: Vec::new(),
    };

    absorb(&mut report, &baseline).expect("the baseline ledger fits the report counters");

    let raised = report.findings.first().expect("one finding");
    assert_eq!(raised.subject, "demo");
    assert_eq!(
        raised.detail, "error[E0432]: unresolved import",
        "a workspace that does not compile is the run's answer about the workspace, and \
         what a person needs is the compiler's first line rather than the whole log"
    );
}

#[test]
fn every_mutation_judged_is_a_row_that_says_what_became_of_it() {
    let mut report = blank();
    let mut placed = judged(
        "aaaa",
        Disposition::Killed {
            by: "one".to_owned(),
        },
        true,
    );
    placed.position = Some(njutest::report::Position {
        line: 12,
        column: 5,
        character_column: 5,
    });
    let mutation = Mutation {
        judged: vec![
            placed,
            judged(
                "bbbb",
                Disposition::Survived {
                    route: Route::Discharged {
                        discharged: vec![Discharge {
                            target: "pkg/lib/pkg".to_owned(),
                            proof: BRANCH_NEVER_TAKEN,
                        }],
                    },
                },
                false,
            ),
        ],
        skips: BTreeMap::from([("macro-invocation".to_owned(), 7u64)]),
        drift: Vec::new(),
    };

    record(&mut report, &mutation, &BTreeSet::new())
        .expect("the mutation ledger fits the report counters");

    let rows = &report.mutants;
    assert_eq!(rows.len(), 2);
    let killed = rows.first().expect("the kill");
    assert_eq!(killed.display_id, "aaaa0000000000000000");
    assert_eq!(killed.outcome.name(), "killed");
    assert_eq!(
        killed.outcome.decided_by(),
        Some("one"),
        "a kill names the target that noticed, which is the one thing a reader cannot \
         work out afterwards"
    );
    assert!(
        killed.reuse.0.read_back().is_some(),
        "and says whether this run established it or read it back"
    );
    assert_eq!(killed.position.line, 12);

    let survived = rows.get(1).expect("the survivor");
    assert_eq!(survived.outcome.name(), "survived");
    assert_eq!(survived.outcome.decided_by(), None);
    assert!(survived.reuse.0.read_back().is_none());
    assert_eq!(
        survived.position.line, 1,
        "a mutation whose position nothing recorded is placed at the first line rather \
         than at none: a row an editor cannot open is one nobody opens"
    );

    assert_eq!(report.accounting.mutants.cataloged, 2);
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    let skipped = report
        .limitations
        .iter()
        .find(|one| one.name == "skipped-macro-invocation")
        .expect("what was not mutated");
    assert_eq!(
        skipped.detail, "7 places were not mutated: macro-invocation",
        "how many places a run passed over is the difference between a catalog that is \
         small and one that is narrow, and which reason they were passed over for is \
         what somebody does about it: {skipped:?}"
    );
}

#[test]
fn a_ledger_entry_cannot_mark_an_outcome_that_is_not_answerable_as_accepted() {
    let killed = judged(
        "aaaa",
        Disposition::Killed {
            by: "one".to_owned(),
        },
        false,
    );
    let mut survivor = judged(
        "bbbb",
        Disposition::Survived {
            route: Route::Discharged {
                discharged: vec![Discharge {
                    target: "pkg/lib/pkg".to_owned(),
                    proof: BRANCH_NEVER_TAKEN,
                }],
            },
        },
        false,
    );
    survivor.catalog_index = 1;
    let accepted = BTreeSet::from([killed.id.clone(), survivor.id.clone()]);
    let mutation = Mutation {
        judged: vec![killed, survivor],
        skips: BTreeMap::new(),
        drift: Vec::new(),
    };
    let mut report = blank();

    record(&mut report, &mutation, &accepted)
        .expect("the mutation ledger fits the report counters");

    let completed = {
        "2026-01-01T00:00:00Z".clone_into(&mut report.timing.started);
        "2026-01-01T00:00:00Z".clone_into(&mut report.timing.finished);
        report.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
        report.limitations.push(njutest::report::Limitation::new(
            "git-metadata-unavailable",
            "the fixture is not a git repository",
        ));
        let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
            njutest::config::DEFAULT_CONFIGURATION.to_owned(),
            rust_mutants::cargo::BuildConfig::default().selection(),
            report.clone(),
        )])
        .expect("one checked build measurement");
        let final_run = rust_mutants::id::RunId::try_from("20260909t000000z-000002")
            .expect("a canonical run id");
        let latticed = njutest::report::across::configured(&final_run, &measurements)
            .expect("one checked complete lattice");
        let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
            panic!("the whole-catalog fixture cannot be a shard");
        };
        latticed
            .complete_without_models()
            .expect("standard-v1 needs no model completion")
    };

    let killed_row = report.mutants.first().expect("the killed row");
    let survivor_row = report.mutants.get(1).expect("the surviving row");
    assert!(!killed_row.accepted, "a kill is already answered by a test");
    assert!(
        survivor_row.accepted,
        "a surviving gap is one a reviewer may answer for"
    );
    assert_eq!(report.accounting.mutants.accepted, 1);
    assert!(
        !njutest::report::audit::validate_for_persistence(&completed)
            .iter()
            .any(|violation| matches!(
                violation,
                njutest::report::audit::Violation::MutantRowIncoherent { .. }
                    | njutest::report::audit::Violation::MutantAccountingDisagrees { .. }
            )),
        "the producer and its independent audit derive acceptance from the same closed domain"
    );
}
