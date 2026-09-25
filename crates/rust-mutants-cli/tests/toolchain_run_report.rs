// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A whole run against a real workspace: what it establishes, what it writes, what it exits with, and reading it back.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::{Fixture, copy_tree};
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};
use std::path::{Path, PathBuf};
use std::process::Output;

include!("support/metadata.rs");

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root])
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

/// A command that takes no workspace, so no `--root` is added to it.
fn rootless(fixture: &Fixture, args: &[&str]) -> Output {
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

fn stdout(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> std::borrow::Cow<'_, str> {
    njutest_devkit::process::strict_utf8(&output.stderr)
}

fn count(value: usize) -> u64 {
    u64::try_from(value).expect("a test collection fits the report schema")
}

/// The report the newest run stored, as a document.
fn stored(fixture: &Fixture) -> serde_json::Value {
    njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    ))
    .expect("the report is a document")
}

#[test]
fn a_whole_run_judges_every_mutant_scores_the_workspace_and_writes_the_report() {
    let fixture = Fixture::copy("fixture-coverage");
    let output = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    let text = stdout(&output);
    assert_eq!(
        output.status.code(),
        Some(1),
        "the fixture compares two values the compiler will not vouch for, so nothing \
         removes that mutation and one test has to notice it: {text}{}",
        stderr(&output)
    );
    assert!(text.contains("mutants were cataloged"), "{text}");
    assert!(text.contains("SCORE     "), "{text}");
    assert!(text.contains("surviving-mutant"), "{text}");

    let document = stored(&fixture);
    assert_eq!(document["document_type"], "rust-mutants/run-report");
    assert_eq!(document["schema_version"], 2);
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
            + number("step_limit_reached")
            + number("waited")
            + number("inconclusive")
            + number("errored"),
        number("executed"),
        "the outcome columns add up to what ran"
    );
    assert!(number("survived") > 0, "{accounting}");
}

#[test]
fn the_report_names_every_mutant_scores_what_it_decided_and_reports_every_gap() {
    let fixture = Fixture::copy("fixture-simple");
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
        number("killed") + number("survived"),
        "a mutation this machine stopped waiting for decided nothing, so it is not among \
         what the score is over"
    );
    let findings = document["findings"].as_array().expect("findings");
    assert_eq!(
        count(findings.len()),
        number("survived") + number("not_run"),
        "a mutation the tests did not notice and one a proof says they could not have are \
         the same gap, and each is reported once"
    );
    assert!(
        findings.iter().all(|one| one["kind"] == "surviving-mutant"
            || one["kind"] == "discharged-mutant"
            || one["kind"] == "unreached-mutant"),
        "{findings:?}"
    );
}

/// Every error `named` reports about `document`, as a reader of the schema would see them.
fn against_schema(named: &str, document: &serde_json::Value) -> Vec<String> {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root()
                .join("schema")
                .join(named),
        )
        .expect("the schema"),
    )
    .expect("the schema is a document");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    validator
        .iter_errors(document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
}

#[test]
fn the_report_validates_against_the_schema_that_is_published_with_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "{}",
        stderr(&output)
    );
    let errors = against_schema("rust-mutants-run-report-v1.json", &stored(&fixture));
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn the_stored_report_is_read_back_by_the_report_command() {
    let fixture = Fixture::copy("fixture-simple");
    let run = against(&fixture, &["run", "--offline", "--locked"]);
    assert_eq!(run.status.code(), Some(1));

    let read_back = against(&fixture, &["report"]);
    assert_eq!(
        read_back.status.code(),
        Some(1),
        "reading a report back reports what the run reported"
    );
    let text = stdout(&read_back);
    assert!(text.contains("mutants were cataloged"), "{text}");

    let as_json = against(&fixture, &["report", "--format", "json"]);
    assert_eq!(as_json.status.code(), Some(0));
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&as_json)).expect("one document");
    assert_eq!(document["document_type"], "rust-mutants/run-report");

    let missing = against(&fixture, &["report", "--run", "20200101T000000000Z"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("RM0007"), "{}", stderr(&missing));
}

