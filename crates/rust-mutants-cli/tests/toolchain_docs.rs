// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the pages promise, against what the binary prints.

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use njutest_devkit::result::{ResultState, result_state};
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What the session is run against, so a reader can follow it.
const DEMO: &str = "fixture-simple";

fn returned<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
    assert_eq!(
        result_state(&result),
        ResultState::Returned,
        "the closed fixture path was refused: {result:?}"
    );
    match result {
        Ok(value) => Some(value),
        Err(_already_reported) => None,
    }
}

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root.as_str()])
            .chain(["--offline", "--locked"])
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        ci: rust_mutants_cli::CiHost::None,
    }
}

/// The session with everything that changes between two runs of it taken out.
fn steady(text: &str, fixture: &Fixture) -> Result<String, rust_mutants::id::SlashedPathError> {
    let slashed_root = rust_mutants::id::slashed(fixture.root())?;
    let mut out = text
        .replace(njutest_devkit::paths::utf8(fixture.root()), ".")
        .replace(&slashed_root, ".");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("run       ") {
            out = out.replace(rest, "<run>");
        }
        if let Some(rest) = line.strip_prefix("workspace ") {
            out = out.replace(rest, "<workspace digest>");
        }
        if let Some(rest) = line.strip_prefix("catalog   ") {
            out = out.replace(rest, "<catalog digest>");
        }
        if let Some(rest) = line.strip_prefix("RUN       ") {
            out = out.replace(rest, "<run>");
        }
        if let Some(rest) = line.strip_prefix("TIMING    ") {
            out = out.replace(rest, "<duration>");
        }
    }
    Ok(out)
}

#[test]
fn the_readme_sample_session_is_the_one_the_engine_prints() {
    let fixture = Fixture::copy(DEMO);
    let ran = against(
        &fixture,
        &["run", "--no-coverage", "--jobs", "1", "--ui", "quiet"],
    );
    assert_eq!(ran.status.code(), Some(1), "{ran:?}");
    let explained = against(&fixture, &["explain", "f0d2"]);
    assert_eq!(explained.status.code(), Some(0), "{explained:?}");

    let mut session = String::from("$ rust-mutants run\n");
    let Some(run_session) = returned(steady(
        &njutest_devkit::process::strict_utf8(&ran.stdout),
        &fixture,
    )) else {
        return;
    };
    session.push_str(&run_session);
    session.push_str("\n$ rust-mutants explain f0d2\n");
    let Some(explain_session) = returned(steady(
        &njutest_devkit::process::strict_utf8(&explained.stdout),
        &fixture,
    )) else {
        return;
    };
    session.push_str(&explain_session);

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/readme-session.golden");
    njutest_devkit::golden::golden(&golden, session.as_bytes())
        .expect("the session is the recorded one");

    let page = njutest_devkit::paths::workspace_root().join("docs/engine/getting-started.md");
    let shown = std::fs::read_to_string(&page).expect("the getting-started page");
    assert!(
        shown.contains(session.trim_end()),
        "{} does not show the session the engine prints, and a sample a person reads \
         has to be one the tool still prints: {session}",
        page.display()
    );
}
