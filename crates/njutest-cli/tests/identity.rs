// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report before anything has run: the question, written down before the answer.

#![expect(
    clippy::expect_used,
    reason = "the helper that builds one request is not itself a test, and a timestamp out of range is a setup failure to report by panicking"
)]

use std::path::PathBuf;

use njutest_cli::assure::identity::Evidence;
use njutest_cli::assure::run::{Request, identity, opened};
use njutest_cli::build::Cargo;
use njutest_cli::config::Config;
use njutest_cli::limitation::WORKSPACE_DIGEST_NOT_COMPUTED;
use njutest_cli::report::{RunKind, UNAVAILABLE};

fn asked(root: &str) -> Request {
    Request {
        configuration: ".njutest.toml".to_owned(),
        root: PathBuf::from(root),
        config: Config::default(),
        build: Config::default().execution.build(),
        built_as: njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        packages: Vec::new(),
        test_args: Vec::new(),
        cargo: Cargo::default(),
        keep_temp: false,
        run_id: "20260909T000000Z-000001".to_owned(),
        started: jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
        engine_trace: rust_mutants::trace::Recorder::disabled(),
        evidence: Evidence::default(),
        changed: None,
        checkpoints: None,
        evidence_store: None,
        shard: None,
    }
}

fn known() -> Evidence {
    Evidence {
        identity: "i".repeat(64),
        tree: "t".repeat(64),
        keying: None,
    }
}

#[test]
fn a_report_says_what_it_is_about_before_it_says_anything_it_found() {
    let mut request = asked("/tmp/somewhere/demo");
    request.evidence = known();
    request.packages = vec!["core".to_owned()];
    request.config.project.exclude = vec!["vendor/**".to_owned()];

    let report = identity(&request);

    assert_eq!(report.run_id, "20260909T000000Z-000001");
    assert_eq!(
        report.timing.started,
        request.started.to_string(),
        "when a run started is what orders two reports of one tree"
    );
    assert_eq!(
        report.repository.root_name, "demo",
        "the name a person calls the workspace, which is the only part of the path a \
         report carries"
    );
    assert_eq!(
        report.repository.configuration_digest,
        request.config.digest(),
        "two runs configured differently asked different questions, and a reader \
         comparing them has no other way to know"
    );
    assert_eq!(
        report.repository.workspace_digest, request.evidence.tree,
        "and the tree they asked it of"
    );
    assert_eq!(report.provenance.identity, request.evidence.identity);
    assert_eq!(
        report.scope.requested_packages,
        vec!["core".to_owned()],
        "what the command line asked for is what the report says was asked for"
    );
    assert_eq!(report.scope.excluded, vec!["vendor/**".to_owned()]);
    assert_eq!(report.scope.shard, None);
    assert!(
        report.limitations.is_empty(),
        "a tree that could be read as one number states nothing: {:?}",
        report.limitations
    );
}

#[test]
fn a_tree_no_number_could_be_read_from_says_so_and_names_no_digest() {
    let report = identity(&asked("/tmp/somewhere/demo"));

    assert_eq!(
        report.repository.workspace_digest, UNAVAILABLE,
        "a digest nobody computed is not an empty one: an empty string beside another \
         empty string reads as two runs of one tree"
    );
    assert!(
        report
            .limitations
            .iter()
            .any(|limitation| limitation.name == WORKSPACE_DIGEST_NOT_COMPUTED),
        "and the run says why, because a result nothing can be keyed to is one no later \
         run may reuse: {:?}",
        report.limitations
    );
    assert_eq!(
        report.provenance.identity, UNAVAILABLE,
        "nothing was established about what this run is, and the sentinel is what says \
         so: a report whose identity is a digest nobody computed would be one a later \
         run keys its own answers against"
    );
}

#[test]
fn the_packages_a_report_names_are_the_ones_the_command_line_asked_for() {
    let mut configured = asked("/tmp/demo");
    configured.config.project.packages = vec!["from-config".to_owned()];

    let mut asked_for = configured.clone();
    asked_for.packages = vec!["from-the-command-line".to_owned()];

    assert_eq!(
        identity(&configured).scope.requested_packages,
        vec!["from-config".to_owned()],
        "a configuration that names packages is what a run with no packages on its \
         command line looked at"
    );
    assert_eq!(
        identity(&asked_for).scope.requested_packages,
        vec!["from-the-command-line".to_owned()],
        "and a command line that names one narrows it: the two are not added together, \
         because a person who names a package is asking about that one"
    );
    assert_eq!(identity(&configured).run_kind, RunKind::Scoped);
}