#[test]
fn an_expectation_the_run_confirms_stops_being_a_finding_and_a_stale_one_starts() {
    let fixture = Fixture::copy("fixture-coverage");
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
        fixture.root().join(".rust-mutants.toml"),
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
    assert!(
        document["expectations"][0]["covered"].is_null(),
        "a claim that names one mutation carries an explicit absence rather than inventing a \
         count: {}",
        document["expectations"][0]
    );
    let findings = document["findings"].as_array().expect("findings");
    assert!(
        findings
            .iter()
            .all(|one| one["mutant"] != serde_json::Value::String(survivor.clone())),
        "the declared survivor is accounted for, not reported: {findings:?}"
    );

    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
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

/// An incomplete mutation mode is about the environment a process inherits, so this one starts a process.
#[test]
fn a_process_with_an_incomplete_touch_mode_is_refused_before_anything_runs() {
    let fixture = Fixture::copy("fixture-simple");
    let output = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .args([
            "list",
            "--root",
            njutest_devkit::paths::utf8(fixture.root()),
        ])
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .env_remove("RUST_MUTANTS_ACTIVE")
        .env_remove("RUST_MUTANTS_CATALOG")
        .env("RUST_MUTANTS_TOUCH", "not-a-run")
        .output()
        .expect("rust-mutants runs");
    assert_eq!(output.status.code(), Some(2));
    let error_text = stderr(&output);
    assert!(error_text.contains("RM0006"), "{error_text}");
    assert!(error_text.contains("RUST_MUTANTS_TOUCH"), "{error_text}");
}
#[test]
fn init_writes_a_configuration_that_changes_nothing_and_refuses_to_overwrite() {
    let fixture = Fixture::copy("fixture-simple");
    let first = against(&fixture, &["init"]);
    assert_eq!(first.status.code(), Some(0), "{}", stdout(&first));
    let path = fixture.root().join(".rust-mutants.toml");
    assert!(test_metadata(&path).is_file());

    let again = against(&fixture, &["init"]);
    assert_eq!(again.status.code(), Some(2));
    assert!(stderr(&again).contains("RM0008"), "{}", stderr(&again));
    let forced = against(&fixture, &["init", "--force"]);
    assert_eq!(forced.status.code(), Some(0));
}

#[test]
fn doctor_names_the_toolchain_the_workspace_and_where_temporary_trees_go() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["doctor"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
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
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        &root,
    );
    let temp = dir.path().join("temp");
    let cache = dir.path().join("cache");
    for made in [&temp, &cache] {
        std::fs::create_dir_all(made).expect("mkdir");
    }
    let at = environment_at(&root, &temp, &cache);
    let named = njutest_devkit::paths::utf8(&root).to_owned();

    let run = asked(
        &at,
        &[
            "run",
            "--offline",
            "--locked",
            "--no-report",
            "--root",
            &named,
        ],
    );
    assert_eq!(run.status.code(), Some(1), "{}", stdout(&run));

    let listed = asked(&at, &["cache"]);
    let text = stdout(&listed);
    assert_eq!(listed.status.code(), Some(0), "{text}");
    assert!(text.contains("caches       1 reclaimable"), "{text}");
    assert!(
        text.contains("snapshots    0 reclaimable"),
        "a finished run removes its snapshot and keeps its cache: {text}"
    );

    let swept = asked(&at, &["cache", "--gc"]);
    let text = stdout(&swept);
    assert!(
        text.contains("caches       0 removed") && text.contains("1 kept for the next run"),
        "a sweep keeps the build caches a later run can still look up, so the next run \
         is still fast: {text}"
    );

    let collected = asked(&at, &["cache", "--gc", "--all"]);
    let text = stdout(&collected);
    assert!(text.contains("caches       1 removed"), "{text}");
    let left: Vec<PathBuf> = std::fs::read_dir(&temp)
        .expect("the temporary directory")
        .map(|entry| entry.expect("read a temporary-directory entry"))
        .map(|entry| entry.path())
        .collect();
    assert!(left.is_empty(), "and --all collects them: {left:?}");
}

