// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every dimension's column is read off the records alone, each record placed by one exhaustive match (ADR 0033).

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest::report::Limitation;
use njutest::report::faults::{FaultDecision, FaultRecord};
use njutest::report::matrix::{Column, Dimension, Evidence, pooled, rows};

fn fault(decision: FaultDecision) -> FaultRecord {
    FaultRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "c".repeat(20),
        path: "src/lib.rs".to_owned(),
        item: "load".to_owned(),
        position: None,
        decision,
    }
}

fn column(evidence: &Evidence<'_>, dimension: Dimension) -> Column {
    rows(evidence)
        .into_iter()
        .find(|row| row.dimension == dimension)
        .map(|row| row.column)
        .expect("every dimension has a row")
}

const fn counts(column: &Column) -> Option<(usize, usize, usize)> {
    match column {
        Column::Measured {
            catalogued,
            answered,
            holes,
            ..
        } => Some((*catalogued, *answered, holes.len())),
        Column::Unmeasured { .. } | Column::NotAsked | Column::NothingToAsk { .. } => None,
    }
}

#[test]
fn each_fault_decision_is_answered_a_hole_or_a_class_the_column_does_not_speak_about() {
    let faults: Vec<FaultRecord> = FaultDecision::every().into_iter().map(fault).collect();
    let evidence = Evidence {
        mutations: (4, vec!["cccccccccc: waited".to_owned()]),
        knobs: &[],
        faults: &faults,
        crashes: &[],
        concurrency: &[],
        targets: &[],
        seams: &[],
        limitations: &[],
        findings: &[],
    };
    let faulted = column(&evidence, Dimension::Fault);
    assert_eq!(counts(&faulted), Some((6, 4, 2)), "{faulted:?}");
    assert_eq!(
        counts(&column(&evidence, Dimension::Mutation)),
        Some((5, 4, 1))
    );
    assert_eq!(column(&evidence, Dimension::Repeatable), Column::NotAsked);
    assert!(
        matches!(
            column(&evidence, Dimension::Schedule),
            Column::NothingToAsk { .. }
        ),
        "with no test binary there is no schedule to ask about"
    );
}

#[test]
fn every_seam_that_could_not_be_watched_is_a_hole_of_its_own() {
    let unwatched = [
        Limitation::new(
            njutest::limitation::Limitation::SeamNotWatched,
            "payments could not be watched",
        ),
        Limitation::new(
            njutest::limitation::Limitation::SeamNotWatched,
            "search could not be watched",
        ),
    ];
    let evidence = Evidence {
        mutations: (0, Vec::new()),
        knobs: &[],
        faults: &[],
        crashes: &[],
        concurrency: &[],
        targets: &[],
        seams: &[],
        limitations: &unwatched,
        findings: &[],
    };
    assert_eq!(counts(&column(&evidence, Dimension::Wire)), Some((2, 0, 2)));
}

#[test]
fn a_hole_in_any_build_is_a_hole_of_every_build_together() {
    let measured = |catalogued, answered, holes: &[&str]| Column::Measured {
        catalogued,
        answered,
        holes: holes.iter().map(|hole| (*hole).to_owned()).collect(),
        speaks_not_about: Vec::new(),
    };
    assert_eq!(
        counts(&pooled(vec![
            measured(3, 3, &[]),
            measured(2, 1, &["pkg/test/it: sampled"])
        ])),
        Some((5, 4, 1)),
        "where every build measured it, the counts add"
    );
    assert!(
        matches!(
            pooled(vec![
                measured(3, 3, &[]),
                Column::Unmeasured {
                    why: "no baseline".to_owned()
                }
            ]),
            Column::Unmeasured { .. }
        ),
        "one build's unmeasured column is not hidden under another's counts"
    );
    assert_eq!(
        pooled(vec![measured(3, 3, &[]), Column::NotAsked]),
        Column::NotAsked
    );
}

