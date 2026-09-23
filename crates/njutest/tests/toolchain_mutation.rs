// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phase the program exists for, end to end.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
#![cfg_attr(
    unix,
    expect(
        clippy::format_push_string,
        clippy::indexing_slicing,
        clippy::disallowed_methods,
        reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table, and what these permit is what a test that reads a published report needs, which this platform cannot publish: those tests are behind cfg(unix) one by one, so what their shapes permit is behind it too"
    )
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-mutation-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
    args.extend_from_slice(extra);
    asked(&of(&fixture.root, &[]), &args)
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, what a test named, and nothing else.
fn environment(root: &Path, cache: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[]);
    for (name, value) in named {
        vars.push((OsString::from(*name), OsString::from(*value)));
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

#[cfg(unix)]
fn document_text(fixture: &Fixture) -> String {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    std::fs::read_to_string(path).expect("the document")
}

#[cfg(unix)]
fn document(fixture: &Fixture) -> serde_json::Value {
    let whole: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&document_text(fixture)).expect("JSON");
    assert_eq!(whole["document_type"], "complete", "{whole}");
    whole["report"].clone()
}

#[cfg(unix)]
fn part(fixture: &Fixture) -> serde_json::Value {
    document(fixture)["builds"][0]["parts"][0].clone()
}

#[cfg(unix)]
fn verdict(fixture: &Fixture) -> &'static str {
    njutest::report::json::parse(&document_text(fixture))
        .expect("the report reads back")
        .verdict()
        .name()
}

#[cfg(unix)]
#[test]
fn a_suite_that_notices_every_change_is_assured() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let report = part(&fixture);
    assert_eq!(verdict(&fixture), "ASSURED");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 4);
    assert_eq!(mutants["executed"], 4);
    assert_eq!(mutants["killed"], 4);
    assert_eq!(mutants["survived"], 0);
    assert_eq!(mutants["unreached"], 0);
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);

    for mutant in report["mutants"].as_array().expect("mutants") {
        assert_eq!(mutant["decision"]["outcome"], "killed", "{mutant}");
        assert!(
            mutant["decision"]["killed_by"]
                .as_str()
                .is_some_and(|by| !by.is_empty()),
            "a kill names the test that noticed: {mutant}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_gap_the_suite_cannot_see_is_insufficient_and_named() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let report = part(&fixture);
    assert_eq!(verdict(&fixture), "INSUFFICIENT");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 14);
    assert_eq!(mutants["killed"], 10);
    assert_eq!(
        mutants["survived"], 3,
        "what the ignored test would have caught, said three ways: both boundaries, and \
         the whole third branch nothing ever reaches while `sign(0)` is not asked for"
    );

    let held = document(&fixture);
    let findings = held["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings");
    assert_eq!(findings.len(), 4);
    let rules: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["decision"]["outcome"] == "survived")
        .filter_map(|mutant| mutant["rule"].as_str())
        .collect();
    assert_eq!(
        rules,
        ["gt-to-ge", "condition-to-true", "lt-to-le"],
        "the two sides of the zero nobody tests, and the branch on the far side of them \
         that nothing reaches at all while it goes untested"
    );
    for finding in findings {
        assert_eq!(finding["kind"], "surviving-mutant");
        assert!(
            finding["position"]["line"].as_u64().unwrap_or_default() > 0,
            "a finding names where to look: {finding}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_mutant_a_reviewer_accepted_stops_being_a_finding() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivors: Vec<String> = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| {
            mutant["decision"]["outcome"] == "survived"
                || mutant["decision"]["outcome"] == "unreached"
        })
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert_eq!(survivors.len(), 4);

    let mut configuration = String::from("version = 1\n");
    for id in &survivors {
        let prefix = id.get(..12).expect("a long unique prefix");
        configuration.push_str(&format!(
            "\n[[acceptance]]\nid = \"{prefix}\"\nreason = \"the boundary is checked by an ignored test\"\n"
        ));
    }
    std::fs::write(fixture.root.join(".njutest.toml"), configuration).expect("a configuration");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an accepted survivor is a decision somebody made, not a gap: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = part(&fixture);
    assert_eq!(verdict(&fixture), "ASSURED");
    assert_eq!(report["accounting"]["mutants"]["accepted"], 4);
    assert_eq!(
        report["accounting"]["mutants"]["survived"], 3,
        "an acceptance does not rewrite what was measured"
    );
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);
}