#[test]
fn a_shard_is_not_a_shard_unless_it_names_a_part_of_something() {
    let fixture = Fixture::copy("fixture-simple");
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
    let whole_fixture = Fixture::copy("fixture-simple");
    let output = against(
        &whole_fixture,
        &["run", "--offline", "--locked", "--tier", "all"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let whole = stored(&whole_fixture);

    let parts_fixture = Fixture::copy("fixture-simple");
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
            stderr(&output)
        );
        let document = stored(&parts_fixture);
        assert_eq!(document["run"]["shard"], part);
        let path = parts_fixture
            .root()
            .join(format!("part-{}.json", part.replace('/', "-")));
        std::fs::write(&path, serde_json::to_string(&document).expect("renders")).expect("write");
        written.push(path);
    }

    let mut arguments = vec!["merge".to_owned()];
    arguments.extend(
        written
            .iter()
            .map(|path| njutest_devkit::paths::utf8(path).to_owned()),
    );
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let merged_output = rootless(&parts_fixture, &borrowed);
    let whole_code = whole["run"]["exit_code"].as_i64().expect("an exit code");
    assert_eq!(
        merged_output.status.code().map(i64::from),
        Some(whole_code),
        "the whole and its parts reach the same answer: {}",
        stderr(&merged_output)
    );
    let merged: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&merged_output)).expect("one document");

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
    let fixture = Fixture::copy("fixture-simple");
    let first = against(
        &fixture,
        &["run", "--offline", "--locked", "--shard", "1/2"],
    );
    assert!(first.status.code().is_some(), "{}", stdout(&first));
    let one = fixture.root().join("one.json");
    std::fs::write(
        &one,
        serde_json::to_string(&stored(&fixture)).expect("renders"),
    )
    .expect("write");

    let named = njutest_devkit::paths::utf8(&one);
    let output = rootless(&fixture, &["merge", named, named]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("more than one"),
        "the same part twice is not two parts: {}",
        stderr(&output)
    );
}

#[test]
fn a_warm_cache_reaches_the_same_answer_without_executing_a_mutant() {
    let fixture = Fixture::copy("fixture-simple");
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
            .filter(|one| one["outcome"] != "not_run")
            .all(|one| one["source_run_id"] == first["run"]["id"]),
        "and it read every answer back rather than running it again: {second}"
    );
    assert!(
        second["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|one| one["outcome"] == "not_run")
            .all(|one| one["source_run_id"] == serde_json::Value::Null),
        "a mutant the first run proved rather than executed left no answer to read back, and \
         the second proves it again for nothing: {second}"
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
    let fixture = Fixture::copy("fixture-simple");
    assert_eq!(
        against(&fixture, &["run", "--offline", "--locked"])
            .status
            .code(),
        Some(1)
    );
    let path = fixture.root().join("src/lib.rs");
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

#[test]
fn every_mutant_row_carries_what_re_minting_its_id_needs() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let document = stored(&fixture);
    let rows = document["mutants"].as_array().expect("the rows");
    assert!(!rows.is_empty(), "{document}");
    for row in rows {
        let text = |key: &str| {
            row[key]
                .as_str()
                .unwrap_or_else(|| panic!("{key} of {row}"))
                .to_owned()
        };
        let number = |key: &str| {
            u32::try_from(
                row[key]
                    .as_u64()
                    .unwrap_or_else(|| panic!("{key} of {row}")),
            )
            .unwrap_or_else(|error| panic!("{key} of {row}: {error}"))
        };
        let identity = rust_mutants::id::Identity {
            path: text("path"),
            rule_name: text("rule"),
            rule_version: number("rule_version"),
            span: rust_mutants::span::Span {
                start: number("start_byte"),
                end: number("end_byte"),
            },
            source_digest: text("source_digest"),
            original_digest: rust_mutants::id::digest(text("original").as_bytes()),
            replacement_digest: rust_mutants::id::digest(text("replacement").as_bytes()),
        };
        let minted = identity.id().expect("the row is a complete identity");
        assert_eq!(
            minted.as_str(),
            text("id"),
            "a row a reader cannot re-mint leaves the identity unaudited: {row}"
        );
        assert!(
            minted.as_str().starts_with(&text("display_id")),
            "the short identity is the head of the full one: {row}"
        );
    }
}

#[test]
fn a_rejection_row_carries_its_catalog_index() {
    let fixture = Fixture::copy("fixture-rejectable");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-verify",
        ],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let document = stored(&fixture);
    let rejections = document["rejections"].as_array().expect("the refusals");
    assert!(!rejections.is_empty(), "the fixture exists to be refused");
    let mut indices: Vec<u64> = rejections
        .iter()
        .map(|row| row["index"].as_u64().unwrap_or_else(|| panic!("{row}")))
        .chain(
            document["mutants"]
                .as_array()
                .expect("the rows")
                .iter()
                .map(|row| row["index"].as_u64().unwrap_or_else(|| panic!("{row}"))),
        )
        .collect();
    indices.sort_unstable();
    let dense: Vec<u64> = (0..count(indices.len())).collect();
    assert_eq!(
        indices, dense,
        "the accepted and the refused together are the whole catalog"
    );
}