#[test]
fn a_whole_contract_puts_every_fault_and_knob_and_refuses_a_document_that_says_not_to() {
    let path = std::path::Path::new(".njutest.toml");
    let whole = njutest::config::Config::parse("version = 1\ncontract = \"whole-v1\"\n", path)
        .expect("a whole contract parses");
    assert!(whole.faults.inject, "whole-v1 puts every fault");
    assert_eq!(
        whole.repeatable.knobs,
        njutest::report::knobs::Knob::ALL.to_vec(),
        "and every knob"
    );
    assert!(whole.durability.crash, "whole-v1 puts every crash");
    assert!(whole.schedules.explore > 0, "and explores schedules");
    for contradiction in [
        "[faults]\ninject = false\n",
        "[repeatable]\nknobs = [\"timezone\"]\n",
        "[durability]\ncrash = false\n",
        "[schedules]\nexplore = 0\n",
    ] {
        let refused = njutest::config::Config::parse(
            &format!("version = 1\ncontract = \"whole-v1\"\n{contradiction}"),
            path,
        );
        assert!(
            refused.is_err(),
            "a document that asks not to measure a dimension is not a whole run: {contradiction}"
        );
    }
}

#[test]
fn a_whole_contract_accepts_every_knob_named_in_any_order() {
    let path = std::path::Path::new(".njutest.toml");
    let mut every: Vec<&str> = njutest::report::knobs::Knob::ALL
        .into_iter()
        .map(njutest::report::knobs::Knob::name)
        .collect();
    every.reverse();
    let named = every
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<String>>()
        .join(", ");
    let parsed = njutest::config::Config::parse(
        &format!("version = 1\ncontract = \"whole-v1\"\n[repeatable]\nknobs = [{named}]\n"),
        path,
    );
    assert!(
        parsed.is_ok(),
        "a document naming every knob asks every knob, whatever order it names them in: {parsed:?}"
    );
}

const fn nothing() -> Evidence<'static> {
    Evidence {
        mutations: (1, Vec::new()),
        knobs: &[],
        faults: &[],
        crashes: &[],
        concurrency: &[],
        targets: &[],
        seams: &[],
        limitations: &[],
        findings: &[],
    }
}

#[test]
fn a_knob_this_machine_could_not_put_is_a_hole_and_one_no_machine_puts_is_not() {
    use njutest::report::knobs::{Knob, KnobRecord, NotPut, Standing};
    let knob = |knob: Knob, why: NotPut| KnobRecord {
        target: "pkg/test/it".to_owned(),
        knob,
        standing: Standing::NotPut { why },
    };
    let lacked = [
        knob(Knob::Locale, NotPut::LocaleMissing),
        knob(Knob::Timezone, NotPut::ZoneMissing),
    ];
    let repeatable = column(
        &Evidence {
            knobs: &lacked,
            ..nothing()
        },
        Dimension::Repeatable,
    );
    assert_eq!(
        counts(&repeatable),
        Some((2, 0, 2)),
        "another machine could put these, so a run on this one leaves them open: {repeatable:?}"
    );
    let inherent = [knob(Knob::Threads, NotPut::NotLibtest)];
    let repeatable = column(
        &Evidence {
            knobs: &inherent,
            ..nothing()
        },
        Dimension::Repeatable,
    );
    assert!(
        matches!(repeatable, Column::Unmeasured { .. }),
        "a column whose every record it cannot speak about measured nothing: {repeatable:?}"
    );
}

#[test]
fn a_column_that_put_nothing_does_not_read_as_measured() {
    let refused = [fault(FaultDecision::NotPut {
        diagnostic: "E0277".to_owned(),
    })];
    let faulted = column(
        &Evidence {
            faults: &refused,
            ..nothing()
        },
        Dimension::Fault,
    );
    assert!(
        matches!(faulted, Column::Unmeasured { .. }),
        "every fault refused is nothing measured: {faulted:?}"
    );
    assert!(
        matches!(
            column(&nothing(), Dimension::Wire),
            Column::NothingToAsk { .. }
        ),
        "a configuration that names no seam has no question to ask"
    );
    assert!(
        matches!(
            column(
                &Evidence {
                    mutations: (0, Vec::new()),
                    ..nothing()
                },
                Dimension::Mutation
            ),
            Column::NothingToAsk { .. }
        ),
        "a tree with nothing to mutate has nothing to ask"
    );
}

#[test]
fn a_run_whose_baseline_did_not_build_measured_no_dimension() {
    let failed = [njutest::report::Finding::new(
        njutest::report::FindingKind::BuildFailure,
        "demo",
        "error[E0425]: cannot find function `undefined_function` in this scope",
    )];
    let evidence = Evidence {
        mutations: (0, Vec::new()),
        findings: &failed,
        ..nothing()
    };
    for row in rows(&evidence) {
        assert!(
            matches!(row.column, Column::Unmeasured { .. }),
            "a tree that did not build was asked and measured nothing, which is neither nothing \
             to ask nor a dimension nobody asked about: {} is {:?}",
            row.dimension.name(),
            row.column
        );
    }
}

