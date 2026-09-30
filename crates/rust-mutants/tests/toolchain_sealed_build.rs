// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sealed build of a real tree: every test target that builds for `wasm32-wasip1` has a module holding its sources, and one that does not is unsealed by name.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::{Path, PathBuf};

use njutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    BuildConfig, BuildDir, CompileKind, CompileOptions, Driver, LocateOptions, Metadata,
    MetadataOptions, Toolchain, compile,
};
use rust_mutants::runner::Cancel;
use rust_mutants::sealed::record::Came;
use rust_mutants::sealed::rerun::{Now, Recorded, Reproduction, Reran, Unmade};
use rust_mutants::sealed::{SealedBuild, Sealing, TARGET, Unsealed, installed};
use rust_mutants::trace::{MemorySink, Payload, Recorder, Sink};

fn toolchain(dir: &Path) -> Toolchain {
    let options = LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        ..LocateOptions::default()
    };
    Toolchain::locate(&options, dir, &Cancel::new()).expect("locate")
}

fn build(
    root: &Path,
    kind: CompileKind,
    target: Option<&str>,
    into: &Path,
) -> rust_mutants::cargo::Compiled {
    let tc = toolchain(root);
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let driver = Driver {
        toolchain: &tc,
        dir: root,
        cancel: &cancel,
        trace: &trace,
    };
    let options = CompileOptions {
        kind,
        offline: true,
        build: BuildConfig {
            target: target.map(str::to_owned),
            ..BuildConfig::default()
        },
        ..CompileOptions::new(BuildDir::new(into.to_path_buf(), Vec::new()))
    };
    compile(&driver, &options).expect("cargo ran and said how the build came out")
}

fn built(root: &Path) -> (Vec<rust_mutants::execute::TestTarget>, SealedBuild) {
    let tc = toolchain(root);
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let driver = Driver {
        toolchain: &tc,
        dir: root,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: false,
            offline: true,
        },
    )
    .expect("metadata");
    let packages: Vec<rust_mutants::cargo::Package> = metadata.members().cloned().collect();
    let native_dir = root.join("target-native");
    let native = build(root, CompileKind::Tests, None, &native_dir);
    let native_targets =
        rust_mutants::execute::targets_of(&native.messages, &packages, &native_dir)
            .expect("the native targets");
    let sealed_dir = root.join("target-sealed");
    let sealed: Vec<rust_mutants::cargo::Compiled> = rust_mutants::sealed::BUILDS
        .into_iter()
        .map(|kind| build(root, kind, Some(TARGET), &sealed_dir))
        .collect();
    let sealed = SealedBuild::of(
        &native_targets,
        (&sealed, std::collections::BTreeMap::new()),
        (&packages, &sealed_dir),
    )
    .expect("the sealed build is read");
    (native_targets, sealed)
}

fn sysroot() -> PathBuf {
    let root = njutest_devkit::paths::fixtures_dir().join("fixture-simple");
    toolchain(&root)
        .sysroot()
        .expect("the pinned toolchain names its sysroot")
        .to_path_buf()
}

#[test]
fn the_pinned_toolchain_holds_the_sealed_targets_standard_library() {
    assert!(
        installed(&sysroot()).expect("the sysroot can be listed"),
        "no standard library for {TARGET} in {}: run `{}`",
        sysroot().display(),
        Unsealed::TargetMissing.remedy()
    );
}

#[test]
fn every_test_target_of_a_tree_that_builds_for_wasm_has_a_module_holding_its_sources() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        temp.path(),
    );
    let root = temp.path().canonicalize().expect("a physical spelling");
    let (native, sealed) = built(&root);
    assert!(!native.is_empty(), "the fixture has test targets");
    assert!(
        sealed.unsealed.is_empty(),
        "every target of the fixture builds for {TARGET}: {:?}",
        sealed.unsealed
    );
    for target in &native {
        let module = sealed
            .modules
            .get(target.id())
            .unwrap_or_else(|| panic!("{} has a sealed module", target.id()));
        assert_eq!(
            module
                .target
                .executable
                .extension()
                .and_then(|ext| ext.to_str()),
            Some("wasm"),
            "{} is a WebAssembly module",
            module.target.executable.display()
        );
    }
    let library = native
        .iter()
        .find(|target| target.id().contains("lib"))
        .expect("the fixture tests its library");
    assert!(
        sealed.compiled(library.id(), &root.join("src/lib.rs")),
        "the library's own source is compiled into its module: {:?}",
        sealed
            .modules
            .get(library.id())
            .map(|module| &module.sources)
    );
}

