// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run decides before it measures anything: how much of the workspace it is about, whether it may measure at all, and whether it may measure two things at once.

#![expect(
    clippy::expect_used,
    reason = "the helper that builds one request is not itself a test, and a timestamp out of range is a setup failure to report by panicking"
)]

use njutest_cli::app::plan::{Planned, line};
use njutest_cli::assure::baseline::{Baseline, Measured};
use njutest_cli::assure::run::{
    Request, alone, first_line, kind_of, measurable, requested, resolve_acceptances, resolved,
    reusable, selected, stated,
};
use njutest_cli::config::{Acceptance, Config};
use njutest_cli::report::{FindingKind, RunKind, TargetStatus};
use rust_mutants::catalog::{Builder, Candidate};
use rust_mutants::id::digest;
use rust_mutants::rule::Registry;
use rust_mutants::span::Span;

const CATALOG_SOURCE: &[u8] = b"pub fn f(a: i32, b: i32) -> bool { a < b && a == b }\n";

fn candidate(path: &str, span: (u32, u32)) -> Candidate {
    Candidate {
        path: path.to_owned(),
        rule: Registry::canonical()
            .lookup("true-to-false")
            .expect("a registered rule"),
        span: Span::new(span.0, span.1).expect("a well-formed span"),
        original: b"true".to_vec(),
        replacement: b"false".to_vec(),
        source_digest: digest(CATALOG_SOURCE),
    }
}

fn acceptance(id: &str, expires: Option<jiff::Timestamp>) -> Acceptance {
    Acceptance {
        id: id.to_owned(),
        path: None,
        item: None,
        rule: None,
        original: None,
        line: None,
        reason: "reviewed".to_owned(),
        expires,
        owner: None,
        ticket: None,
    }
}

fn request(config: Config, packages: &[&str]) -> Request {
    Request {
        configuration: ".njutest.toml".to_owned(),
        root: std::path::PathBuf::from("/nowhere"),
        build: config.execution.build(),
        built_as: njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        config,
        packages: packages.iter().map(|name| (*name).to_owned()).collect(),
        test_args: Vec::new(),
        cargo: njutest_cli::build::Cargo {
            offline: true,
            locked: true,
        },
        keep_temp: false,
        run_id: "20260909T000000Z-000001".to_owned(),
        started: jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
        engine_trace: rust_mutants::trace::Recorder::disabled(),
        evidence: njutest_cli::assure::identity::Evidence::default(),
        changed: None,
        checkpoints: None,
        evidence_store: None,
        shard: None,
    }
}

#[test]
fn only_an_unexpired_acceptance_that_uniquely_names_this_catalog_is_honoured() {
    let first = candidate("crates/a/src/a.rs", (39, 43));
    let second = candidate("crates/a/src/a.rs", (200, 204));
    assert_eq!(
        first.id().expect("an identity").get(..4),
        Some("6d62"),
        "the fixture pins the ambiguous prefix"
    );
    assert_eq!(
        second.id().expect("an identity").get(..4),
        Some("6d62"),
        "the fixture pins the ambiguous prefix"
    );
    let unique = candidate("crates/a/src/unique.rs", (0, 4));
    let unique_id = unique.id().expect("an identity");
    let unique_prefix = unique_id.get(..8).expect("eight characters");
    let mut builder = Builder::new();
    builder
        .add_all([first, second, unique])
        .expect("coherent candidates");
    let catalog = builder.build().expect("a catalog");
    let now = jiff::Timestamp::from_second(1_800_000_000).expect("in range");
    let expired = jiff::Timestamp::from_second(1).expect("in range");

    let resolved = resolve_acceptances(
        &catalog,
        &|_locator| Err(String::from("this test writes no locators")),
        &[
            acceptance(unique_prefix, None),
            acceptance("not-hex", None),
            acceptance("ffff", None),
            acceptance("6d62", None),
            acceptance("also-invalid", Some(expired)),
        ],
        now,
    );

    assert_eq!(resolved.ids, std::collections::BTreeSet::from([unique_id]));
    let subjects: Vec<&str> = resolved
        .findings
        .iter()
        .map(|finding| finding.subject.as_str())
        .collect();
    assert_eq!(subjects, ["not-hex", "ffff", "6d62"]);
    assert!(
        resolved
            .findings
            .iter()
            .all(|finding| finding.kind == FindingKind::UnmatchedAcceptance),
        "every refusal is explicit and non-suppressing: {:?}",
        resolved.findings
    );
}

