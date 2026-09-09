// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run decides before it measures anything: how much of the workspace it is about, whether it may measure at all, and whether it may measure two things at once.

#![expect(
    clippy::expect_used,
    reason = "the helper that builds one request is not itself a test, and a timestamp out of range is a setup failure to report by panicking"
)]

use mjutest_cli::assure::baseline::{Baseline, Measured};
use mjutest_cli::assure::run::{Request, alone, kind_of, measurable, requested, resolved};
use mjutest_cli::config::Config;
use mjutest_cli::report::{RunKind, TargetStatus};

fn request(config: Config, packages: &[&str]) -> Request {
    Request {
        root: std::path::PathBuf::from("/nowhere"),
        config,
        packages: packages.iter().map(|name| (*name).to_owned()).collect(),
        test_args: Vec::new(),
        cargo: mjutest_cli::build::Cargo {
            offline: true,
            locked: true,
        },
        keep_temp: false,
        run_id: "20260909T000000Z-000001".to_owned(),
        started: jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
        engine_trace: rust_mutants::trace::Recorder::disabled(),
        evidence: mjutest_cli::assure::identity::Evidence::default(),
        changed: None,
        checkpoints: None,
        evidence_store: None,
        shard: None,
    }
}

fn measured(name: &str, status: TargetStatus) -> Measured {
    Measured {
        target: mjutest_cli::targets::Target {
            id: format!("id-{name}"),
            package: "core".to_owned(),
            unit: mjutest_cli::targets::UnitKind::Lib,
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
    changed.changed = Some(mjutest_cli::git::Change {
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
fn resource(exclusive: bool) -> mjutest_cli::config::Resource {
    mjutest_cli::config::Resource {
        command: vec!["true".to_owned()],
        timeout: std::time::Duration::from_secs(30),
        shared: !exclusive,
        exclusive,
        environment: Vec::new(),
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
