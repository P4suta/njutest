// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Narrowing a run: which mutants it is about, when it stops, and what it would cost.

use std::ffi::OsString;
use std::process::Output;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn run(fixture: &Fixture, extra: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all"])
            .chain(["--offline", "--locked", "--no-coverage", "--jobs", "1"])
            .chain(extra.iter().copied())
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    mjutest_devkit::process::answered(code, out, err)
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The outcome of every mutant the lines named.
fn judged(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.strip_prefix('['))
        .filter_map(|line| line.split_once("] "))
        .filter_map(|(_, rest)| rest.split_once(' '))
        .map(|(id, rest)| {
            (
                id.to_owned(),
                rest.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_filtered_out_mutant_is_not_run_for_the_stated_reason_and_is_not_a_finding() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--ui", "plain", "--rule", "gt-to-ge"]);
    let text = said(&output);
    assert_eq!(
        judged(&text).len(),
        1,
        "only what the rule names ran: {text}"
    );
    assert!(
        text.contains("not_run=11"),
        "and the rest are accounted for rather than left out: {text}"
    );
    assert!(
        !text.contains("not-run-mutant"),
        "a mutant nobody selected is not a hole in the tests: {text}"
    );
    assert!(
        text.contains("discharged-mutant"),
        "a mutant nobody ran because a proof said running it establishes nothing is: {text}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the one that was selected is a gap in the tests, proved rather than measured: {text}"
    );
}

#[test]
fn a_family_a_file_and_an_identity_each_narrow_the_same_way() {
    let fixture = Fixture::copy("fixture-simple");
    let by_family = said(&run(&fixture, &["--ui", "plain", "--family", "comparison"]));
    assert_eq!(judged(&by_family).len(), 2, "{by_family}");
    let by_lines = said(&run(
        &fixture,
        &["--ui", "plain", "--file", "src/lib.rs:16-16"],
    ));
    assert_eq!(judged(&by_lines).len(), 6, "{by_lines}");
    let skipped = said(&run(
        &fixture,
        &["--ui", "plain", "--skip-family", "literal"],
    ));
    assert_eq!(judged(&skipped).len(), 8, "{skipped}");
    let bad = run(&fixture, &["--file", "src/lib.rs:nine"]);
    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("--file"),
        "a value a flag cannot take names the flag: {bad:?}"
    );
}

#[test]
fn fail_fast_stops_at_the_first_finding_and_states_why_the_rest_did_not_run() {
    let fixture = Fixture::copy("fixture-coverage");
    let output = run(&fixture, &["--ui", "plain", "--fail-fast"]);
    let text = said(&output);
    let judged = judged(&text);
    assert!(
        judged
            .iter()
            .any(|(_, outcome)| outcome == "survived" || outcome == "not_run"),
        "it stopped at something a reader has to act on — a mutation the tests did not \
         notice, or one a proof says they could not have: {text}"
    );
    assert!(
        judged.len() < 14,
        "and did not measure the rest: {} of 14",
        judged.len()
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        !text.contains("INTERRUPTED"),
        "a run that stopped because it was asked to is not one that was killed: {text}"
    );
}

#[test]
fn a_dry_run_says_what_it_would_cost_without_executing_a_mutant() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--dry-run"]);
    let text = said(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(
        text.contains("WOULD START"),
        "it says the size of the job in work rather than in time: {text}"
    );
    assert!(
        text.contains("(11 mutants against"),
        "it says how many mutants and how many targets: {text}"
    );
    assert!(
        text.contains("REMOVED BY"),
        "and what a proof already took off the bill: {text}"
    );
    assert!(
        text.lines().filter(|line| line.starts_with('#')).count() == 11,
        "and what each one is: {text}"
    );
    let roughly = text
        .lines()
        .position(|line| line.starts_with("ROUGHLY"))
        .expect("a guess at the time");
    let would = text
        .lines()
        .position(|line| line.starts_with("WOULD START"))
        .expect("a count of the work");
    assert!(
        would < roughly,
        "the count comes first and the guess about this machine comes last: {text}"
    );
    assert!(
        !text.contains("OUTCOMES"),
        "nothing was executed, so there is nothing to report: {text}"
    );
}

#[test]
fn a_scoped_dry_run_counts_the_whole_catalog_and_marks_unvalidated_candidates_unselected() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--rule", "gt-to-ge", "--dry-run"]);
    let text = said(&output);

    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(
        text.contains("(11 mutants against"),
        "the estimate retains the complete catalog denominator: {text}"
    );
    assert!(
        text.contains("unselected=10"),
        "the ten candidates omitted before compiler validation remain explicit: {text}"
    );
    assert_eq!(
        text.lines().filter(|line| line.starts_with('#')).count(),
        1,
        "only the selected candidate needs a route and a cost line: {text}"
    );
}

#[test]
fn from_report_reruns_what_the_last_run_left() {
    let fixture = Fixture::copy("fixture-coverage");
    let first = run(&fixture, &["--ui", "plain"]);
    assert_eq!(first.status.code(), Some(1), "{first:?}");
    let left = judged(&said(&first))
        .into_iter()
        .filter(|(_, outcome)| outcome == "survived")
        .count();
    assert!(left > 0, "the fixture leaves something to measure again");
    let again = run(&fixture, &["--ui", "plain", "--from-report", "--no-cache"]);
    let text = said(&again);
    assert_eq!(
        judged(&text).len(),
        left,
        "only what the last run left is measured again: {text}"
    );
}

#[test]
fn from_report_that_names_nothing_measures_nothing_rather_than_everything() {
    let fixture = Fixture::copy("fixture-simple");
    let first = run(&fixture, &["--ui", "quiet"]);
    assert_eq!(first.status.code(), Some(1), "{first:?}");
    let again = run(&fixture, &["--ui", "plain", "--from-report", "--no-cache"]);
    let text = said(&again);
    assert_eq!(
        judged(&text).len(),
        0,
        "a run whose survivors the last one has none of is a run with nothing to measure \
         again, not a run of the whole catalog: {text}"
    );
    assert!(
        text.contains("not_run=11"),
        "and every mutant says why it was not measured: {text}"
    );
    assert_eq!(again.status.code(), Some(0), "{text}");
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: mjutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

#[test]
fn a_file_the_workspace_does_not_hold_is_refused_rather_than_measured_as_empty() {
    let fixture = Fixture::copy("fixture-simple");

    let narrowed = run(&fixture, &["--file", "src/nosuch.rs", "--dry-run"]);
    assert_ne!(
        narrowed.status.code(),
        Some(0),
        "a run narrowed to a name nobody wrote measures nothing and reports that nothing \
         was missed, which is the one answer a person cannot tell from a clean one: {}",
        said(&narrowed)
    );
    let refusal = String::from_utf8_lossy(&narrowed.stderr).into_owned();
    assert!(
        refusal.contains("src/nosuch.rs") && refusal.contains("--file"),
        "and the refusal names the path and the flag: {refusal}"
    );

    let held = run(&fixture, &["--file", "src/lib.rs", "--dry-run"]);
    assert_eq!(
        held.status.code(),
        Some(0),
        "while a file the workspace holds is measured: {}",
        String::from_utf8_lossy(&held.stderr)
    );

    let lines = run(&fixture, &["--file", "src/nosuch.rs:1-3", "--dry-run"]);
    assert_ne!(
        lines.status.code(),
        Some(0),
        "and naming lines of a file that is not there is the same mistake: {}",
        said(&lines)
    );
}

#[test]
fn a_file_the_walk_found_nothing_in_is_still_a_file_it_read() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = "// SPDX-FileCopyrightText: 2026 mjutest contributors\n\
                 // SPDX-License-Identifier: MIT OR Apache-2.0\n\n\
                 //! Nothing here for a rule to target.\n";
    std::fs::write(fixture.root().join("src/nothing.rs"), empty).expect("a file with no candidate");
    let lib = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&lib).expect("the library");
    std::fs::write(&lib, format!("{source}\npub mod nothing;\n")).expect("the module declared");

    let narrowed = run(&fixture, &["--file", "src/nothing.rs", "--dry-run"]);
    assert_eq!(
        narrowed.status.code(),
        Some(0),
        "a file that is there and yields no candidate is not a mistake: the walk read it, \
         and `no candidate here` is a true answer about it: {}",
        String::from_utf8_lossy(&narrowed.stderr)
    );
}

