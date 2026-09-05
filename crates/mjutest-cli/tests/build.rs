// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The two builds a run performs, against real cargo and real fixtures.
//!
//! The instrumented one is the one worth watching: it has to land in its own
//! layer, keep the project's own flags, and actually write a profile when a
//! test process runs — otherwise coverage routing would silently measure
//! nothing and every mutant would look unreachable.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;
use std::path::PathBuf;

use mjutest_cli::build::{BuildOptions, Built, Cargo, Flavour, Selection, build};
use mjutest_cli::trace::{MemorySink, Recorder, StartRecord};
use mjutest_cli::watch::Watch;

use rust_mutants::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};
use rust_mutants::runner::{Cancel, Spec, run};

struct Built0 {
    built: Built,
    toolchain: Toolchain,
    _target: tempfile::TempDir,
    target_dir: PathBuf,
}

fn env() -> Vec<(OsString, OsString)> {
    // Enough for cargo and rustc to work, and nothing else: what the run is
    // given is what the run sees.
    std::env::vars_os()
        .filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        })
        .collect()
}

fn build_fixture(fixture: &str, flavour: Flavour, packages: &[&str]) -> Built0 {
    let root = mjutest_devkit::paths::fixtures_dir().join(fixture);
    let target = tempfile::Builder::new()
        .prefix("mjutest-build-")
        .tempdir()
        .expect("tempdir");
    let target_dir = target.path().join("layer");
    let cancel = Cancel::new();
    let engine_trace = rust_mutants::trace::Recorder::disabled();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            search_path: None,
            env: Some(env()),
        },
        &root,
        &cancel,
    )
    .expect("a toolchain");
    let metadata = Metadata::load(
        &Driver {
            toolchain: &toolchain,
            dir: &root,
            cancel: &cancel,
            trace: &engine_trace,
        },
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");

    let built = build(
        &toolchain,
        &metadata.packages,
        &BuildOptions {
            root,
            selection: Selection {
                packages: packages.iter().map(|name| (*name).to_owned()).collect(),
                ..Selection::default()
            },
            flavour,
            target_dir: target_dir.clone(),
            scratch_build_dir: target.path().join("scratch"),
            env: env(),
            cargo: Cargo {
                offline: true,
                locked: true,
            },
            timeout: None,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the build runs");

    Built0 {
        built,
        toolchain,
        _target: target,
        target_dir,
    }
}

// --- the plain build ----------------------------------------------------------------

#[test]
fn a_plain_build_produces_the_test_binaries_in_the_layer_it_was_given() {
    let outcome = build_fixture("fixture-simple", Flavour::Native, &[]);
    assert!(
        outcome.built.failure.is_none(),
        "{:?}",
        outcome.built.failure
    );
    assert!(!outcome.built.units.is_empty());
    for unit in &outcome.built.units {
        assert!(unit.executable.is_file(), "{}", unit.executable.display());
        assert!(
            unit.executable.starts_with(&outcome.target_dir),
            "built into the layer it was told to: {}",
            unit.executable.display()
        );
        assert!(unit.cwd.is_dir(), "the package's manifest directory");
    }
}

#[test]
fn a_unit_carries_the_environment_cargo_would_have_given_its_process() {
    let outcome = build_fixture("fixture-simple", Flavour::Native, &[]);
    let unit = &outcome.built.units[0];
    let names: Vec<String> = unit
        .env
        .iter()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();
    for name in ["CARGO_MANIFEST_DIR", "CARGO_PKG_NAME", "CARGO_TARGET_DIR"] {
        assert!(
            names.contains(&name.to_owned()),
            "{name} is missing: {names:?}"
        );
    }
    let target_dir = unit
        .env
        .iter()
        .find(|(key, _)| key == "CARGO_TARGET_DIR")
        .map(|(_, value)| PathBuf::from(value))
        .expect("the scratch layer");
    assert!(
        !target_dir.starts_with(&outcome.target_dir),
        "a cargo the suite spawns must not write into the layer that survives the run"
    );
}

#[test]
fn only_the_packages_that_were_asked_for_are_built() {
    let outcome = build_fixture("fixture-workspace", Flavour::Native, &["fixture-core"]);
    assert!(
        outcome.built.failure.is_none(),
        "{:?}",
        outcome.built.failure
    );
    let packages: std::collections::BTreeSet<String> = outcome
        .built
        .units
        .iter()
        .map(|unit| unit.package.clone())
        .collect();
    assert_eq!(
        packages,
        std::iter::once("fixture-core".to_owned()).collect(),
        "the whole workspace was not built"
    );
}

// --- the instrumented build ---------------------------------------------------------

#[test]
fn an_instrumented_build_lands_under_the_host_triple_and_writes_a_profile_when_it_runs() {
    let outcome = build_fixture("fixture-simple", Flavour::Coverage, &[]);
    assert!(
        outcome.built.failure.is_none(),
        "{:?}",
        outcome.built.failure
    );
    let unit = outcome
        .built
        .units
        .first()
        .expect("at least one test binary");
    let host = outcome.toolchain.host();
    assert!(
        unit.executable.starts_with(outcome.target_dir.join(host)),
        "--target puts the artifacts under the triple, which is what keeps \
         RUSTFLAGS off build scripts and proc macros: {}",
        unit.executable.display()
    );

    let profiles = outcome.target_dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("somewhere to write");
    let mut spec = Spec::new([unit.executable.as_os_str().to_owned()]);
    spec.dir = Some(unit.cwd.clone());
    let mut env = unit.env.clone();
    env.push((
        OsString::from("LLVM_PROFILE_FILE"),
        profiles.join("baseline.%p.profraw").into_os_string(),
    ));
    spec.env = Some(env);
    let ran = run(&spec, &Cancel::new());
    assert!(
        ran.ok(),
        "the fixture's own tests pass: {}",
        String::from_utf8_lossy(&ran.output)
    );

    let written: Vec<PathBuf> = std::fs::read_dir(&profiles)
        .expect("the directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    assert!(
        !written.is_empty(),
        "an instrumented process writes its profile, or coverage routing measures nothing"
    );
    assert!(
        written.iter().all(|path| path
            .extension()
            .is_some_and(|extension| extension == "profraw")),
        "{written:?}"
    );
}

#[test]
fn the_build_says_what_it_did_into_the_trace_without_saying_what_the_variables_hold() {
    let trace = Recorder::new(
        mjutest_cli::trace::Sink::Memory(MemorySink::unbounded()),
        mjutest_cli::trace::Clock::Wall,
        StartRecord::of(
            "run",
            mjutest_cli::report::RunKind::Full,
            mjutest_cli::config::Contract::StandardV1,
        ),
    );
    let root = mjutest_devkit::paths::fixtures_dir().join("fixture-simple");
    let target = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            search_path: None,
            env: Some(env()),
        },
        &root,
        &cancel,
    )
    .expect("a toolchain");
    build(
        &toolchain,
        &[],
        &BuildOptions {
            root,
            selection: Selection::default(),
            flavour: Flavour::Coverage,
            target_dir: target.path().join("layer"),
            scratch_build_dir: target.path().join("scratch"),
            env: vec![
                (OsString::from("SECRET_TOKEN"), OsString::from("hunter2")),
                (OsString::from("PATH"), path_of()),
            ],
            cargo: Cargo {
                offline: true,
                locked: true,
            },
            timeout: None,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the build runs");
    trace.run_end("COMPLETED", None, None);

    let events = trace.events();
    let exec = events
        .iter()
        .find_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Exec { exec } => Some(exec),
            _ => None,
        })
        .expect("the build was recorded");
    assert!(
        exec.env_names.contains(&"SECRET_TOKEN".to_owned()),
        "the shape of the execution: {:?}",
        exec.env_names
    );
    let line = serde_json::to_string(&events).expect("one document");
    assert!(!line.contains("hunter2"), "and none of its secrets");
    assert!(
        exec.argv.iter().any(|argument| argument == "--no-run"),
        "{:?}",
        exec.argv
    );
}

fn path_of() -> OsString {
    env()
        .into_iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value)
        .unwrap_or_default()
}
