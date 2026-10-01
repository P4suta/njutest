// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{
    Asked, Configured, FAILURE_STATUS, Own, Unaccounted, harness_report, harness_reports, listing,
};

const PASSED: &str = "\nrunning 2 tests\ntest a ... ok\ntest b ... ok\n\n\
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";

const FAILED: &str = "\nrunning 2 tests\ntest a ... ok\ntest b ... FAILED\n\nfailures:\n\n\
    ---- b stdout ----\n\nthread 'b' panicked at src/lib.rs:9:5:\nassertion failed\n\n\n\
    failures:\n    b\n\n\
    test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn an_ordinary_run_is_accounted_for_with_the_counts_its_harness_announced() {
    let said = harness_report(PASSED.as_bytes(), Asked::Whole, Some(0));
    let Ok(said) = said else {
        panic!("an ordinary passing run is accounted for: {said:?}");
    };
    assert_eq!(said.announced, Some(2));
    assert_eq!(said.summary.passed, 2);
    assert!(said.failed.is_empty());
}

#[test]
fn a_failing_run_names_the_tests_its_closing_list_names() {
    let said = harness_report(FAILED.as_bytes(), Asked::Whole, Some(FAILURE_STATUS));
    let Ok(said) = said else {
        panic!("a failing run is accounted for: {said:?}");
    };
    assert_eq!(said.failed, names(&["b"]));
}

#[test]
fn a_run_whose_process_ended_before_its_summary_is_unfinished() {
    let exited_early = "\nrunning 2 tests\ntest a ... ok\n";
    assert_eq!(
        harness_report(exited_early.as_bytes(), Asked::Whole, Some(0)),
        Err(Unaccounted::Unfinished)
    );
}

#[test]
fn a_nested_report_left_last_does_not_close_the_run_that_announced_more() {
    let nested = "\nrunning 2 tests\ntest a ... ok\n\nrunning 1 test\ntest inner ... ok\n\n\
        test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(
        harness_report(nested.as_bytes(), Asked::Whole, None),
        Err(Unaccounted::CountsDisagree {
            announced: 2,
            accounted: 1
        })
    );
}

#[test]
fn a_nested_report_in_the_middle_is_passed_over_for_the_harnesses_own() {
    let nested = "\nrunning 2 tests\n\nrunning 1 test\ntest inner ... ok\n\n\
        test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
        test a ... ok\ntest b ... ok\n\n\
        test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    let said = harness_report(nested.as_bytes(), Asked::Whole, Some(0));
    let Ok(said) = said else {
        panic!("the harness's own report closes the run: {said:?}");
    };
    assert_eq!(said.summary.passed, 2);
}

#[test]
fn an_exit_status_the_summary_does_not_explain_is_refused() {
    assert_eq!(
        harness_report(PASSED.as_bytes(), Asked::Whole, Some(1)),
        Err(Unaccounted::ExitContradicts { code: 1 })
    );
    assert_eq!(
        harness_report(FAILED.as_bytes(), Asked::Whole, Some(0)),
        Err(Unaccounted::ExitContradicts { code: 0 })
    );
}

#[test]
fn a_failure_the_run_did_not_ask_for_is_refused() {
    let foreign = "\nrunning 1 test\ntest a ... FAILED\n\nfailures:\n    z\n\n\
        test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
    assert_eq!(
        harness_report(
            foreign.as_bytes(),
            Asked::Exact(&names(&["a"])),
            Some(FAILURE_STATUS)
        ),
        Err(Unaccounted::FailuresDisagree {
            named: 1,
            failed: 1
        })
    );
}

#[test]
fn a_selection_the_harness_announced_differently_is_refused() {
    assert_eq!(
        harness_report(
            PASSED.as_bytes(),
            Asked::Exact(&names(&["a", "b", "c"])),
            Some(0)
        ),
        Err(Unaccounted::SelectionDisagrees {
            asked: 3,
            announced: 2
        })
    );
}

