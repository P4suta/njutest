// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phase the program exists for, end to end.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document by the names \
              its own fixture put there"
)]

use mjutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;

use mjutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-mutation-")
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
    let code = mjutest_cli::run_from(
        std::iter::once("mjutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    mjutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, what a test named, and nothing else.
fn environment(root: &Path, cache: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> = mjutest_devkit::paths::environment_for_a_run()
        .into_iter()
        .filter(|(name, _)| {
            matches!(
                name.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME"
            )
        })
        .collect();
    for (name, value) in named {
        vars.push((OsString::from(*name), OsString::from(*value)));
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: mjutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        vars,
        cancel: Cancel::new(),
    }
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index).expect("the index")).expect("JSON");
    let path = fixture
        .root
        .join(value["directory"].as_str().expect("a directory"))
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    serde_json::from_str(&std::fs::read_to_string(path).expect("the document")).expect("JSON")
}

#[test]
fn a_suite_that_notices_every_change_is_assured() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = document(&fixture);
    assert_eq!(report["verdict"], "ASSURED");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 4);
    assert_eq!(mutants["executed"], 4);
    assert_eq!(mutants["killed"], 4);
    assert_eq!(mutants["survived"], 0);
    assert_eq!(mutants["unreached"], 0);
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);

    for mutant in report["mutants"].as_array().expect("mutants") {
        assert_eq!(mutant["outcome"], "killed", "{mutant}");
        assert!(
            mutant["killed_by"]
                .as_str()
                .is_some_and(|by| !by.is_empty()),
            "a kill names the test that noticed: {mutant}"
        );
    }
}

#[test]
fn a_gap_the_suite_cannot_see_is_insufficient_and_named() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = document(&fixture);
    assert_eq!(report["verdict"], "INSUFFICIENT");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 10);
    assert_eq!(mutants["killed"], 7);
    assert_eq!(
        mutants["survived"], 2,
        "the two boundary mutations only the ignored test would have caught"
    );

    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 3);
    let rules: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["outcome"] == "survived")
        .filter_map(|mutant| mutant["rule"].as_str())
        .collect();
    assert_eq!(
        rules,
        ["gt-to-ge@1", "lt-to-le@1"],
        "the two sides of the zero nobody tests"
    );
    for finding in findings {
        assert_eq!(finding["kind"], "surviving-mutant");
        assert!(
            finding["position"]["line"].as_u64().unwrap_or_default() > 0,
            "a finding names where to look: {finding}"
        );
    }
}

#[test]
fn a_mutant_a_reviewer_accepted_stops_being_a_finding() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivors: Vec<String> = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["outcome"] == "survived" || mutant["outcome"] == "unreached")
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert_eq!(survivors.len(), 3);

    let mut configuration = String::from("version = 1\n");
    for id in &survivors {
        let _written = write!(
            configuration,
            "\n[[acceptance]]\nid = \"{id}\"\nreason = \"the boundary is checked by an ignored test\"\n"
        );
    }
    std::fs::write(fixture.root.join(".mjutest.toml"), configuration).expect("a configuration");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an accepted survivor is a decision somebody made, not a gap: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(report["verdict"], "ASSURED");
    assert_eq!(report["accounting"]["mutants"]["accepted"], 3);
    assert_eq!(
        report["accounting"]["mutants"]["survived"], 2,
        "an acceptance does not rewrite what was measured"
    );
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);
}

#[test]
fn every_mutant_is_routed_to_the_tests_that_reach_it_and_no_others() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(0));

    let recording = std::fs::read_dir(fixture.root.join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(recording.join(mjutest_cli::trace::FILE_NAME)).expect("the stream"),
    ))
    .expect("the events");

    let routes: Vec<&mjutest_cli::trace::RouteRecord> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Route { route } => Some(route),
            _ => None,
        })
        .collect();
    assert_eq!(routes.len(), 4, "one route per mutant: {routes:?}");
    for route in &routes {
        assert_eq!(route.granularity, "block", "{route:?}");
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

fn mjutest(fixture: &Fixture, args: &[&str]) -> Output {
    asked(&of(&fixture.root, &[]), args)
}

fn survivors(fixture: &Fixture) -> Vec<String> {
    named(fixture, &["survived"])
}

fn unanswered(fixture: &Fixture) -> Vec<String> {
    named(fixture, &["survived", "unreached"])
}

fn named(fixture: &Fixture, outcomes: &[&str]) -> Vec<String> {
    document(fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| outcomes.iter().any(|outcome| mutant["outcome"] == *outcome))
        .filter_map(|mutant| mutant["display_id"].as_str().map(ToOwned::to_owned))
        .collect()
}

#[test]
fn explain_says_everything_the_run_recorded_about_one_mutant() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivor = survivors(&fixture).first().cloned().expect("a survivor");

    let output = mjutest(&fixture, &["explain", &survivor]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("MUTANT\t"), "{text}");
    assert!(text.contains("WHERE\tsrc/lib.rs:"), "{text}");
    assert!(text.contains("OUTCOME\tsurvived"), "{text}");
    assert!(text.contains("FINDING\tsurviving-mutant"), "{text}");
}