#[test]
fn a_rule_or_family_this_release_does_not_know_is_refused() {
    let fixture = Fixture::copy("fixture-simple");

    for (flag, value) in [
        ("--rule", "nosuch-rule"),
        ("--skip-rule", "nosuch-rule"),
        ("--family", "nosuch-family"),
        ("--skip-family", "nosuch-family"),
    ] {
        let refused = run(&fixture, &[flag, value, "--dry-run"]);
        assert_ne!(
            refused.status.code(),
            Some(0),
            "{flag} {value} names nothing: narrowing to it measures nothing and reports \
             that nothing was missed, and skipping by it runs the rule a person meant to \
             pass over: {}",
            said(&refused)
        );
        let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
        assert!(
            refusal.contains(value) && refusal.contains(flag) && refusal.contains("rules"),
            "and the refusal names the flag, the value, and where the names are: {refusal}"
        );
    }

    let held = run(&fixture, &["--rule", "gt-to-ge", "--dry-run"]);
    assert_eq!(
        held.status.code(),
        Some(0),
        "while a rule this release knows narrows the run: {}",
        String::from_utf8_lossy(&held.stderr)
    );
    let by_family = run(&fixture, &["--family", "comparison", "--dry-run"]);
    assert_eq!(
        by_family.status.code(),
        Some(0),
        "and so does a family: {}",
        String::from_utf8_lossy(&by_family.stderr)
    );
}