#[test]
fn a_target_that_does_not_build_for_wasm_is_unsealed_and_the_rest_still_seal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("a physical spelling");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::create_dir_all(root.join("tests")).expect("mkdir");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"half\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .expect("manifest");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn two() -> u8 { 2 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn two() { assert_eq!(super::two(), 2); }\n}\n",
    )
    .expect("library");
    std::fs::write(
        root.join("tests/host_only.rs"),
        "#[cfg(target_family = \"wasm\")]\ncompile_error!(\"not for wasm\");\n\n#[test]\nfn host() {}\n",
    )
    .expect("a test that builds on every host and never for wasm");
    let (native, sealed) = built(&root);
    let host_only = native
        .iter()
        .find(|target| target.id().contains("host_only"))
        .expect("the native build has the host-only test");
    assert_eq!(
        sealed.unsealed.get(host_only.id()),
        Some(&Unsealed::NotBuilt),
        "a target that does not build for {TARGET} is unsealed by name: {:?}",
        sealed.unsealed
    );
    assert_eq!(
        sealed.modules.len(),
        native.len() - 1,
        "every other target still seals: {:?}",
        sealed.modules.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_prepared_session_that_asks_for_sealing_holds_the_instrumented_trees_sealed_modules() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::Balanced)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let sealed = session.sealed();
    for target in session.targets() {
        match target.kind() {
            rust_mutants::execute::TargetKind::Doc => assert!(
                sealed.doctests.contains_key(target.id()),
                "a library's doctests are built for {TARGET} and captured: {:?}",
                sealed.unsealed
            ),
            rust_mutants::execute::TargetKind::ProcMacro => assert_eq!(
                sealed.unsealed.get(target.id()),
                Some(&Unsealed::ProcMacro),
                "a procedural macro's tests are unsealed as one"
            ),
            rust_mutants::execute::TargetKind::Lib
            | rust_mutants::execute::TargetKind::Bin
            | rust_mutants::execute::TargetKind::Test
            | rust_mutants::execute::TargetKind::Example => {
                let module = sealed.modules.get(target.id()).unwrap_or_else(|| {
                    panic!(
                        "{} of the instrumented tree builds for {TARGET}: {:?}",
                        target.id(),
                        sealed.unsealed
                    )
                });
                let metadata =
                    std::fs::metadata(&module.target.executable).unwrap_or_else(|error| {
                        panic!("{} is on disk: {error}", module.target.executable.display())
                    });
                assert!(
                    metadata.is_file(),
                    "{} is a file",
                    module.target.executable.display()
                );
            }
        }
    }
}

#[test]
fn a_prepared_session_that_does_not_ask_for_sealing_says_so_for_every_target() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::Off,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::Balanced)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    assert!(session.sealed().modules.is_empty());
    assert!(
        session
            .targets()
            .iter()
            .all(|target| session.sealed().unsealed.get(target.id()) == Some(&Unsealed::NotAsked)),
        "{:?}",
        session.sealed().unsealed
    );
}

#[test]
fn every_mutant_a_fixture_kills_natively_is_detected_by_a_sealed_execution_of_a_test_that_reaches_it()
 {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    for (target, station) in &bench.stations {
        for (test, control) in &station.controls {
            assert!(
                control.is_ok(),
                "{target} {test}: a control of a passing fixture passes sealed: {control:?}"
            );
        }
    }
    let mut detected = 0_usize;
    let mut undetected = Vec::new();
    for mutant in session.catalog().mutants() {
        let mut standing = None;
        for (target, station) in &bench.stations {
            for (test, control) in &station.controls {
                let Ok(control) = control else { continue };
                if !control.reached.contains(&mutant.index) {
                    continue;
                }
                let said = bench
                    .put(target, test, mutant.id.as_str())
                    .expect("the host runs the execution");
                if let Some(put) = said
                    && matches!(
                        put.came_to,
                        rust_mutants_decision::evidence::Sealed::Detected(_)
                    )
                {
                    standing = Some(put.came_to);
                }
            }
        }
        match standing {
            Some(_) => detected += 1,
            None => undetected.push(mutant.display_id.to_string()),
        }
    }
    assert_eq!(
        detected, 12,
        "the fixture's README names twelve kills; sealed executions detected {detected}, and not {undetected:?}"
    );
}