fn measured(name: &str, status: TargetStatus) -> Measured {
    Measured {
        target: njutest_cli::targets::Target {
            id: format!("id-{name}"),
            package: "core".to_owned(),
            unit: njutest_cli::targets::UnitKind::Lib,
            unit_name: "core".to_owned(),
            path: name.to_owned(),
            ignored: false,
            executable: std::path::PathBuf::from("/nowhere"),
            cwd: std::path::PathBuf::from("/nowhere"),
            env: Vec::new(),
        },
        status,
        duration_ms: 1,
        tests: 1,
        message: None,
    }
}

#[test]
fn what_a_run_is_about_is_what_it_was_asked_for_narrowed_to_what_is_there() {
    let members = ["core".to_owned(), "app".to_owned()];

    let whole = request(Config::default(), &[]);
    assert!(
        requested(&whole).is_empty() && resolved(&whole, &members).is_empty(),
        "a run that named nothing is about the workspace, and says so by naming \
         nothing rather than by listing what it happens to hold today: a list would \
         make two runs of one workspace differ because somebody added a package"
    );

    let named = request(Config::default(), &["app"]);
    assert_eq!(
        resolved(&named, &members),
        ["app"],
        "a run that named a package is about that one"
    );
    let elsewhere: Vec<String> = resolved(&named, &["core".to_owned()]);
    assert!(
        elsewhere.is_empty(),
        "and a package the workspace does not hold is not something this run is about, \
         however it was asked for: reporting it would put a name in the scope that \
         nothing under it was ever measured"
    );

    let mut configured = Config::default();
    configured.project.packages = vec!["core".to_owned()];
    assert_eq!(
        requested(&request(configured.clone(), &[])),
        ["core"],
        "a configuration that names packages is what a run with no packages on its \
         command line is about"
    );
    assert_eq!(
        requested(&request(configured, &["app"])),
        ["app"],
        "and the command line narrows it, because that is the one a person typed just \
         now"
    );
}

#[test]
fn how_much_of_the_workspace_a_run_looked_at_is_part_of_what_it_may_conclude() {
    assert_eq!(
        kind_of(&request(Config::default(), &[])),
        RunKind::Full,
        "a run that named nothing looked at everything, and the contract reserves its \
         strongest verdict for that"
    );
    assert_eq!(
        kind_of(&request(Config::default(), &["app"])),
        RunKind::Scoped,
        "a run that named a package looked at that package: calling it a whole run \
         would let a verdict over one corner stand for the workspace"
    );

    let mut configured = Config::default();
    configured.project.packages = vec!["core".to_owned()];
    assert_eq!(
        kind_of(&request(configured, &[])),
        RunKind::Scoped,
        "and a configuration that names packages narrows it exactly as a command line \
         does, because what a run looked at is what it looked at"
    );

    let mut changed = request(Config::default(), &[]);
    changed.changed = Some(njutest_cli::git::Change {
        base: "main".to_owned(),
        merge_base: None,
        files: vec!["src/lib.rs".to_owned()],
    });
    assert_eq!(
        kind_of(&changed),
        RunKind::Changed,
        "and a run about what differs from a revision is about that, whatever else it \
         was told: naming no package does not make a change set into the workspace"
    );
}

#[test]
fn a_baseline_that_does_not_pass_is_not_one_a_mutation_can_be_put_to() {
    let passing = Baseline {
        targets: vec![
            measured("one", TargetStatus::Passed),
            measured("two", TargetStatus::Passed),
        ],
        failure: None,
        limitations: Vec::new(),
    };
    assert!(measurable(&passing));

    let failing = Baseline {
        targets: vec![
            measured("one", TargetStatus::Passed),
            measured("two", TargetStatus::Failed),
        ],
        ..passing.clone()
    };
    assert!(
        !measurable(&failing),
        "a mutation is noticed when a test that passed stops passing, so a target that \
         was already failing can notice nothing: measuring against it would report \
         every mutation of the code it covers as caught by a test that was red before \
         anything was changed"
    );

    let skipped = Baseline {
        targets: vec![
            measured("one", TargetStatus::Passed),
            measured("two", TargetStatus::Skipped),
            measured("three", TargetStatus::Missing),
        ],
        ..passing.clone()
    };
    assert!(
        measurable(&skipped),
        "while a target that ran nothing is a gap the report states rather than a \
         reason to measure nothing at all: the rest of the suite still answers"
    );

    let refused = Baseline {
        failure: Some("the workspace does not compile".to_owned()),
        ..passing
    };
    assert!(
        !measurable(&refused),
        "and a workspace that does not compile has no suite to put anything to"
    );
}