#[test]
fn an_unreached_finding_is_a_finding_the_schema_knows() {
    let fixture = Fixture::copy("fixture-unreached");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--coverage",
            "--tier",
            "balanced",
        ],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let document = stored(&fixture);
    let kinds: Vec<&str> = document["findings"]
        .as_array()
        .expect("the findings")
        .iter()
        .filter_map(|finding| finding["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"unreached-mutant"),
        "the fixture exists for its unreached mutation: {kinds:?}"
    );
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-run-report-v1.json"),
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
fn a_v1_reader_refuses_fields_outside_its_exact_schema() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let directory = rust_mutants_cli::app::stored::Store::read(fixture.root()).root();
    let pointer: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(directory.join("latest.json")).expect("a pointer"),
    )
    .expect("the pointer is a document");
    let path = directory.join(pointer["document"].as_str().expect("a document path"));
    let mut document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&path).expect("the report"),
    )
    .expect("the report is a document");
    document["a_field_from_a_later_release"] = serde_json::json!("whatever it means");
    document["mutants"][0]["another_one"] = serde_json::json!(7);
    document["rejections"] = serde_json::json!([]);
    std::fs::write(&path, document.to_string()).expect("writing the newer report");

    let read = against(&fixture, &["report"]);
    assert_eq!(
        read.status.code(),
        Some(2),
        "one v1 identity cannot silently acquire a later shape: {}",
        stderr(&read)
    );
    assert!(stderr(&read).contains("RM0007"), "{}", stderr(&read));
    assert!(stdout(&read).is_empty(), "{}", stdout(&read));
}

/// Runs each part of the catalog and writes its report beside the tree, returning the paths.
fn parts(fixture: &Fixture, shards: &[&str]) -> Vec<PathBuf> {
    let mut written = Vec::new();
    for part in shards {
        let output = against(
            fixture,
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
            output.status.code().is_some_and(|code| code < 2),
            "{part}: {}",
            stderr(&output)
        );
        let document = stored(fixture);
        let path = fixture
            .root()
            .join(format!("part-{}.json", part.replace('/', "-")));
        std::fs::write(&path, serde_json::to_string(&document).expect("renders")).expect("write");
        written.push(path);
    }
    written
}