#[test]
fn every_mutant_of_a_sealable_fixture_stands_on_sealed_executions_alone() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let mut killed = 0_usize;
    let mut other = Vec::new();
    for mutant in session.catalog().mutants() {
        if !session.accepted().contains(&mutant.index) {
            continue;
        }
        let file = session.snapshot_root().join(&mutant.candidate.path);
        let answer = rust_mutants::sealed::standing::answer(
            (&bench, session.sealed()),
            mutant,
            &file,
            &session.route(mutant),
        )
        .expect("the host runs every execution");
        match answer.standing {
            rust_mutants_decision::evidence::Standing::Established(verdict)
                if matches!(
                    verdict.found(),
                    rust_mutants_decision::evidence::Found::Killed { .. }
                ) =>
            {
                killed += 1;
            }
            standing => other.push((mutant.display_id.to_string(), standing)),
        }
    }
    assert_eq!(
        killed, 12,
        "the README's twelve kills stand as sealed kills; the rest stood as {other:?}"
    );
    assert!(
        other.iter().all(|(_, standing)| matches!(
            standing,
            rust_mutants_decision::evidence::Standing::Established(_)
        )),
        "every mutant of a fixture that builds and passes sealed has a verdict: {other:?}"
    );
}

#[test]
fn a_sealed_build_refused_before_its_modules_exist_releases_its_cache() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let config = fixture.root().join(".cargo");
    std::fs::create_dir_all(&config).expect("the cargo configuration directory");
    std::fs::write(
        config.join("config.toml"),
        "[target.'cfg(debug_assertions)']\nrustflags = []\n",
    )
    .expect("a predicate only cargo decides");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&every_rule(), &Cancel::new())
    .expect("prepare");
    assert!(session.sealed().modules.is_empty());
    assert!(
        session
            .sealed()
            .unsealed
            .values()
            .all(|why| *why == Unsealed::FlagsUnmerged),
        "the target's flags could not be merged"
    );
    let cache = std::fs::read_dir(fixture.temp())
        .expect("the build caches")
        .map(|entry| entry.expect("a build cache").path())
        .find(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with("rust-mutants-target-sealed-"))
        })
        .expect("the claimed sealed build cache");
    assert!(
        rust_mutants::tempowner::read_marker(&cache)
            .expect("the owner marker")
            .released,
        "no modules hold a refused build's cache even while its process lives"
    );
}

#[test]
fn the_sealed_build_cache_is_released_only_when_its_last_modules_are_dropped() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All),
        &Cancel::new(),
    )
    .expect("prepare");
    let modules = session.sealed().clone();
    assert!(
        !modules.modules.is_empty(),
        "the session holds sealed modules"
    );
    let cache = std::fs::read_dir(fixture.temp())
        .expect("the build caches")
        .map(|entry| entry.expect("a build cache").path())
        .find(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with("rust-mutants-target-sealed-"))
        })
        .expect("the content-addressed sealed build cache");
    session.close().expect("the session closes");
    assert!(
        !rust_mutants::tempowner::read_marker(&cache)
            .expect("the owner marker")
            .released,
        "a copy of the modules still holds the build cache"
    );
    drop(modules);
    assert!(
        rust_mutants::tempowner::read_marker(&cache)
            .expect("the owner marker")
            .released,
        "the last modules let the build cache go even while their process lives"
    );
}

