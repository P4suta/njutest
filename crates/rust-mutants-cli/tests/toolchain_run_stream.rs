// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as a program reads it: one JSON object per line, as it happens.

#![expect(
    clippy::expect_used,
    reason = "a process-level test reports fixture, process, and assertion failures by panicking"
)]

use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output};

use njutest_devkit::fixture::Fixture;
use rust_mutants::report::stream::{Line, read, read_line};
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// A streamed command that is always killed and reaped if the test unwinds before waiting.
struct StreamingChild(Option<Child>);

impl StreamingChild {
    fn launch(command: &mut Command) -> Self {
        Self(Some(command.spawn().expect("rust-mutants starts")))
    }

    fn wait(mut self) -> std::io::Result<ExitStatus> {
        self.0
            .take()
            .expect("the streaming child is still owned")
            .wait()
    }

    fn cleanup(&mut self) -> std::io::Result<()> {
        let Some(mut child) = self.0.take() else {
            return Ok(());
        };
        match child.try_wait()? {
            Some(status) => {
                debug_assert!(status.code().is_some() || !status.success());
                Ok(())
            }
            None => {
                child.kill()?;
                child.wait().map(|status| {
                    debug_assert!(status.code().is_some() || !status.success());
                })
            }
        }
    }
}

impl Drop for StreamingChild {
    fn drop(&mut self) {
        let cleaned = self.cleanup();
        debug_assert!(
            matches!(&cleaned, Ok(())),
            "an unwinding test kills and reaps its child: {cleaned:?}"
        );
    }
}

fn run(fixture: &Fixture, extra: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    asked(
        fixture,
        &std::iter::once("run")
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all"])
            .chain(["--offline", "--locked", "--no-coverage", "--jobs", "1"])
            .chain(extra.iter().copied())
            .collect::<Vec<&str>>(),
    )
}

/// One command, driven in this process against the fixture's own directories.
fn asked(fixture: &Fixture, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
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

#[test]
fn a_json_run_streams_one_object_per_event_as_it_happens() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json"]);
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let lines = read(&text).expect("every line reads back through the engine's own reader");

    assert!(
        matches!(lines.first(), Some(Line::RunStart { schema, .. }) if schema == "rust-mutants-run-stream-v1"),
        "the first line says what the stream is: {:?}",
        lines.first()
    );
    assert!(
        matches!(lines.last(), Some(Line::RunEnd { exit_code: 1, .. })),
        "the last says how it ended: {:?}",
        lines.last()
    );
    let judged: Vec<&Line> = lines
        .iter()
        .filter(|line| matches!(line, Line::Mutant { .. }))
        .collect();
    assert_eq!(judged.len(), 13, "one line per mutant");
    let mut seen = 0;
    for line in &judged {
        let Line::Mutant {
            completed, mutant, ..
        } = line
        else {
            continue;
        };
        seen += 1;
        assert_eq!(*completed, seen, "the count is what has been delivered");
        assert!(
            !mutant.id.is_empty() && !mutant.rule.is_empty(),
            "{mutant:?}"
        );
        assert!(
            mutant.line > 0,
            "a reader is told where to look: {mutant:?}"
        );
    }
    assert!(
        lines
            .iter()
            .any(|line| matches!(line, Line::Finding { .. })),
        "and what stops the run from being clean"
    );
    assert!(
        lines
            .iter()
            .any(|line| matches!(line, Line::PhaseEnd { phase, .. } if phase == "verify")),
        "preparing is in the stream too: {lines:?}"
    );
}

#[test]
fn a_json_run_that_fails_after_opening_ends_with_a_typed_error() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(
        &fixture,
        &[
            "--json",
            "--skip-target",
            "fixture-simple/lib/no-such-target",
        ],
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let lines = read(&text).expect("even the refusal reads through the engine's own reader");

    assert!(
        matches!(lines.first(), Some(Line::RunStart { .. })),
        "a reader first learns which run failed: {lines:?}"
    );
    assert!(
        matches!(lines.last(), Some(Line::Error { code, message, remedy: Some(remedy) })
            if code == "RM5004"
                && message.contains("no-such-target")
                && !remedy.is_empty()),
        "the open stream closes with the machine-readable refusal: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| matches!(line, Line::RunEnd { .. })),
        "a failed run is not also said to have ended normally: {lines:?}"
    );
}

