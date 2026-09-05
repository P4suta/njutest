// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A whole run against a real workspace: what it establishes, what it writes, what it exits with, and reading it back.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    cache: PathBuf,
    /// Every snapshot and build cache the run makes goes in here, so the tree a test leaves behind is the tree it took with it.
    temp: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-run-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_dir(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).expect("mkdir");
    Fixture {
        root,
        cache,
        temp,
        _dir: dir,
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", &fixture.temp);
    command.env("XDG_CACHE_HOME", &fixture.cache);
    command.args(args);
    command.args(["--root", &fixture.root.to_string_lossy()]);
    command.output().expect("rust-mutants runs")
}

/// A command that takes no workspace, so no `--root` is added to it.
fn rootless(fixture: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("TMPDIR", &fixture.temp)
        .env("XDG_CACHE_HOME", &fixture.cache)
        .output()
        .expect("rust-mutants runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn stored(fixture: &Fixture) -> serde_json::Value {
    let directory = fixture.root.join("reports/mutation");
    let pointer: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(directory.join("latest.json"))
            .expect("a pointer to the newest run"),
    )
    .expect("the pointer is a document");
    let relative = pointer["document"].as_str().expect("a document path");
    serde_json::from_str(&std::fs::read_to_string(directory.join(relative)).expect("the report"))
        .expect("the report is a document")
}

#[test]
fn a_whole_run_judges_every_mutant_scores_the_workspace_and_writes_the_report() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    let text = stdout(&output);
    assert_eq!(
        output.status.code(),
        Some(1),
        "fixture-simple has one test and mutants it cannot notice: {text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("MUTANTS   cataloged="), "{text}");
    assert!(text.contains("SCORE     "), "{text}");
    assert!(text.contains("surviving-mutant"), "{text}");

    let document = stored(&fixture);
    assert_eq!(document["document_type"], "rust-mutants/run-report");
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["run"]["exit_code"], 1);
    assert!(!document["run"]["interrupted"].as_bool().expect("a flag"));
    assert_eq!(document["selection"]["tier"], "all");

    let accounting = &document["accounting"];
    let number = |key: &str| accounting[key].as_u64().unwrap_or_else(|| panic!("{key}"));
    assert!(number("cataloged") > 0);
    assert_eq!(
        number("executed"),
        number("cataloged") - number("not_run"),
        "every mutant is executed or accounted for as unexecuted"
    );
    assert_eq!(
        number("killed")
            + number("survived")
            + number("timed_out")
            + number("inconclusive")
            + number("errored"),
        number("executed"),
        "the outcome columns add up to what ran"
    );
    assert!(number("survived") > 0, "{accounting}");
}

#[test]
fn the_report_names_every_mutant_scores_what_it_decided_and_reports_every_survivor() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let document = stored(&fixture);
    let accounting = &document["accounting"];
    let number = |key: &str| accounting[key].as_u64().unwrap_or_else(|| panic!("{key}"));

    let mutants = document["mutants"].as_array().expect("mutants");
    assert_eq!(count(mutants.len()), number("cataloged"));
    assert!(
        mutants
            .iter()
            .all(|one| one["path"] == "src/lib.rs" && one["package"] == "fixture-simple"),
        "{mutants:?}"
    );
    assert_eq!(
        document["score"]["decided"].as_u64().expect("decided"),
        number("killed") + number("timed_out") + number("survived")
    );
    let findings = document["findings"].as_array().expect("findings");
    assert_eq!(count(findings.len()), number("survived"));
    assert!(findings.iter().all(|one| one["kind"] == "surviving-mutant"));
}

