// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving the fuzz targets a tree holds: what is found, what is driven, and what a crash becomes.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;

use mjutest_cli::assure::fuzz::{found, targets_of};

fn tree(targets: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let holding = dir.path().join("fuzz/fuzz_targets");
    std::fs::create_dir_all(&holding).expect("mkdir");
    for target in targets {
        std::fs::write(holding.join(format!("{target}.rs")), "// a target\n").expect("write");
    }
    std::fs::write(holding.join("README.md"), "not a target\n").expect("write");
    dir
}

#[test]
fn the_targets_a_tree_holds_are_the_rust_files_of_its_fuzz_directory() {
    let dir = tree(&["parse", "decode"]);
    assert_eq!(targets_of(dir.path()), ["decode", "parse"]);
}

#[test]
fn a_tree_with_no_fuzz_directory_holds_no_targets() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(targets_of(dir.path()).is_empty());
    assert!(targets_of(Path::new("/nonexistent")).is_empty());
}

#[test]
fn targets_that_are_here_and_were_not_driven_are_said_to_be() {
    let limitation = found(&["parse".to_owned()]);
    assert_eq!(limitation.name, "fuzz-not-executed");
    assert!(limitation.detail.contains("parse"), "{limitation:?}");
}

#[cfg(unix)]
mod driving {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use mjutest_cli::assure::fuzz::{Fuzzing, fuzz};
    use mjutest_cli::report::FindingKind;
    use mjutest_cli::trace::Recorder;
    use mjutest_cli::watch::Watch;
    use rust_mutants::runner::Cancel;

    use super::tree;

    fn cargo(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;

        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn driven(cargo: &Path, root: &Path, targets: &[String]) -> mjutest_cli::assure::fuzz::Fuzzed {
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        fuzz(
            &Fuzzing {
                root,
                cargo,
                env: std::env::vars_os()
                    .filter(|(name, _)| name == "PATH")
                    .collect(),
                targets,
                max_total_time: Duration::from_secs(1),
                timeout: Some(Duration::from_secs(30)),
            },
            Watch::new(&cancel, &trace),
        )
    }

    #[test]
    fn a_target_that_finds_nothing_leaves_nothing_to_apply() {
        let dir = tree(&["parse"]);
        let command = cargo(dir.path(), "cargo-quiet", "echo 'Done 1000 runs'\nexit 0");
        let done = driven(&command, dir.path(), &[]);
        assert_eq!(done.ran, ["parse"]);
        assert!(done.crashes.is_empty(), "{done:?}");
        assert!(done.findings.is_empty(), "{done:?}");
    }

    #[test]
    fn an_input_that_crashes_a_target_is_kept_as_a_candidate_for_the_corpus() {
        let dir = tree(&["parse"]);
        let command = cargo(
            dir.path(),
            "cargo-crash",
            "mkdir -p fuzz/artifacts/parse\nprintf 'bad input' > fuzz/artifacts/parse/crash-abc\n\
             echo 'thread panicked'\nexit 77",
        );
        let done = driven(&command, dir.path(), &[]);
        assert_eq!(done.ran, ["parse"]);
        let crash = done.crashes.first().expect("a crash");
        assert_eq!(crash.target, "parse");
        assert_eq!(crash.artifact, "fuzz/artifacts/parse/crash-abc");
        assert_eq!(crash.corpus, "fuzz/corpus/parse/crash-abc");
        assert_eq!(crash.content, b"bad input");
        assert_eq!(
            done.findings.first().map(|one| one.kind),
            Some(FindingKind::FailingTest)
        );
    }

    #[test]
    fn an_artifact_that_was_already_there_is_not_a_crash_this_run_found() {
        let dir = tree(&["parse"]);
        std::fs::create_dir_all(dir.path().join("fuzz/artifacts/parse")).expect("mkdir");
        std::fs::write(
            dir.path().join("fuzz/artifacts/parse/crash-old"),
            "old input",
        )
        .expect("write");
        let command = cargo(dir.path(), "cargo-quiet", "echo 'Done'\nexit 0");
        let done = driven(&command, dir.path(), &[]);
        assert!(done.crashes.is_empty(), "{done:?}");
    }

    #[test]
    fn a_toolchain_with_no_fuzzer_says_what_it_did_not_drive() {
        let dir = tree(&["parse"]);
        let command = cargo(
            dir.path(),
            "cargo-bare",
            "echo \"error: no such command: \\`fuzz\\`\"\nexit 101",
        );
        let done = driven(&command, dir.path(), &[]);
        assert!(done.ran.is_empty(), "{done:?}");
        assert_eq!(
            done.limitations.first().map(|one| one.name.clone()),
            Some("cargo-fuzz-unavailable".to_owned())
        );
        assert_eq!(
            done.findings.first().map(|one| one.kind),
            Some(FindingKind::NotMeasured)
        );
    }

    #[test]
    fn only_the_targets_the_configuration_names_are_driven() {
        let dir = tree(&["parse", "decode"]);
        let command = cargo(dir.path(), "cargo-quiet", "echo 'Done'\nexit 0");
        let done = driven(&command, dir.path(), &["decode".to_owned()]);
        assert_eq!(done.ran, ["decode"]);
    }
}
