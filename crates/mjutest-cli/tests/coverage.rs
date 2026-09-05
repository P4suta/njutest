// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage: which regions of which files one test reached, in the units
//! the tools actually use.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use mjutest_cli::coverage::{
    Block, CoverageErrorKind, INSTRUMENT_FLAG, PROFILE_ENV, Point, REGION_KIND_CODE, Region,
    parse_export,
};

// --- reading an export ------------------------------------------------------------

const EXPORT: &str = r#"{
  "type": "llvm.coverage.json.export",
  "version": "3.1.0",
  "data": [{
    "files": [{"filename": "/w/src/lib.rs"}],
    "functions": [
      {
        "name": "_RNvCs_1w3max",
        "count": 1,
        "filenames": ["/w/src/lib.rs"],
        "regions": [
          [2, 1, 2, 34, 1, 0, 0, 0],
          [3, 8, 3, 13, 1, 0, 0, 0],
          [3, 16, 3, 17, 0, 0, 0, 0],
          [9, 1, 9, 2, 4, 0, 0, 2]
        ]
      },
      {
        "name": "_RNvCs_1w7unused",
        "count": 0,
        "filenames": ["/w/src/other.rs"],
        "regions": [[7, 5, 7, 10, 0, 0, 0, 0]]
      }
    ],
    "totals": {}
  }]
}"#;

#[test]
fn an_export_is_read_into_regions_per_file() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let paths: Vec<&Path> = files.iter().map(|file| file.path.as_path()).collect();
    assert_eq!(
        paths,
        [Path::new("/w/src/lib.rs"), Path::new("/w/src/other.rs")],
        "one entry per file that has regions, in path order"
    );
}

#[test]
fn every_region_keeps_its_place_its_count_and_its_kind() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let lib = files
        .iter()
        .find(|file| file.path == Path::new("/w/src/lib.rs"))
        .expect("the library");
    assert_eq!(
        lib.regions,
        [
            Region {
                start: Point { line: 2, column: 1 },
                end: Point {
                    line: 2,
                    column: 34
                },
                count: 1,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point { line: 3, column: 8 },
                end: Point {
                    line: 3,
                    column: 13
                },
                count: 1,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point {
                    line: 3,
                    column: 16
                },
                end: Point {
                    line: 3,
                    column: 17
                },
                count: 0,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point { line: 9, column: 1 },
                end: Point { line: 9, column: 2 },
                count: 4,
                kind: 2,
            },
        ]
    );
    let other = files
        .iter()
        .find(|file| file.path == Path::new("/w/src/other.rs"))
        .expect("the other file");
    assert_eq!(other.regions.len(), 1);
    assert_eq!(other.regions[0].count, 0);
}

#[test]
fn only_a_code_region_that_ran_is_a_covered_block() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let blocks = mjutest_cli::coverage::covered(&files);
    assert_eq!(
        blocks,
        [
            Block {
                file: PathBuf::from("/w/src/lib.rs"),
                start: Point { line: 2, column: 1 },
                end: Point {
                    line: 2,
                    column: 34
                },
            },
            Block {
                file: PathBuf::from("/w/src/lib.rs"),
                start: Point { line: 3, column: 8 },
                end: Point {
                    line: 3,
                    column: 13
                },
            },
        ]
        .into_iter()
        .collect(),
        "the uncovered region, the region of another kind, and the other file's zero are all out"
    );
}

#[test]
fn a_block_holds_a_position_by_its_line_and_byte_column() {
    let block = Block {
        file: PathBuf::from("/w/src/lib.rs"),
        start: Point { line: 3, column: 8 },
        end: Point { line: 4, column: 5 },
    };
    let inside = [(3, 8), (3, 99), (4, 1), (4, 4)];
    for (line, column) in inside {
        assert!(
            block.contains(Path::new("/w/src/lib.rs"), Point { line, column }),
            "{line}:{column} is inside"
        );
    }
    for (line, column) in [(3, 7), (4, 5), (5, 1), (2, 100)] {
        assert!(
            !block.contains(Path::new("/w/src/lib.rs"), Point { line, column }),
            "{line}:{column} is outside; the end is exclusive"
        );
    }
    assert!(
        !block.contains(Path::new("/w/src/other.rs"), Point { line: 3, column: 9 }),
        "another file is another place"
    );
}

// --- refusals -----------------------------------------------------------------------