#[test]
fn a_run_cancelled_once_its_bench_stands_decides_no_mutant_a_sealed_test_would() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let cancel = Cancel::new();
    let bench = session
        .bench(&runner, &cancel)
        .expect("the bench is assembled");
    cancel.cancel();
    let mut interrupted = 0_usize;
    let mut decided = Vec::new();
    for mutant in session.catalog().mutants() {
        if !session.accepted().contains(&mutant.index) {
            continue;
        }
        match rust_mutants::run::sealed_verdict(&session, mutant, &bench)
            .expect("an interruption is no failure of the host")
        {
            rust_mutants::run::Sealing::Interrupted => interrupted += 1,
            rust_mutants::run::Sealing::Established(judged)
                if judged.not_run_reason == Some(rust_mutants::run::NotRunReason::Unreached) => {}
            sealing @ (rust_mutants::run::Sealing::Established(_)
            | rust_mutants::run::Sealing::Unproven(_)) => {
                decided.push((mutant.display_id.to_string(), sealing));
            }
        }
    }
    assert!(
        decided.is_empty() && interrupted >= 12,
        "once the run is cancelled no mutant is put to a sealed test, so only one no control \
         reached is decided; {interrupted} were interrupted, and these were decided: {decided:?}"
    );
    assert!(
        matches!(
            session.bench(&runner, &cancel),
            Err(rust_mutants::EngineError::Interrupted)
        ),
        "a bench asked for once the run is cancelled runs no control"
    );
}

#[test]
fn a_sealed_control_passes_under_every_harness_option_a_run_may_be_configured_with() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            harness_args: vec![
                "--test-threads=2".to_owned(),
                "--nocapture".to_owned(),
                "--show-output".to_owned(),
            ],
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::Balanced)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    assert!(
        bench
            .stations
            .values()
            .any(|station| !station.controls.is_empty()),
        "the fixture's tests are listed sealed: {:?}",
        bench.unsealed
    );
    for (target, station) in &bench.stations {
        for (test, control) in &station.controls {
            assert!(
                control.is_ok(),
                "{target} {test}: a sealed invocation sets the thread count and the capture \
                 itself, and a configured one gives way to it rather than being passed twice, \
                 which libtest refuses: {control:?}"
            );
        }
    }
}

/// A workspace whose member reads a file beside its manifest by a relative path and by the manifest directory its build baked in, in a unit test, an integration test and a doctest, with a decoy of the same name at the workspace's root.
const READING_WORKSPACE: [(&str, &str); 7] = [
    (
        "Cargo.toml",
        "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n",
    ),
    (
        "Cargo.lock",
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"reader\"\nversion = \"0.1.0\"\n",
    ),
    ("tests/data.txt", "the workspace root's own\n"),
    (
        "member/Cargo.toml",
        "[package]\nname = \"reader\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n",
    ),
    ("member/tests/data.txt", "beside the manifest\n"),
    (
        "member/src/lib.rs",
        "//! Reads a file the tests keep beside the manifest.\n\n/// The text of the file at `path`, trimmed, or nothing where it cannot be read.\n///\n/// ```\n/// assert_eq!(reader::trimmed(\"tests/data.txt\").as_deref(), Some(\"beside the manifest\"));\n/// ```\npub fn trimmed(path: impl AsRef<std::path::Path>) -> Option<String> {\n    match std::fs::read_to_string(path) {\n        Ok(text) => Some(text.trim().to_owned()),\n        Err(_unread) => None,\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_relative_path_is_read_from_the_package() {\n        assert_eq!(super::trimmed(\"tests/data.txt\").as_deref(), Some(\"beside the manifest\"));\n    }\n}\n",
    ),
    (
        "member/tests/reads.rs",
        "use std::path::Path;\n\n#[test]\nfn a_relative_path_is_read_from_the_package() {\n    assert_eq!(reader::trimmed(\"tests/data.txt\").as_deref(), Some(\"beside the manifest\"));\n}\n\n#[test]\nfn the_manifest_directory_is_read_however_it_is_joined() {\n    let manifest = Path::new(env!(\"CARGO_MANIFEST_DIR\"));\n    assert_eq!(\n        reader::trimmed(manifest.join(\"tests\").join(\"data.txt\")).as_deref(),\n        Some(\"beside the manifest\")\n    );\n    assert_eq!(\n        reader::trimmed(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/tests/data.txt\")).as_deref(),\n        Some(\"beside the manifest\")\n    );\n}\n",
    ),
];

