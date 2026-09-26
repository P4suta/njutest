// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run established each item of the source pins and leaves free, read from its report alone.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::presentation::Terminal;
use njutest::report::{
    Answered, Decided, Discharged, Established, MutantRecord, Outcome, Report, Reuse, RunKind,
    StepBoundary,
};
use njutest::spec::{
    Asked, Change, Edit, Free, Held, Pin, Same, Section, Specification, Subject, Unsettled,
    specified,
};
use njutest::testkit::reports::{completed, routed, row};
use rust_mutants::session::NEVER_INFECTED;

const LIB: &str = "pkg/lib/pkg";
const IT: &str = "pkg/test/it";
const MORE: &str = "pkg/test/more";

/// The whole report of a full run that measured one build per entry of `builds`.
fn run(builds: Vec<(&str, Vec<MutantRecord>)>) -> Report {
    completed("the-run", RunKind::Full, builds).expect("rows a report can hold")
}

/// One change `>` made `>=` on line 3 of `sign`, decided as `outcome`.
fn at(index: u32, outcome: Decided) -> MutantRecord {
    row(
        index,
        ("src/lib.rs", "sign", 3),
        ("gt-to-ge", ">", ">="),
        outcome,
    )
}

/// The one change a specification of one row holds.
fn only(spec: &Specification) -> &Change {
    let [item] = spec.items() else {
        panic!("one item: {spec:#?}");
    };
    let [change] = item.changes() else {
        panic!("one change: {spec:#?}");
    };
    change
}

/// What the one build of a one-row run established about its change, and where.
fn held(record: MutantRecord) -> (Held, Established) {
    let report = run(vec![("default", vec![record])]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    let answers: Vec<(Held, Established)> = only(&spec)
        .answers()
        .map(|answer| (answer.held(), answer.established().clone()))
        .collect();
    let [answer] = answers.as_slice() else {
        panic!("one build, one answer: {spec:#?}");
    };
    answer.clone()
}

/// The page a one-build run of `rows` is drawn as, at a width nothing folds at.
fn drawn(rows: Vec<MutantRecord>) -> String {
    let report = run(vec![("default", rows)]);
    let spec = specified(&report, &Subject::Everything).expect("a run with changes");
    njutest::presentation::spec::page(&spec, Terminal::plain(400))
}

#[test]
fn a_change_nothing_was_established_about_is_neither_pinned_nor_free() {
    let boundary = StepBoundary::new(10, 11).expect("the first count beyond the allowance");
    let report = run(vec![(
        "default",
        vec![
            at(0, Decided::Waited { on: IT.to_owned() }),
            at(1, Decided::Errored { on: IT.to_owned() }),
            at(2, Decided::Unconfirmed { on: IT.to_owned() }),
            at(
                3,
                Decided::StepLimitReached {
                    on: IT.to_owned(),
                    boundary,
                },
            ),
        ],
    )]);
    let spec = specified(&report, &Subject::Everything).expect("a run with changes");
    let changes = spec.items()[0].changes();
    assert_eq!(
        changes.iter().map(Change::section).collect::<Vec<_>>(),
        [Section::Unsettled; 4],
        "an attempt that established nothing is not a chance the tests failed to take, so it \
         is in neither the list of what they pin nor the list of what they leave free"
    );
    let holdings: Vec<Held> = changes
        .iter()
        .flat_map(|change| change.answers().map(njutest::spec::Answer::held))
        .collect();
    assert_eq!(
        holdings,
        [
            Held::Unsettled(Unsettled::Waited { on: IT.to_owned() }),
            Held::Unsettled(Unsettled::Errored { on: IT.to_owned() }),
            Held::Unsettled(Unsettled::Unconfirmed { on: IT.to_owned() }),
            Held::Unsettled(Unsettled::StepLimit {
                on: IT.to_owned(),
                observed: 11,
            }),
        ]
    );
}

#[test]
fn a_change_one_build_noticed_and_another_did_not_is_free_and_says_which_is_which() {
    let mut killed = at(0, Decided::Killed { by: LIB.to_owned() });
    killed.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Killed)]));
    let mut survived = at(0, Decided::Survived);
    survived.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Survived)]));
    let report = run(vec![("default", vec![killed]), ("release", vec![survived])]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    let change = only(&spec);
    assert_eq!(
        change.section(),
        Section::Free,
        "a change a build noticed nothing of is free in that build, whatever the other said"
    );
    let builds: Vec<(&str, Held)> = change
        .answers()
        .map(|answer| (answer.build(), answer.held()))
        .collect();
    assert_eq!(
        builds,
        [
            (
                "default",
                Held::Pinned(Pin::Tests {
                    by: LIB.to_owned(),
                    asked: Asked::Here {
                        before: Vec::new(),
                        unasked: Vec::new(),
                    },
                })
            ),
            (
                "release",
                Held::Free {
                    free: Free::Ran {
                        answered: vec![LIB.to_owned()],
                        removed: Vec::new(),
                    },
                    accepted: false,
                }
            ),
        ]
    );
}

