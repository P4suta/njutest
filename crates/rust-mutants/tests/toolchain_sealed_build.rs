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
use rust_mutants::sealed::{SealedBuild, Sealing, TARGET, Unsealed, installed};
use rust_mutants::trace::Recorder;

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
    let bench = session.bench(&runner).expect("the bench is assembled");
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
                if matches!(
                    said,
                    Some(rust_mutants_decision::evidence::Sealed::Detected(_))
                ) {
                    standing = said;
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
    let bench = session.bench(&runner).expect("the bench is assembled");
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