/// One resource a run is told to hold, exclusive or not.
fn resource(exclusive: bool) -> njutest_cli::config::Resource {
    njutest_cli::config::Resource {
        command: vec!["true".to_owned()],
        timeout: std::time::Duration::from_secs(30),
        shared: !exclusive,
        exclusive,
        environment: Vec::new(),
        interpose: String::new(),
        wire: njutest_cli::wire::Wire::Raw,
        hold: std::time::Duration::from_secs(30),
    }
}

#[test]
fn a_resource_only_one_test_may_hold_makes_the_run_measure_one_at_a_time() {
    let mut config = Config::default();
    assert!(
        !alone(&config),
        "a run with nothing to hold measures as many mutations at once as it was told to"
    );

    config
        .resources
        .insert("shared".to_owned(), resource(false));
    assert!(
        !alone(&config),
        "and a resource two tests may hold at once changes nothing"
    );

    config
        .resources
        .insert("database".to_owned(), resource(true));
    assert!(
        alone(&config),
        "one resource only a single test may hold at a time decides it for the whole \
         run: two measurements sharing it would answer about each other rather than \
         about the mutations, and which of them was wrong is not something a report \
         could say afterwards"
    );
}

/// Cargo's answer about a workspace of these packages, each manifest where the name says.
fn metadata(packages: &[(&str, &str)]) -> rust_mutants::cargo::Metadata {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "workspace_root": "/w",
        "target_directory": "/w/target",
        "workspace_members": [],
        "packages": packages
            .iter()
            .map(|(name, manifest)| serde_json::json!({
                "id": format!("path+file:///w#{name}"),
                "name": name,
                "version": "0.1.0",
                "manifest_path": manifest,
            }))
            .collect::<Vec<_>>(),
    }))
    .expect("metadata this release reads")
}

#[test]
fn the_packages_an_inventory_walks_are_the_ones_in_scope_that_have_somewhere_to_walk() {
    let workspace = metadata(&[
        ("core", "/w/core/Cargo.toml"),
        ("edge", "/w/edge/Cargo.toml"),
    ]);
    assert_eq!(
        selected(&[], &workspace),
        vec![
            ("core".to_owned(), std::path::PathBuf::from("/w/core")),
            ("edge".to_owned(), std::path::PathBuf::from("/w/edge")),
        ],
        "a scope that names nothing is a run that looked at everything, so an empty list \
         widens: narrowing on it would inventory nothing at all and report a workspace \
         with no unsafe in it"
    );
    assert_eq!(
        selected(&["edge".to_owned()], &workspace),
        vec![("edge".to_owned(), std::path::PathBuf::from("/w/edge"))],
        "and a scope that names one package walks that one"
    );

    for named in ["Cargo.toml", ""] {
        assert_eq!(
            selected(&[], &metadata(&[("nowhere", named)])),
            Vec::new(),
            "while a package whose manifest names no directory is left out rather than \
             entered under one nobody has: an inventory is the files under a directory, \
             and this one would be walked from wherever the process happens to stand, \
             which is the whole machine as readily as the package. A bare name has a \
             parent and it is the empty path, so the two ways of naming no directory \
             are two guards and not one: {named:?}"
        );
    }
}

#[test]
fn a_tree_the_compiler_vouches_for_entirely_has_no_limitation_to_state() {
    assert_eq!(
        stated(&njutest_cli::soundness::Inventory::default()),
        Vec::new(),
        "a limitation on every report of every tree with no unsafe in it is a line a \
         reader learns to skip, and the next one they skip is one that mattered"
    );

    let found = njutest_cli::soundness::Inventory {
        items: vec![njutest_cli::soundness::Item {
            package: "core".to_owned(),
            path: "core/src/lib.rs".to_owned(),
            line: 7,
            kind: njutest_cli::soundness::Kind::Block,
        }],
        packages: vec!["core".to_owned()],
        unreadable: Vec::new(),
    };
    let found_states = stated(&found);
    assert_eq!(
        found_states
            .iter()
            .map(|one| one.name.clone())
            .collect::<Vec<_>>(),
        vec!["soundness-not-executed".to_owned()],
        "while a place the compiler stops vouching for is one this contract counts and \
         does not execute, which is a limitation the report states rather than a claim \
         it makes: {found_states:?}"
    );

    let unread = njutest_cli::soundness::Inventory {
        unreadable: vec!["core/src/odd.rs".to_owned()],
        ..found
    };
    let unread_states = stated(&unread);
    assert_eq!(
        unread_states
            .iter()
            .map(|one| one.name.clone())
            .collect::<Vec<_>>(),
        vec![
            "soundness-source-unreadable".to_owned(),
            "soundness-not-executed".to_owned()
        ],
        "and a file that was not read is not a file with nothing in it, so the run says \
         both what it found and what it could not look at: a count taken over part of a \
         tree, reported as a count over the tree, is the one number a reader cannot \
         check: {unread_states:?}"
    );
}