#[test]
fn a_change_that_is_the_same_program_in_one_build_and_free_in_another_is_free() {
    let mut survived = at(0, Decided::Survived);
    survived.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Survived)]));
    let report = run(vec![
        ("default", vec![at(0, Decided::Equivalent)]),
        ("release", vec![survived]),
    ]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    assert_eq!(only(&spec).section(), Section::Free);
}

#[test]
fn a_change_one_build_noticed_and_another_found_the_same_program_is_pinned() {
    let mut killed = at(0, Decided::Killed { by: LIB.to_owned() });
    killed.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Killed)]));
    let report = run(vec![
        ("default", vec![killed]),
        ("release", vec![at(0, Decided::Equivalent)]),
    ]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    assert_eq!(
        only(&spec).section(),
        Section::Pinned,
        "a build where the change is the same program leaves nothing free, so the build that \
         noticed it decides where it stands"
    );
}

#[test]
fn a_kill_names_who_ran_it_first_and_who_reaches_it_and_was_never_asked() {
    let mut killed = at(0, Decided::Killed { by: IT.to_owned() });
    killed.routing = Some(routed(
        &[LIB, IT, MORE],
        &[],
        &[(LIB, Outcome::Survived), (IT, Outcome::Killed)],
    ));
    assert_eq!(
        held(killed).0,
        Held::Pinned(Pin::Tests {
            by: IT.to_owned(),
            asked: Asked::Here {
                before: vec![Answered {
                    target: LIB.to_owned(),
                    outcome: Outcome::Survived,
                }],
                unasked: vec![MORE.to_owned()],
            },
        }),
        "the run stops at the first target that notices, so the one that did is the first in \
         route order and not a distinguished one, and the ones after it were never asked"
    );
}

#[test]
fn a_survivor_a_proof_removed_every_target_of_says_the_proof_and_not_that_anything_ran_it() {
    let mut removed = at(0, Decided::Survived);
    removed.routing = Some(routed(&[], &[LIB], &[]));
    assert_eq!(
        held(removed).0,
        Held::Free {
            free: Free::Removed(vec![Discharged {
                target: LIB.to_owned(),
                proof: NEVER_INFECTED,
            }]),
            accepted: false,
        }
    );
}

#[test]
fn a_survivor_some_targets_ran_and_a_proof_removed_the_rest_of_says_both() {
    let mut partly = at(0, Decided::Survived);
    partly.routing = Some(routed(&[IT], &[LIB], &[(IT, Outcome::Survived)]));
    assert_eq!(
        held(partly).0,
        Held::Free {
            free: Free::Ran {
                answered: vec![IT.to_owned()],
                removed: vec![Discharged {
                    target: LIB.to_owned(),
                    proof: NEVER_INFECTED,
                }],
            },
            accepted: false,
        }
    );
}

#[test]
fn a_change_nothing_executes_is_free_because_nothing_ran_it_and_not_because_nothing_noticed() {
    assert_eq!(
        held(at(0, Decided::Unreached)).0,
        Held::Free {
            free: Free::Never,
            accepted: false,
        }
    );
}

