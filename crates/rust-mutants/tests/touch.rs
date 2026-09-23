// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The record the guards leave, read as data: what it refuses, and what it says about one test.

use std::collections::BTreeSet;

use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::touch::{self, Seen, TouchError};

const CATALOG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A log of `records`, with the header a runtime writes first.
fn log(records: &str) -> String {
    format!("{}{records}", touch::header_line(CATALOG))
}

fn set(indices: &[u32]) -> BTreeSet<u32> {
    indices.iter().copied().collect()
}

/// What one test reported and what nothing could be attributed to.
fn seen(named: &[(&str, &[u32])], loose: &[u32]) -> Seen {
    let mut seen = Seen::default();
    for (test, indices) in named {
        seen.tests
            .entry((*test).to_owned())
            .or_insert_with(|| set(indices));
    }
    seen.loose = set(loose);
    seen
}

#[test]
fn what_one_test_reported_is_not_what_another_did() {
    let seen = seen(&[("alpha", &[1, 2]), ("beta", &[3])], &[]);

    assert!(seen.by("alpha", 1), "alpha reported it");
    assert!(
        !seen.by("alpha", 3),
        "beta did, and that is not the same test"
    );
    assert!(!seen.by("beta", 1));
    assert!(
        !seen.by("gamma", 1),
        "a test the record does not name reported nothing"
    );
    assert!(
        seen.any(3),
        "though something of the target did, which is what a whole-target question asks"
    );
    assert!(!seen.any(4), "and nothing of it reported this one");
    assert_eq!(seen.who(1), ["alpha"], "by name, so a route can ask for it");
    assert!(seen.who(4).is_empty());
}

#[test]
fn a_report_nothing_could_attribute_is_one_every_test_made() {
    let seen = seen(&[("alpha", &[1])], &[9]);

    for test in ["alpha", "beta", "a test that never ran"] {
        assert!(
            seen.by(test, 9),
            "the record could not say which test made it, and the answer to not knowing is to \
             run more rather than fewer: {test}"
        );
    }
    assert!(seen.any(9));
    assert!(
        seen.who(9).is_empty(),
        "and no test is named as the one that made it"
    );
}

#[test]
fn the_last_site_the_catalog_holds_is_one_a_record_may_name() {
    let touches = touch::read(
        &log("t\talpha\t3\n"),
        CATALOG,
        touch::Bounds {
            mutants: 4,
            items: 0,
        },
    );
    assert_eq!(result_state(&touches), Returned, "log: {touches:?}");
    let Ok(touches) = touches else { return };
    assert_eq!(
        touches.reached.tests.get("alpha"),
        Some(&set(&[3])),
        "a catalog of four holds index three"
    );
    assert!(
        matches!(
            touch::read(
                &log("t\talpha\t4\n"),
                CATALOG,
                touch::Bounds {
                    mutants: 4,
                    items: 0
                }
            ),
            Err(TouchError::BeyondCatalog {
                index: 4,
                count: 4,
                ..
            })
        ),
        "and does not hold index four"
    );
}

#[test]
fn a_header_that_names_anything_but_one_catalog_says_nothing() {
    for rest in [
        format!("{CATALOG} {CATALOG}"),
        String::new(),
        format!("{CATALOG} extra"),
    ] {
        let text = format!("{} {rest}\nt\talpha\t0\n", touch::SCHEMA);
        assert!(
            matches!(
                touch::read(
                    &text,
                    CATALOG,
                    touch::Bounds {
                        mutants: 4,
                        items: 0
                    }
                ),
                Err(TouchError::Malformed { line: 1, .. })
            ),
            "a header is the schema and one catalog, and anything else is a header this run \
             cannot say is about it: {rest:?}"
        );
    }
}

#[test]
fn a_record_that_is_not_a_kind_a_thread_and_a_list_says_nothing() {
    for record in ["t\talpha\n", "t\talpha\t0\textra\n", "t\n", "\t\t\n"] {
        assert!(
            matches!(
                touch::read(
                    &log(record),
                    CATALOG,
                    touch::Bounds {
                        mutants: 4,
                        items: 0
                    }
                ),
                Err(TouchError::Malformed { line: 2, .. })
            ),
            "a record has three fields and this has not: {record:?}"
        );
    }
}

#[test]
fn a_site_that_is_not_a_number_says_nothing() {
    for indices in ["x", "0,x", "-1", "0,", "4294967296"] {
        let text = log(&format!("t\talpha\t{indices}\n"));
        assert!(
            matches!(
                touch::read(
                    &text,
                    CATALOG,
                    touch::Bounds {
                        mutants: 4,
                        items: 0
                    }
                ),
                Err(TouchError::Malformed { .. })
            ),
            "a list of sites is a list of numbers: {indices:?}"
        );
    }
}