#[test]
fn explain_refuses_a_prefix_that_names_more_than_one() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let output = mjutest(&fixture, &["explain", ""]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("names 10 mutants"), "{stderr}");
}

#[test]
fn accept_records_the_decision_where_the_next_run_will_read_it() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let names = unanswered(&fixture);
    assert_eq!(names.len(), 3);

    for name in &names {
        let output = mjutest(
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
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let written =
        std::fs::read_to_string(fixture.root.join(".mjutest.toml")).expect("a configuration");
    assert_eq!(written.matches("[[acceptance]]").count(), 3, "{written}");
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

#[test]
fn accept_refuses_a_mutant_that_did_not_survive() {
    let fixture = fixture("fixture-assured");
    verify(&fixture, &[]);
    let killed = document(&fixture)["mutants"].as_array().expect("mutants")[0]["display_id"]
        .as_str()
        .expect("a mutant")
        .to_owned();

    let output = mjutest(&fixture, &["accept", &killed, "--reason", "no"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("only a mutation nothing noticed is a decision to accept"),
        "{stderr}"
    );
}

#[test]
fn what_a_run_concludes_does_not_depend_on_how_many_workers_measured_it() {
    let alone = fixture("fixture-baseline");
    std::fs::write(
        alone.root.join(".mjutest.toml"),
        "version = 1\n\n[execution]\njobs = 1\n",
    )
    .expect("a configuration");
    verify(&alone, &[]);

    let together = fixture("fixture-baseline");
    std::fs::write(
        together.root.join(".mjutest.toml"),
        "version = 1\n\n[execution]\njobs = 4\n",
    )
    .expect("a configuration");
    verify(&together, &[]);

    assert_eq!(
        mjutest_devkit::report::normalize(&document(&alone)),
        mjutest_devkit::report::normalize(&document(&together)),
        "measuring two mutations at once changes which processes overlap and nothing a report \
         says"
    );
}

#[test]
fn accept_records_a_mutation_no_test_reaches() {
    let fixture = fixture("fixture-unreached");
    verify(&fixture, &[]);
    let unreached = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["outcome"] == "unreached")
        .and_then(|mutant| mutant["display_id"].as_str().map(ToOwned::to_owned))
        .expect("a mutation no test reaches");

    let output = mjutest(
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
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(fixture.root.join(".mjutest.toml")).expect("the file");
    assert!(written.contains(&unreached), "{written}");
}

#[test]
fn accept_keeps_the_comments_of_the_file_it_edits() {
    let fixture = fixture("fixture-baseline");
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        "version = 1\n\n# a note the maintainer left\ncontract = \"standard-v1\"\n",
    )
    .expect("a configuration");
    verify(&fixture, &[]);
    let survivor = survivors(&fixture).first().cloned().expect("a survivor");
    mjutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);

    let written =
        std::fs::read_to_string(fixture.root.join(".mjutest.toml")).expect("a configuration");
    assert!(
        written.contains("# a note the maintainer left"),
        "an edit that ate the comments would be an edit nobody trusts: {written}"
    );
}

#[test]
fn a_mutant_no_test_reaches_that_a_reviewer_accepted_is_counted_as_accepted() {
    let fixture = fixture("fixture-unreached");
    verify(&fixture, &[]);
    let unreached: Vec<String> = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["outcome"] == "unreached")
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        !unreached.is_empty(),
        "the fixture exists to have a mutation no measured test reaches"
    );

    let mut configuration = String::from("version = 1\n");
    for id in &unreached {
        let _written = write!(
            configuration,
            "\n[[acceptance]]\nid = \"{id}\"\nreason = \"nothing reaches it and that is the \
             decision\"\n"
        );
    }
    std::fs::write(fixture.root.join(".mjutest.toml"), configuration).expect("a configuration");

    verify(&fixture, &[]);
    let report = document(&fixture);
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
    let cache = mjutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}

#[test]
fn a_test_that_writes_into_the_tree_while_it_is_measured_is_said_to_have_done_so() {
    let fixture = fixture("fixture-writes-tree");
    let output = verify(&fixture, &[]);
    assert!(
        output.status.code().is_some_and(|code| code <= 2),
        "the run establishes something: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = document(&fixture);
    let named: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        named.contains(&mjutest_cli::limitation::TREE_WRITTEN_DURING_MEASUREMENT),
        "a test wrote into the tree, so every mutation measured after it was measured \
         against what it wrote rather than against the tree the report names: a run that \
         did not say so reads as a measurement of the workspace. {named:?}"
    );
    let detail = report["limitations"]
        .as_array()
        .and_then(|all| {
            all.iter()
                .find(|one| one["name"] == mjutest_cli::limitation::TREE_WRITTEN_DURING_MEASUREMENT)
        })
        .and_then(|one| one["detail"].as_str())
        .unwrap_or_default();
    assert!(
        detail.contains("wrote into the tree"),
        "and says what it means, because the name alone tells a reader nothing to do: \
         {detail}"
    );
}