#[test]
fn the_parts_of_a_catalog_over_two_packages_and_two_targets_are_the_whole_of_it() {
    let whole_fixture = Fixture::copy("fixture-workspace");
    let output = against(
        &whole_fixture,
        &["run", "--offline", "--locked", "--tier", "all"],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let whole = stored(&whole_fixture);
    let answered: std::collections::BTreeSet<&str> = whole["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter_map(|row| row["target"].as_str())
        .filter(|target| !target.is_empty())
        .collect();
    let packages: std::collections::BTreeSet<&str> = whole["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter_map(|row| row["package"].as_str())
        .collect();
    assert!(
        answered.len() >= 2 && packages.len() >= 2,
        "a shard test over one package and one target proves nothing about a catalog cut \
         across them: {answered:?} {packages:?}"
    );

    let parts_fixture = Fixture::copy("fixture-workspace");
    let written = parts(&parts_fixture, &["1/4", "2/4", "3/4", "4/4"]);
    let mut arguments = vec!["merge".to_owned()];
    arguments.extend(
        written
            .iter()
            .map(|path| njutest_devkit::paths::utf8(path).to_owned()),
    );
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let merged_output = rootless(&parts_fixture, &borrowed);
    let merged: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&merged_output)).expect("one document");
    assert_eq!(merged["accounting"], whole["accounting"]);
    assert_eq!(merged["score"], whole["score"]);
    let fates = |document: &serde_json::Value| -> Vec<(String, String, String)> {
        let mut rows: Vec<(String, String, String)> = document["mutants"]
            .as_array()
            .expect("the rows")
            .iter()
            .map(|row| {
                (
                    row["id"].as_str().unwrap_or_default().to_owned(),
                    row["outcome"].as_str().unwrap_or_default().to_owned(),
                    row["target"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(
        fates(&merged),
        fates(&whole),
        "every mutation of every package reaches the same fate against the same target, \
         whichever part it was cut into"
    );
}

/// The documents a run of `fixture-coverage` leaves, measured the way `extra` asks.
fn evidence_of(extra: &[&str]) -> (PathBuf, Fixture) {
    let fixture = Fixture::copy("fixture-coverage");
    let output = against(
        &fixture,
        &[&["run", "--offline", "--locked", "--tier", "all"], extra].concat(),
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&output)
    );
    let directory = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    (directory, fixture)
}

fn document_at(path: &Path) -> serde_json::Value {
    njutest_devkit::strictjson::decode_str(&std::fs::read_to_string(path).expect("the document"))
        .expect("a document")
}

#[test]
fn a_run_keeps_what_the_audit_re_derives_its_routes_from() {
    let (directory, fixture) = evidence_of(&[]);
    assert!(
        test_metadata(fixture.root()).is_dir(),
        "the evidence source remains alive while inspected"
    );
    for name in ["touched-v1.json", "catalog-v1.json"] {
        assert!(
            test_metadata(&directory.join(name)).is_file(),
            "a proof layer removed executions, and a report that says so without the premises \
             is a claim rather than a proof: {name}"
        );
    }
    let touched = document_at(&directory.join("touched-v1.json"));
    assert!(
        touched["targets"]
            .as_object()
            .is_some_and(|targets| !targets.is_empty()),
        "what each target's guards recorded is what a route rests on: {touched}"
    );
    assert!(
        touched["targets"]
            .as_object()
            .expect("the targets")
            .values()
            .any(|one| one["bodies"]
                .as_object()
                .is_some_and(|bodies| !bodies.is_empty())),
        "and the bodies they entered are what `branch-never-taken` rests on: {touched}"
    );
    let catalog = document_at(&directory.join("catalog-v1.json"));
    assert!(
        catalog["mutants"]
            .as_array()
            .expect("the rows")
            .iter()
            .any(|row| row["branch"].is_object()),
        "with the body the compiler vouched for: {catalog}"
    );

    for (name, document) in [
        ("rust-mutants-touched-v1.json", &touched),
        ("rust-mutants-catalog-v1.json", &catalog),
    ] {
        let errors = against_schema(name, document);
        assert!(
            errors.is_empty(),
            "the premises a run keeps are read by whoever audits it, and {name} is what says \
             how: {errors:#?}"
        );
    }
    assert!(
        touched["narrowing"]["compared"].is_array(),
        "including which mutants the tree could record anything about, without which an \
         absence in the record says nothing: {touched}"
    );
}

#[test]
fn a_coverage_run_keeps_the_measurement_its_own_discharges_rest_on() {
    let (directory, fixture) = evidence_of(&["--coverage"]);
    assert!(
        test_metadata(fixture.root()).is_dir(),
        "the evidence source remains alive while inspected"
    );
    let reached = document_at(&directory.join("reached-v1.json"));
    assert!(
        reached["targets"]
            .as_object()
            .is_some_and(|targets| !targets.is_empty()),
        "the measurement each target left behind is what a region-based discharge rests on: \
         {reached}"
    );
}

#[test]
fn a_claim_written_for_several_mutations_says_how_many_it_was_resolved_against() {
    let fixture = Fixture::copy("fixture-families");
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        "version = 1\n\n[[mutation.expect]]\npath = \"src/lib.rs\"\nitem = \"results\"\nrule = \
         \"question-to-unwrap\"\noriginal = \"?\"\ncount = 2\nreason = \"both of them parse the \
         same text, so one reason is written for the pair\"\n",
    )
    .expect("write the configuration");

    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--include",
            "src/lib.rs",
            "--operator",
            "question-to-unwrap",
        ],
    );
    let text = stdout(&output);
    let document = stored(&fixture);
    assert_eq!(
        document["expectations"][0]["covered"], 2,
        "the claim was resolved against both mutations its locator names, and a report that does \
         not say how wide a reason is leaves a reader to re-derive it from the catalog: {text}"
    );
}

/// The document with everything a second run of one catalog is allowed to say differently taken out.
fn timeless(mut document: serde_json::Value) -> serde_json::Value {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.retain(|name, _| {
                    ![
                        "id",
                        "run",
                        "run_id",
                        "document",
                        "started_at",
                        "finished_at",
                        "duration_ms",
                        "elapsed_ms",
                        "established_tests",
                        "timing",
                        "source_run_id",
                    ]
                    .contains(&name.as_str())
                });
                for held in fields.values_mut() {
                    strip(held);
                }
            }
            serde_json::Value::Array(held) => {
                for one in held {
                    strip(one);
                }
            }
            _ => {}
        }
    }
    strip(&mut document);
    document
}