#[cfg(unix)]
#[test]
fn an_acceptance_that_names_no_single_catalog_entry_suppresses_nothing() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[[acceptance]]\nid = \"not-a-mutant\"\nreason = \"stale review\"\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unmatched acceptance is an insufficiency, not a successful suppression: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = part(&fixture);
    assert_eq!(verdict(&fixture), "INSUFFICIENT");
    assert_eq!(report["accounting"]["mutants"]["accepted"], 0);
    let held = document(&fixture);
    let global = held["global_findings"]
        .as_array()
        .expect("run-wide findings");
    let unmatched: Vec<&serde_json::Value> = global
        .iter()
        .filter(|finding| finding["kind"] == "unmatched-acceptance")
        .collect();
    assert_eq!(unmatched.len(), 1, "{global:?}");
    assert_eq!(unmatched[0]["subject"], "not-a-mutant");
    assert_eq!(
        held["builds"][0]["parts"][0]["findings"]
            .as_array()
            .expect("part findings")
            .iter()
            .filter(|finding| finding["kind"] == "surviving-mutant")
            .count(),
        4,
        "every survivor remains visible: {held:?}"
    );
}

#[cfg(unix)]
#[test]
fn every_mutant_is_routed_to_the_tests_that_reach_it_and_no_others() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(0));

    let recording = std::fs::read_dir(fixture.root.join(".njutest/trace"))
        .expect("the trace directory")
        .map(|entry| entry.expect("every trace entry is readable"))
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let events = njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(recording.join(njutest::trace::FILE_NAME)).expect("the stream"),
    ))
    .expect("the events");

    let routes: Vec<&njutest::trace::RouteRecord> = events
        .iter()
        .filter_map(|event| njutest::testkit::payload::of(&event.payload).route())
        .collect();
    assert_eq!(routes.len(), 4, "one route per mutant: {routes:?}");
    for route in &routes {
        assert_eq!(
            route.granularity,
            rust_mutants::session::Granularity::Block,
            "{route:?}"
        );
        assert_eq!(
            route.reaching.len(),
            1,
            "each mutation is reached by exactly the one test that covers it: {route:?}"
        );
        assert!(
            route.discharged.is_empty(),
            "nothing here carries a proof that would remove a test: {route:?}"
        );
    }
}

#[cfg(unix)]
fn njutest(fixture: &Fixture, args: &[&str]) -> Output {
    asked(&of(&fixture.root, &[]), args)
}

#[cfg(unix)]
fn survivors(fixture: &Fixture) -> Vec<String> {
    named(fixture, &["survived"])
}

#[cfg(unix)]
fn unanswered(fixture: &Fixture) -> Vec<String> {
    named(fixture, &["survived", "unreached"])
}

#[cfg(unix)]
fn named(fixture: &Fixture, outcomes: &[&str]) -> Vec<String> {
    part(fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| {
            outcomes
                .iter()
                .any(|outcome| mutant["decision"]["outcome"] == *outcome)
        })
        .filter_map(|mutant| mutant["display_id"].as_str().map(ToOwned::to_owned))
        .collect()
}

#[cfg(unix)]
#[test]
fn explain_says_everything_the_run_recorded_about_one_mutant() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivor = survivors(&fixture).first().cloned().expect("a survivor");

    let output = njutest(&fixture, &["explain", &survivor]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(text.contains("MUTANT\t"), "{text}");
    assert!(text.contains("WHERE\tsrc/lib.rs:"), "{text}");
    assert!(text.contains("DECISION\tunnoticed"), "{text}");
    assert!(text.contains("FINDING\tsurviving-mutant"), "{text}");
}

