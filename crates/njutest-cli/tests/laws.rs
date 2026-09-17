// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What holds for every run rather than for the ones somebody thought to write down.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "a law about counts is written the way a reader adds them up; the values are a report's own and cannot approach the width they are held in"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "a law reads a document by the names its own subject put there, and a document missing one of them is a failure to report by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};

use njutest_cli::assure::mutation::{Disposition, Judged, Mutation, Unconfirmed};
use njutest_cli::config::Contract;
use njutest_cli::evidence::digest::{Inputs, Mode, identity};
use proptest::prelude::*;
use rust_mutants::session::{Fallback, Route};

/// One disposition of each shape, as a run can reach it.
fn disposition() -> impl Strategy<Value = Disposition> {
    let route = || Route::All {
        reaching: vec!["pkg/lib/pkg".to_owned()],
        fallback: Fallback::NotMeasured,
    };
    prop_oneof![
        Just(Disposition::Rejected {
            diagnostic: "no".to_owned()
        }),
        Just(Disposition::Killed {
            by: "pkg/lib/pkg".to_owned()
        }),
        Just(Disposition::TimedOut {
            on: "pkg/lib/pkg".to_owned()
        }),
        Just(Disposition::Survived { route: route() }),
        Just(Disposition::Unreached),
        Just(Disposition::Equivalent { route: route() }),
        Just(Disposition::Unconfirmed {
            on: "pkg/lib/pkg".to_owned(),
            why: Unconfirmed::DidNotReproduce,
        }),
        Just(Disposition::Errored {
            on: "pkg/lib/pkg".to_owned(),
            detail: "no binary".to_owned(),
        }),
    ]
}

/// A run that judged these mutations, each under an identity of its own.
fn judged_from(dispositions: Vec<Disposition>) -> Mutation {
    Mutation {
        judged: dispositions
            .into_iter()
            .enumerate()
            .map(|(at, disposition)| Judged {
                id: format!("{at:064x}"),
                display_id: format!("{at:020x}"),
                path: "src/lib.rs".to_owned(),
                rule: "add-to-sub@1".to_owned(),
                position: None,
                disposition,
                source_run_id: None,
            })
            .collect(),
        skips: BTreeMap::new(),
    }
}

proptest! {
    /// Every mutation is in exactly one of the four columns a reader adds up.
    #[test]
    fn what_a_run_catalogued_is_what_it_refused_ran_could_not_reach_or_proved_identical(
        dispositions in proptest::collection::vec(disposition(), 0..24)
    ) {
        let counts = judged_from(dispositions).accounting(&BTreeSet::new());
        prop_assert_eq!(
            counts.cataloged,
            counts.rejected + counts.executed + counts.unreached + counts.equivalent,
            "this is the identity a reader adds up to check every other number in the \
             report, and the audit refuses a run that breaks it: {:?}",
            counts
        );
        prop_assert!(
            counts.killed + counts.survived + counts.timed_out <= counts.executed,
            "and what ran is at least what the outcomes of running account for: {:?}",
            counts
        );
    }

    /// A reviewer answers for what nothing noticed, and for nothing else.
    #[test]
    fn an_acceptance_covers_a_survivor_an_unreached_mutation_or_an_equivalent_one(
        dispositions in proptest::collection::vec(disposition(), 0..24)
    ) {
        let mutation = judged_from(dispositions);
        let every: BTreeSet<String> = mutation.judged.iter().map(|one| one.id.clone()).collect();
        let counts = mutation.accounting(&every);
        prop_assert_eq!(
            counts.accepted,
            counts.survived + counts.unreached + counts.equivalent,
            "a ledger that accepts every mutation the run judged answers for exactly the \
             ones nothing noticed. Counting a timeout or a harness that failed among them \
             would let a reviewer sign off on an outcome nobody established: {:?}",
            counts
        );
        let none = judged_from(Vec::new()).accounting(&every);
        prop_assert_eq!(none.accepted, 0, "and a run that judged nothing accepts nothing");
    }
}