#[test]
fn two_runs_of_one_catalog_write_one_report() {
    let fixture = Fixture::copy("fixture-coverage");
    let asked = [
        "run",
        "--offline",
        "--locked",
        "--no-cache",
        "--rule",
        "le-to-lt",
    ];
    let first = against(&fixture, &asked);
    assert_eq!(first.status.code(), Some(1), "{}", stdout(&first));
    let earlier = timeless(stored(&fixture));

    let second = against(&fixture, &asked);
    assert_eq!(second.status.code(), Some(1), "{}", stdout(&second));
    let later = timeless(stored(&fixture));

    assert_eq!(
        earlier, later,
        "what a run says about a catalog is what the next run of it says, or `report-diff` \
         between two runs answers about the order things came back in rather than about the code"
    );

    let ordered: Vec<u64> = later["mutants"]
        .as_array()
        .expect("the mutants of the report")
        .iter()
        .map(|one| one["index"].as_u64().expect("a catalog index"))
        .collect();
    let mut ascending = ordered.clone();
    ascending.sort_unstable();
    assert_eq!(
        ordered, ascending,
        "the rows are in catalog order rather than in the order the run happened to finish \
         them, which is what makes the comparison above a fact rather than a coincidence of \
         scheduling: {ordered:?}"
    );
    assert!(
        ordered.len() > 1,
        "this fixture is one with several mutants in it"
    );
    let unselected = later["mutants"]
        .as_array()
        .expect("the mutants of the report")
        .iter()
        .filter(|one| one["not_run_reason"] == "unselected")
        .count();
    assert!(
        unselected > 0,
        "and the run left some of them unselected, which is what puts rows in the report that \
         the execution never ordered: {}",
        later["accounting"]
    );
}

