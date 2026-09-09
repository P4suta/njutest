// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run writes into its report once a phase has answered.

use std::collections::{BTreeMap, BTreeSet};

use mjutest_cli::assure::baseline::Baseline;
use mjutest_cli::assure::mutation::{Disposition, Judged, Mutation};
use mjutest_cli::assure::route::{BRANCH_NEVER_TAKEN, Discharge, Route};
use mjutest_cli::assure::run::{about, absorb, record};
use mjutest_cli::config::Contract;
use mjutest_cli::report::{Report, RunKind};

fn judged(display_id: &str, disposition: Disposition, reused: bool) -> Judged {
    Judged {
        id: display_id.repeat(4),
        display_id: display_id.to_owned(),
        path: "src/lib.rs".to_owned(),
        rule: "add-to-sub@1".to_owned(),
        position: None,
        disposition,
        source_run_id: reused.then(|| "20260905T081500Z-000000".to_owned()),
    }
}

fn blank() -> Report {
    Report::new(
        "20260909T000000Z-000001",
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

    absorb(&mut report, &baseline);

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

    absorb(&mut report, &baseline);

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
    placed.position = Some(mjutest_cli::report::Position {
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
    };

    record(&mut report, &mutation, &BTreeSet::new());

    let rows = &report.mutants;
    assert_eq!(rows.len(), 2);
    let killed = rows.first().expect("the kill");
    assert_eq!(killed.display_id, "aaaa");
    assert_eq!(killed.outcome, "killed");
    assert_eq!(
        killed.killed_by.as_deref(),
        Some("one"),
        "a kill names the target that noticed, which is the one thing a reader cannot \
         work out afterwards"
    );
    assert!(
        killed.reused,
        "and says whether this run established it or read it back"
    );
    assert_eq!(killed.position.line, 12);

    let survived = rows.get(1).expect("the survivor");
    assert_eq!(survived.outcome, "survived");
    assert_eq!(survived.killed_by, None);
    assert!(!survived.reused);
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