#[test]
fn a_suite_that_writes_nothing_says_nothing_about_a_tree_that_was_written_to() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = document(&fixture);
    let named: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        !named.contains(&mjutest_cli::limitation::TREE_WRITTEN_DURING_MEASUREMENT),
        "a limitation stated where it does not apply is one a reader stops believing: \
         {named:?}"
    );
}

#[test]
fn recording_an_acceptance_keeps_the_configuration_a_person_wrote() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let path = fixture.root.join(".mjutest.toml");
    std::fs::write(
        &path,
        "version = 1\n\n# the contract this project promises\n[contract]\nname = \"standard-v1\"\n",
    )
    .expect("a configuration somebody wrote");

    let survivor = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["outcome"] == "survived" || mutant["outcome"] == "unreached")
        .and_then(|mutant| mutant["id"].as_str())
        .expect("a survivor")
        .to_owned();

    let recorded = mjutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);
    assert_eq!(
        recorded.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
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
        after.contains(&survivor) && after.contains("reviewed"),
        "with the acceptance appended: {after}"
    );

    let again = mjutest(
        &fixture,
        &["accept", &survivor, "--reason", "reviewed again"],
    );
    assert_eq!(
        again.status.code(),
        Some(0),
        "accepting what is already accepted is not a failure: a script that records a \
         decision twice has recorded it: {}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("already accepted"),
        "and says so rather than saying it wrote one: {}",
        String::from_utf8_lossy(&again.stdout)
    );
    let twice = std::fs::read_to_string(&path).expect("the configuration");
    assert_eq!(
        twice.matches(&survivor).count(),
        1,
        "and the file holds one, because two acceptances of one mutation are two \
         reviewers disagreeing with themselves: {twice}"
    );
    assert!(
        !twice.contains("reviewed again"),
        "the first reason stands: it is the one somebody wrote when they looked: {twice}"
    );
}

#[test]
fn a_configuration_nobody_can_parse_is_refused_rather_than_rewritten() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let path = fixture.root.join(".mjutest.toml");
    let broken = "version = 1\n[contract\nname = ]\n";
    std::fs::write(&path, broken).expect("a configuration nobody can parse");

    let survivor = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["outcome"] == "survived" || mutant["outcome"] == "unreached")
        .and_then(|mutant| mutant["id"].as_str())
        .expect("a survivor")
        .to_owned();

    let refused = mjutest(&fixture, &["accept", &survivor, "--reason", "reviewed"]);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a file this release could not read is one it must not write: appending to what \
         it could not parse would lose whatever it did not understand: {}",
        String::from_utf8_lossy(&refused.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        broken,
        "and the file is exactly as it was"
    );
}

#[test]
fn a_file_the_configuration_excludes_is_not_mutated_and_is_still_built_and_run() {
    let fixture = fixture("fixture-workspace");
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        "version = 1\n\n[project]\nexclude = [\"crates/core/src/util.rs\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = document(&fixture);
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
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        paths.contains(&"crates/core/src/lib.rs"),
        "and takes out nothing else: {paths:?}"
    );
    assert_eq!(
        report["scope"]["excluded"],
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

#[test]
fn the_features_the_configuration_turns_on_are_the_features_the_run_compiles() {
    let fixture = fixture("fixture-features");
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        "version = 1\n\n[execution]\nfeatures = [\"imperial\"]\n",
    )
    .expect("a configuration");

    let output = verify(&fixture, &[]);
    let report = document(&fixture);
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
        fixture.root.join(".mjutest.toml"),
        "version = 1\n\n[project]\npackages = [\"fixture-core\"]\n",
    )
    .expect("a configuration");

    let output = asked(&of(&fixture.root, &[]), &["plan", "--offline", "--locked"]);
    let text = String::from_utf8_lossy(&output.stdout);
    let targets: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("TARGET\t"))
        .collect();

    assert!(
        !targets.iter().any(|line| line.contains("fixture-app/")),
        "a plan says what a run would measure, and a run of this tree measures one \
         package because the configuration says so. A plan that reads none of the \
         configuration is a plan for a run nobody asked for: {text}"
    );
    assert!(
        targets.iter().any(|line| line.contains("fixture-core/")),
        "and it still names the package that is in scope: {text}"
    );
}

#[test]
fn the_harness_arguments_the_configuration_writes_are_the_ones_the_suite_runs_with() {
    let plain = fixture("fixture-ignored");
    verify(&plain, &[]);
    let before = document(&plain);
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
        fixture.root.join(".mjutest.toml"),
        "version = 1\n\n[execution]\ntest_binary_args = [\"--include-ignored\"]\n",
    )
    .expect("a configuration");
    let output = verify(&fixture, &[]);
    let mutants = &document(&fixture)["accounting"]["mutants"];

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
