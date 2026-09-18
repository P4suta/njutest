// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The paths every suite resolves through the devkit.

use njutest_devkit::paths::{cargo_binary, fixtures_dir, workspace_root};

const ACTIVE: &str = "RUST_MUTANTS_ACTIVE";
const CATALOG: &str = "RUST_MUTANTS_CATALOG";
const TOUCH: &str = "RUST_MUTANTS_TOUCH";
const PROFILE: &str = "LLVM_PROFILE_FILE";
const COVERAGE: &str = "CARGO_LLVM_COV";
const COVERAGE_TARGET: &str = "CARGO_LLVM_COV_TARGET_DIR";
const COVERAGE_PRIVATE: &str = "__CARGO_LLVM_COV_RUSTC_WRAPPER";
const RUSTC_WRAPPER: &str = "RUSTC_WRAPPER";
const EXPECTED_ACTIVE: &str = "NJUTEST_DEVKIT_EXPECTED_ACTIVE";
const EXPECTED_CATALOG: &str = "NJUTEST_DEVKIT_EXPECTED_CATALOG";
const EXPECTED_TOUCH: &str = "NJUTEST_DEVKIT_EXPECTED_TOUCH";

fn add_synthetic_mutation_identity(command: &mut std::process::Command) {
    if option_env!("RUST_MUTANTS_COMPILED_CATALOG").is_none() {
        command
            .env(ACTIVE, "active")
            .env(CATALOG, "catalog")
            .env(TOUCH, "touch");
    }
}

#[test]
fn workspace_root_holds_the_workspace_manifest() {
    let root = workspace_root();
    assert!(root.is_absolute(), "{}", root.display());
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("root Cargo.toml");
    assert!(
        manifest.contains("[workspace]"),
        "not the workspace root: {}",
        root.display()
    );
}

#[test]
fn fixtures_dir_is_the_fixtures_directory_of_the_workspace() {
    assert_eq!(fixtures_dir(), workspace_root().join("fixtures"));
    assert!(fixtures_dir().is_dir(), "{}", fixtures_dir().display());
}

#[test]
fn cargo_binary_is_the_cargo_that_built_the_tests() {
    let cargo = cargo_binary();
    let output = std::process::Command::new(&cargo)
        .arg("--version")
        .output()
        .expect("runs");
    assert!(output.status.success(), "{}", cargo.display());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
}

#[test]
fn an_in_process_inner_run_receives_none_of_the_outer_measurement() {
    const STAGE: &str = "NJUTEST_DEVKIT_INNER_RUN_STAGE";
    match std::env::var(STAGE).as_deref() {
        Ok("coverage") => {
            let names: Vec<std::ffi::OsString> = njutest_devkit::paths::environment_for_a_run()
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            for removed in [
                ACTIVE,
                CATALOG,
                TOUCH,
                PROFILE,
                COVERAGE,
                COVERAGE_TARGET,
                COVERAGE_PRIVATE,
                RUSTC_WRAPPER,
            ] {
                assert!(
                    !names.iter().any(|name| name == removed),
                    "an inner run inherited {removed}"
                );
            }
            return;
        }
        Ok("ordinary-wrapper") => {
            let environment = njutest_devkit::paths::environment_for_a_run();
            assert!(environment.iter().any(|(name, value)| {
                name == RUSTC_WRAPPER && value == "callers-own-rustc-wrapper"
            }));
            return;
        }
        Ok(other) => panic!("unknown test stage {other}"),
        Err(_) => {}
    }

    let profiles = tempfile::tempdir().expect("a profile directory");
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args([
            "--exact",
            "an_in_process_inner_run_receives_none_of_the_outer_measurement",
        ])
        .current_dir(profiles.path())
        .env(STAGE, "coverage")
        .env(PROFILE, profiles.path().join("profiles-%p.profraw"))
        .env(COVERAGE, "1")
        .env(COVERAGE_TARGET, profiles.path().join("outer-target"))
        .env(COVERAGE_PRIVATE, "1")
        .env(RUSTC_WRAPPER, "outer-coverage-wrapper");
    add_synthetic_mutation_identity(&mut command);
    let output = command.output().expect("the nested test runs");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "an_in_process_inner_run_receives_none_of_the_outer_measurement",
        ])
        .current_dir(profiles.path())
        .env(STAGE, "ordinary-wrapper")
        .env_remove(COVERAGE)
        .env_remove(COVERAGE_TARGET)
        .env_remove(COVERAGE_PRIVATE)
        .env(RUSTC_WRAPPER, "callers-own-rustc-wrapper")
        .output()
        .expect("the ordinary-wrapper test runs");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_subprocess_keeps_mutation_identity_and_drops_only_the_coverage_sink() {
    const STAGE: &str = "NJUTEST_DEVKIT_SUBPROCESS_STAGE";
    match std::env::var(STAGE).as_deref() {
        Ok("forward") => {
            let mut command =
                njutest_devkit::paths::command(&std::env::current_exe().expect("this test binary"));
            command
                .args([
                    "--exact",
                    "a_subprocess_keeps_mutation_identity_and_drops_only_the_coverage_sink",
                ])
                .env(STAGE, "inspect");
            for (name, expected) in [
                (ACTIVE, EXPECTED_ACTIVE),
                (CATALOG, EXPECTED_CATALOG),
                (TOUCH, EXPECTED_TOUCH),
            ] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(expected, value);
                } else {
                    command.env_remove(expected);
                }
            }
            let output = command.output().expect("the subprocess runs");
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Ok("inspect") => {
            for (name, expected) in [
                (ACTIVE, EXPECTED_ACTIVE),
                (CATALOG, EXPECTED_CATALOG),
                (TOUCH, EXPECTED_TOUCH),
            ] {
                assert_eq!(std::env::var_os(name), std::env::var_os(expected), "{name}");
            }
            assert_eq!(std::env::var_os(PROFILE), None);
            return;
        }
        Ok(other) => panic!("unknown test stage {other}"),
        Err(_) => {}
    }

    let profiles = tempfile::tempdir().expect("a profile directory");
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args([
            "--exact",
            "a_subprocess_keeps_mutation_identity_and_drops_only_the_coverage_sink",
        ])
        .current_dir(profiles.path())
        .env(STAGE, "forward")
        .env(PROFILE, profiles.path().join("profiles-%p.profraw"));
    add_synthetic_mutation_identity(&mut command);
    let output = command.output().expect("the forwarding test runs");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn only_a_compilation_cache_is_handed_to_a_nested_run_through_the_wrapper() {
    use njutest_devkit::paths::names_a_cache;
    use std::ffi::OsStr;

    for named in ["sccache", "/opt/homebrew/bin/sccache", "SCCACHE.EXE"] {
        assert!(
            names_a_cache(OsStr::new(named)),
            "a cache keyed on content is what makes three hundred isolated fixture \
             builds affordable, and it is safe to hand on because it changes nothing \
             about what is compiled: {named}"
        );
    }
    for named in [
        "/Users/somebody/.cargo/bin/cargo-llvm-cov",
        "/tmp/coverage-shim",
        "",
    ] {
        assert!(
            !names_a_cache(OsStr::new(named)),
            "and everything else under this name is refused, because what \
             `cargo-llvm-cov` puts here instruments whatever it wraps — a coverage run \
             of this suite that let that reach a fixture would be measuring its own \
             instrumentation rather than the fixture's tests: {named:?}"
        );
    }
}