/// What a run is, as the digest reads it.
fn inputs() -> Inputs {
    Inputs {
        tree: "a".repeat(64),
        corpus: "b".repeat(64),
        dependencies: "c".repeat(64),
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: vec![("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned())],
        contract: Contract::StandardV1,
        configuration: "d".repeat(64),
        test_args: vec!["--test-threads=1".to_owned()],
        mode: Mode::Full,
        shard: None,
    }
}

/// One field of a run's identity, and how to make it say something else.
type Field = (&'static str, fn(&mut Inputs, &str));

/// Every field a run's identity is computed from.
const FIELDS: [Field; 9] = [
    ("tree", |it, value| value.clone_into(&mut it.tree)),
    ("corpus", |it, value| value.clone_into(&mut it.corpus)),
    ("dependencies", |it, value| {
        value.clone_into(&mut it.dependencies);
    }),
    ("toolchain", |it, value| value.clone_into(&mut it.toolchain)),
    ("platform", |it, value| value.clone_into(&mut it.platform)),
    ("configuration", |it, value| {
        value.clone_into(&mut it.configuration);
    }),
    ("environment", |it, value| {
        it.environment.push(("EXTRA".to_owned(), value.to_owned()));
    }),
    ("test_args", |it, value| {
        it.test_args.push(value.to_owned());
    }),
    ("shard", |it, value| {
        it.shard = Some(format!("1/{}", value.len().max(1)));
    }),
];

proptest! {
    /// Nothing a run is a function of may be changed without changing what it is.
    #[test]
    fn a_run_that_differs_in_any_one_of_its_inputs_is_a_different_run(
        value in "[a-z0-9]{1,24}"
    ) {
        let base = identity(&inputs());
        for (named, change) in FIELDS {
            let mut altered = inputs();
            change(&mut altered, &value);
            prop_assert_ne!(
                identity(&altered),
                base.clone(),
                "two runs that differ in {} share an identity, so one of them will be \
                 handed the other's answer. A table of cases is as thorough as whoever \
                 wrote it; this is every field there is",
                named
            );
        }
    }

    /// The identity is a function of the inputs and of nothing else about how they were collected.
    #[test]
    fn the_order_and_the_repetition_of_an_environment_are_not_facts_about_a_run(
        mut environment in proptest::collection::vec(
            ("[A-Z_]{1,8}", "[a-z0-9 =/-]{0,16}"),
            0..8,
        )
    ) {
        environment.sort();
        environment.dedup_by(|one, other| one.0 == other.0);
        let ordered = Inputs { environment: environment.clone(), ..inputs() };
        let mut shuffled = environment.clone();
        shuffled.reverse();
        prop_assert_eq!(
            identity(&Inputs { environment: shuffled, ..inputs() }),
            identity(&ordered),
            "a process lists its environment in whatever order it likes, and a run whose \
             identity depended on that order would never read back its own answer"
        );
        let mut twice = environment.clone();
        twice.extend(environment);
        prop_assert_eq!(
            identity(&Inputs { environment: twice, ..inputs() }),
            identity(&ordered),
            "and a variable named twice with one value is one variable: a process cannot \
             be started with two of them and read anything but one"
        );
    }
}

/// What a test can print and a page must not become.
const MARKUP: &str = "<script>alert('x')</script>";

/// One finding of any kind, about a place a run may or may not know.
fn finding() -> impl Strategy<Value = njutest_cli::report::Finding> {
    (
        prop_oneof![
            Just(njutest_cli::report::FindingKind::SurvivingMutant),
            Just(njutest_cli::report::FindingKind::FailingTest),
            Just(njutest_cli::report::FindingKind::TargetMissing),
            Just(njutest_cli::report::FindingKind::Timeout),
            Just(njutest_cli::report::FindingKind::NotMeasured),
            Just(njutest_cli::report::FindingKind::UnmatchedAcceptance),
            Just(njutest_cli::report::FindingKind::UndefinedBehaviour),
        ],
        "[a-z0-9]{1,20}",
        "[a-zA-Z0-9 <>&\"'/:@-]{1,60}".prop_map(|said| format!("{MARKUP}{said}")),
        proptest::option::of((
            "(src|tests)/[a-z_]{1,8}\\.rs",
            prop_oneof![Just(0_u32), 1_u32..500],
        )),
    )
        .prop_map(|(kind, subject, detail, place)| {
            let mut one = njutest_cli::report::Finding::new(kind, &subject, &detail);
            if let Some((path, line)) = place {
                one.path = Some(path);
                one.position = Some(njutest_cli::report::Position {
                    line,
                    column: line,
                    character_column: line,
                });
            }
            one
        })
}

/// A report of a run that found these things.
fn reported(findings: Vec<njutest_cli::report::Finding>) -> njutest_cli::report::Report {
    let mut report = njutest_cli::report::Report::new(
        "20260909T000000Z-000001",
        njutest_cli::report::RunKind::Full,
        Contract::StandardV1,
    );
    "workspace".clone_into(&mut report.repository.root_name);
    report.verdict = njutest_cli::report::Verdict::Insufficient;
    report.mutants = findings
        .iter()
        .filter_map(|one| Some((one.path.clone()?, one.position?)))
        .enumerate()
        .map(|(at, (path, position))| njutest_cli::report::MutantRecord {
            id: format!("{at:064x}"),
            display_id: format!("{at:020x}"),
            path,
            position,
            rule: "add-to-sub@1".to_owned(),
            outcome: "survived".to_owned(),
            killed_by: None,
            reused: false,
            source_run_id: None,
        })
        .collect();
    report.findings = findings;
    report
}

proptest! {
    /// Whatever a run found, the document a machine reads is the one it can read.
    #[test]
    fn every_projection_of_every_report_is_one_its_reader_accepts(
        findings in proptest::collection::vec(finding(), 0..8)
    ) {
        let report = reported(findings);

        let rendered = njutest_cli::report::json::render(&report).expect("a report renders");
        let read = njutest_cli::report::json::parse(&rendered).expect("and reads back");
        prop_assert_eq!(
            njutest_cli::report::json::render(&read).expect("and renders again"),
            rendered,
            "a report that does not survive being written and read is one no later run \
             and no other tool can be handed"
        );

        let log = njutest_cli::report::sarif::document(&report);
        let run = &log["runs"][0];
        let rules: Vec<String> = run["tool"]["driver"]["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .filter_map(|rule| rule["id"].as_str().map(str::to_owned))
            .collect();
        for result in run["results"].as_array().expect("results") {
            let id = result["ruleId"].as_str().unwrap_or_default().to_owned();
            prop_assert!(
                rules.contains(&id),
                "a consumer refuses the whole log for one result whose rule the driver \
                 does not declare: {}",
                result
            );
            if let Some(place) = result["locations"].as_array().and_then(|all| all.first()) {
                let region = &place["physicalLocation"]["region"];
                prop_assert!(
                    region["startLine"].as_u64().is_some_and(|line| line >= 1),
                    "and for one region at a line no file has: {}",
                    result
                );
            }
        }

        let page = njutest_cli::report::html::document(&report);
        prop_assert!(
            !page.contains(MARKUP),
            "nothing a test printed becomes markup in a page somebody opens, and what a \
             test prints is whatever the code under test printed"
        );
        let document = njutest_cli::report::junit::document(&report);
        prop_assert!(
            !document.contains(MARKUP),
            "nor a tag in the document a reporter parses"
        );
        prop_assert!(
            document.starts_with("<?xml") && document.ends_with("</testsuites>\n"),
            "and the document a test reporter reads is a whole one: {document}"
        );
        let stream = njutest_cli::report::lines::stream(&report);
        prop_assert!(
            stream.lines().all(|line| !line.contains('\n')),
            "and every record a person greps for is one line"
        );
    }
}

proptest! {
    /// However many workers a run is given, every mutation is answered for once, in order.
    #[test]
    fn a_run_answers_for_every_mutation_once_and_in_the_order_it_catalogued_them(
        count in prop_oneof![Just(0_usize), Just(1_usize), 2_usize..40],
        workers in prop_oneof![Just(1_usize), 2_usize..12],
    ) {
        let items: Vec<usize> = (0..count).collect();
        let answers = njutest_cli::assure::schedule::measure(&items, workers, |at, item| {
            prop_assert_eq!(at, *item, "a worker is told which item it has");
            Ok(at.saturating_mul(2))
        });
        let answers: Vec<usize> = answers
            .into_iter()
            .collect::<Result<Vec<usize>, TestCaseError>>()?;
        prop_assert_eq!(
            answers,
            items.iter().map(|at| at.saturating_mul(2)).collect::<Vec<usize>>(),
            "a report's rows are the catalog's order, so a run that handed them back in \
             the order they happened to finish would give two runs of one tree two \
             reports and every diff between them would be about the machine"
        );
    }

    /// How many measurements run at once follows the configuration, the machine, and what a resource forbids.
    #[test]
    fn how_widely_a_run_measures_is_what_it_was_told_bounded_by_what_it_has(
        jobs in prop_oneof![Just(0_u32), 1_u32..64],
        available in prop_oneof![Just(0_usize), 1_usize..64],
        exclusive in proptest::bool::ANY,
    ) {
        let workers = njutest_cli::assure::schedule::workers(jobs, available, exclusive);
        prop_assert!(
            workers >= 1,
            "a run measures something: nought workers is a run that never finishes"
        );
        if exclusive {
            prop_assert_eq!(
                workers, 1,
                "a resource only one test may hold at a time decides it for the whole \
                 run, whatever was configured and whatever the machine offers"
            );
        } else if jobs > 0 {
            prop_assert_eq!(
                workers,
                usize::try_from(jobs).unwrap_or(1),
                "and a run told how many gets that many: the machine's own count is a \
                 default, not a bound on what somebody asked for"
            );
        } else {
            prop_assert!(
                workers <= njutest_cli::assure::schedule::CAP,
                "while a run that said nothing takes what the machine offers up to the \
                 cap, because every worker is a test process and a machine's whole \
                 parallelism spent on those leaves nothing to run them"
            );
            prop_assert!(workers <= available.max(1));
        }
    }
}