#[test]
fn an_accepted_survivor_is_free_and_says_a_reviewer_accepted_it() {
    let mut accepted = at(0, Decided::Survived);
    accepted.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Survived)]));
    accepted.accepted = true;
    assert_eq!(
        held(accepted).0,
        Held::Free {
            free: Free::Ran {
                answered: vec![LIB.to_owned()],
                removed: Vec::new(),
            },
            accepted: true,
        }
    );
}

#[test]
fn what_the_compiler_refuses_the_types_pin_and_what_it_renders_identically_is_the_same_program() {
    assert_eq!(
        held(at(0, Decided::CompileRejected)).0,
        Held::Pinned(Pin::Types),
        "the type system is an observer, and a refusal is it noticing"
    );
    assert_eq!(
        held(at(0, Decided::Equivalent)).0,
        Held::Same(Same::Compiled)
    );
}

#[test]
fn an_answer_an_earlier_run_established_says_which_run_and_claims_nothing_about_who_else_was_asked()
{
    let mut reused = at(0, Decided::Killed { by: IT.to_owned() });
    reused.routing = Some(routed(&[LIB, IT], &[], &[]));
    reused.reuse = Reuse(Established::ReadBackFrom("an-earlier-run".to_owned()));
    let (held, established) = held(reused.clone());
    assert_eq!(
        established,
        Established::ReadBackFrom("an-earlier-run".to_owned())
    );
    assert_eq!(
        held,
        Held::Pinned(Pin::Tests {
            by: IT.to_owned(),
            asked: Asked::Unrecorded,
        }),
        "the route is this run's and the answer is the earlier one's, so who else that run \
         asked is not in this record"
    );
    let page = drawn(vec![reused]);
    assert!(page.contains("established by run an-earlier-run"), "{page}");
}

#[test]
fn an_answer_inherited_without_a_route_does_not_invent_who_ran_it() {
    assert_eq!(
        held(at(0, Decided::Survived)).0,
        Held::Free {
            free: Free::Unrecorded,
            accepted: false,
        }
    );
}