#[test]
fn a_test_that_reads_the_tree_by_a_path_its_build_gave_it_passes_its_control_sealed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("tree");
    for (path, contents) in READING_WORKSPACE {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a directory")).expect("mkdir");
        std::fs::write(&file, contents).expect("the workspace's file");
    }
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("mkdir");
    let session = rust_mutants::workspace::Workspace::open(
        &root.canonicalize().expect("a physical spelling"),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            &scratch.canonicalize().expect("a physical spelling"),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let failed: Vec<String> = bench
        .stations
        .iter()
        .flat_map(|(target, station)| {
            station
                .controls
                .iter()
                .filter(|(_test, control)| control.is_err())
                .map(move |(test, control)| format!("{target} {test}: {control:?}"))
        })
        .collect();
    assert!(
        bench.unsealed.is_empty() && failed.is_empty(),
        "a test the native run passed reading beside its manifest passes its control sealed, as \
         cargo runs it, in its package's directory; unsealed: {:#?}, failed: {failed:#?}",
        bench.unsealed
    );
    let stations: Vec<&str> = bench.stations.keys().map(String::as_str).collect();
    assert_eq!(
        stations,
        [
            "reader/doc/reader",
            "reader/lib/reader",
            "reader/test/reads"
        ],
        "the member's doctests, unit tests and integration test each have a station"
    );
    let controlled: usize = bench
        .stations
        .values()
        .map(|station| station.controls.len())
        .sum();
    assert_eq!(controlled, 4, "every test is controlled");
}

#[test]
fn a_test_that_writes_where_cargo_gives_an_integration_test_to_write_passes_its_control_sealed() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-target-tmpdir");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let station = bench
        .stations
        .get("fixture-target-tmpdir/test/scratch")
        .expect("the integration test has a station");
    let control = station
        .controls
        .get("what_is_kept_in_the_targets_scratch_reads_back")
        .expect("the station holds the test");
    assert!(
        control.is_ok(),
        "a test that writes into CARGO_TARGET_TMPDIR, as cargo lets an integration test, passes \
         its control sealed: {control:?}"
    );
}

#[test]
fn a_test_that_reads_where_its_build_script_wrote_passes_its_control_sealed() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-build-script");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let station = bench
        .stations
        .get("fixture-build-script/lib/fixture_build_script")
        .expect("the unit tests have a station");
    let failed: Vec<String> = station
        .controls
        .iter()
        .filter(|(_test, control)| control.is_err())
        .map(|(test, control)| format!("{test}: {control:?}"))
        .collect();
    assert!(
        failed.is_empty() && station.controls.len() == 3,
        "a test that reads, through OUT_DIR, what its build script wrote there passes its control \
         sealed, as it does natively: {failed:#?}"
    );
}

#[test]
fn a_test_that_keeps_files_in_the_temporary_directory_passes_sealed_however_it_asks_for_it() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-temporary");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &rust_mutants::session::PrepareOptions {
            sealing: Sealing::On,
            ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let station = bench
        .stations
        .get("fixture-temporary/test/scratch")
        .expect("the integration test has a station");
    let why: Vec<(&str, Option<&str>)> = station
        .controls
        .iter()
        .map(|(test, control)| {
            let why = match control {
                Ok(_control) => None,
                Err(why) => Some(why.name()),
            };
            (test.as_str(), why)
        })
        .collect();
    assert_eq!(
        why,
        [
            (
                "a_file_kept_in_the_temporary_directory_of_std_reads_back",
                None
            ),
            ("a_file_kept_where_tmpdir_names_reads_back", None),
        ],
        "a file kept where TMPDIR names, or where `std::env::temp_dir` answers, which the sealed \
         build makes the standard library read from TMPDIR, is kept in the instance's own \
         temporary directory"
    );
}