#[test]
fn the_report_validates_against_the_schema_that_is_published_with_it() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document = stored(&fixture);
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-run-report-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is a document");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn the_stored_report_is_read_back_by_the_report_command() {
    let fixture = fixture("fixture-simple");
    let run = against(&fixture, &["run", "--offline", "--locked"]);
    assert_eq!(run.status.code(), Some(1));

    let read_back = against(&fixture, &["report"]);
    assert_eq!(
        read_back.status.code(),
        Some(1),
        "reading a report back reports what the run reported"
    );
    let text = stdout(&read_back);
    assert!(text.contains("MUTANTS   cataloged="), "{text}");

    let as_json = against(&fixture, &["report", "--format", "json"]);
    assert_eq!(as_json.status.code(), Some(0));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&as_json)).expect("one document");
    assert_eq!(document["document_type"], "rust-mutants/run-report");

    let missing = against(&fixture, &["report", "--run", "20200101T000000000Z"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("RM0007"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}

#[test]
fn an_expectation_the_run_confirms_stops_being_a_finding_and_a_stale_one_starts() {
    let fixture = fixture("fixture-simple");
    let first = against(&fixture, &["run", "--offline", "--locked", "--no-report"]);
    assert_eq!(first.status.code(), Some(1));
    let survivor = stdout(&first)
        .lines()
        .find_map(|line| line.strip_prefix("surviving-mutant       "))
        .and_then(|detail| detail.split_whitespace().nth(3))
        .expect("a survivor is named")
        .trim_end_matches(';')
        .to_owned();

    std::fs::write(
        fixture.root.join(".rust-mutants.toml"),
        format!(
            "version = 1\n\n[[mutation.expect]]\nid = \"{survivor}\"\nreason = \"the fixture \
             documents this one as unreachable by its single test\"\n"
        ),
    )
    .expect("write the configuration");

    let second = against(&fixture, &["run", "--offline", "--locked"]);
    let text = stdout(&second);
    let document = stored(&fixture);
    assert_eq!(document["accounting"]["expected"], 1, "{text}");
    assert_eq!(document["expectations"][0]["standing"], "met");
    let findings = document["findings"].as_array().expect("findings");
    assert!(
        findings
            .iter()
            .all(|one| one["mutant"] != serde_json::Value::String(survivor.clone())),
        "the declared survivor is accounted for, not reported: {findings:?}"
    );

    std::fs::write(
        fixture.root.join(".rust-mutants.toml"),
        "version = 1\n\n[[mutation.expect]]\nid = \"0000deadbeef\"\nreason = \"a mutant that is \
         not in this catalog\"\n",
    )
    .expect("write the configuration");
    let third = against(&fixture, &["run", "--offline", "--locked"]);
    assert_eq!(third.status.code(), Some(1));
    let document = stored(&fixture);
    assert_eq!(document["expectations"][0]["standing"], "unmatched");
    assert!(
        document["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|one| one["kind"] == "unmatched-expectation"),
        "{document}"
    );
}

#[test]
fn a_process_that_already_selects_a_mutant_is_refused_before_anything_runs() {
    let fixture = fixture("fixture-simple");
    let output = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(["list", "--root", &fixture.root.to_string_lossy()])
        .env("NO_COLOR", "1")
        .env("TMPDIR", &fixture.temp)
        .env("RUST_MUTANTS_ACTIVE", "0".repeat(64))
        .output()
        .expect("rust-mutants runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RM0006"), "{stderr}");
    assert!(stderr.contains("RUST_MUTANTS_ACTIVE"), "{stderr}");
}

#[test]
fn init_writes_a_configuration_that_changes_nothing_and_refuses_to_overwrite() {
    let fixture = fixture("fixture-simple");
    let first = against(&fixture, &["init"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));
    let path = fixture.root.join(".rust-mutants.toml");
    assert!(path.is_file());

    let again = against(&fixture, &["init"]);
    assert_eq!(again.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("RM0008"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let forced = against(&fixture, &["init", "--force"]);
    assert_eq!(forced.status.code(), Some(0));
}

#[test]
fn doctor_names_the_toolchain_the_workspace_and_where_temporary_trees_go() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    for label in ["cargo", "rustc", "host", "workspace", "config", "temp"] {
        assert!(text.contains(label), "{label} is missing from {text}");
    }
    assert!(text.contains("rust-mutants-snap-"), "{text}");
}

#[test]
fn cache_says_what_a_run_left_in_the_temporary_directory_and_gc_reclaims_it() {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-cache-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join("fixture-simple");
    copy_dir(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        &root,
    );
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");

    let against_temp = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
            .args(args)
            .args(["--root", &root.to_string_lossy()])
            .env("NO_COLOR", "1")
            .env("TMPDIR", &temp)
            .output()
            .expect("rust-mutants runs")
    };

    let run = against_temp(&["run", "--offline", "--locked", "--no-report"]);
    assert_eq!(run.status.code(), Some(1), "{}", stdout(&run));

    let listed = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .arg("cache")
        .env("NO_COLOR", "1")
        .env("TMPDIR", &temp)
        .output()
        .expect("rust-mutants runs");
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    assert_eq!(listed.status.code(), Some(0), "{text}");
    assert!(text.contains("caches      1 reclaimable"), "{text}");
    assert!(
        text.contains("snapshots   0 reclaimable"),
        "a finished run removes its snapshot and keeps its cache: {text}"
    );

    let collected = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(["cache", "--gc"])
        .env("NO_COLOR", "1")
        .env("TMPDIR", &temp)
        .output()
        .expect("rust-mutants runs");
    let text = String::from_utf8_lossy(&collected.stdout).into_owned();
    assert!(text.contains("caches      1 removed"), "{text}");
    let left: Vec<PathBuf> = std::fs::read_dir(&temp)
        .expect("the temporary directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert!(left.is_empty(), "gc collects the caches: {left:?}");
}

#[test]
fn a_shard_is_not_a_shard_unless_it_names_a_part_of_something() {
    let fixture = fixture("fixture-simple");
    for bad in ["0/2", "3/2", "1/0", "one/two", "2", ""] {
        let output = against(&fixture, &["run", "--offline", "--locked", "--shard", bad]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{bad:?} is not a shard: {}",
            stdout(&output)
        );
    }
}

#[test]
fn the_parts_of_a_catalog_put_back_together_are_the_whole_of_it() {
    let whole_fixture = fixture("fixture-simple");
    let output = against(
        &whole_fixture,
        &["run", "--offline", "--locked", "--tier", "all"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let whole = stored(&whole_fixture);

    let parts_fixture = fixture("fixture-simple");
    let mut written = Vec::new();
    for part in ["1/3", "2/3", "3/3"] {
        let output = against(
            &parts_fixture,
            &[
                "run",
                "--offline",
                "--locked",
                "--tier",
                "all",
                "--shard",
                part,
            ],
        );
        assert!(
            output.status.code() == Some(0) || output.status.code() == Some(1),
            "{part}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document = stored(&parts_fixture);
        assert_eq!(document["run"]["shard"], part);
        let path = parts_fixture
            .root
            .join(format!("part-{}.json", part.replace('/', "-")));
        std::fs::write(&path, serde_json::to_string(&document).expect("renders")).expect("write");
        written.push(path);
    }

    let mut arguments = vec!["merge".to_owned()];
    arguments.extend(written.iter().map(|path| path.display().to_string()));
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let merged_output = rootless(&parts_fixture, &borrowed);
    let whole_code = whole["run"]["exit_code"].as_i64().expect("an exit code");
    assert_eq!(
        merged_output.status.code().map(i64::from),
        Some(whole_code),
        "the whole and its parts reach the same answer: {}",
        String::from_utf8_lossy(&merged_output.stderr)
    );
    let merged: serde_json::Value =
        serde_json::from_str(&stdout(&merged_output)).expect("one document");

    assert_eq!(merged["run"]["shard"], serde_json::Value::Null);
    assert_eq!(merged["accounting"], whole["accounting"]);
    assert_eq!(merged["score"], whole["score"]);
    assert_eq!(
        merged["mutants"].as_array().map(Vec::len),
        whole["mutants"].as_array().map(Vec::len)
    );
    let outcomes = |document: &serde_json::Value| -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = document["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .map(|one| {
                (
                    one["id"].as_str().unwrap_or_default().to_owned(),
                    one["outcome"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        found.sort();
        found
    };
    assert_eq!(
        outcomes(&merged),
        outcomes(&whole),
        "every mutant reached the same verdict in a part as it did in the whole"
    );
}

#[test]
fn reports_that_are_not_the_parts_of_one_whole_are_refused() {
    let fixture = fixture("fixture-simple");
    let first = against(
        &fixture,
        &["run", "--offline", "--locked", "--shard", "1/2"],
    );
    assert!(first.status.code().is_some(), "{}", stdout(&first));
    let one = fixture.root.join("one.json");
    std::fs::write(
        &one,
        serde_json::to_string(&stored(&fixture)).expect("renders"),
    )
    .expect("write");

    let output = rootless(
        &fixture,
        &["merge", &one.to_string_lossy(), &one.to_string_lossy()],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("more than one"),
        "the same part twice is not two parts: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_warm_cache_reaches_the_same_answer_without_executing_a_mutant() {
    let fixture = fixture("fixture-simple");
    let cold = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    assert_eq!(cold.status.code(), Some(1), "{}", stdout(&cold));
    let first = stored(&fixture);
    assert!(
        first["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["source_run_id"] == serde_json::Value::Null),
        "a cold run establishes everything itself: {first}"
    );

    let warm = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    assert_eq!(warm.status.code(), Some(1), "{}", stdout(&warm));
    let second = stored(&fixture);
    assert_eq!(
        second["accounting"], first["accounting"],
        "a warm run reaches the answer a cold one did"
    );
    assert_eq!(second["score"], first["score"]);
    assert!(
        second["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["source_run_id"] == first["run"]["id"]),
        "and it read every one of them back rather than running it: {second}"
    );

    let afresh = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-cache",
        ],
    );
    assert_eq!(afresh.status.code(), Some(1));
    assert!(
        stored(&fixture)["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["source_run_id"] == serde_json::Value::Null),
        "a run told to establish everything afresh does"
    );
}

#[test]
fn a_tree_that_changed_is_a_different_question_and_is_answered_again() {
    let fixture = fixture("fixture-simple");
    assert_eq!(
        against(&fixture, &["run", "--offline", "--locked"])
            .status
            .code(),
        Some(1)
    );
    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("{source}\n// one more line\n")).expect("write");

    assert_eq!(
        against(&fixture, &["run", "--offline", "--locked"])
            .status
            .code(),
        Some(1)
    );
    assert!(
        stored(&fixture)["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["source_run_id"] == serde_json::Value::Null),
        "a record is about the tree it was established on, and this is another tree"
    );
}