#[cfg(unix)]
#[test]
fn explain_refuses_a_prefix_that_names_more_than_one() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let output = njutest(&fixture, &["explain", ""]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(stderr.contains("names 14 mutants"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn accept_records_the_decision_where_the_next_run_will_read_it() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let names = unanswered(&fixture);
    assert_eq!(names.len(), 4);

    for name in &names {
        let output = njutest(
            &fixture,
            &[
                "accept",
                name,
                "--reason",
                "an ignored test covers this boundary",
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
    }
    let written =
        std::fs::read_to_string(fixture.root.join(".njutest.toml")).expect("a configuration");
    assert_eq!(written.matches("[[acceptance]]").count(), 4, "{written}");
    assert!(
        written.contains("an ignored test covers this boundary"),
        "{written}"
    );

    assert_eq!(
        verify(&fixture, &[]).status.code(),
        Some(0),
        "the decision the reviewer recorded is the one the next run reads"
    );
}

#[cfg(unix)]
#[test]
fn accept_refuses_a_mutant_that_did_not_survive() {
    let fixture = fixture("fixture-assured");
    verify(&fixture, &[]);
    let killed = part(&fixture)["mutants"].as_array().expect("mutants")[0]["display_id"]
        .as_str()
        .expect("a mutant")
        .to_owned();

    let output = njutest(&fixture, &["accept", &killed, "--reason", "no"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        stderr.contains("only a mutation nothing noticed is a decision to accept"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn what_a_run_concludes_does_not_depend_on_how_many_workers_measured_it() {
    let alone = fixture("fixture-baseline");
    std::fs::write(
        alone.root.join(".njutest.toml"),
        "version = 1\n\n[execution]\njobs = 1\n",
    )
    .expect("a configuration");
    verify(&alone, &[]);

    let together = fixture("fixture-baseline");
    std::fs::write(
        together.root.join(".njutest.toml"),
        "version = 1\n\n[execution]\njobs = 4\n",
    )
    .expect("a configuration");
    verify(&together, &[]);

    assert_eq!(
        njutest_devkit::report::normalize(&document(&alone)),
        njutest_devkit::report::normalize(&document(&together)),
        "measuring two mutations at once changes which processes overlap and nothing a report \
         says"
    );
}

#[cfg(unix)]
#[test]
fn accept_records_a_mutation_no_test_reaches() {
    let fixture = fixture("fixture-unreached");
    verify(&fixture, &[]);
    let found = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["decision"]["outcome"] == "unreached")
        .cloned()
        .expect("a mutation no test reaches");
    let unreached = found["display_id"]
        .as_str()
        .expect("a name for it")
        .to_owned();
    let rule = found["rule"].as_str().expect("the rule").to_owned();

    let output = njutest(
        &fixture,
        &[
            "accept",
            &unreached,
            "--reason",
            "nothing reaches it and that is the decision",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "a mutation nothing reached raises the same finding as one every reaching test passed, \
         so it is a decision a reviewer can record: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let written = std::fs::read_to_string(fixture.root.join(".njutest.toml")).expect("the file");
    assert!(
        written.contains(&rule) && written.contains("src/lib.rs"),
        "an acceptance names where the mutation is rather than the identity the next \
         edit re-mints: {written}"
    );
}

#[cfg(unix)]
#[test]
fn accept_keeps_the_comments_of_the_file_it_edits() {
    let fixture = fixture("fixture-baseline");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n# a note the maintainer left\ncontract = \"standard-v1\"\n",
    )
    .expect("a configuration");
    verify(&fixture, &[]);
    let survivor = survivors(&fixture).first().cloned().expect("a survivor");
    njutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);

    let written =
        std::fs::read_to_string(fixture.root.join(".njutest.toml")).expect("a configuration");
    assert!(
        written.contains("# a note the maintainer left"),
        "an edit that ate the comments would be an edit nobody trusts: {written}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutant_no_test_reaches_that_a_reviewer_accepted_is_counted_as_accepted() {
    let fixture = fixture("fixture-unreached");
    verify(&fixture, &[]);
    let unreached: Vec<String> = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["decision"]["outcome"] == "unreached")
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        !unreached.is_empty(),
        "the fixture exists to have a mutation no measured test reaches"
    );

    let mut configuration = String::from("version = 1\n");
    for id in &unreached {
        configuration.push_str(&format!(
            "\n[[acceptance]]\nid = \"{id}\"\nreason = \"nothing reaches it and that is the \
             decision\"\n"
        ));
    }
    std::fs::write(fixture.root.join(".njutest.toml"), configuration).expect("a configuration");

    verify(&fixture, &[]);
    let report = part(&fixture);
    let counted = report["accounting"]["mutants"]["accepted"]
        .as_u64()
        .expect("a count");
    assert_eq!(
        counted,
        u64::try_from(unreached.len()).unwrap_or(u64::MAX),
        "a mutant whose finding an acceptance removed is one the accounting says was \
         accepted: {report}"
    );
}

/// The environment of a fixture, with the cache and the scratch beside its root.
fn of(root: &Path, named: &[(&str, &str)]) -> Environment {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}

#[cfg(unix)]
#[test]
fn a_test_that_writes_into_the_tree_while_it_is_measured_is_said_to_have_done_so() {
    let fixture = fixture("fixture-writes-tree");
    let output = verify(&fixture, &[]);
    assert!(
        output.status.code().is_some_and(|code| code <= 2),
        "the run establishes something: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let report = part(&fixture);
    let named: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        named.contains(&njutest::limitation::TREE_WRITTEN_DURING_MEASUREMENT),
        "a test wrote into the tree, so every mutation measured after it was measured \
         against what it wrote rather than against the tree the report names: a run that \
         did not say so reads as a measurement of the workspace. {named:?}"
    );
    let detail = report["limitations"]
        .as_array()
        .and_then(|all| {
            all.iter()
                .find(|one| one["name"] == njutest::limitation::TREE_WRITTEN_DURING_MEASUREMENT)
        })
        .and_then(|one| one["detail"].as_str())
        .unwrap_or_default();
    assert!(
        detail.contains("wrote into the tree"),
        "and says what it means, because the name alone tells a reader nothing to do: \
         {detail}"
    );
}

#[cfg(unix)]
#[test]
fn a_suite_that_writes_nothing_says_nothing_about_a_tree_that_was_written_to() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = part(&fixture);
    let named: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        !named.contains(&njutest::limitation::TREE_WRITTEN_DURING_MEASUREMENT),
        "a limitation stated where it does not apply is one a reader stops believing: \
         {named:?}"
    );
}

#[cfg(unix)]
#[test]
fn recording_an_acceptance_keeps_the_configuration_a_person_wrote() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let path = fixture.root.join(".njutest.toml");
    std::fs::write(
        &path,
        "version = 1\n\n# the contract this project promises\ncontract = \"standard-v1\"\n",
    )
    .expect("a configuration somebody wrote");

    let found = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["decision"]["outcome"] == "survived"
                || mutant["decision"]["outcome"] == "unreached"
        })
        .cloned()
        .expect("a survivor");
    let survivor = found["id"].as_str().expect("its identity").to_owned();
    let rule = found["rule"].as_str().expect("the rule").to_owned();

    let recorded = njutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);
    assert_eq!(
        recorded.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&recorded.stderr)
    );

    let after = std::fs::read_to_string(&path).expect("the configuration");
    assert!(
        after.contains("# the contract this project promises"),
        "a command that rewrote the file would take the comments with it, and the \
         comments are the reviews of every survivor this project has looked at: {after}"
    );
    assert!(
        after.contains("standard-v1"),
        "and everything else the file said: {after}"
    );
    assert!(
        after.contains(&rule) && after.contains("reviewed"),
        "with the acceptance appended: {after}"
    );

    let again = njutest(
        &fixture,
        &["accept", &survivor, "--reason", "reviewed again"],
    );
    assert_eq!(
        again.status.code(),
        Some(0),
        "accepting what is already accepted is not a failure: a script that records a \
         decision twice has recorded it: {}",
        njutest_devkit::process::strict_utf8(&again.stderr)
    );
    assert!(
        njutest_devkit::process::strict_utf8(&again.stdout).contains("already accepted"),
        "and says so rather than saying it wrote one: {}",
        njutest_devkit::process::strict_utf8(&again.stdout)
    );
    let twice = std::fs::read_to_string(&path).expect("the configuration");
    assert_eq!(
        twice.matches("[[acceptance]]").count(),
        1,
        "and the file holds one, because two acceptances of one mutation are two \
         reviewers disagreeing with themselves: {twice}"
    );
    assert!(
        !twice.contains("reviewed again"),
        "the first reason stands: it is the one somebody wrote when they looked: {twice}"
    );
}