#[test]
fn a_blank_line_is_not_the_end_of_the_log() {
    let touches = touch::read(
        &log("t\talpha\t0\n\nt\tbeta\t1\n"),
        CATALOG,
        touch::Bounds {
            mutants: 4,
            items: 0,
        },
    );
    assert_eq!(result_state(&touches), Returned, "log: {touches:?}");
    let Ok(touches) = touches else { return };
    assert_eq!(
        touches.reached.tests.get("beta"),
        Some(&set(&[1])),
        "a process that wrote an empty line wrote everything after it too"
    );
}

#[test]
fn the_header_a_runtime_writes_is_the_one_the_reader_wants() {
    let header = touch::header_line(CATALOG);
    assert_eq!(
        header,
        format!("{} {CATALOG}\n", touch::SCHEMA),
        "the generated runtime writes this line and this reader reads it, so it is one thing \
         said twice and this is where the two are held together"
    );
    let parsed = touch::read(
        &header,
        CATALOG,
        touch::Bounds {
            mutants: 4,
            items: 0,
        },
    );
    assert_eq!(
        result_state(&parsed),
        Returned,
        "a log of nothing but the header is a process whose guards never ran"
    );
}

#[test]
fn an_entered_item_is_bounded_by_the_item_catalog_and_not_by_the_mutants() {
    let bounds = touch::Bounds {
        mutants: 1,
        items: 3,
    };
    let touches = touch::read(&log("e\talpha\t2\ne\t-\t0\n"), CATALOG, bounds);
    assert_eq!(result_state(&touches), Returned, "log: {touches:?}");
    let Ok(touches) = touches else { return };
    assert_eq!(
        touches.entered,
        seen(&[("alpha", &[2])], &[0]),
        "item 2 is past the one mutant and within the three items, which is the catalog it names"
    );
    assert!(
        touches.reached == Seen::default(),
        "and an entry is not a site"
    );
    assert!(
        matches!(
            touch::read(&log("e\talpha\t3\n"), CATALOG, bounds),
            Err(TouchError::BeyondItems {
                index: 3,
                count: 3,
                line: 2
            })
        ),
        "the item catalog holds three"
    );
    assert!(
        matches!(
            touch::read(&log("t\talpha\t2\n"), CATALOG, bounds),
            Err(TouchError::BeyondCatalog { index: 2, .. })
        ),
        "and a site is still bounded by the mutants"
    );
}

/// A target whose three tests entered what `entered` says.
fn target(entered: Seen) -> touch::TargetTouches {
    let mut touches = touch::TargetTouches::default();
    touches.entered = entered;
    touches.ran = vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()];
    touches
}

#[test]
fn which_tests_entered_an_item_is_read_the_way_which_reached_a_site_is() {
    let touches = target(seen(&[("alpha", &[0, 1]), ("beta", &[1])], &[2]));
    assert_eq!(
        touches.entering(0),
        touch::Reaching::Tests(vec!["alpha".to_owned()])
    );
    assert_eq!(
        touches.entering(2),
        touch::Reaching::Whole,
        "an entry nothing could attribute is one every test made"
    );
    assert_eq!(touches.entering(3), touch::Reaching::Nothing);
    assert_eq!(touches.entered_by("beta"), set(&[1, 2]));
    assert_eq!(
        touches.entered_by("delta"),
        set(&[2]),
        "a test the record does not name entered what nothing could attribute"
    );
    assert_eq!(touches.entered_by_any(), set(&[0, 1, 2]));
}

#[test]
fn a_change_belongs_to_the_innermost_item_whose_body_holds_all_of_it() {
    let touched = serde_json::from_value::<touch::Touched>(serde_json::json!({
        "targets": {},
        "limitations": [],
        "items": [
            { "index": 0, "package": "demo", "path": "src/lib.rs", "name": "outer",
              "span": { "start": 0, "end": 100 }, "body": { "start": 10, "end": 100 },
              "measurable": true },
            { "index": 1, "package": "demo", "path": "src/lib.rs", "name": "outer::inner",
              "span": { "start": 20, "end": 50 }, "body": { "start": 30, "end": 50 },
              "measurable": true },
            { "index": 2, "package": "demo", "path": "src/other.rs", "name": "elsewhere",
              "span": { "start": 0, "end": 100 }, "body": { "start": 0, "end": 100 },
              "measurable": true }
        ]
    }));
    assert_eq!(result_state(&touched), Returned, "document: {touched:?}");
    let Ok(touched) = touched else { return };
    let at = |start, end| {
        touched
            .item_holding("src/lib.rs", rust_mutants::span::Span { start, end })
            .map(|item| item.name.as_str())
    };
    assert_eq!(at(35, 40), Some("outer::inner"));
    assert_eq!(at(60, 70), Some("outer"));
    assert_eq!(
        at(45, 60),
        Some("outer"),
        "a change that leaves the inner body is a change of the outer one"
    );
    assert_eq!(
        at(22, 25),
        Some("outer"),
        "a change to the inner item's signature is a change of the body it sits in"
    );
    assert_eq!(at(0, 5), None, "the signature of an item is in no body");
}