#[test]
fn a_part_of_a_catalog_says_which_part_it_judged() {
    let mut request = asked("/tmp/demo");
    request.shard = Some(rust_mutants::run::Shard::parse("2/5").expect("a part of a catalog"));

    assert_eq!(
        identity(&request).scope.shard.as_deref(),
        Some("2/5"),
        "a part judged a fifth of the catalog and measured the whole baseline, and a \
         reader handed its report without that number would read a whole run"
    );
}

#[test]
fn a_workspace_at_the_root_of_a_filesystem_is_still_named() {
    assert_eq!(
        identity(&asked("/")).repository.root_name,
        UNAVAILABLE,
        "a path with no last component names no workspace, and an empty name would read \
         as one somebody forgot to record"
    );
}

#[test]
fn a_run_that_could_not_ask_git_says_so_before_it_compiles_anything() {
    let root = tempfile::tempdir().expect("a directory");
    let parent = tempfile::tempdir().expect("another directory");
    let mut request = asked(&root.path().display().to_string());
    request.evidence = known();
    let scratch = njutest_cli::scratch::Scratch::create(
        parent.path(),
        "20260909T000000Z-000001",
        request.started,
    )
    .expect("a directory to work in");
    let cancel = rust_mutants::runner::Cancel::new();
    let trace = njutest_cli::trace::Recorder::disabled();
    let environment = njutest_cli::cli::Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        working_directory: root.path().to_owned(),
        temp_directory: parent.path().to_owned(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: parent.path().to_owned(),
        cancel: cancel.clone(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let mut report = identity(&request);
    opened(
        &mut report,
        &request,
        &environment,
        (&scratch, njutest_cli::watch::Watch::new(&cancel, &trace)),
    );

    assert!(
        report.repository.git.said().is_none(),
        "a directory that is not a repository is one git has nothing to say about"
    );
    assert!(
        report
            .limitations
            .iter()
            .any(|limitation| limitation.name == njutest_cli::limitation::GIT_METADATA_UNAVAILABLE),
        "and the run says so here, before it compiles anything, because a report that \
         cannot name the commit it verified is one nobody can go back to: {:?}",
        report.limitations
    );
    assert!(
        !report
            .limitations
            .iter()
            .any(|limitation| limitation.name == njutest_cli::limitation::TEMP_DIRECTORY_UNCLAIMED),
        "the directory it works in was claimed, so nothing is said about a sweep taking \
         it: {:?}",
        report.limitations
    );

    std::fs::write(root.path().join("one.rs"), "fn f() {}\n").expect("a file to commit");
    njutest_devkit::repo::commit_tree(root.path());
    let mut committed = identity(&request);
    opened(
        &mut committed,
        &request,
        &environment,
        (&scratch, njutest_cli::watch::Watch::new(&cancel, &trace)),
    );

    assert!(
        committed.repository.git.said().is_some(),
        "and a directory that is a repository is one git answers about, so the report \
         carries what it said rather than the sentinel it started with"
    );
    assert!(
        !committed
            .limitations
            .iter()
            .any(|limitation| limitation.name == njutest_cli::limitation::GIT_METADATA_UNAVAILABLE),
        "and states nothing, because there is nothing it could not do: {:?}",
        committed.limitations
    );
}

#[test]
fn every_limitation_a_report_states_before_it_runs_is_a_finished_sentence() {
    let root = tempfile::tempdir().expect("a directory");
    let parent = tempfile::tempdir().expect("another directory");
    let request = asked(&root.path().display().to_string());
    let scratch = njutest_cli::scratch::Scratch::create(
        parent.path(),
        "20260909T000000Z-000001",
        request.started,
    )
    .expect("a directory to work in");
    let cancel = rust_mutants::runner::Cancel::new();
    let trace = njutest_cli::trace::Recorder::disabled();
    let environment = njutest_cli::cli::Environment {
        vars: Vec::new(),
        working_directory: root.path().to_owned(),
        temp_directory: parent.path().to_owned(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: parent.path().to_owned(),
        cancel: cancel.clone(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let mut report = identity(&request);
    opened(
        &mut report,
        &request,
        &environment,
        (&scratch, njutest_cli::watch::Watch::new(&cancel, &trace)),
    );

    assert!(
        report.limitations.len() >= 2,
        "a tree with no digest and a directory that is not a repository state one each: \
         {:?}",
        report.limitations
    );
    for limitation in &report.limitations {
        assert!(
            !limitation.name.trim().is_empty(),
            "a limitation with no name is one nobody can look up: {limitation:?}"
        );
        assert!(
            limitation.detail.trim().len() > 20 && !limitation.detail.contains("  "),
            "and one with no sentence names something a run could not do and says \
             nothing about what that means for the answer, which is the only part a \
             reader acts on: {limitation:?}"
        );
    }
}
