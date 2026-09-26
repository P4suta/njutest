// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a real coverage build measures, in the tools' own units.

#![expect(
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::PathBuf;

use rust_mutants::coverage::{Block, INSTRUMENT_FLAG, PROFILE_ENV, Point};

/// Builds a fixture with instrumentation, runs one test, and reports what it covered — the whole path a run takes for one target.
struct Measured {
    root: PathBuf,
    files: Vec<rust_mutants::coverage::FileRegions>,
    _target: tempfile::TempDir,
    _profiles: tempfile::TempDir,
}
fn measure(fixture: &str, test: &str) -> Measured {
    use std::ffi::OsString;

    use rust_mutants::cargo::{
        CompileKind, CompileOptions, Driver, LocateOptions, Message, Metadata, MetadataOptions,
        Toolchain, compile,
    };
    use rust_mutants::coverage::{Tools, profile_pattern, written_profiles};
    use rust_mutants::runner::{Cancel, Spec, Watched, run};
    use rust_mutants::trace::Recorder;

    let root = njutest_devkit::paths::fixtures_dir().join(fixture);
    let target = tempfile::Builder::new()
        .prefix("njutest-coverage-")
        .tempdir()
        .expect("tempdir");
    let profiles = tempfile::Builder::new()
        .prefix("njutest-profiles-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let engine_trace = Recorder::disabled();

    let mut env: rust_mutants::vars::Variables = std::env::vars_os().collect();
    env.remove("RUSTFLAGS");
    env.set(
        "CARGO_ENCODED_RUSTFLAGS",
        INSTRUMENT_FLAG.replace(' ', "\u{1f}"),
    );

    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(env.clone()),
            ..LocateOptions::default()
        },
        &root,
        &cancel,
    )
    .expect("locate");
    let driver = Driver {
        toolchain: &toolchain,
        dir: &root,
        cancel: &cancel,
        trace: &engine_trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    drop(metadata);
    let built = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: Vec::new(),
            target_dir: Some(rust_mutants::cargo::BuildDir::new(
                target.path().to_path_buf(),
                Vec::new(),
            )),
            locked: true,
            offline: true,
            timeout: None,
            env: rust_mutants::vars::Variables::empty(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("build");
    assert!(built.success, "the instrumented fixture builds");
    let executable = built
        .messages
        .iter()
        .find_map(|message| match message {
            Message::CompilerArtifact(artifact) if artifact.profile.test => {
                artifact.executable.clone()
            }
            _ => None,
        })
        .expect("a test binary");

    let mut spec = Spec::new(
        [
            executable.as_os_str().to_owned(),
            OsString::from(test),
            OsString::from("--exact"),
        ],
        rust_mutants::runner::Bound::Unbounded,
    );
    spec.dir = Some(root.clone());
    let mut run_env = env;
    run_env.set(PROFILE_ENV, profile_pattern(profiles.path(), "one"));
    spec.env = Some(run_env);
    let ran = run(&spec, &cancel);
    assert!(
        ran.succeeded(),
        "{}",
        std::str::from_utf8(&ran.output).expect("the fixture writes exact UTF-8")
    );

    let tools =
        Tools::locate(&toolchain, &root, &Watched::new(&cancel, &trace)).expect("the llvm tools");
    let raw = written_profiles(profiles.path(), "one").expect("profiles");
    assert!(!raw.is_empty(), "the test process wrote a profile");
    let merged = profiles.path().join("one.profdata");
    tools
        .merge(&raw, &merged, &Watched::new(&cancel, &trace))
        .expect("merge");
    let files = tools
        .export(
            &merged,
            std::slice::from_ref(&executable),
            &Watched::new(&cancel, &trace),
        )
        .expect("export");
    Measured {
        root,
        files,
        _target: target,
        _profiles: profiles,
    }
}
#[test]
fn a_region_column_is_a_byte_column_and_the_fixture_holds_the_tool_to_it() {
    let measured = measure("fixture-unicode", "tests::大きい方を選ぶ");
    let source = measured.root.join("src/lib.rs");
    let covered = rust_mutants::coverage::covered(&measured.files);
    let instrumented = rust_mutants::coverage::instrumented(&measured.files);
    assert!(!covered.is_empty(), "the test covered something");

    let text = std::fs::read_to_string(&source).expect("read");
    let (line_number, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("pub fn 大きい方"))
        .expect("the function");
    let line_number = u32::try_from(line_number).expect("small") + 1;
    let comparison = "γ > β";
    let at = line.find(comparison).expect("the comparison");
    let byte_start = u32::try_from(line[..at].len()).expect("small") + 1;
    let byte_end = byte_start + u32::try_from(comparison.len()).expect("small");
    let char_start = u32::try_from(line[..at].chars().count()).expect("small") + 1;
    let char_end = char_start + u32::try_from(comparison.chars().count()).expect("small");
    assert_ne!(
        byte_start, char_start,
        "the fixture discriminates the two units"
    );

    let block = |start: u32, end: u32| Block {
        file: source.clone(),
        start: Point {
            line: line_number,
            column: start,
        },
        end: Point {
            line: line_number,
            column: end,
        },
    };
    assert!(
        covered.contains(&block(byte_start, byte_end)),
        "the comparison's region is exactly its byte span; the regions on this line are {:?}",
        covered
            .iter()
            .filter(|one| one.file == source && one.start.line == line_number)
            .map(|one| (one.start.column, one.end.column))
            .collect::<Vec<(u32, u32)>>()
    );
    assert!(
        !covered.contains(&block(char_start, char_end)),
        "and not its character span, which is what reading the wrong unit looks like"
    );

    let (unused_line, unused) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("δ * 2"))
        .expect("the unused body");
    let unused_line = u32::try_from(unused_line).expect("small") + 1;
    let unused_column = u32::try_from(unused.len() - unused.trim_start().len()).expect("small") + 1;
    let position = Point {
        line: unused_line,
        column: unused_column,
    };
    assert!(
        instrumented
            .iter()
            .any(|one| one.contains(&source, position)),
        "the unused function is instrumented"
    );
    assert!(
        !covered.iter().any(|one| one.contains(&source, position)),
        "and uncovered"
    );
}