/// Every row of a stored report that says why it was not run, with its route.
fn unrun(document: &serde_json::Value) -> Vec<(String, String)> {
    document["mutants"]
        .as_array()
        .expect("the mutants of the report")
        .iter()
        .filter(|one| one["outcome"] == "not_run")
        .map(|one| {
            (
                one["not_run_reason"].as_str().unwrap_or("").to_owned(),
                one["route"]["granularity"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_proof_removing_a_mutation_and_nothing_reaching_it_are_two_answers() {
    for (name, granularity, reason, finding) in [
        (
            "fixture-coverage",
            "discharged",
            "discharged",
            "discharged-mutant",
        ),
        (
            "fixture-unreached",
            "unreached",
            "unreached",
            "unreached-mutant",
        ),
    ] {
        let fixture = Fixture::copy(name);
        let output = against(&fixture, &["run", "--offline", "--locked", "--tier", "all"]);
        let document = stored(&fixture);
        let rows = unrun(&document);
        let mine: Vec<&(String, String)> =
            rows.iter().filter(|(_, was)| was == granularity).collect();
        assert!(
            !mine.is_empty(),
            "{name} is the fixture whose run answers {granularity}: {rows:?} {}",
            stdout(&output)
        );
        for (said, _) in &mine {
            assert_eq!(
                said, reason,
                "a mutation a proof removed and one nothing reaches are two answers, and a \
                 reader told the wrong one checks the proof when they should write a test, or \
                 the other way about: {rows:?}"
            );
        }
        let counted = u64::try_from(mine.len()).expect("the fixture count fits u64");
        assert_eq!(
            document["accounting"][reason].as_u64(),
            Some(counted),
            "and the column a reader counts them in is the one they answer to: {rows:?}"
        );
        let kinds: Vec<&str> = document["findings"]
            .as_array()
            .expect("the findings")
            .iter()
            .filter_map(|one| one["kind"].as_str())
            .collect();
        assert!(
            kinds.contains(&finding),
            "and the finding a reader acts on says the same: {kinds:?}"
        );
    }
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment of a tree a test laid out itself rather than copied as a fixture.
fn environment_at(root: &Path, temp: &Path, cache: &Path) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: temp.to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

#[test]
fn the_parts_of_a_catalog_can_be_named_by_a_glob_against_the_report_directory() {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        ran.status.code().is_some_and(|code| code <= 1),
        "{}",
        stderr(&ran)
    );
    let reports = rust_mutants_cli::app::stored::Store::read(fixture.root()).root();
    let stored_id = std::fs::read_dir(&reports)
        .expect("the report directory")
        .map(|entry| entry.expect("stored report directory entry"))
        .map(|entry| njutest_devkit::paths::owned_utf8(entry.file_name()))
        .find(|name| name.starts_with("2026"))
        .expect("the run that just happened");

    let one = against(&fixture, &["merge", "--runs", &stored_id]);
    assert!(
        one.status.code().is_some_and(|code| code <= 1),
        "a name that matches one stored run is that run: a person sharding by day names \
         the day: {}",
        stderr(&one)
    );
    let merged: serde_json::Value = njutest_devkit::strictjson::decode_str(&stdout(&one))
        .expect("the merge answers with a document");
    assert_eq!(
        merged["accounting"],
        stored(&fixture)["accounting"],
        "and the parts of one part are the whole of it"
    );

    let nothing = against(&fixture, &["merge", "--runs", "20240101*"]);
    assert_eq!(
        nothing.status.code(),
        Some(2),
        "a pattern that matches no stored run is not an empty merge: a report of nothing \
         reads as a catalog with nothing in it: {}",
        stdout(&nothing)
    );
    let said = stderr(&nothing);
    let default_reports = rust_mutants_cli::config::Config::default()
        .reports
        .directory;
    assert!(
        said.contains("20240101*") && said.contains(njutest_devkit::paths::utf8(&default_reports)),
        "and the refusal says which pattern found nothing and where it looked, because \
         the usual cause is a report directory somewhere else: {said}"
    );

    let malformed = against(&fixture, &["merge", "--runs", "/absolute"]);
    assert_eq!(
        malformed.status.code(),
        Some(2),
        "and a pattern that is not one is refused rather than matched literally: {}",
        stdout(&malformed)
    );
    assert!(
        stderr(&malformed).contains("--runs"),
        "naming the flag it was given to: {}",
        stderr(&malformed)
    );
}

#[test]
fn a_glob_that_names_the_same_part_twice_is_still_the_same_part_twice() {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        ran.status.code().is_some_and(|code| code <= 1),
        "{}",
        stderr(&ran)
    );
    let reports = rust_mutants_cli::app::stored::Store::read(fixture.root()).root();
    let document = serde_json::to_string(&stored(&fixture)).expect("renders");
    for name in ["20260101T000000000Z", "20260102T000000000Z"] {
        let directory = reports.join(name);
        std::fs::create_dir_all(&directory).expect("a second name for the same part");
        std::fs::write(directory.join("run-report-v1.json"), &document).expect("write");
    }

    let merged = against(&fixture, &["merge", "--runs", "2026010*"]);
    assert_eq!(
        merged.status.code(),
        Some(2),
        "a glob is a way of naming parts and not a way of excusing one named twice: a \
         merge that took this would count every mutant of the shard twice and report a \
         catalog twice its size: {}",
        stdout(&merged)
    );
    assert!(
        stderr(&merged).contains("more than one"),
        "{}",
        stderr(&merged)
    );
}

#[test]
fn an_edit_to_a_file_the_build_read_misses_the_outcome_store() {
    let fixture = Fixture::copy("fixture-carry");
    let asked = ["run", "--offline", "--locked", "--tier", "all"];
    let first = against(&fixture, &asked);
    assert!(
        first.status.code() == Some(0) || first.status.code() == Some(1),
        "{}",
        stderr(&first)
    );
    for (edited, text) in [
        ("src/answer.txt", "30\n"),
        ("waive", ""),
        (
            "build.rs",
            "fn main() {\n    let out = std::env::var_os(\"OUT_DIR\").expect(\"cargo sets OUT_DIR\");\n    std::fs::write(std::path::Path::new(&out).join(\"limit.rs\"), \"1\").expect(\"write the limit\");\n    println!(\"cargo::rerun-if-changed=build.rs\");\n}\n",
        ),
        (
            "build.rs",
            "fn main() {\n    let out = std::env::var_os(\"OUT_DIR\").expect(\"cargo sets OUT_DIR\");\n    std::fs::write(std::path::Path::new(&out).join(\"limit.rs\"), \"1\").expect(\"write the limit\");\n    println!(\"cargo::rustc-check-cfg=cfg(waived)\");\n    println!(\"cargo::rustc-cfg=waived\");\n    println!(\"cargo::rerun-if-changed=build.rs\");\n}\n",
        ),
    ] {
        std::fs::write(fixture.root().join(edited), text).expect("edit the input");
        let again = against(&fixture, &asked);
        assert!(
            again.status.code() == Some(0) || again.status.code() == Some(1),
            "{}",
            stderr(&again)
        );
        let report = stored(&fixture);
        let read_back: Vec<&serde_json::Value> = report["mutants"]
            .as_array()
            .expect("the rows")
            .iter()
            .filter(|row| !row["source_run_id"].is_null())
            .collect();
        assert!(
            read_back.is_empty(),
            "{edited} changed what the compiled code computes, so no answer from before the \
             edit may be read back as though the program were the same: {read_back:#?}"
        );
    }
}

/// The skeletons the newest run of `fixture` kept.
fn skeletons_of(fixture: &Fixture) -> serde_json::Value {
    let directory = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    document_at(&directory.join("skeletons-v1.json"))
}

/// The evidence of the item whose name ends in `name`.
fn item_named<'a>(skeletons: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    skeletons["items"]
        .as_array()
        .expect("the items")
        .iter()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|named| named == name || named.ends_with(&format!("::{name}")))
        })
        .expect("the item is cataloged")
}