#[test]
fn a_place_a_run_chose_not_to_mutate_is_a_hole_and_one_no_run_can_mutate_is_not() {
    use rust_mutants::syntax::SkipReason;
    let skipped = [
        Limitation::new(
            SkipReason::Configured,
            "2 places were not mutated: configured",
        ),
        Limitation::new(
            SkipReason::MacroInvocation,
            "1 place was not mutated: macro-invocation",
        ),
    ];
    let mutated = column(
        &Evidence {
            limitations: &skipped,
            ..nothing()
        },
        Dimension::Mutation,
    );
    let Column::Measured {
        holes,
        speaks_not_about,
        ..
    } = &mutated
    else {
        panic!("a run that decided a mutation measured the column: {mutated:?}");
    };
    assert!(
        holes.iter().any(|hole| hole.contains("configured"))
            && !speaks_not_about
                .iter()
                .any(|class| class.contains("configured")),
        "a place the configuration passed over is one another run could mutate, so it is open: \
         {mutated:?}"
    );
    assert!(
        speaks_not_about
            .iter()
            .any(|class| class.contains("macro-invocation"))
            && !holes.iter().any(|hole| hole.contains("macro-invocation")),
        "a place no run of the engine can mutate is a class the column does not speak about: \
         {mutated:?}"
    );
}

fn drawn(matrix: Vec<njutest::report::matrix::Row>) -> String {
    use njutest::presentation::{Headline, Terminal, Told, human};
    human::draw(
        &Told {
            headline: Headline {
                verdict: njutest::report::Verdict::Insufficient,
                cataloged: 1,
                killed: 1,
                survived: 0,
                unreached: 0,
                step_limit_reached: 0,
                waited: 0,
                duration_ms: 1,
                kept: "runs/20260101T000000Z-aaaaaa".to_owned(),
            },
            places: Vec::new(),
            diagnostics: Vec::new(),
            limitations: Vec::new(),
            matrix,
        },
        Terminal::plain(200),
    )
}

#[test]
fn the_schedule_row_names_which_binary_is_open_and_why() {
    use njutest::report::concurrency::{ConcurrencyRecord, Exploration};
    let records = [
        ConcurrencyRecord {
            target: "pkg/test/sampled".to_owned(),
            standing: njutest::concurrency::proof::Standing::Concurrent {
                because: vec![njutest::concurrency::proof::Because::LooseReach],
            },
            explored: Exploration::Sampled {
                asked: 2,
                delayed: vec![0, 1],
            },
        },
        ConcurrencyRecord {
            target: "pkg/test/alone".to_owned(),
            standing: njutest::concurrency::proof::Standing::SingleThreaded,
            explored: Exploration::Unexplored {
                why: njutest::report::concurrency::Unexplored::NotNeeded,
            },
        },
    ];
    let evidence = Evidence {
        concurrency: &records,
        ..nothing()
    };
    let scheduled = column(&evidence, Dimension::Schedule);
    let said = format!("{scheduled:?}");
    assert!(
        said.contains("pkg/test/sampled") && !said.contains("pkg/test/alone"),
        "the row names the binary it leaves open, and only that one: {said}"
    );
    let drawing = drawn(rows(&evidence));
    assert!(
        drawing.contains("pkg/test/sampled") && drawing.contains("sample"),
        "the drawing says which binary is open and why:\n{drawing}"
    );
}

#[test]
fn the_drawing_says_what_a_dimension_does_not_speak_about() {
    use njutest::report::crashes::{CrashDecision, CrashRecord};
    let crashes = [CrashRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "d".repeat(64),
        display_id: "d".repeat(20),
        path: "src/lib.rs".to_owned(),
        item: "save".to_owned(),
        position: None,
        decision: CrashDecision::Restarted {
            on: "pkg/test/it::saves".to_owned(),
            left: vec!["state.json".to_owned()],
        },
        sealed: false,
    }];
    let evidence = Evidence {
        crashes: &crashes,
        ..nothing()
    };
    let drawing = drawn(rows(&evidence));
    assert!(
        drawing.contains("writes the system had not yet flushed to disk"),
        "a class a column cannot put is stated where the reader reads the column:\n{drawing}"
    );
}
