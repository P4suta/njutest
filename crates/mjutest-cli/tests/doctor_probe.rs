// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The doctor's optional-tool probes, driven through an isolated search path.

#![expect(
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::Path;

use mjutest_cli::cli::Environment;
use mjutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use rust_mutants::runner::Cancel;

const CARGO_BANNER: &str = "cargo 1.98.0 (abc 2026-08-05)\nrelease: 1.98.0\ncommit-hash: abc\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\n";
const RUSTC_BANNER: &str = "rustc 1.98.0 (abc 2026-08-05)\nbinary: rustc\nrelease: 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 20.1.0\n";

struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(
    root: &Path,
    path: Option<OsString>,
    mut vars: Vec<(OsString, OsString)>,
) -> Environment {
    if let Some(path) = path {
        vars.push((OsString::from("PATH"), path));
    }
    Environment {
        cache_directory: root.join("cache"),
        working_directory: root.to_path_buf(),
        temp_directory: root.join("temp"),
        vars,
        cancel: Cancel::new(),
    }
}

fn asked(environment: &Environment) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        ["mjutest", "doctor"].into_iter().map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    Said {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

fn row<'a>(said: &'a Said, name: &str) -> &'a str {
    said.out
        .lines()
        .find(|line| line.split_whitespace().nth(1) == Some(name))
        .unwrap_or_else(|| panic!("no {name} row in:\n{}{}", said.out, said.err))
}

fn scripted(git: Invocation) -> Installed {
    install(
        &Script::new()
            .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
            .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
            .answering(
                Invocation::new("rustc", &["--print", "sysroot"]).printing("/not-a-real-sysroot\n"),
            )
            .answering(git),
    )
}