#[test]
fn a_test_that_keeps_a_setting_under_the_home_directory_passes_its_control_sealed() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-home");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&every_rule(), &Cancel::new())
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let control = bench
        .stations
        .get("fixture-home/test/writes")
        .and_then(|station| {
            station
                .controls
                .get("a_setting_kept_is_the_setting_recalled")
        })
        .expect("the station holds the test");
    assert!(
        control.is_ok(),
        "`std::env::home_dir`, which the sealed target's standard library compiles into its \
         caller as no home at all, answers the home HOME names in a sealed build, so a test \
         that keeps a setting there passes its control sealed: {control:?}"
    );
}

#[test]
fn a_mutant_that_sends_a_test_to_an_absolute_path_meets_a_refusal_rather_than_the_package() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-absolute-path");
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&every_rule(), &Cancel::new())
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let target = "fixture-absolute-path/test/reads";
    let test = "the_setting_beside_the_manifest_is_read_by_a_relative_path";
    let control = bench
        .stations
        .get(target)
        .and_then(|station| station.controls.get(test))
        .expect("the station holds the test");
    assert!(
        control.is_ok(),
        "a relative path is read from the package's directory, where the guest starts: {control:?}"
    );
    let mut came_to = Vec::new();
    for mutant in session.catalog().mutants() {
        if !matches!(
            mutant.candidate.rule.name,
            "negate-condition" | "condition-to-true"
        ) {
            continue;
        }
        let said = bench
            .put(target, test, mutant.id.as_str())
            .expect("the host runs the execution");
        came_to.push((mutant.candidate.rule.name, said.map(|put| put.came_to)));
    }
    assert_eq!(
        came_to,
        [
            (
                "negate-condition",
                Some(rust_mutants_decision::evidence::Sealed::Doubted(
                    rust_mutants_decision::evidence::Doubt::Refused
                ))
            ),
            (
                "condition-to-true",
                Some(rust_mutants_decision::evidence::Sealed::Doubted(
                    rust_mutants_decision::evidence::Doubt::Refused
                ))
            ),
        ],
        "`/setting.txt` names no place in the tree, so it is refused rather than read from the \
         package's directory, where a file of that name lies and the machine's root holds none"
    );
}

/// The options every preparation of a fixture below is made with, the one that records and the one that runs again alike.
fn every_rule() -> rust_mutants::session::PrepareOptions {
    rust_mutants::session::PrepareOptions {
        sealing: Sealing::On,
        ..rust_mutants::session::PrepareOptions::new(rust_mutants::rule::Tier::All)
    }
}

/// Every sealed execution a run of `fixture` puts its accepted mutants to, as a report records them.
fn recorded_by_a_run(fixture: &njutest_devkit::fixture::Fixture) -> Vec<Recorded> {
    let session = rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::testkit::opening::opening(
            &njutest_devkit::paths::cargo_binary(),
            fixture.temp(),
        ),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&every_rule(), &Cancel::new())
    .expect("prepare");
    let runner = rust_mutants_sealed::SealedRunner::new(rust_mutants::sealed::bench::WATCHDOG)
        .expect("the sealed runner starts");
    let bench = session
        .bench(&runner, &Cancel::new())
        .expect("the bench is assembled");
    let mut recorded = Vec::new();
    for mutant in session.catalog().mutants() {
        if !session.accepted().contains(&mutant.index) {
            continue;
        }
        let answer = rust_mutants::sealed::standing::answer(
            (&bench, session.sealed()),
            mutant,
            &session.snapshot_root().join(&mutant.candidate.path),
            &session.route(mutant),
        )
        .expect("the host runs every execution");
        recorded.extend(answer.puts.iter().map(|put| Recorded {
            mutant: mutant.id.to_string(),
            target: put.target.clone(),
            test: put.test.clone(),
            came_to: Came::of(put.came_to),
        }));
    }
    drop(bench);
    session
        .close()
        .expect("the recording run's snapshot is removed");
    recorded
}