#[test]
fn an_export_that_is_not_one_is_refused_rather_than_read_in_part() {
    for bad in [
        "",
        "not json",
        r#"{"data": []}"#,
        r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[{"name":"f","count":0,"filenames":[],"regions":[[1,1,1,2,0,0,0,0]]}]}]}"#,
        r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[{"name":"f","count":0,"filenames":["/w/a.rs"],"regions":[[1,1,1]]}]}]}"#,
    ] {
        let error = parse_export(bad.as_bytes()).expect_err("refused");
        assert_eq!(error.kind(), CoverageErrorKind::Unreadable, "{bad}");
        assert!(error.to_string().contains("MJ4001"), "{error}");
    }
}

#[test]
fn the_flags_the_tools_are_driven_with_are_frozen() {
    assert_eq!(INSTRUMENT_FLAG, "-C instrument-coverage");
    assert_eq!(PROFILE_ENV, "LLVM_PROFILE_FILE");
}

// --- the real tools, on a real workspace ------------------------------------------------

/// Builds a fixture with instrumentation, runs one test, and reports what
/// it covered — the whole path a run takes for one target.
struct Measured {
    root: PathBuf,
    files: Vec<mjutest_cli::coverage::FileRegions>,
    _target: tempfile::TempDir,
    _profiles: tempfile::TempDir,
}

fn measure(fixture: &str, test: &str) -> Measured {
    use std::ffi::OsString;

    use mjutest_cli::coverage::{Tools, profile_pattern, written_profiles};
    use mjutest_cli::trace::Recorder;
    use mjutest_cli::watch::Watch;
    use rust_mutants::cargo::{
        CompileKind, CompileOptions, Driver, LocateOptions, Message, Metadata, MetadataOptions,
        Toolchain, compile,
    };
    use rust_mutants::runner::{Cancel, Spec, run};

    let root = mjutest_devkit::paths::fixtures_dir().join(fixture);
    let target = tempfile::Builder::new()
        .prefix("mjutest-coverage-")
        .tempdir()
        .expect("tempdir");
    let profiles = tempfile::Builder::new()
        .prefix("mjutest-profiles-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let engine_trace = rust_mutants::trace::Recorder::disabled();

    // The instrumentation flag reaches rustc through the environment, so a
    // .cargo/config.toml's own rustflags are not silently replaced.
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| name != "RUSTFLAGS" && name != "CARGO_ENCODED_RUSTFLAGS")
        .collect();
    env.push((
        OsString::from("CARGO_ENCODED_RUSTFLAGS"),
        OsString::from(INSTRUMENT_FLAG.replace(' ', "\u{1f}")),
    ));

    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
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
    let _metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let built = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Tests,
            target_dir: Some(target.path().to_path_buf()),
            locked: true,
            offline: true,
            timeout: None,
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

    let mut spec = Spec::new([
        executable.as_os_str().to_owned(),
        OsString::from(test),
        OsString::from("--exact"),
    ]);
    spec.dir = Some(root.clone());
    let mut run_env = env;
    run_env.push((
        OsString::from(PROFILE_ENV),
        profile_pattern(profiles.path(), "one").into_os_string(),
    ));
    spec.env = Some(run_env);
    let ran = run(&spec, &cancel);
    assert!(ran.ok(), "{}", String::from_utf8_lossy(&ran.output));

    let tools =
        Tools::locate(&toolchain, &root, Watch::new(&cancel, &trace)).expect("the llvm tools");
    let raw = written_profiles(profiles.path(), "one").expect("profiles");
    assert!(!raw.is_empty(), "the test process wrote a profile");
    let merged = profiles.path().join("one.profdata");
    tools
        .merge(&raw, &merged, Watch::new(&cancel, &trace))
        .expect("merge");
    let files = tools
        .export(&merged, &executable, Watch::new(&cancel, &trace))
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
    let covered = mjutest_cli::coverage::covered(&measured.files);
    let instrumented = mjutest_cli::coverage::instrumented(&measured.files);
    assert!(!covered.is_empty(), "the test covered something");

    // The comparison on the line of `大きい方` has a byte column and a
    // character column twelve apart. Exactly one region is the comparison,
    // and asking for it by its byte span finds it.
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

    // The function nothing calls is instrumented and uncovered, which is
    // what "no test reaches this" looks like.
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
