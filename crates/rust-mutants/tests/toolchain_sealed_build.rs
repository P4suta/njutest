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
use rust_mutants::sealed::{SealedBuild, TARGET, Unsealed, installed};
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
    let sealed = SealedBuild::of(&native_targets, &sealed, (&packages, &sealed_dir))
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
        root.join("tests/unix_only.rs"),
        "use std::os::unix::fs::PermissionsExt as _;\n\n#[test]\nfn mode() { let _ = std::fs::Permissions::from_mode(0o644); }\n",
    )
    .expect("a test that only builds on unix");
    let (native, sealed) = built(&root);
    let unix_only = native
        .iter()
        .find(|target| target.id().contains("unix_only"))
        .expect("the native build has the unix-only test");
    assert_eq!(
        sealed.unsealed.get(unix_only.id()),
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
