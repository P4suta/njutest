// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind: `report`, `trace`, `diagnostics`, and the one that says what a run would do without doing it, `plan`.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-commands-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn njutest(fixture: &Fixture, args: &[&str]) -> Output {
    let cache = njutest_devkit::paths::cache_beside(&fixture.root).expect("a cache directory");
    njutest_caching(fixture, args, &cache)
}

fn njutest_caching(fixture: &Fixture, args: &[&str], cache: &Path) -> Output {
    asked(&environment(&fixture.root, cache, &[]), args)
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, what a test named, and nothing else.
fn environment(root: &Path, cache: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[]);
    for (name, value) in named {
        vars.push((OsString::from(*name), OsString::from(*value)));
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
    }
}

/// A fixture with one completed, traced run behind it.
fn verified(name: &str) -> Fixture {
    let fixture = fixture(name);
    let output = njutest(&fixture, &["verify", "--offline", "--locked", "--trace"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn report_prints_the_records_of_the_latest_run() {
    let fixture = verified("fixture-baseline");
    let output = njutest(&fixture, &["report"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("RUN\t"), "{text}");
    assert!(text.ends_with("VERDICT\tINSUFFICIENT\n"), "{text}");
}

#[test]
fn report_json_prints_the_document_the_run_wrote_byte_for_byte() {
    let fixture = verified("fixture-baseline");
    let output = njutest(&fixture, &["report", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));

    let path = njutest_cli::app::reports::Store::read(&fixture.root)
        .run_of(njutest_cli::app::reports::Index::Any)
        .expect("the index names a run")
        .join(njutest_cli::app::reports::DOCUMENT_NAME);
    assert_eq!(
        stdout(&output),
        std::fs::read_to_string(path).expect("the document"),
        "a reader piping this and a reader opening the file see the same bytes"
    );
}

#[test]
fn report_names_a_run_that_is_not_there_rather_than_answering_about_another() {
    let fixture = verified("fixture-baseline");
    let output = njutest(&fixture, &["report", "20200101T000000Z-000000"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("20200101T000000Z-000000"), "{stderr}");
    assert!(stderr.contains("NJ6005"), "{stderr}");
}

#[test]
fn report_without_a_run_at_all_says_so() {
    let fixture = fixture("fixture-baseline");
    let output = njutest(&fixture, &["report"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("NJ6005"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn trace_summary_counts_the_events_and_finds_nothing_wrong_with_a_complete_recording() {
    let fixture = verified("fixture-baseline");
    let output = njutest(&fixture, &["trace", "summary"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("run-start"), "{text}");
    assert!(text.contains("exec"), "{text}");
    assert!(text.contains("phase"), "{text}");
    assert!(
        text.contains("no problems"),
        "a complete recording has none: {text}"
    );
}

#[test]
fn trace_summary_says_how_many_executions_each_proof_removed() {
    let fixture = fixture("fixture-probeable");
    std::fs::write(fixture.root.join(".njutest.toml"), "version = 1\n").expect("a configuration");
    let verified = njutest(&fixture, &["verify", "--offline", "--locked", "--trace"]);
    assert_eq!(
        verified.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    let output = njutest(&fixture, &["trace", "summary"]);
    let text = stdout(&output);
    assert!(
        text.contains("PROOF\tnever-infected"),
        "a reader who sees a run go faster asks which proof did it: {text}"
    );
}

#[test]
fn trace_summary_says_what_took_the_longest_and_reads_the_engine_beside_it() {
    let fixture = verified("fixture-baseline");

    let output = njutest(&fixture, &["trace", "summary"]);

    let text = stdout(&output);
    assert!(
        text.contains("SLOWEST\t"),
        "a reader asking where a run went reads what took the longest, rather than writing a \
         script to find out: {text}"
    );
    assert!(
        text.contains("ENGINE\t"),
        "most of a run is the engine's, and its recording lies beside this one: {text}"
    );
}

#[test]
fn trace_summary_reports_a_recording_that_lost_its_end() {
    let fixture = verified("fixture-baseline");
    let stream = trace_stream(&fixture);
    let text = std::fs::read_to_string(&stream).expect("the stream");
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("\"run-end\""))
        .collect();
    let mut cut = kept.join("\n");
    cut.push('\n');
    std::fs::write(&stream, cut).expect("a truncated recording");

    let output = njutest(&fixture, &["trace", "summary"]);
    assert_eq!(output.status.code(), Some(2), "a problem is not a success");
    assert!(stdout(&output).contains("run-end"), "{}", stdout(&output));
}

#[test]
fn trace_diff_says_which_phases_moved() {
    let fixture = verified("fixture-baseline");
    let first = only_recording(&fixture);
    let second = njutest(&fixture, &["verify", "--offline", "--locked", "--trace"]);
    assert_eq!(second.status.code(), Some(2));
    let names = recordings(&fixture);
    assert_eq!(names.len(), 2, "{names:?}");

    let other = names
        .iter()
        .find(|name| *name != &first)
        .expect("the second recording");
    let output = njutest(&fixture, &["trace", "diff", &first, other]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("baseline"), "{text}");
    assert!(text.contains(&first), "{text}");
}

fn recordings(fixture: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixture.root.join(".njutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn only_recording(fixture: &Fixture) -> String {
    let names = recordings(fixture);
    assert_eq!(names.len(), 1, "{names:?}");
    names[0].clone()
}

fn trace_stream(fixture: &Fixture) -> PathBuf {
    fixture
        .root
        .join(".njutest/trace")
        .join(only_recording(fixture))
        .join(njutest_cli::trace::FILE_NAME)
}

#[test]
fn diagnostics_bundles_the_report_and_the_recording_of_one_run() {
    let fixture = verified("fixture-baseline");
    let run = only_recording(&fixture);
    let output = njutest(&fixture, &["diagnostics", &run]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle = fixture.root.join(".njutest/diagnostics").join(&run);
    assert!(bundle.is_dir(), "{}", bundle.display());
    assert!(
        bundle
            .join(njutest_cli::app::reports::DOCUMENT_NAME)
            .is_file()
    );
    assert!(bundle.join(njutest_cli::trace::FILE_NAME).is_file());
    assert!(
        bundle.join("bundle.json").is_file(),
        "what the bundle holds and what it could not find"
    );
    assert!(
        stdout(&output).contains(&bundle.display().to_string()),
        "it says where it put it: {}",
        stdout(&output)
    );
}

#[test]
fn diagnostics_of_a_run_that_never_happened_is_an_error() {
    let fixture = verified("fixture-baseline");
    let output = njutest(&fixture, &["diagnostics", "20200101T000000Z-000000"]);
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn plan_names_every_target_a_run_would_measure_without_measuring_one() {
    let fixture = fixture("fixture-baseline");
    let output = njutest(&fixture, &["plan", "--offline", "--locked"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(
        text.contains("fixture-baseline/lib/fixture_baseline"),
        "a plan names the binaries a run measures, which is what a run reports: {text}"
    );
    assert!(text.contains("fixture-baseline/test/doubling"), "{text}");
    assert!(text.contains("TARGETS\t2"), "{text}");
    assert!(
        !njutest_cli::app::reports::Store::read(&fixture.root)
            .runs()
            .exists(),
        "a plan is not a run: it writes no report"
    );
}

#[test]
fn plan_why_says_what_put_each_target_in_scope() {
    let fixture = fixture("fixture-baseline");
    let output = njutest(&fixture, &["plan", "--offline", "--locked", "--why"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(
        text.contains("every workspace member"),
        "the scope a reader did not have to guess: {text}"
    );
    assert!(
        text.contains("ignored"),
        "and how many of a binary's tests libtest will not run: {text}"
    );
}

#[test]
fn cache_leaves_the_engine_build_cache_to_the_engine() {
    let fixture = fixture("fixture-baseline");
    let temp = njutest_devkit::paths::temp_beside(&fixture.root).expect("a temporary directory");
    let target = temp.join(format!(
        "{}kept",
        rust_mutants::workspace::TARGET_DIR_PREFIX
    ));
    std::fs::create_dir_all(&target).expect("the directory");
    let mut owner = rust_mutants::tempowner::claim_cache_of(
        &target,
        jiff::Timestamp::now(),
        rust_mutants::workspace::TARGET_OWNER_SCHEMA,
        &fixture.root,
    )
    .expect("the engine's build cache");
    owner.release().expect("release the engine's claim");

    let output = njutest(&fixture, &["cache", "--gc"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(
        !text.lines().any(|line| line.starts_with("builds    "))
            && !text.contains("build cache")
            && !text.contains(&target.display().to_string()),
        "the runner neither reports nor collects compiled artifacts: {text}"
    );
    assert!(
        target.is_dir(),
        "the engine's cache remains for the engine: {}",
        target.display()
    );
}

#[test]
fn answers_one_machine_established_are_the_answers_another_one_holds() {
    let fixture = verified("fixture-baseline");
    let carried = fixture.root.join("answers.jsonl");
    let path = carried.to_str().expect("a path");

    let output = njutest(&fixture, &["cache", "--export", path]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    let text = stdout(&output);
    assert!(
        text.contains("exported  1 answers"),
        "a machine that has answered for a tree says how many answers left it, because \
         a job that exports nothing and says nothing is a matrix that quietly builds \
         everything as many times as it has jobs: {text}"
    );

    let elsewhere = fixture.root.join("another-machine");
    std::fs::create_dir_all(&elsewhere).expect("a second cache");
    let output = njutest_caching(&fixture, &["cache"], &elsewhere);
    assert!(
        stdout(&output).contains("holds     0 answers"),
        "the second machine starts knowing nothing: {}",
        stdout(&output)
    );

    let output = njutest_caching(&fixture, &["cache", "--import", path], &elsewhere);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("imported  1 answers"),
        "and says how many arrived: {}",
        stdout(&output)
    );
    let output = njutest_caching(&fixture, &["cache"], &elsewhere);
    assert!(
        stdout(&output).contains("holds     1 answers"),
        "which is what it holds afterwards, so the next run of the same tree on it \
         reads an answer rather than establishing one: {}",
        stdout(&output)
    );

    let output = njutest(&fixture, &["cache", "--export", path, "--import", path]);
    assert_eq!(
        output.status.code(),
        Some(i32::from(njutest_cli::cli::EXIT_ERROR)),
        "and a command told to carry answers both ways at once is refused, because \
         which of the two it did would decide whether the file is the answers or the \
         answers are the file: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Where the run a fixture just finished wrote its report.
fn latest_report(fixture: &Fixture) -> PathBuf {
    njutest_cli::app::reports::Store::read(&fixture.root)
        .run_of(njutest_cli::app::reports::Index::Any)
        .expect("the index names a run")
        .join(njutest_cli::app::reports::DOCUMENT_NAME)
}

/// Judges one part of a catalog and keeps the report it wrote.
fn shard(fixture: &Fixture, part: &str) -> PathBuf {
    let output = njutest(
        fixture,
        &["verify", "--offline", "--locked", "--shard", part],
    );
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(2),
        "a part of a catalog is judged like any other run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    latest_report(fixture)
}

#[test]
fn merge_combines_the_parts_of_one_catalog_and_refuses_the_parts_of_two() {
    let fixture = fixture("fixture-baseline");
    let one = shard(&fixture, "1/2");
    let two = shard(&fixture, "2/2");
    assert_ne!(one, two, "two runs, two reports");

    let whole = fixture.root.join("whole.json");
    let output = njutest(
        &fixture,
        &[
            "merge",
            &one.display().to_string(),
            &two.display().to_string(),
            "--output",
            &whole.display().to_string(),
        ],
    );
    assert!(
        whole.is_file(),
        "a merge that was asked for a file writes one: {}{}",
        stdout(&output),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&whole).expect("the whole");
    let combined = njutest_cli::report::json::parse(&text).expect("the whole reads back");
    assert_eq!(
        output.status.code(),
        Some(i32::from(combined.verdict.exit_code())),
        "the parts are put together and the exit code is the whole's verdict, because a \
         pipeline that shards has nothing else to fail on: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        combined.scope.shard, None,
        "and the whole is not a part: a combined report that still named one of them \
         would be read as the answer for that part alone: {text}"
    );

    let missing = fixture.root.join("nowhere.json");
    let refused = njutest(
        &fixture,
        &[
            "merge",
            &one.display().to_string(),
            &missing.display().to_string(),
        ],
    );
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        refused.status.code() == Some(3) && said.contains("nowhere.json"),
        "a part that is not there is a part nobody judged, and the whole would be the \
         rest of the catalog wearing the name of all of it. It names the path a person \
         has to go and look at: {said}"
    );

    fixture_changed(&fixture);
    let elsewhere = shard(&fixture, "1/2");
    let refused = njutest(
        &fixture,
        &[
            "merge",
            &two.display().to_string(),
            &elsewhere.display().to_string(),
        ],
    );
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        refused.status.code() == Some(3) && said.contains("the tree"),
        "and two parts of two trees are not two parts of one: added up they would be a \
         verdict about a tree neither of them measured, which is the one thing sharding \
         may never buy. It said {said}"
    );
}

/// Changes the tree under the fixture, so that a later run is a run of another one.
fn fixture_changed(fixture: &Fixture) {
    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the library");
    std::fs::write(&path, format!("{source}\npub const ADDED: u8 = 1;\n")).expect("a change");
}

#[test]
fn a_plan_refuses_a_package_that_is_not_one_the_same_way_a_verification_does() {
    let fixture = fixture("fixture-workspace");

    let refused = njutest(
        &fixture,
        &["plan", "--package", "nosuch", "--offline", "--locked"],
    );
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a plan is what a person asks before a run to find out what will happen, so it \
         cannot answer a mistyped package with a plan of nothing: {}",
        String::from_utf8_lossy(&refused.stdout)
    );
    let said = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        said.contains("nosuch") && said.contains("not a workspace member"),
        "and the refusal is the one a verification gives, in the same words: {said}"
    );
    assert!(
        said.contains("RM2007"),
        "with the code a person greps for: {said}"
    );

    let planned = njutest(
        &fixture,
        &["plan", "--package", "fixture-core", "--offline", "--locked"],
    );
    assert_eq!(
        planned.status.code(),
        Some(0),
        "while a package the workspace holds is planned: {}",
        String::from_utf8_lossy(&planned.stderr)
    );
}
