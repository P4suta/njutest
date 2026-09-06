// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The proof that instrumentation is honest: a real workspace, instrumented and built by a real cargo, behaves exactly as it did until a mutant is activated, and then behaves as that one edit says.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mjutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    CompileKind, CompileOptions, Driver, LocateOptions, Message, Metadata, MetadataOptions,
    Toolchain, compile,
};
use rust_mutants::catalog::Catalog;
use rust_mutants::discover::{DiscoverOptions, Input, discover};
use rust_mutants::instrument::{
    ACTIVE_ENV, CATALOG_ENV, STALE_CATALOG_EXIT, instrument_file, plan_file,
};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::runner::{Cancel, RunResult, Spec, run};
use rust_mutants::syntax::Selection;
use rust_mutants::trace::Recorder;

static REGISTRY: Registry = Registry::canonical();

/// A copy of a fixture, instrumented, with its test binaries built.
struct Tree {
    root: PathBuf,
    catalog: Catalog,
    binaries: BTreeMap<String, PathBuf>,
    _dir: tempfile::TempDir,
    _target: tempfile::TempDir,
}

fn toolchain(dir: &Path, cancel: &Cancel) -> Toolchain {
    Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        dir,
        cancel,
    )
    .expect("locate")
}

/// Copies `fixture`, instruments every mutable file, and builds its tests.
fn prepare(fixture: &str) -> Tree {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-tree-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(fixture);
    copy_tree(&mjutest_devkit::paths::fixtures_dir().join(fixture), &root);
    let target = tempfile::Builder::new()
        .prefix("rust-mutants-tree-target-")
        .tempdir()
        .expect("tempdir");

    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let toolchain = toolchain(&root, &cancel);
    let driver = Driver {
        toolchain: &toolchain,
        dir: &root,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let checked = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(target.path().to_path_buf()),
            locked: true,
            offline: true,
            timeout: None,
            env: Vec::new(),
        },
    )
    .expect("check");
    assert!(checked.success, "the pristine copy compiles");

    let discovery = discover(
        &Input {
            root: &root,
            metadata: &metadata,
            units: &checked.units,
        },
        &DiscoverOptions {
            selection: Selection::tier(&REGISTRY, Tier::All),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
        },
        &trace,
    )
    .expect("discover");

    let found: Vec<rust_mutants::syntax::Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut paths: Vec<&str> = found
        .iter()
        .map(|one| one.candidate.path.as_str())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    for path in paths {
        let placements = plan_file(&discovery.catalog, path, &found).expect("plan");
        let source = std::fs::read(root.join(path)).expect("read");
        let file = instrument_file(path, &source, &placements, discovery.catalog.digest())
            .expect("instrument");
        assert!(file.instrumented, "{path}");
        std::fs::write(root.join(path), file.text).expect("write");
    }

    let mut spec = toolchain.command(
        &root,
        [
            "test",
            "--workspace",
            "--all-targets",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
        ],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let built = run(&spec, &cancel);
    assert!(
        built.ok(),
        "the instrumented tree builds: {}",
        String::from_utf8_lossy(&built.output)
    );
    let binaries = rust_mutants::cargo::parse_messages(&built.stdout)
        .expect("messages")
        .into_iter()
        .filter_map(|message| match message {
            Message::CompilerArtifact(artifact) if artifact.profile.test => artifact
                .executable
                .map(|executable| (artifact.target.name.clone(), executable)),
            _ => None,
        })
        .collect();
    Tree {
        root,
        catalog: discovery.catalog,
        binaries,
        _dir: dir,
        _target: target,
    }
}

impl Tree {
    /// The identity of the one mutant of `rule` in the fixture, which the catalog is small enough to name unambiguously.
    fn mutant(&self, rule: &str) -> String {
        let matching: Vec<&rust_mutants::catalog::Mutant> = self
            .catalog
            .mutants()
            .iter()
            .filter(|mutant| mutant.candidate.rule.name == rule)
            .collect();
        assert_eq!(matching.len(), 1, "{rule}: {matching:?}");
        matching[0].id.clone()
    }

    /// Runs one test binary with the given activation.
    fn exec(&self, binary: &str, active: Option<&str>, catalog: Option<&str>) -> RunResult {
        let executable = self.binaries.get(binary).expect("a built test binary");
        let mut spec = Spec::new([executable.as_os_str()]);
        spec.dir = Some(self.root.clone());
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(key, _)| key != ACTIVE_ENV && key != CATALOG_ENV)
            .collect();
        if let Some(active) = active {
            env.push((ACTIVE_ENV.into(), active.into()));
            env.push((
                CATALOG_ENV.into(),
                catalog.unwrap_or_else(|| self.catalog.digest()).into(),
            ));
        }
        spec.env = Some(env);
        run(&spec, &Cancel::new())
    }
}

#[test]
fn an_instrumented_tree_builds_and_behaves_exactly_as_it_did_until_a_mutant_is_activated() {
    let tree = prepare("fixture-simple");
    assert!(tree.binaries.contains_key("fixture_simple"));
    assert!(tree.binaries.contains_key("parity"));

    for binary in tree.binaries.keys() {
        let result = tree.exec(binary, None, None);
        assert!(
            result.ok(),
            "{binary} passes with no mutant active: {}",
            String::from_utf8_lossy(&result.output)
        );
    }

    let killed = tree.mutant("return-default");
    let result = tree.exec("fixture_simple", Some(&killed), None);
    assert_eq!(
        result.exit_code,
        101,
        "{}",
        String::from_utf8_lossy(&result.output)
    );
    assert!(
        String::from_utf8_lossy(&result.output).contains("max_picks_the_larger"),
        "{}",
        String::from_utf8_lossy(&result.output)
    );

    let result = tree.exec("parity", Some(&killed), None);
    assert!(result.ok(), "{}", String::from_utf8_lossy(&result.output));

    let survivor = tree.mutant("gt-to-ge");
    let result = tree.exec("fixture_simple", Some(&survivor), None);
    assert!(result.ok(), "{}", String::from_utf8_lossy(&result.output));

    let elsewhere = "0".repeat(64);
    let result = tree.exec("fixture_simple", Some(&elsewhere), None);
    assert!(result.ok(), "{}", String::from_utf8_lossy(&result.output));
}

#[test]
fn a_stale_catalog_ends_the_test_process_rather_than_reporting_a_survivor() {
    let tree = prepare("fixture-simple");
    let mutant = tree.mutant("return-default");
    let stale = "f".repeat(64);
    let result = tree.exec("fixture_simple", Some(&mutant), Some(&stale));
    assert_eq!(
        result.exit_code,
        STALE_CATALOG_EXIT,
        "{}",
        String::from_utf8_lossy(&result.output)
    );
    let said = String::from_utf8_lossy(&result.output);
    assert!(said.contains("rust-mutants"), "{said}");
    assert!(
        said.contains(tree.catalog.digest()),
        "it names the catalog it was built from: {said}"
    );
}