#[test]
fn a_deletion_is_said_as_one() {
    let report = run(vec![(
        "default",
        vec![row(
            0,
            ("src/lib.rs", "sign", 3),
            ("delete-assignment", "log(n);", ""),
            Decided::Unreached,
        )],
    )]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    assert_eq!(only(&spec).edit(), Edit::Deleted { was: "log(n);" });
}

#[test]
fn a_subject_is_a_file_an_item_or_an_item_in_a_file() {
    assert_eq!(Subject::parse(None), Subject::Everything);
    assert_eq!(
        Subject::parse(Some("src/lib.rs")),
        Subject::File("src/lib.rs".to_owned())
    );
    assert_eq!(
        Subject::parse(Some("lib.RS")),
        Subject::File("lib.RS".to_owned())
    );
    assert_eq!(
        Subject::parse(Some("src/lib.rs:Parser::new")),
        Subject::InFile {
            path: "src/lib.rs".to_owned(),
            item: "Parser::new".to_owned(),
        }
    );
    assert_eq!(
        Subject::parse(Some("Parser::new")),
        Subject::Item("Parser::new".to_owned())
    );
    assert_eq!(Subject::parse(Some("new")), Subject::Item("new".to_owned()));
}

#[test]
fn an_item_names_itself_the_items_inside_it_and_nothing_that_only_starts_the_same() {
    let retry = Subject::parse(Some("retry"));
    for (item, named) in [
        ("retry", true),
        ("Baseline::retry", true),
        ("retry::inner", true),
        ("Baseline::retry::inner", true),
        ("retry_all", false),
        ("Baseline::retry_all", false),
        ("reretry", false),
    ] {
        assert_eq!(
            retry.names("src/lib.rs", item),
            named,
            "`retry` and `{item}`"
        );
    }
    let qualified = Subject::parse(Some("Baseline::retry"));
    assert!(qualified.names("src/lib.rs", "outer::Baseline::retry"));
    assert!(!qualified.names("src/lib.rs", "Other::retry"));
    let file = Subject::parse(Some("lib.rs"));
    assert!(file.names("src/lib.rs", "anything"));
    assert!(!file.names("src/mylib.rs", "anything"));
    let in_file = Subject::parse(Some("src/lib.rs:retry"));
    assert!(in_file.names("src/lib.rs", "Baseline::retry"));
    assert!(!in_file.names("src/main.rs", "Baseline::retry"));
}

#[test]
fn every_item_a_subject_names_is_listed_under_its_own_path() {
    let never = |index, path, item, line| {
        row(
            index,
            (path, item, line),
            ("return-default", "Vec::new()", "Default::default()"),
            Decided::Unreached,
        )
    };
    let report = run(vec![(
        "default",
        vec![
            never(0, "src/a.rs", "Parser::new", 4),
            never(1, "src/b.rs", "Lexer::new", 9),
            never(2, "src/b.rs", "Lexer::next", 20),
        ],
    )]);
    let spec = specified(&report, &Subject::parse(Some("new"))).expect("two items named new");
    let named: Vec<(&str, &str)> = spec
        .items()
        .iter()
        .map(|item| (item.path(), item.name()))
        .collect();
    assert_eq!(
        named,
        [("src/a.rs", "Parser::new"), ("src/b.rs", "Lexer::new")]
    );
}

#[test]
fn a_subject_naming_nothing_the_run_changed_is_refused_with_its_code() {
    let report = run(vec![("default", vec![at(0, Decided::Unreached)])]);
    let Err(error) = specified(&report, &Subject::parse(Some("nowhere"))) else {
        panic!("a subject that names nothing is not an empty specification");
    };
    assert_eq!(error.code().code, "NJ6006");
    assert!(
        error.to_string().contains("the-run") && error.to_string().contains("a full run"),
        "the refusal names the run and how much of the workspace it asked about: {error}"
    );
    let changed = completed(
        "a-changed-run",
        RunKind::Changed,
        vec![("default", vec![at(0, Decided::Unreached)])],
    )
    .expect("rows a report can hold");
    let Err(error) = specified(&changed, &Subject::parse(Some("nowhere"))) else {
        panic!("a subject that names nothing is not an empty specification");
    };
    assert!(
        error.to_string().contains("asked only about what changed"),
        "a changed run is not the whole workspace, and a reader told it made no change \
         somewhere is told why that may be: {error}"
    );
}

/// The words a page must say about `held`, one arm per way a build can hold a change, so a way added later is one somebody writes the sentence for.
fn said(held: &Held) -> Vec<&'static str> {
    match held {
        Held::Pinned(Pin::Tests { asked, .. }) => match asked {
            Asked::Here { .. } => vec![
                "pkg/test/it notices it",
                "pkg/lib/pkg ran it first and did not notice",
                "1 more target reaches it and was never asked: pkg/test/more",
            ],
            Asked::Unrecorded => vec!["pkg/test/it notices it"],
        },
        Held::Pinned(Pin::Types) => vec!["the compiler refuses it"],
        Held::Pinned(Pin::Model) => {
            vec!["the model checker finds an input that tells the two apart"]
        }
        Held::Free { free, accepted } => {
            let mut words = match free {
                Free::Ran { .. } => vec![
                    "pkg/test/it ran it and did not notice",
                    "never-infected removed pkg/lib/pkg",
                ],
                Free::Removed(_) => vec![
                    "never-infected removed every target that reaches it (pkg/lib/pkg), so \
                     nothing ran it",
                    "check the proof, not the tests",
                ],
                Free::Never => vec!["nothing the suite runs executes it"],
                Free::Unrecorded => {
                    vec!["nothing noticed it, and the record does not say what ran it"]
                }
            };
            if *accepted {
                words.push("a reviewer accepted it");
            }
            words
        }
        Held::Same(Same::Compiled) => vec!["the compiler renders it identically"],
        Held::Same(Same::Model) => {
            vec!["the model checker proves the two equal throughout its domain"]
        }
        Held::Unsettled(Unsettled::StepLimit { .. }) => {
            vec!["pkg/test/it crossed its step allowance at 11 without a verdict"]
        }
        Held::Unsettled(Unsettled::Waited { .. }) => {
            vec!["this machine stopped waiting for pkg/test/it"]
        }
        Held::Unsettled(Unsettled::Unconfirmed { .. }) => {
            vec!["pkg/test/it did not answer the same way twice"]
        }
        Held::Unsettled(Unsettled::Errored { .. }) => vec!["pkg/test/it could not be measured"],
    }
}