/// Runs fixture-carry as it stands and reads the skeletons the run kept.
fn carried(fixture: &Fixture) -> serde_json::Value {
    let ran = against(fixture, &["run", "--offline", "--locked", "--tier", "all"]);
    assert!(
        ran.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&ran)
    );
    skeletons_of(fixture)
}

/// Rewrites `from` as `to` in fixture-carry's library.
fn edited(fixture: &Fixture, from: &str, to: &str) {
    let lib = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&lib).expect("the library");
    assert!(source.contains(from), "{from} is in the library");
    std::fs::write(&lib, source.replace(from, to)).expect("the edit");
}

#[test]
fn a_run_keeps_the_skeletons_an_answer_would_be_carried_by() {
    let fixture = Fixture::copy("fixture-carry");
    let kept = carried(&fixture);
    let errors = against_schema("rust-mutants-skeletons-v1.json", &kept);
    assert!(
        errors.is_empty(),
        "whoever carries an answer reads these, and the schema says how: {errors:#?}"
    );
    assert_eq!(
        item_named(&kept, "over")["sealed"],
        true,
        "a body that only compares is sealed: {kept:#}"
    );
    assert_eq!(
        (
            &item_named(&kept, "recorded")["unsealed"]["why"],
            &item_named(&kept, "recorded")["unsealed"]["name"]
        ),
        (
            &serde_json::json!("macro"),
            &serde_json::json!("include_str")
        ),
        "a body that reads a file is not: {kept:#}"
    );
    assert_eq!(
        item_named(&kept, "WAIVED")["unsealed"]["why"],
        "evaluated",
        "and a constant is evaluated where nothing enters it: {kept:#}"
    );
    let units: Vec<(String, String, bool)> = kept["units"]
        .as_array()
        .expect("the units")
        .iter()
        .map(|unit| {
            (
                unit["target"].as_str().expect("a target").to_owned(),
                unit["kind"].as_str().expect("a kind").to_owned(),
                unit["test"].as_bool().expect("a flag"),
            )
        })
        .collect();
    for unit in [
        ("build-script-build", "custom-build", false),
        ("fixture_carry", "lib", false),
        ("fixture_carry", "lib", true),
    ] {
        assert!(
            units.contains(&(unit.0.to_owned(), unit.1.to_owned(), unit.2)),
            "{unit:?} is a unit of the build: {units:?}"
        );
    }
}

#[test]
fn a_unit_names_every_entry_its_skeleton_folds() {
    let fixture = Fixture::copy("fixture-carry");
    let kept = carried(&fixture);
    let library = kept["units"]
        .as_array()
        .expect("the units")
        .iter()
        .find(|unit| unit["target"] == "fixture_carry" && unit["test"] == false)
        .expect("the library unit");
    let names: Vec<&str> = library["entries"]
        .as_object()
        .expect("the entries")
        .keys()
        .map(String::as_str)
        .collect();
    for wanted in ["$root/src/lib.rs", "$root/src/answer.txt", "$env/OUT_DIR"] {
        assert!(
            names.contains(&wanted),
            "{wanted} is something the library compiled: {names:?}"
        );
    }
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("$target/") && name.ends_with("/limit.rs"))
            && names
                .iter()
                .any(|name| name.starts_with("$emitted/$target/")),
        "and so are the file its build script generated and what that script emitted, named \
         from the target directory rather than from wherever this run put it: {names:?}"
    );
}

#[test]
fn an_edit_inside_a_sealed_body_moves_only_its_digest_and_one_outside_moves_the_skeleton() {
    let fixture = Fixture::copy("fixture-carry");
    let then = carried(&fixture);
    edited(
        &fixture,
        "recorded() > LIMIT || WAIVED",
        "WAIVED || recorded() > LIMIT",
    );
    let now = carried(&fixture);
    assert_ne!(
        item_named(&then, "over")["body_digest"],
        item_named(&now, "over")["body_digest"],
        "the edited body is another body"
    );
    assert_eq!(
        then["units"], now["units"],
        "and nothing outside a sealed body moved, so no unit's skeleton did"
    );
    edited(
        &fixture,
        "const WAIVED: bool = cfg!(waived);",
        "const WAIVED: bool = cfg!(waived) && true;",
    );
    let constant = carried(&fixture);
    assert_ne!(
        now["units"], constant["units"],
        "a constant is outside every sealed body, so an edit to it moves the skeleton"
    );
}