#[cfg(unix)]
#[test]
fn a_configuration_nobody_can_parse_is_refused_rather_than_rewritten() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let path = fixture.root.join(".njutest.toml");
    let broken = "version = 1\n[contract\nname = ]\n";
    let survivor = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["decision"]["outcome"] == "survived"
                || mutant["decision"]["outcome"] == "unreached"
        })
        .and_then(|mutant| mutant["display_id"].as_str())
        .expect("a survivor to answer for")
        .to_owned();
    std::fs::write(&path, broken).expect("a configuration nobody can parse");

    let refused = njutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a file this release could not read is one it must not write: appending to what \
         it could not parse would lose whatever it did not understand: {}",
        njutest_devkit::process::strict_utf8(&refused.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        broken,
        "and the file is exactly as it was"
    );
}

#[cfg(unix)]
#[test]
fn a_file_the_configuration_excludes_is_not_mutated_and_is_still_built_and_run() {
    let fixture = fixture("fixture-workspace");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[project]\nexclude = [\"crates/core/src/util.rs\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = part(&fixture);
    let paths: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|mutant| mutant["path"].as_str())
        .collect();

    assert!(
        !paths.contains(&"crates/core/src/util.rs"),
        "a pattern the configuration excludes takes the file out of the mutations, or it \
         narrows nothing and says it did: {paths:?} (exit {:?}, {})",
        output.status.code(),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        paths.contains(&"crates/core/src/lib.rs"),
        "and takes out nothing else: {paths:?}"
    );
    assert_eq!(
        document(&fixture)["scope"]["excluded"],
        serde_json::json!(["crates/core/src/util.rs"]),
        "the report says what was left out"
    );
    assert!(
        paths.contains(&"crates/app/src/main.rs"),
        "the excluded file is still compiled and still run — `total` calls into it, and a \
         tree without it would not have built at all: {paths:?}"
    );

    let stated: Vec<&str> = report["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|limitation| limitation["name"].as_str())
        .collect();
    assert!(
        stated.contains(&"skipped-excluded"),
        "and the report says how many places went unasked and why, because a place \
         nothing was put to is not a place the tests noticed everything about: {stated:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_features_the_configuration_turns_on_are_the_features_the_run_compiles() {
    let fixture = fixture("fixture-features");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[execution]\nfeatures = [\"imperial\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = part(&fixture);
    let unreached = report["accounting"]["mutants"]["unreached"]
        .as_u64()
        .unwrap_or_default();

    assert_eq!(
        unreached,
        0,
        "the second conversion is tested only by a module behind the `imperial` feature, \
         so a run that turned it on reaches every mutation and one that did not reaches \
         half of them. A configuration that names features and a build that compiles \
         without them measure two different programs: {} (exit {:?})",
        report["accounting"]["mutants"],
        output.status.code()
    );
}

#[test]
fn a_plan_is_about_the_run_the_configuration_describes() {
    let fixture = fixture("fixture-workspace");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[project]\npackages = [\"fixture-core\"]\n",
    )
    .expect("a configuration");

    let output = asked(&of(&fixture.root, &[]), &["plan", "--offline", "--locked"]);
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a plan that could not be made says why, and the test says it too, since an empty \
         plan and a plan that failed read the same from its stdout alone: {said}"
    );
    let targets: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("TARGET\t"))
        .collect();

    assert!(
        !targets.iter().any(|line| line.contains("fixture-app/")),
        "a plan says what a run would measure, and a run of this tree measures one \
         package because the configuration says so. A plan that reads none of the \
         configuration is a plan for a run nobody asked for: {text}{said}"
    );
    assert!(
        targets.iter().any(|line| line.contains("fixture-core/")),
        "and it still names the package that is in scope: {text}{said}"
    );
}

#[cfg(unix)]
#[test]
fn the_harness_arguments_the_configuration_writes_are_the_ones_the_suite_runs_with() {
    let plain = fixture("fixture-ignored");
    verify(&plain, &[]);
    let before = part(&plain);
    assert!(
        before["accounting"]["mutants"]["unreached"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "every test of this fixture is `#[ignore]`d, so its target runs nothing and its \
         mutations reach nothing: {}",
        before["accounting"]["mutants"]
    );

    let fixture = fixture("fixture-ignored");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[execution]\ntest_binary_args = [\"--include-ignored\"]\n",
    )
    .expect("a configuration");
    let output = verify(&fixture, &[]);
    let mutants = &part(&fixture)["accounting"]["mutants"];

    assert_eq!(
        mutants["unreached"],
        0,
        "and the one argument this configuration writes is the one that runs them. The \
         baseline is one run of this project's suite, so it runs the suite the way the \
         project does: a baseline taken one way and mutations measured another compares \
         two suites, and a mutation noticed by a test the baseline never ran is a kill \
         nothing vouched for: {mutants} (exit {:?})",
        output.status.code()
    );
    assert!(
        mutants["killed"].as_u64().unwrap_or_default() > 0,
        "with the tests that notice them now running: {mutants}"
    );
}

#[cfg(unix)]
#[test]
fn a_configured_target_is_left_out_by_name_and_the_report_says_so() {
    let fixture = fixture("fixture-workspace");
    let skipped = "fixture-app/test/cli";
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        format!("version = 1\n\n[execution]\nskip_targets = [{skipped:?}]\n"),
    )
    .expect("a configuration");

    let output = verify(&fixture, &["--trace"]);
    assert_ne!(
        output.status.code(),
        Some(3),
        "a declared target is a valid exclusion: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = part(&fixture);
    let limitation = report["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .find(|one| one["name"] == "target-skipped-by-configuration")
        .expect("the deliberate gap is visible in the durable report");
    assert!(
        limitation["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains(skipped)),
        "the limitation names what was omitted: {limitation}"
    );

    let run_id = document(&fixture)["run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();
    let recording = std::fs::read_dir(fixture.root.join(".njutest/trace"))
        .expect("the trace directory")
        .map(|entry| entry.expect("every trace entry is readable"))
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == run_id)
        })
        .expect("a recording for the run");
    let mut engine: Vec<PathBuf> = std::fs::read_dir(recording.join("builds"))
        .expect("the builds directory")
        .map(|entry| entry.expect("every build entry is readable"))
        .map(|entry| entry.path().join("engine").join(njutest::trace::FILE_NAME))
        .collect();
    engine.sort();
    let trace = std::fs::read_to_string(engine.first().expect("one engine recording"))
        .expect("the engine recording");
    let skipped_in_build = trace.lines().any(|line| {
        match njutest_devkit::strictjson::decode_str::<serde_json::Value>(line) {
            Ok(event) if event["payload"]["type"] == "build" => {
                event["payload"]["build"]["details"]
                    .as_array()
                    .cloned()
                    .is_some_and(|details| {
                        details.iter().any(|target| {
                            target["id"] == skipped
                                && target["limitations"].as_array().is_some_and(|names| {
                                    names
                                        .iter()
                                        .any(|name| name == "target-skipped-by-configuration")
                                })
                        })
                    })
            }
            Ok(_) | Err(_) => false,
        }
    });
    assert!(
        skipped_in_build,
        "the engine, not only the report projection, left out {skipped}: {trace}"
    );
}

#[test]
fn a_configured_skip_that_names_no_target_is_refused() {
    let fixture = fixture("fixture-assured");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[execution]\nskip_targets = [\"nobody/test/missing\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);

    assert_eq!(output.status.code(), Some(3));
    let error = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(error.contains("nobody/test/missing"), "{error}");
    assert!(
        error.contains("fixture-assured/lib/fixture_assured"),
        "the refusal names what can be configured instead: {error}"
    );
}

#[cfg(unix)]
#[test]
fn the_packages_the_configuration_names_are_the_packages_the_run_measures() {
    let fixture = fixture("fixture-workspace");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[project]\npackages = [\"fixture-core\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = part(&fixture);
    let paths: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|mutant| mutant["path"].as_str())
        .collect();

    assert!(
        !paths.iter().any(|path| path.starts_with("crates/app/")),
        "a run narrowed to one package measures that package. The verdict already reads \
         SCOPE_ASSURED because the configuration named a scope, so a run that measured \
         everything and said so is a narrower claim than the evidence, made about a \
         wider tree than the one it names: {paths:?} (exit {:?})",
        output.status.code()
    );
    assert!(
        paths.iter().any(|path| path.starts_with("crates/core/")),
        "and it does measure the one it names: {paths:?}"
    );
    assert_eq!(
        document(&fixture)["scope"]["resolved_packages"],
        serde_json::json!(["fixture-core"]),
        "which is what the report says it settled on"
    );
}

#[test]
fn a_package_the_configuration_names_that_nobody_wrote_is_refused() {
    let fixture = fixture("fixture-assured");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[project]\npackages = [\"nosuch\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a run narrowed to a package nobody wrote measured everything and called it \
         SCOPE_ASSURED, which is a green answer to a question about a package that is \
         not there. `njutest plan` has refused the same mistake all along: {}",
        njutest_devkit::process::strict_utf8(&output.stdout)
    );
}

#[cfg(unix)]
#[test]
fn an_acceptance_whose_expiry_has_passed_answers_for_nothing() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivors: Vec<String> = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| {
            mutant["decision"]["outcome"] == "survived"
                || mutant["decision"]["outcome"] == "unreached"
        })
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert_eq!(survivors.len(), 4);

    let mut configuration = String::from("version = 1\n");
    for id in &survivors {
        configuration.push_str(&format!(
            "\n[[acceptance]]\nid = \"{id}\"\nreason = \"the boundary is checked by an ignored test\"\nexpires = \"2020-01-01T00:00:00Z\"\n"
        ));
    }
    std::fs::write(fixture.root.join(".njutest.toml"), configuration).expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = part(&fixture);
    assert_eq!(
        report["accounting"]["mutants"]["accepted"],
        0,
        "an acceptance is a person saying they looked, and the expiry is when they said \
         to look again. One that has passed answers for nothing, or the date is a \
         comment: {} (exit {:?})",
        report["accounting"]["mutants"],
        output.status.code()
    );
    assert_eq!(
        report["findings"].as_array().expect("findings").len(),
        4,
        "and the findings it was hiding are back"
    );
}

#[cfg(unix)]
#[test]
fn accept_writes_the_expiry_it_is_given_and_a_run_reads_it() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivor = part(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["decision"]["outcome"] == "survived")
        .and_then(|mutant| mutant["id"].as_str())
        .expect("a survivor")
        .to_owned();

    let output = asked(
        &of(&fixture.root, &[]),
        &[
            "accept",
            &survivor,
            "--reason",
            "the boundary is checked by an ignored test",
            "--expires",
            "2020-01-01T00:00:00Z",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "an acceptance carries an expiry, and the command that writes acceptances is \
         where a person writes one: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let written = std::fs::read_to_string(fixture.root.join(".njutest.toml")).expect("the file");
    assert!(
        written.contains("expires = \"2020-01-01T00:00:00Z\""),
        "written where the next run reads it: {written}"
    );
}

/// What a command wrote to standard output.
#[cfg(unix)]
fn said(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
}

#[cfg(unix)]
#[test]
fn every_surface_that_prints_a_command_names_the_mutation_the_same_way() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let document = part(&fixture);
    let survivor = document["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|one| one["decision"]["outcome"] == "survived")
        .expect("a survivor")
        .clone();
    let hash = survivor["display_id"].as_str().expect("a display id");
    let locator = format!(
        "{}:{}:{}@{}",
        survivor["path"].as_str().unwrap_or_default(),
        survivor["item"].as_str().unwrap_or_default(),
        survivor["rule"].as_str().unwrap_or_default(),
        survivor["position"]["line"].as_u64().unwrap_or_default()
    );

    let mut hashed = Vec::new();
    for (surface, text) in [
        ("verify", said(&verify(&fixture, &["--format", "lines"]))),
        (
            "verify --format agent",
            said(&verify(&fixture, &["--format", "agent"])),
        ),
        ("explain", said(&njutest(&fixture, &["explain", &locator]))),
        (
            "report --format agent",
            said(&njutest(&fixture, &["report", "--format", "agent"])),
        ),
    ] {
        for line in text.lines().filter(|line| line.contains("njutest accept")) {
            if line.contains(hash) {
                hashed.push(format!("{surface}: {line}"));
            }
        }
    }
    assert!(
        hashed.is_empty(),
        "one run told a reader two names for one mutation. A name is what somebody types \
         back, and a tool that prints `{locator}` everywhere and `{hash}` in one place has \
         taught them that the readable one is not the real one. That is the defect \
         `naming::locator` exists to end, left in whichever surface did not go through \
         it:\n{}",
        hashed.join("\n")
    );
}