/// `fixture` prepared to run recorded executions again, with every event the preparation records going to `trace`.
fn rerunnable(
    fixture: &njutest_devkit::fixture::Fixture,
    trace: Recorder,
) -> rust_mutants::session::Rerunnable {
    rust_mutants::workspace::Workspace::open(
        fixture.root(),
        rust_mutants::workspace::OpenOptions {
            trace,
            ..rust_mutants::testkit::opening::opening(
                &njutest_devkit::paths::cargo_binary(),
                fixture.temp(),
            )
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare_to_rerun(&every_rule(), &Cancel::new())
    .expect("prepared to run recorded executions again")
}

#[test]
fn a_preparation_to_rerun_starts_no_test_natively_and_reproduces_every_execution() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let recorded = recorded_by_a_run(&fixture);
    assert_eq!(
        recorded.iter().filter(|one| one.came_to.detected()).count(),
        12,
        "the README's twelve kills each rest on one sealed detection: {recorded:?}"
    );
    let trace = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let prepared = rerunnable(&fixture, trace.clone());
    let mut phases: Vec<String> = Vec::new();
    for event in trace.events() {
        if let Payload::PhaseStart { phase } = event.payload {
            phases.push(phase.name);
        }
    }
    assert!(
        phases.iter().any(|phase| phase == "build")
            && !phases
                .iter()
                .any(|phase| phase == "verify" || phase == "coverage"),
        "the preparation builds as a run does and starts no test natively, since a recorded \
         execution names its test and no native baseline has to say which tests are the suite's: \
         {phases:?}"
    );
    let Reproduction::Reproduced(again) = prepared
        .rerun(&recorded, &Cancel::new())
        .expect("the host runs every execution again")
    else {
        panic!("every recorded execution is reproduced");
    };
    assert_eq!(again.len(), recorded.len());
    assert!(
        again.iter().zip(&recorded).all(|(again, recorded)| {
            again.recorded == *recorded
                && again.same()
                && matches!(&again.now, Now::Came { transcript, .. } if transcript.len() == 64)
        }),
        "every recorded execution runs again, each test's control first, and comes to what it \
         came to"
    );
    let spent = trace
        .sealed_counts()
        .spent()
        .expect("the trace counts the host's work when reproducing a stored report");
    assert!(spent.compiles > 0 && spent.instances >= 12);
    assert_eq!(
        spent.answered, 0,
        "a report reissue runs every execution again"
    );
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(
        matches!(
            prepared.rerun(&recorded, &cancel),
            Err(rust_mutants::EngineError::Interrupted)
        ),
        "a run cancelled before its executions ran again reproduces nothing"
    );
    prepared.close().expect("the snapshot is removed");
}

#[test]
fn running_recorded_executions_again_stops_at_the_first_that_comes_to_something_else_now() {
    let fixture = njutest_devkit::fixture::Fixture::copy("fixture-simple");
    let recorded = recorded_by_a_run(&fixture);
    let prepared = rerunnable(&fixture, Recorder::disabled());
    let flipped = recorded
        .iter()
        .position(|one| one.came_to.detected())
        .expect("a detection");
    let mut contradicted = recorded.clone();
    contradicted
        .get_mut(flipped)
        .expect("the detection found")
        .came_to = Came::Passed;
    let Reproduction::Differed { agreed, first } = prepared
        .rerun(&contradicted, &Cancel::new())
        .expect("the host runs every execution again")
    else {
        panic!("a recorded pass the mutant is detected by now is a difference");
    };
    let came_to_now = |now: &Now| match now {
        Now::Came { came_to, .. } => Some(*came_to),
        Now::Unmade(_) => None,
    };
    assert_eq!(
        (agreed.len(), Some(&first.recorded), came_to_now(&first.now)),
        (
            flipped,
            contradicted.get(flipped),
            recorded.get(flipped).map(|truly| truly.came_to)
        ),
        "the executions before the contradicted one agree, it comes to what it truly comes to, \
         and nothing after it runs"
    );

    let uncataloged = Recorded {
        mutant: "0".repeat(64),
        ..recorded.first().expect("a recorded execution").clone()
    };
    assert_eq!(
        prepared
            .rerun(std::slice::from_ref(&uncataloged), &Cancel::new())
            .expect("nothing had to run"),
        Reproduction::Differed {
            agreed: Vec::new(),
            first: Reran {
                recorded: uncataloged.clone(),
                now: Now::Unmade(Unmade::Uncataloged),
            },
        },
        "an execution of a mutant this tree does not catalog cannot be made again, which is a \
         difference and never an agreement"
    );
    prepared.close().expect("the snapshot is removed");
}