/// One of every way a build can hold a change, with the names `said` expects.
fn every_holding() -> Vec<Held> {
    let removed = || Discharged {
        target: LIB.to_owned(),
        proof: NEVER_INFECTED,
    };
    let every = vec![
        Held::Pinned(Pin::Tests {
            by: IT.to_owned(),
            asked: Asked::Here {
                before: vec![Answered {
                    target: LIB.to_owned(),
                    outcome: Outcome::Survived,
                }],
                unasked: vec![MORE.to_owned()],
            },
        }),
        Held::Pinned(Pin::Tests {
            by: IT.to_owned(),
            asked: Asked::Unrecorded,
        }),
        Held::Pinned(Pin::Types),
        Held::Pinned(Pin::Model),
        Held::Free {
            free: Free::Ran {
                answered: vec![IT.to_owned()],
                removed: vec![removed()],
            },
            accepted: false,
        },
        Held::Free {
            free: Free::Removed(vec![removed()]),
            accepted: false,
        },
        Held::Free {
            free: Free::Never,
            accepted: true,
        },
        Held::Free {
            free: Free::Unrecorded,
            accepted: false,
        },
        Held::Same(Same::Compiled),
        Held::Same(Same::Model),
        Held::Unsettled(Unsettled::StepLimit {
            on: IT.to_owned(),
            observed: 11,
        }),
        Held::Unsettled(Unsettled::Waited { on: IT.to_owned() }),
        Held::Unsettled(Unsettled::Unconfirmed { on: IT.to_owned() }),
        Held::Unsettled(Unsettled::Errored { on: IT.to_owned() }),
    ];
    for held in &every {
        match held {
            Held::Pinned(_) | Held::Free { .. } | Held::Same(_) | Held::Unsettled(_) => {}
        }
    }
    every
}

#[test]
fn every_way_a_build_holds_a_change_is_said_in_the_words_that_are_true_of_it() {
    for held in every_holding() {
        let worded = njutest::presentation::spec::worded(&held);
        for words in said(&held) {
            assert!(
                worded.contains(words),
                "{held:?} must say {words:?}, and it says: {worded}"
            );
        }
    }
}

#[test]
fn a_page_puts_each_change_under_the_heading_of_where_it_stands() {
    let page = drawn(vec![
        at(0, Decided::CompileRejected),
        at(1, Decided::Unreached),
        at(2, Decided::Equivalent),
        at(3, Decided::Errored { on: IT.to_owned() }),
    ]);
    let at_heading: Vec<Option<usize>> = Section::ALL
        .into_iter()
        .map(|section| page.find(njutest::presentation::spec::heading(section)))
        .collect();
    assert!(
        at_heading.iter().all(Option::is_some) && at_heading.is_sorted(),
        "every section with something in it is drawn, in the order a reader meets them:\n{page}"
    );
    for (words, heading) in [
        ("the compiler refuses it", "what is pinned"),
        ("nothing the suite runs executes it", "what is left free"),
        (
            "the compiler renders it identically",
            "what is the same program",
        ),
        (
            "pkg/test/it could not be measured",
            "what the run could not tell",
        ),
    ] {
        let under = page
            .find(heading)
            .and_then(|from| page.get(from..))
            .and_then(|rest| rest.find(words));
        assert!(under.is_some(), "{words:?} is under {heading:?}:\n{page}");
    }
}