fn with_fake(
    root: &Path,
    installed: &Installed,
    path: OsString,
    extra: &[(&str, &str)],
) -> Environment {
    let mut vars = installed.env();
    vars.extend(
        extra
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
    environment(root, Some(path), vars)
}

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[test]
fn a_configuration_that_is_present_is_named_by_its_path() {
    let root = tempfile::tempdir().expect("a directory");
    let path = root.path().join(mjutest_cli::config::FILE_NAME);
    std::fs::write(&path, "version = 1\n").expect("a configuration");

    let said = asked(&environment(root.path(), None, Vec::new()));

    assert!(row(&said, "configuration").contains(&path.display().to_string()));
}

#[test]
fn a_configuration_that_is_absent_says_the_defaults_apply() {
    let root = tempfile::tempdir().expect("a directory");

    let said = asked(&environment(root.path(), None, Vec::new()));

    let configuration = row(&said, "configuration");
    assert!(configuration.contains("none"), "{configuration}");
    assert!(configuration.contains("defaults apply"), "{configuration}");
}

#[test]
fn a_probe_runs_in_the_supplied_directory() {
    let root = tempfile::tempdir().expect("a directory");
    let installed = scripted(Invocation::new("git", &["--version"]).printing("git cwd {{cwd}}\n"));
    let environment = with_fake(
        root.path(),
        &installed,
        installed.bin().as_os_str().to_owned(),
        &[],
    );

    let said = asked(&environment);

    assert!(
        row(&said, "git").contains(&root.path().display().to_string()),
        "{}",
        row(&said, "git")
    );
}

#[test]
fn a_probe_receives_the_supplied_environment() {
    let root = tempfile::tempdir().expect("a directory");
    let installed = scripted(
        Invocation::new("git", &["--version"])
            .when("MJUTEST_DOCTOR_SENTINEL", "present")
            .printing("git environment present\n"),
    );
    let environment = with_fake(
        root.path(),
        &installed,
        installed.bin().as_os_str().to_owned(),
        &[("MJUTEST_DOCTOR_SENTINEL", "present")],
    );

    let said = asked(&environment);

    assert!(row(&said, "git").contains("git environment present"));
}

#[test]
fn a_successful_probe_keeps_only_its_trimmed_first_line() {
    let root = tempfile::tempdir().expect("a directory");
    let installed = scripted(
        Invocation::new("git", &["--version"])
            .printing("  git version 9.8.7  \na second line is not the version\n"),
    );
    let environment = with_fake(
        root.path(),
        &installed,
        installed.bin().as_os_str().to_owned(),
        &[],
    );

    let said = asked(&environment);
    let git = row(&said, "git");

    assert!(git.contains("ok  git version 9.8.7"), "{git}");
    assert!(!git.contains("second line"), "{git}");
}

#[test]
fn an_empty_successful_probe_is_missing() {
    let root = tempfile::tempdir().expect("a directory");
    let installed = scripted(Invocation::new("git", &["--version"]));
    let environment = with_fake(
        root.path(),
        &installed,
        installed.bin().as_os_str().to_owned(),
        &[],
    );

    let said = asked(&environment);

    assert!(
        row(&said, "git").ends_with("missing"),
        "{}",
        row(&said, "git")
    );
}

#[test]
fn a_probe_that_cannot_be_started_is_missing() {
    let root = tempfile::tempdir().expect("a directory");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a program directory");
    std::fs::write(bin.join(executable("git")), "not an executable").expect("a plain file");

    let said = asked(&environment(
        root.path(),
        Some(bin.into_os_string()),
        Vec::new(),
    ));

    assert!(
        row(&said, "git").ends_with("missing"),
        "{}",
        row(&said, "git")
    );
}

#[test]
fn a_probe_that_exits_nonzero_is_missing_even_when_it_prints_a_version() {
    let root = tempfile::tempdir().expect("a directory");
    let mut git = Invocation::new("git", &["--version"]).printing("git lying 9.8.7\n");
    git.exit = 7;
    let installed = scripted(git);
    let environment = with_fake(
        root.path(),
        &installed,
        installed.bin().as_os_str().to_owned(),
        &[],
    );

    let said = asked(&environment);
    let git = row(&said, "git");

    assert!(git.ends_with("missing"), "{git}");
    assert!(!git.contains("lying"), "{git}");
}

#[test]
fn a_probe_searches_later_path_entries() {
    let root = tempfile::tempdir().expect("a directory");
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).expect("an empty path entry");
    let installed =
        scripted(Invocation::new("git", &["--version"]).printing("git from the second entry\n"));
    let path = std::env::join_paths([empty.as_path(), installed.bin()]).expect("a search path");
    let environment = with_fake(root.path(), &installed, path, &[]);

    let said = asked(&environment);

    assert!(row(&said, "git").contains("from the second entry"));
}

#[test]
fn a_non_file_candidate_is_skipped_for_a_later_executable() {
    let root = tempfile::tempdir().expect("a directory");
    let first = root.path().join("first");
    std::fs::create_dir_all(first.join(executable("git")))
        .expect("a directory named like a program");
    let installed =
        scripted(Invocation::new("git", &["--version"]).printing("git after the directory\n"));
    let path = std::env::join_paths([first.as_path(), installed.bin()]).expect("a search path");
    let environment = with_fake(root.path(), &installed, path, &[]);

    let said = asked(&environment);

    assert!(row(&said, "git").contains("after the directory"));
}

#[test]
fn a_missing_path_is_an_absent_probe_and_not_a_panic() {
    let root = tempfile::tempdir().expect("a directory");

    let said = asked(&environment(root.path(), None, Vec::new()));

    assert_eq!(
        said.code,
        mjutest_cli::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(
        row(&said, "git").ends_with("missing"),
        "{}",
        row(&said, "git")
    );
}

#[test]
fn the_program_name_is_a_file_before_a_probe_uses_it() {
    let root = tempfile::tempdir().expect("a directory");
    let first = root.path().join("first");
    std::fs::create_dir_all(first.join(executable("git")))
        .expect("a directory named like a program");
    let path = std::env::join_paths([first.as_path()]).expect("a search path");

    let said = asked(&environment(root.path(), Some(path), Vec::new()));

    assert!(
        row(&said, "git").ends_with("missing"),
        "{}",
        row(&said, "git")
    );
}
