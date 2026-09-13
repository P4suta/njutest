// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The paths every suite resolves through the devkit.

use mjutest_devkit::paths::{cargo_binary, fixtures_dir, workspace_root};

const ACTIVE: &str = "RUST_MUTANTS_ACTIVE";
const CATALOG: &str = "RUST_MUTANTS_CATALOG";
const TOUCH: &str = "RUST_MUTANTS_TOUCH";
const PROFILE: &str = "LLVM_PROFILE_FILE";
const EXPECTED_ACTIVE: &str = "MJUTEST_DEVKIT_EXPECTED_ACTIVE";
const EXPECTED_CATALOG: &str = "MJUTEST_DEVKIT_EXPECTED_CATALOG";
const EXPECTED_TOUCH: &str = "MJUTEST_DEVKIT_EXPECTED_TOUCH";

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
    const STAGE: &str = "MJUTEST_DEVKIT_INNER_RUN_STAGE";
    if std::env::var_os(STAGE).is_some() {
        let names: Vec<std::ffi::OsString> = mjutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for removed in [ACTIVE, CATALOG, TOUCH, PROFILE] {
            assert!(
                !names.iter().any(|name| name == removed),
                "an inner run inherited {removed}"
            );
        }
        return;
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
        .env(STAGE, "inspect")
        .env(PROFILE, profiles.path().join("profiles-%p.profraw"));
    add_synthetic_mutation_identity(&mut command);
    let output = command.output().expect("the nested test runs");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_subprocess_keeps_mutation_identity_and_drops_only_the_coverage_sink() {
    const STAGE: &str = "MJUTEST_DEVKIT_SUBPROCESS_STAGE";
    match std::env::var(STAGE).as_deref() {
        Ok("forward") => {
            let mut command =
                mjutest_devkit::paths::command(&std::env::current_exe().expect("this test binary"));
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
