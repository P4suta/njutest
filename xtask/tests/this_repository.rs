// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every assertion whose subject is this repository's committed tree, as a binary apart from the suite, which a measurement that copies and rewrites the tree leaves out.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use xtask::gates;

include!("support/uncoded.rs");

/// A gate refusal, or a report whose contract was not the one under test.
#[derive(Debug, thiserror::Error)]
enum TestError {
    /// A repository gate refused the tree.
    #[error(transparent)]
    Gate(#[from] gates::GateError),
    /// A path returned by the production source walker escaped its root.
    #[error("{} is outside {}", path.display(), root.display())]
    OutsideRoot {
        path: std::path::PathBuf,
        root: std::path::PathBuf,
        #[source]
        source: std::path::StripPrefixError,
    },
    /// A committed source path was not exact UTF-8.
    #[error("{} is not UTF-8", path.display())]
    NonUtf8Path { path: std::path::PathBuf },
    /// A gate returned a report whose contract was not the one under test.
    #[error("{0}")]
    Contract(String),
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), TestError> {
    if condition {
        Ok(())
    } else {
        Err(TestError::Contract(message.into()))
    }
}

/// Runs git in `root` with nothing in the environment pointing it at another repository.
fn git(root: &Path, arguments: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(arguments);
    for variable in gates::REDIRECTING_GIT {
        command.env_remove(variable);
    }
    command.output().expect("git runs")
}

#[test]
fn the_seam_ledger_agrees_with_the_tree() -> Result<(), TestError> {
    let report = gates::devgates(&gates::workspace_root())?;
    require(report.starts_with("devgates: "), report)
}

#[test]
fn every_internal_dependency_points_in_the_allowed_direction() -> Result<(), TestError> {
    let report = gates::deps(&gates::workspace_root())?;
    require(report.starts_with("deps: "), report)
}

#[test]
fn every_fixture_follows_the_conventions() -> Result<(), TestError> {
    let report = gates::fixtures(&gates::workspace_root())?;
    require(report.starts_with("fixtures: "), report)
}

#[test]
fn the_release_versions_agree() -> Result<(), TestError> {
    let report = gates::release_check(&gates::workspace_root())?;
    require(report.starts_with("release-check: "), report)
}

#[test]
fn every_milestone_reference_resolves_to_the_roadmap() -> Result<(), TestError> {
    let report = gates::milestones(&gates::workspace_root())?;
    require(report.starts_with("milestones: "), report)
}

#[test]
fn every_crate_surface_has_a_compiler_checked_meaning() -> Result<(), TestError> {
    let report = gates::surfaces(&gates::workspace_root())?;
    require(report.starts_with("surfaces: "), report)
}

#[test]
fn production_sources_exclude_test_support() -> Result<(), TestError> {
    let root = gates::workspace_root();
    let files: Vec<String> = gates::production_sources(&root)?
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .map_err(|source| TestError::OutsideRoot {
                    path: path.clone(),
                    root: root.clone(),
                    source,
                })?;
            let relative = relative
                .to_str()
                .ok_or_else(|| TestError::NonUtf8Path { path: path.clone() })?;
            Ok(relative.replace('\\', "/"))
        })
        .collect::<Result<_, TestError>>()?;
    require(
        files.iter().any(|f| f == "crates/rust-mutants/src/lib.rs"),
        format!("missing production library from {files:?}"),
    )?;
    require(
        files.iter().any(|f| f == "xtask/src/devgates.rs"),
        format!("missing production gate from {files:?}"),
    )?;
    require(
        files
            .iter()
            .all(|f| !f.starts_with("crates/njutest-devkit/")),
        format!("test support entered production sources: {files:?}"),
    )?;
    require(
        files.iter().all(|f| !f.contains("/tests/")),
        format!("tests entered production sources: {files:?}"),
    )
}

#[test]
fn a_function_behind_a_test_feature_is_test_support_rather_than_an_unreached_capability() {
    let root = gates::workspace_root();
    let report = gates::reached(&root).expect("the tree is clean under this gate");
    assert!(
        report.contains("0 public function"),
        "every public function of an incidental surface is reached by something that ships. \
         Sixteen were reported before this gate read `cfg(feature = \"testkit\")` as the \
         declaration of test support that it is: {report}"
    );
}

/// Every source file of xtask's library, by its path from the workspace root.
fn xtask_sources() -> Vec<(String, String)> {
    let root = njutest_devkit::paths::workspace_root();
    let mut sources: Vec<(String, String)> = walkdir::WalkDir::new(root.join("xtask/src"))
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("xtask/src is walkable"))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
        })
        .map(|entry| {
            let path = entry
                .path()
                .strip_prefix(&root)
                .expect("under the workspace root")
                .display()
                .to_string();
            let text = std::fs::read_to_string(entry.path()).expect("a source file reads");
            (path, text)
        })
        .collect();
    sources.sort();
    sources
}

#[test]
fn every_xtask_error_type_carries_a_code() {
    let sources = xtask_sources();
    assert!(
        sources.len() > 20,
        "the walk reads xtask's library: {}",
        sources.len()
    );
    let uncoded = uncoded(&sources);
    assert!(
        uncoded.is_empty(),
        "an xtask failure is read by its code, so every error type implements \
         `xtask::error::Coded` in the file that declares it, and the gate maps it with \
         `error.coded()`; these have none:\n{}",
        uncoded.join("\n")
    );
}

#[test]
fn this_repository_commits_no_build_output_and_ignores_every_target_directory() {
    let root = gates::workspace_root();
    let passed = gates::tracked(&root).expect("nothing a build wrote is committed");
    assert!(passed.contains("tracked paths read"), "{passed}");
    let ignored = git(
        &root,
        &[
            "check-ignore",
            "-q",
            "--no-index",
            "crates/njutest-macros/target/tests/trybuild/CACHEDIR.TAG",
        ],
    );
    assert!(
        ignored.status.success(),
        "a target directory inside a crate is ignored as the root's is, so `git add -A` never \
         stages what trybuild or a crate-local build wrote: exit {:?}",
        ignored.status.code()
    );
}
