// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the pages promise, against what the binary prints.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;
use std::process::Output;

use mjutest_devkit::fixture::Fixture;

/// What the session is run against, so a reader can follow it.
const DEMO: &str = "fixture-simple";

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(args)
        .args(["--root", &fixture.root().to_string_lossy()])
        .args(["--offline", "--locked"])
        .output()
        .expect("rust-mutants runs")
}

/// The session with everything that changes between two runs of it taken out.
fn steady(text: &str, fixture: &Fixture) -> String {
    let mut out = text.replace(&fixture.root().to_string_lossy().into_owned(), ".");
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
    out
}

#[test]
fn the_readme_sample_session_is_the_one_the_engine_prints() {
    let fixture = Fixture::copy(DEMO);
    let ran = against(
        &fixture,
        &["run", "--no-coverage", "--jobs", "1", "--ui", "quiet"],
    );
    assert_eq!(ran.status.code(), Some(1), "{ran:?}");
    let explained = against(&fixture, &["explain", "e5e8"]);
    assert_eq!(explained.status.code(), Some(0), "{explained:?}");

    let mut session = String::from("$ rust-mutants run\n");
    session.push_str(&steady(&String::from_utf8_lossy(&ran.stdout), &fixture));
    session.push_str("\n$ rust-mutants explain e5e8\n");
    session.push_str(&steady(
        &String::from_utf8_lossy(&explained.stdout),
        &fixture,
    ));

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/readme-session.golden");
    mjutest_devkit::golden::golden(&golden, session.as_bytes())
        .expect("the session is the recorded one");

    let readme = std::fs::read_to_string(mjutest_devkit::paths::workspace_root().join("README.md"))
        .expect("the README");
    assert!(
        readme.contains(session.trim_end()),
        "README.md does not show the session the engine prints:\n{session}"
    );
}