#[test]
fn lines_that_are_not_text_are_lost_and_nothing_else() {
    let mut output = b"\nrunning 2 tests\ntest a ... ok\n".to_vec();
    output.extend_from_slice(&[0xff, 0xfe, b'\n']);
    output.extend_from_slice(
        b"test b ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    let said = harness_report(&output, Asked::Whole, Some(0));
    let Ok(said) = said else {
        panic!("a line that is not text is lost, not fatal: {said:?}");
    };
    assert_eq!(said.announced, Some(2));
}

#[test]
fn a_run_whose_output_kept_only_its_tail_is_judged_by_its_summary() {
    let truncated = format!(
        "{}: the process produced 9 bytes, only the tail is kept\ntest b ... ok\n\n\
         test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        crate::runner::OUTPUT_TRUNCATED_PREFIX
    );
    let said = harness_report(truncated.as_bytes(), Asked::Whole, Some(0));
    assert!(
        matches!(&said, Ok(accounted) if accounted.announced.is_none()),
        "{said:?}"
    );
}

#[test]
fn a_summary_without_an_announcement_in_whole_output_is_unannounced() {
    let unannounced = "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(
        harness_report(unannounced.as_bytes(), Asked::Whole, Some(0)),
        Err(Unaccounted::Unannounced)
    );
}

#[test]
fn a_run_that_ran_nothing_is_accounted_for_as_nothing() {
    let nothing = "\nrunning 0 tests\n\n\
        test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
    let said = harness_report(nothing.as_bytes(), Asked::Whole, Some(0));
    assert!(
        matches!(&said, Ok(accounted) if accounted.summary.ran_nothing()),
        "{said:?}"
    );
}

#[test]
fn an_ok_summary_with_a_failure_contradicts_itself() {
    let contradiction = "\nrunning 1 test\n\nfailures:\n    a\n\n\
        test result: ok. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(
        harness_report(contradiction.as_bytes(), Asked::Whole, Some(0)),
        Err(Unaccounted::VerdictContradicts)
    );
}

#[test]
fn a_should_panic_test_libtest_ignores_on_wasm_is_accounted_for_as_ignored() {
    let ignored = "\nrunning 1 test\ntest t - should panic ... ignored\n\n\
        test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 4 filtered out; finished in 0.00s\n";
    let said = harness_report(ignored.as_bytes(), Asked::Exact(&names(&["t"])), Some(0));
    assert!(
        matches!(&said, Ok(accounted) if accounted.summary.ignored == 1 && accounted.summary.passed == 0),
        "{said:?}"
    );
}

#[test]
fn a_listing_is_the_names_its_closing_count_accounts_for() {
    let listed = "tests::a: test\ntests::b: test\nbench::c: benchmark\n\n2 tests, 1 benchmark\n";
    assert_eq!(
        listing(listed.as_bytes()),
        Some(names(&["tests::a", "tests::b"])),
        "every test the harness listed, and nothing it listed that is not a test"
    );
    assert_eq!(
        listing(b"\n0 tests, 0 benchmarks\n"),
        Some(Vec::new()),
        "a harness that says it holds no test has listed nothing, which is an answer"
    );
    assert_eq!(
        listing(b"only: test\n\n1 test, 0 benchmarks\n"),
        Some(names(&["only"])),
        "libtest says one test in the singular"
    );
}

#[test]
fn a_listing_its_harness_did_not_close_lists_nothing() {
    for (unclosed, why) in [
        (
            "",
            "a harness that printed nothing, as one without libtest does, listed nothing",
        ),
        (
            "all the checks passed\n",
            "a harness that ignored --list and ran lists nothing",
        ),
        (
            "tests::a: test\ntests::b: test\n",
            "names with no closing count may be the head of a listing cut short",
        ),
        (
            "tests::a: test\n\n2 tests, 0 benchmarks\n",
            "a count the names do not come to is a listing that lost one",
        ),
        (
            "tests::a: test\nsomething else\n\n1 test, 0 benchmarks\n",
            "a line that is neither a test nor a benchmark is not libtest's",
        ),
        (
            "tests::a: test\n\n1 tests, 0 benchmarks\n",
            "libtest never writes one in the plural",
        ),
        (
            "tests::a: test\n\n1 testsuite, 0 benchmarks\n",
            "a word that only begins with the noun is another word",
        ),
    ] {
        assert_eq!(listing(unclosed.as_bytes()), None, "{why}: {unclosed:?}");
    }
}

const MERGED_THEN_ALONE: &str = "\nrunning 5 tests\ntest src/lib.rs - add (line 15) ... ignored\n\
    test src/lib.rs - add (line 11) - compile ... ok\ntest src/lib.rs - add (line 3) ... ok\n\
    test src/lib.rs - sub (line 26) ... ok\ntest src/lib.rs - add (line 7) - should panic ... ok\n\n\
    test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n\n\
    running 1 test\ntest src/lib.rs - add (line 19) - compile fail ... ok\n\n\
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s\n\n\
    all doctests ran in 0.18s; merged doctests compilation took 0.14s\n";

