// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a claim holds: a `cfg` predicate over the names a target alone decides, judged against what the toolchain prints for that target (ADR 0042).

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants::facts::{Facts, Predicate, WhereError};

/// What `rustc --print cfg --target x86_64-unknown-linux-gnu` prints, in its own order, with a name a build's profile decides among them.
const LINUX: &str = "debug_assertions\npanic=\"unwind\"\ntarget_arch=\"x86_64\"\ntarget_endian=\"little\"\ntarget_env=\"gnu\"\ntarget_family=\"unix\"\ntarget_has_atomic=\"64\"\ntarget_has_atomic=\"ptr\"\ntarget_os=\"linux\"\ntarget_pointer_width=\"64\"\ntarget_vendor=\"unknown\"\nunix\n";

fn holds(predicate: &str) -> bool {
    Predicate::parse(predicate)
        .expect("every predicate this test judges is one")
        .holds(&Facts::printed(LINUX))
}

#[test]
fn a_predicate_is_judged_against_what_the_target_prints() {
    assert!(holds("unix"));
    assert!(holds("target_os = \"linux\""));
    assert!(holds("all(unix, target_pointer_width = \"64\")"));
    assert!(holds("not(windows)"));
    assert!(holds("target_has_atomic = \"ptr\""));
    assert!(!holds("any(target_os = \"macos\", windows)"));
    assert!(!holds("target_os = \"macos\""));
    assert!(
        holds("all()") && !holds("any()"),
        "an empty all holds and an empty any does not, as in Cargo"
    );
}

#[test]
fn a_name_the_target_alone_does_not_decide_is_refused_by_name() {
    for (predicate, name) in [
        ("debug_assertions", "debug_assertions"),
        ("test", "test"),
        ("feature = \"serde\"", "feature"),
        ("all(unix, target_feature = \"avx2\")", "target_feature"),
        ("not(my_cfg)", "my_cfg"),
    ] {
        match Predicate::parse(predicate) {
            Err(WhereError::Undecided { name: refused }) => assert_eq!(
                refused, name,
                "{predicate}: the refusal names the fact nobody measured"
            ),
            other => panic!(
                "{predicate}: a predicate over a fact a probe of the target cannot report is a \
                 claim nobody can check, so it is refused rather than judged: {other:?}"
            ),
        }
    }
}

#[test]
fn a_predicate_that_does_not_parse_is_refused_where_it_stops() {
    for predicate in [
        "",
        "target_os = linux",
        "all(unix",
        "unix)",
        "unix windows",
        "target_os = \"linux",
        "not(unix, windows)",
    ] {
        assert!(
            matches!(
                Predicate::parse(predicate),
                Err(WhereError::Unparsable { .. })
            ),
            "{predicate:?} is not a cfg predicate"
        );
    }
}

#[test]
fn the_facts_a_run_records_are_the_target_s_alone_and_sorted() {
    let facts = Facts::printed(LINUX);
    let names = facts.recorded();
    assert!(
        !names.iter().any(|fact| fact == "debug_assertions"),
        "a profile's name is not a fact of the target: {names:?}"
    );
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "the audit re-evaluates from exactly this list"
    );
    assert!(
        names.contains(&"target_os=\"linux\"".to_owned()),
        "{names:?}"
    );
    assert!(names.contains(&"unix".to_owned()), "{names:?}");
    assert_eq!(
        Facts::recorded_back(&names),
        Some(facts),
        "what a report records reads back as the facts it was judged against"
    );
}

#[test]
fn a_predicate_reads_back_as_itself_from_what_it_writes() {
    for text in [
        "unix",
        "target_os = \"linux\"",
        "all(unix, not(target_os = \"macos\"))",
        "any()",
    ] {
        let predicate = Predicate::parse(text).expect("a predicate");
        assert_eq!(
            Predicate::parse(&predicate.to_string()),
            Ok(predicate),
            "{text}: a report names the predicate that did not hold, and an audit reads it back"
        );
    }
}