#[test]
fn the_command_a_page_prints_for_a_change_names_that_change_and_is_never_folded() {
    let report = run(vec![(
        "default",
        vec![row(
            0,
            (
                "src/a/rather/long/path/to/lib.rs",
                "Parser::new_from_parts",
                3,
            ),
            (
                "return-default",
                "Parser { tokens, at: 0 }",
                "Default::default()",
            ),
            Decided::Unreached,
        )],
    )]);
    let spec = specified(&report, &Subject::Everything).expect("a run with one change");
    let command =
        "njutest explain src/a/rather/long/path/to/lib.rs:Parser::new_from_parts:return-default@3";
    for width in 30..=160 {
        let page = njutest::presentation::spec::page(&spec, Terminal::plain(width));
        assert!(
            page.lines().any(|line| line.contains(command)),
            "a command broken across two lines is one nobody can select, so at {width} columns \
             it is on one line, beside the change where it fits and on a line of its own where \
             it does not:\n{page}"
        );
    }
}

#[test]
fn a_fixture_cannot_hold_a_rule_no_run_writes() {
    let Err(error) = completed(
        "the-run",
        RunKind::Full,
        vec![(
            "default",
            vec![row(
                0,
                ("src/lib.rs", "sign", 3),
                ("gt-to-ge@1", ">", ">="),
                Decided::Unreached,
            )],
        )],
    ) else {
        panic!("a row naming a versioned rule is one no report carries");
    };
    assert!(
        matches!(error, njutest::testkit::reports::UnmadeReportError::Rule { ref name } if name == "gt-to-ge@1"),
        "a report names a rule bare, so a fixture that versions it prints a locator no \
         command resolves: {error}"
    );
}

#[test]
fn a_free_change_that_rests_on_a_target_whose_reach_moved_says_so_and_a_kill_does_not() {
    let moved = njutest::report::drift::Drift::Moved {
        target: LIB.to_owned(),
        reached: njutest::report::drift::Moved {
            gained: std::collections::BTreeSet::from([0]),
            lost: std::collections::BTreeSet::new(),
        },
        bodies: njutest::report::drift::Moved {
            gained: std::collections::BTreeSet::new(),
            lost: std::collections::BTreeSet::new(),
        },
        infected: njutest::report::drift::Moved {
            gained: std::collections::BTreeSet::new(),
            lost: std::collections::BTreeSet::new(),
        },
        entered: njutest::report::drift::Moved {
            gained: std::collections::BTreeSet::new(),
            lost: std::collections::BTreeSet::new(),
        },
    };
    let mut kept_off = at(0, Decided::Survived);
    kept_off.routing = Some(routed(&[IT], &[], &[(IT, Outcome::Survived)]));
    let mut asked = at(1, Decided::Survived);
    asked.routing = Some(routed(&[LIB], &[], &[(LIB, Outcome::Survived)]));
    let mut killed = at(2, Decided::Killed { by: IT.to_owned() });
    killed.routing = Some(routed(&[IT], &[], &[(IT, Outcome::Killed)]));
    let report = njutest::testkit::reports::completed_with_drift(
        "the-run",
        RunKind::Full,
        vec![("default", vec![kept_off, asked, killed], vec![moved])],
    )
    .expect("rows a report can hold");
    let spec = specified(&report, &Subject::Everything).expect("a run with changes");
    let unfounded: Vec<Vec<String>> = spec.items()[0]
        .changes()
        .iter()
        .flat_map(|change| change.answers().map(njutest::spec::Answer::unfounded))
        .collect();
    assert_eq!(
        unfounded,
        [vec![LIB.to_owned()], Vec::new(), Vec::new()],
        "a survivor the route kept off the moved target rests on a reach that is not a \
         measurement, one the moved target ran does not, and a kill rests on no reach at all"
    );
    let page = njutest::presentation::spec::page(&spec, Terminal::plain(400));
    assert!(
        page.contains("pkg/lib/pkg reached something on a control that its baseline did not"),
        "{page}"
    );
}