#[test]
fn rustdocs_reports_one_after_another_are_accounted_for_together() {
    let said = harness_reports(MERGED_THEN_ALONE.as_bytes(), Some(0));
    let Ok(said) = said else {
        panic!("a merged binary's report and rustdoc's own are one run: {said:?}");
    };
    assert_eq!(said.announced, Some(6));
    assert_eq!(
        (
            said.summary.passed,
            said.summary.ignored,
            said.summary.failed
        ),
        (5, 1, 0)
    );
    assert_eq!(
        harness_report(MERGED_THEN_ALONE.as_bytes(), Asked::Whole, Some(0)),
        Err(Unaccounted::CountsDisagree {
            announced: 5,
            accounted: 1
        }),
        "one test binary's output is one report, so the same output read as one is refused"
    );
}

#[test]
fn a_failure_in_any_of_rustdocs_reports_fails_the_run_and_is_named() {
    let failing = MERGED_THEN_ALONE
        .replace(
            "test src/lib.rs - add (line 3) ... ok",
            "test src/lib.rs - add (line 3) ... FAILED",
        )
        .replace(
            "test result: ok. 4 passed; 0 failed; 1 ignored",
            "failures:\n    src/lib.rs - add (line 3)\n\ntest result: FAILED. 3 passed; 1 failed; 1 ignored",
        );
    let said = harness_reports(failing.as_bytes(), Some(FAILURE_STATUS));
    let Ok(said) = said else {
        panic!("a failure one report names is the run's: {said:?}");
    };
    assert!(!said.summary.ok);
    assert_eq!(said.failed, names(&["src/lib.rs - add (line 3)"]));
    assert_eq!(
        harness_reports(failing.as_bytes(), Some(0)),
        Err(Unaccounted::ExitContradicts { code: 0 }),
        "rustdoc exits with the failure status where any report failed"
    );
}

#[test]
fn a_report_rustdoc_left_open_or_never_opened_is_refused() {
    let open = MERGED_THEN_ALONE.replace(
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s",
        "",
    );
    assert_eq!(
        harness_reports(open.as_bytes(), Some(0)),
        Err(Unaccounted::Unfinished)
    );
    let unopened = MERGED_THEN_ALONE.replace("running 1 test\n", "");
    assert_eq!(
        harness_reports(unopened.as_bytes(), Some(0)),
        Err(Unaccounted::Unannounced)
    );
    assert_eq!(harness_reports(b"", Some(0)), Err(Unaccounted::Unannounced));
}

#[test]
fn an_option_an_invocation_sets_itself_takes_the_place_of_the_configured_one_however_spelled() {
    let configured = Configured::new(names(&[
        "--test-threads=4",
        "--include-ignored",
        "--nocapture",
        "--test-threads",
        "2",
        "--show-output",
    ]));
    assert_eq!(
        configured.beside(&[Own::OneThread, Own::Uncaptured]),
        names(&[
            "--test-threads=1",
            "--nocapture",
            "--include-ignored",
            "--show-output"
        ]),
        "libtest refuses an option given twice, so the invocation's own is the only one given"
    );
    assert_eq!(
        configured.beside(&[Own::OneThread]),
        names(&[
            "--test-threads=1",
            "--include-ignored",
            "--nocapture",
            "--show-output"
        ]),
        "an option the invocation leaves alone is passed on as configured"
    );
    assert_eq!(
        configured.beside(&[]),
        names(&[
            "--test-threads=4",
            "--include-ignored",
            "--nocapture",
            "--test-threads",
            "2",
            "--show-output",
        ]),
        "an invocation that sets nothing passes on everything"
    );
}

#[test]
fn a_word_after_the_end_of_the_options_is_a_filter_and_never_an_option_given_twice() {
    let configured = Configured::new(names(&["--nocapture", "--", "--nocapture"]));
    assert_eq!(
        configured.beside(&[Own::Uncaptured]),
        names(&["--nocapture", "--", "--nocapture"]),
    );
}

#[test]
fn every_option_an_invocation_sets_is_one_libtest_takes_once() {
    for own in Own::ALL {
        let configured = Configured::new(names(&[own.spelled()]));
        assert_eq!(
            configured.beside(&[own]),
            names(&[own.spelled()]),
            "{own:?} configured and set is given once"
        );
    }
}
