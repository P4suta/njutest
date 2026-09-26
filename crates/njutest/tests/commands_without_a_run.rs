// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind, driven in this process where no run has been.

#![expect(
    clippy::expect_used,
    reason = "a setup that fails is reported by panicking"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::cli::Environment;
use rust_mutants::runner::Cancel;

/// Where the runs of one workspace live, spelled as the documented default.
#[cfg(unix)]
fn runs_root(root: &Path) -> PathBuf {
    root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
}

/// What one command said, driven in this process.
#[derive(Debug)]
struct Said {
    code: u8,
    out: String,
    err: String,
}

/// The environment a run of the tree at `root` is given, with what it writes beside the tree rather than in it.
fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn ask(root: &Path, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(root),
        &mut out,
        &mut err,
    );
    Said {
        code,
        out: njutest_devkit::process::strict_utf8(&out).into_owned(),
        err: njutest_devkit::process::strict_utf8(&err).into_owned(),
    }
}

#[cfg(unix)]
#[test]
fn accept_propagates_both_a_missing_run_and_an_unreadable_report() {
    let dir = njutest_devkit::paths::Project::fresh();
    let no_run = ask(dir.path(), &["accept", "abcdef", "--reason", "reviewed"]);
    assert_eq!(no_run.code, 3, "{}{}", no_run.out, no_run.err);
    assert!(
        no_run.err.contains("no run has completed"),
        "{}",
        no_run.err
    );

    let run = "20260101T000000Z-broken";
    std::fs::create_dir_all(runs_root(dir.path()).join(run))
        .expect("a run directory without a report");
    let unreadable = ask(
        dir.path(),
        &["accept", "abcdef", "--reason", "reviewed", "--run", run],
    );
    assert_eq!(unreadable.code, 3, "{}{}", unreadable.out, unreadable.err);
    assert!(
        unreadable.err.contains(run)
            && unreadable
                .err
                .contains(njutest::app::reports::DOCUMENT_NAME),
        "{}",
        unreadable.err
    );
}

#[test]
fn a_configuration_is_written_once_and_never_over_one_somebody_wrote() {
    let dir = njutest_devkit::paths::Project::fresh();
    let root = dir.path().to_path_buf();

    let written = ask(&root, &["init"]);
    assert_eq!(written.code, 0, "{}{}", written.out, written.err);
    assert!(
        written.out.contains(njutest::config::FILE_NAME),
        "it says which file it wrote, because a command that writes in silence is one a \
         person runs again: {}",
        written.out
    );
    let path = root.join(njutest::config::FILE_NAME);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        njutest::config::skeleton(),
        "what init writes is the skeleton, so a person who reads the file and a person \
         who reads the documentation of it are reading one thing"
    );

    std::fs::write(&path, "version = 1\n# mine\n").expect("a configuration somebody wrote");
    let refused = ask(&root, &["init"]);
    assert!(
        refused.err.contains("NJ1005"),
        "and a refusal carries the code a person greps for and says how to mean it: {}",
        refused.err
    );
    assert_eq!(
        refused.code, 3,
        "a second init does not write over a configuration somebody has edited: the \
         acceptances in it are the reviews of every survivor this project has looked at, \
         and they are not recoverable from anywhere else: {}{}",
        refused.out, refused.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        "version = 1\n# mine\n",
        "and leaves it exactly as it was"
    );

    let forced = ask(&root, &["init", "--force"]);
    assert_eq!(
        forced.code, 0,
        "while a person who says to replace it is one who meant to: {}{}",
        forced.out, forced.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        njutest::config::skeleton()
    );
}