#[test]
fn a_run_that_looked_at_less_than_everything_neither_believes_nor_records() {
    assert!(
        reusable(RunKind::Full, &std::collections::BTreeMap::new()),
        "a run over the whole project with nothing started beside it establishes what \
         the next such run may believe"
    );
    for narrowed in [RunKind::Changed, RunKind::Scoped] {
        assert!(
            !reusable(narrowed, &std::collections::BTreeMap::new()),
            "while a run that looked at less established less: what it did not route to \
             a mutant it did not ask, so the survival it would record is a claim over a \
             smaller set wearing the name of the larger one. {narrowed:?} may not"
        );
    }
    let resources: std::collections::BTreeMap<String, njutest_cli::config::Resource> =
        toml::from_str("[database]\ncommand = [\"serve\"]\n").expect("one resource");
    assert!(
        !reusable(RunKind::Full, &resources),
        "and a resource a run started is a fact about the world its tests ran in that no \
         behaviour key covers: the next run may start a different one, or none, and \
         nothing in the record would say so"
    );
}

#[test]
fn a_build_that_failed_without_saying_anything_is_still_reported_as_one() {
    assert_eq!(
        first_line("error: no method named `probe`\n  --> src/lib.rs:7\n"),
        "error: no method named `probe`",
        "a compiler says several things and a report has room for one, which is the \
         first: the ones after it are about the same failure"
    );
    assert_eq!(
        first_line("\n\n  \nerror: late\n"),
        "error: late",
        "and the first thing it said, rather than the first line it printed"
    );
    assert_eq!(
        first_line(""),
        "the workspace does not compile",
        "while a build that failed with nothing on any line is one a person still has to \
         be told about: an empty sentence in a report reads as a run that forgot to fill \
         it in, and the reader looks for the failure somewhere it is not"
    );
}

/// One binary a plan would name, holding `tests` tests of which `ignored` are skipped.
fn planned(unit: njutest_cli::targets::UnitKind, tests: usize, ignored: usize) -> Planned {
    Planned {
        target: njutest_cli::targets::Target {
            id: "id-1".to_owned(),
            package: "demo".to_owned(),
            unit,
            unit_name: "demo".to_owned(),
            path: "whole binary".to_owned(),
            ignored: false,
            executable: std::path::PathBuf::from("/nowhere"),
            cwd: std::path::PathBuf::from("/nowhere"),
            env: Vec::new(),
        },
        tests,
        ignored,
    }
}

#[test]
fn a_plan_names_a_binary_and_says_what_put_it_there_only_when_asked() {
    let ordinary = planned(njutest_cli::targets::UnitKind::Lib, 7, 2);
    assert_eq!(
        line(&ordinary, false),
        format!("TARGET\tid-1\t{}", ordinary.target.name()),
        "a plan nobody asked to explain itself names the binary by its identity and by \
         the name a person reads, and stops: the identity is what a report joins on and \
         the name is what a person recognises"
    );

    let explained = line(&ordinary, true);
    assert!(
        explained.starts_with(&line(&ordinary, false)) && explained.contains("9 tests, 2 of them"),
        "and one that was asked adds the reason to the same line, counting the ignored \
         ones in the total, because a suite that says it has nine and a plan that says \
         seven is a difference a person goes looking for: {explained:?}"
    );

    assert!(
        line(&planned(njutest_cli::targets::UnitKind::Doc, 3, 0), true)
            .contains("a library's documented examples"),
        "a library's examples are named for what they are, since a route cannot narrow \
         them and the count would read as one it could"
    );
    assert!(
        line(&planned(njutest_cli::targets::UnitKind::Test, 0, 0), true)
            .contains("answers by exiting"),
        "and a binary that holds no test it can name is one that answers by exiting, \
         which is a different reason for the same absence"
    );
    assert!(
        line(&planned(njutest_cli::targets::UnitKind::Test, 0, 4), true).contains("4 tests, 4"),
        "while one whose every test is ignored holds tests: a plan that called it a \
         whole binary would hide four tests nobody is running"
    );
}
