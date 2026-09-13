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
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use mjutest_cli::assure::fuzz::{Fuzzing, fuzz};
    use mjutest_cli::report::FindingKind;
    use mjutest_cli::trace::{Clock, MemorySink, Payload, Recorder, Sink, StartRecord};
    use mjutest_cli::watch::Watch;
    use rust_mutants::runner::Cancel;

    use super::tree;

    /// The cargo every test here drives, read rather than written: see the script's own note.
    fn cargo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-cargo.sh")
    }

    /// What that cargo is told to say, what it is told to leave behind, and how it is told to end.
    fn saying(said: &str, code: i32, artifact: Option<&str>) -> Vec<(OsString, OsString)> {
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(name, _)| name == "PATH")
            .collect();
        env.push((OsString::from("FAKE_CARGO_SAYS"), OsString::from(said)));
        env.push((
            OsString::from("FAKE_CARGO_CODE"),
            OsString::from(code.to_string()),
        ));
        if let Some(artifact) = artifact {
            env.push((
                OsString::from("FAKE_CARGO_ARTIFACT"),
                OsString::from(artifact),
            ));
        }
        env
    }

    fn driven(
        env: Vec<(OsString, OsString)>,
        root: &Path,
        targets: &[String],
    ) -> mjutest_cli::assure::fuzz::Fuzzed {
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        let cargo = cargo();
        fuzz(
            &Fuzzing {
                root,
                cargo: &cargo,
                env,
                targets,
                max_total_time: Duration::from_secs(1),
                timeout: Some(Duration::from_secs(30)),
            },
            Watch::new(&cancel, &trace),
        )
    }

    fn recording() -> Recorder {
        Recorder::new(
            Sink::Memory(MemorySink::unbounded()),
            Clock::Wall,
            StartRecord::of(
                "fuzz-contract",
                mjutest_cli::report::RunKind::Full,
                mjutest_cli::config::Contract::StandardV1,
            ),
        )
    }

    #[test]
    fn a_target_that_finds_nothing_leaves_nothing_to_apply() {
        let dir = tree(&["parse"]);
        let done = driven(saying("Done 1000 runs", 0, None), dir.path(), &[]);
        assert_eq!(done.ran, ["parse"]);
        assert!(done.crashes.is_empty(), "{done:?}");
        assert!(done.findings.is_empty(), "{done:?}");
    }

    #[test]
    fn a_target_stopped_before_its_time_was_up_was_driven_for_less_than_it_was_asked() {
        let dir = tree(&["parse"]);
        let mut env = saying("Done 1 runs", 0, None);
        env.push((OsString::from("FAKE_CARGO_SLEEP"), OsString::from("5")));
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        let cargo = cargo();
        let done = fuzz(
            &Fuzzing {
                root: dir.path(),
                cargo: &cargo,
                env,
                targets: &[],
                max_total_time: Duration::from_secs(1),
                timeout: Some(Duration::from_millis(300)),
            },
            Watch::new(&cancel, &trace),
        );

        assert_eq!(
            done.ran,
            ["parse"],
            "a fuzzer that was stopped was still driven: what it covered before the \
             bound is covered, and calling it undriven would throw that away: {done:?}"
        );
        assert!(
            done.limitations.iter().any(|one| one
                .detail
                .contains("ran out of time before it was driven for as long as it was asked")),
            "and how far short it fell is a different thing to do about from a fuzzer \
             that could not be run at all — more time against a toolchain to fix: \
             {done:?}"
        );
    }

    #[test]
    fn a_target_the_fuzzer_never_started_is_not_one_that_found_nothing() {
        let dir = tree(&["parse"]);
        let done = driven(
            saying("error: could not compile `a-fuzz`", 101, None),
            dir.path(),
            &[],
        );
        assert!(
            done.ran.is_empty(),
            "libFuzzer exits non-zero when it finds something, and something is an input \
             it keeps. A status with nothing kept is a target that never started, and \
             counting it as driven says the fuzzer looked and found nothing: {done:?}"
        );
        assert_eq!(
            done.findings.first().map(|one| one.kind),
            Some(FindingKind::NotMeasured),
            "what was asked for and not done is a finding: {done:?}"
        );
    }

    #[test]
    fn an_input_that_crashes_a_target_is_kept_as_a_candidate_for_the_corpus() {
        let dir = tree(&["parse"]);
        let done = driven(
            saying(
                "thread panicked",
                77,
                Some("fuzz/artifacts/parse/crash-abc"),
            ),
            dir.path(),
            &[],
        );
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
        let done = driven(saying("Done", 0, None), dir.path(), &[]);
        assert!(done.crashes.is_empty(), "{done:?}");
    }

    #[test]
    fn a_toolchain_with_no_fuzzer_says_what_it_did_not_drive() {
        let dir = tree(&["parse"]);
        let done = driven(
            saying("error: no such command: fuzz", 101, None),
            dir.path(),
            &[],
        );
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
    fn every_absence_message_is_decisive_even_when_cargo_exits_successfully() {
        for message in [
            "error: no such command: fuzz",
            "error: no such subcommand: fuzz",
            "fuzz is not installed for the toolchain",
        ] {
            let dir = tree(&["parse"]);
            let done = driven(saying(message, 0, None), dir.path(), &[]);
            assert!(done.ran.is_empty(), "{message}: {done:?}");
            assert_eq!(
                done.findings.first().map(|finding| finding.kind),
                Some(FindingKind::NotMeasured),
                "{message}: {done:?}"
            );
        }
    }

    #[test]
    fn a_fuzzer_process_that_cannot_be_spawned_is_not_counted_as_driven() {
        let dir = tree(&["parse"]);
        let missing = dir.path().join("no-such-cargo");
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        let done = fuzz(
            &Fuzzing {
                root: dir.path(),
                cargo: &missing,
                env: Vec::new(),
                targets: &[],
                max_total_time: Duration::from_secs(1),
                timeout: Some(Duration::from_secs(30)),
            },
            Watch::new(&cancel, &trace),
        );

        assert!(done.ran.is_empty(), "{done:?}");
        assert_eq!(
            done.findings.first().map(|finding| finding.kind),
            Some(FindingKind::NotMeasured)
        );
    }

    #[test]
    fn the_fuzzer_command_and_its_trace_are_the_exact_bounded_invocation() {
        let dir = tree(&["parse"]);
        let argv = dir.path().join("argv");
        let mut env = saying("Done", 0, None);
        env.push((
            OsString::from("FAKE_CARGO_ARGV_OUT"),
            argv.clone().into_os_string(),
        ));
        let cancel = Cancel::new();
        let trace = recording();
        let cargo = cargo();
        let done = fuzz(
            &Fuzzing {
                root: dir.path(),
                cargo: &cargo,
                env,
                targets: &[],
                max_total_time: Duration::from_secs(7),
                timeout: Some(Duration::from_secs(30)),
            },
            Watch::new(&cancel, &trace),
        );

        assert_eq!(done.ran, ["parse"]);
        assert_eq!(
            std::fs::read_to_string(argv).expect("the arguments"),
            "+nightly\nfuzz\nrun\nparse\n--\n-max_total_time=7\n"
        );
        let exec = trace
            .events()
            .into_iter()
            .find_map(|event| match event.payload {
                Payload::Exec { exec } => Some(exec),
                _ => None,
            })
            .expect("the fuzz execution in the trace");
        assert_eq!(
            exec.argv
                .iter()
                .skip(1)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "+nightly",
                "fuzz",
                "run",
                "parse",
                "--",
                "-max_total_time=7"
            ]
        );
        let expected_directory = dir.path().to_string_lossy();
        assert_eq!(exec.dir.as_deref(), Some(expected_directory.as_ref()));
        assert_eq!(exec.timeout_ms, Some(30_000));
    }

    #[test]
    fn every_new_artifact_is_read_from_its_real_path_and_reported_in_name_order() {
        let dir = tree(&["parse"]);
        let later = dir.path().join("fuzz/artifacts/parse/z-last");
        let earlier = dir.path().join("fuzz/artifacts/parse/a-first");
        let mut env = saying("crashed", 77, Some(&later.to_string_lossy()));
        env.push((
            OsString::from("FAKE_CARGO_ARTIFACT_TWO"),
            earlier.into_os_string(),
        ));
        let done = driven(env, dir.path(), &[]);

        assert_eq!(
            done.crashes
                .iter()
                .map(|crash| crash.artifact.as_str())
                .collect::<Vec<_>>(),
            [
                "fuzz/artifacts/parse/a-first",
                "fuzz/artifacts/parse/z-last"
            ]
        );
        assert_eq!(
            done.crashes.first().map(|crash| crash.content.clone()),
            Some(b"second input".to_vec())
        );
        assert_eq!(
            done.crashes.last().map(|crash| crash.content.clone()),
            Some(b"bad input".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_in_an_artifact_name_is_normalized_after_the_real_file_is_read() {
        let dir = tree(&["parse"]);
        let artifact = dir.path().join("fuzz/artifacts/parse/nested\\crash-abc");
        let done = driven(
            saying("crashed", 77, Some(&artifact.to_string_lossy())),
            dir.path(),
            &[],
        );

        let crash = done.crashes.first().expect("the crash");
        assert_eq!(crash.artifact, "fuzz/artifacts/parse/nested/crash-abc");
        assert_eq!(crash.corpus, "fuzz/corpus/parse/crash-abc");
        assert_eq!(crash.content, b"bad input");
    }

    #[test]
    fn only_the_targets_the_configuration_names_are_driven() {
        let dir = tree(&["parse", "decode"]);
        let done = driven(saying("Done", 0, None), dir.path(), &["decode".to_owned()]);
        assert_eq!(done.ran, ["decode"]);
    }
}