#[test]
fn an_identity_that_names_no_mutation_of_the_catalog_is_refused() {
    let fixture = Fixture::copy("fixture-simple");

    let refused = run(&fixture, &["--id", "ffffffffffffffffffff", "--dry-run"]);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a run narrowed to an identity nothing holds measures nothing and reports that \
         nothing was missed, which is the answer a person who pasted a stale identity \
         cannot tell from the one they wanted: {}",
        said(&refused)
    );
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        refusal.contains("ffffffffffffffffffff") && refusal.contains("--id"),
        "and the refusal names the flag and the value: {refusal}"
    );

    let listed = run(&fixture, &["--dry-run"]);
    let held = said(&listed)
        .lines()
        .find(|line| line.starts_with('#'))
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("a dry run names each mutation it would ask about")
        .to_owned();
    let narrowed = run(&fixture, &["--id", &held, "--dry-run"]);
    assert_eq!(
        narrowed.status.code(),
        Some(0),
        "while an identity the catalog holds narrows the run: {}",
        String::from_utf8_lossy(&narrowed.stderr)
    );
}

#[test]
fn patterns_that_leave_no_file_to_read_are_refused() {
    let fixture = Fixture::copy("fixture-simple");

    let refused = run(&fixture, &["--include", "src/nosuch/**", "--dry-run"]);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a selection that removed every file measures nothing and scores as though \
         nothing was missed, which is what a mistyped pattern in a configuration looks \
         like for as long as nobody reads the file count: {}",
        said(&refused)
    );
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        refusal.contains("src/nosuch/**"),
        "and the refusal says which patterns left nothing: {refusal}"
    );

    let kept = run(&fixture, &["--include", "src/**", "--dry-run"]);
    assert_eq!(
        kept.status.code(),
        Some(0),
        "while patterns that leave a file are the narrowing a person asked for: {}",
        String::from_utf8_lossy(&kept.stderr)
    );
    let excluded = run(&fixture, &["--exclude", "tests/**", "--dry-run"]);
    assert_eq!(
        excluded.status.code(),
        Some(0),
        "and so is one that removes some files and leaves the rest: {}",
        String::from_utf8_lossy(&excluded.stderr)
    );
}