#[test]
fn a_json_run_whose_store_cannot_be_pruned_has_one_error_terminal() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(
        ".rust-mutants.toml",
        b"version = 1\n\n[reports]\ndirectory = \"blocked\"\n",
    );
    fixture.write(
        "blocked",
        b"a file cannot be enumerated as a report directory\n",
    );
    let output = run(&fixture, &["--json", "--no-report"]);
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let lines = read(&text).expect("the failed stream still reads through its own reader");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        matches!(lines.last(), Some(Line::Error { .. })),
        "the postcondition failure is the one terminal answer: {lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| matches!(line, Line::Error { .. } | Line::RunEnd { .. }))
            .count(),
        1,
        "success and failure cannot both be terminal: {lines:?}"
    );
}

#[test]
fn every_line_validates_against_the_schema_published_with_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json"]);
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-run-stream-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    for (at, line) in text.lines().filter(|line| !line.is_empty()).enumerate() {
        let value: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("a line of JSON");
        let problems: Vec<String> = validator
            .iter_errors(&value)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(problems.is_empty(), "line {}: {problems:?}", at + 1);
    }
}

#[test]
fn the_v1_reader_requires_explicit_null_and_rejects_duplicate_keys() {
    let exact = r#"{"type":"error","code":"RM0001","message":"failed","remedy":null}"#;
    assert!(
        read_line(1, exact).is_ok(),
        "an explicit null is part of the shape"
    );

    let missing = r#"{"type":"error","code":"RM0001","message":"failed"}"#;
    assert!(
        read_line(1, missing).is_err(),
        "missing and explicitly null are not the same v1 document"
    );

    let duplicate =
        r#"{"type":"error","code":"RM0001","code":"RM0002","message":"failed","remedy":null}"#;
    assert!(
        read_line(1, duplicate).is_err(),
        "a duplicate key never becomes a last-key-wins stream fact"
    );
}

#[test]
fn a_stream_and_a_display_are_two_ways_of_saying_one_thing_and_never_both() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json", "--ui", "plain"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(complaint.contains("--ui"), "{complaint}");
}

#[test]
fn the_stream_opens_before_anything_is_prepared() {
    let fixture = Fixture::copy("fixture-simple");
    let path = fixture.temp().join("stream.jsonl");
    let file = std::fs::File::create(&path).expect("somewhere to stream to");
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(["run", "--json", "--offline", "--locked", "--no-coverage"])
        .args(["--root", njutest_devkit::paths::utf8(fixture.root())])
        .stdout(std::process::Stdio::from(file));
    let child = StreamingChild::launch(&mut command);
    let opened = std::time::Instant::now();
    let first = loop {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => panic!("reading {}: {error}", path.display()),
        };
        if let Some(line) = text.lines().next() {
            break line.to_owned();
        }
        assert!(
            opened.elapsed() < std::time::Duration::from_secs(120),
            "a consumer that hears nothing cannot tell a slow run from a hung one"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let line: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&first).expect("the first line is JSON");
    assert_eq!(
        line.get("type").and_then(serde_json::Value::as_str),
        Some("run-start"),
        "{first}"
    );
    let finished = child.wait().expect("rust-mutants finishes");
    assert!(
        finished.code().is_some_and(|code| code < 2),
        "the streamed mutation run reaches a domain verdict: {finished}"
    );
}

#[test]
fn a_run_started_from_inside_the_tree_still_names_the_tree() {
    let fixture = Fixture::copy("fixture-simple");
    let output = asked(
        &fixture,
        &[
            "run",
            "--json",
            "--offline",
            "--locked",
            "--no-coverage",
            "--root",
            ".",
        ],
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    let first = text.lines().next().expect("the stream opens");
    let line: serde_json::Value =
        njutest_devkit::strictjson::decode_str(first).expect("the first line is JSON");
    assert_eq!(
        line.get("root_name").and_then(serde_json::Value::as_str),
        Some("fixture-simple"),
        "a root spelled as a dot is still a directory with a name: {first}"
    );
}
