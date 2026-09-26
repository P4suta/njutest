// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a stored run becomes for other readers: a Stryker report, one page, and one doctor document.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};
use std::process::Output;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root.as_str()])
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

fn measured() -> Fixture {
    let fixture = Fixture::copy("fixture-unicode");
    let ran = against(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        ran.status.code().is_some_and(|code| code <= 1),
        "the run establishes something: {ran:?}"
    );
    fixture
}

#[test]
fn a_stryker_projection_validates_against_the_schema_it_answers_to() {
    let fixture = measured();
    let projected = against(&fixture, &["report", "--format", "stryker"]);
    assert!(
        projected.status.code().is_some_and(|code| code <= 1),
        "reading a report back answers with the run's own exit code: {projected:?}"
    );
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&stdout(&projected))
        .expect("the projection is JSON");

    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root()
                .join("test/vendor/mutation-testing-report-schema.json"),
        )
        .expect("the vendored schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let complaints: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();
    assert!(complaints.is_empty(), "{complaints:?}\n{document}");
}

#[test]
fn a_stryker_projection_counts_columns_in_utf16_as_the_schema_requires() {
    let fixture = measured();
    let projected = against(&fixture, &["report", "--format", "stryker"]);
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&stdout(&projected))
        .expect("the projection is JSON");
    let files = document["files"].as_object().expect("the files");
    let (path, file) = files.iter().next().expect("one mutated file");
    let source = file["source"].as_str().expect("the source");
    assert!(
        source.contains("pub fn"),
        "the whole file is in the document so a reader can show the mutation in place: {path}"
    );
    for mutant in file["mutants"].as_array().expect("the mutants") {
        let line = mutant["location"]["start"]["line"]
            .as_u64()
            .expect("a line");
        let column = mutant["location"]["start"]["column"]
            .as_u64()
            .expect("a column");
        let text = source
            .lines()
            .nth(usize::try_from(line - 1).expect("a line index"));
        let units = text.map_or(0, |text| text.chars().map(char::len_utf16).sum::<usize>());
        assert!(
            usize::try_from(column).expect("a column index") <= units + 1,
            "a column past the end of its line: {mutant}"
        );
        assert!(column >= 1, "{mutant}");
    }
}

#[test]
fn a_page_needs_nothing_from_the_network_to_be_read() {
    let fixture = measured();
    let page = against(&fixture, &["report", "--format", "html"]);
    let text = stdout(&page);
    assert!(text.starts_with("<!doctype html>"), "{text}");
    assert!(text.contains("</html>"), "{text}");
    let outside = njutest_devkit::report::reaches_outside(&text);
    assert!(
        outside.is_empty(),
        "a page that fetches anything is not one that opens offline: {outside:?}"
    );
    assert_eq!(
        text.matches("<script").count(),
        1,
        "the page carries one script of its own, and it is the only one"
    );
    assert!(
        text.contains("<script>"),
        "a script with an attribute is a script that could name a source: {text}"
    );
}

#[test]
fn a_page_written_to_a_file_says_where_it_put_it() {
    let fixture = measured();
    let path = fixture.root().join("report.html");
    let written = against(
        &fixture,
        &[
            "report",
            "--format",
            "html",
            "--output",
            njutest_devkit::paths::utf8(&path),
        ],
    );
    assert!(stdout(&written).contains("report.html"), "{written:?}");
    let text = std::fs::read_to_string(&path).expect("the page");
    assert!(text.starts_with("<!doctype html>"), "{text}");
    assert!(
        !stdout(&written).contains("<!doctype html>"),
        "and the page went to the file rather than to both: a person who asked for a \
         file and got the page on the terminal as well has to scroll past what they \
         asked to be spared: {}",
        stdout(&written)
    );
}

#[test]
fn a_page_that_could_not_be_written_is_a_refusal_naming_the_path() {
    let fixture = measured();
    let occupied = fixture.root().join("a-directory-where-the-page-goes");
    std::fs::create_dir_all(&occupied).expect("a directory where the file would go");

    let refused = against(
        &fixture,
        &[
            "report",
            "--format",
            "html",
            "--output",
            njutest_devkit::paths::utf8(&occupied),
        ],
    );
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a page nobody could write is not a page written: a pipeline that read the exit \
         code would upload the file that is not there: {}",
        stdout(&refused)
    );
    assert!(
        njutest_devkit::process::strict_utf8(&refused.stderr)
            .contains("a-directory-where-the-page-goes"),
        "and the refusal names the path, because the usual cause is a directory that is \
         not there or one that is: {}",
        njutest_devkit::process::strict_utf8(&refused.stderr)
    );
}

#[test]
fn the_doctor_answers_as_a_document_when_it_is_asked_to() {
    let fixture = Fixture::copy("fixture-simple");
    let asked = against(&fixture, &["doctor", "--json"]);
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&asked)).expect("the answer is JSON");
    assert_eq!(document["document_type"], "rust-mutants/doctor");
    assert_eq!(document["schema_version"], 1);
    assert!(document["ok"].is_boolean(), "{document}");
    let checks = document["checks"].as_array().expect("the checks");
    let names: Vec<&str> = checks
        .iter()
        .filter_map(|check| check["name"].as_str())
        .collect();
    assert!(names.contains(&"cargo"), "{names:?}");
    assert!(names.contains(&"workspace"), "{names:?}");
    for check in checks {
        assert!(check["ok"].is_boolean(), "{check}");
        assert!(
            check["detail"].as_str().is_some_and(|it| !it.is_empty()),
            "{check}"
        );
    }
}

#[test]
fn the_doctor_says_the_same_thing_in_lines_and_in_a_document() {
    let fixture = Fixture::copy("fixture-simple");
    let lines = against(&fixture, &["doctor"]);
    let document = against(&fixture, &["doctor", "--json"]);
    assert_eq!(lines.status.code(), document.status.code());
    let value: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&document)).expect("the answer is JSON");
    for check in value["checks"].as_array().expect("the checks") {
        let name = check["name"].as_str().expect("a name");
        assert!(stdout(&lines).contains(name), "{name}");
    }
}

#[test]
fn the_doctor_document_validates_against_the_schema_it_answers_to() {
    let fixture = Fixture::copy("fixture-simple");
    let asked = against(&fixture, &["doctor", "--json"]);
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&asked)).expect("the answer is JSON");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-doctor-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is a document");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}\n{document}");
}

#[test]
fn the_doctor_answers_about_every_thing_a_run_needs() {
    let fixture = Fixture::copy("fixture-simple");
    let asked = against(&fixture, &["doctor", "--json"]);
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&asked)).expect("the answer is JSON");
    let names: Vec<&str> = document["checks"]
        .as_array()
        .expect("the checks")
        .iter()
        .filter_map(|check| check["name"].as_str())
        .collect();
    for needed in [
        "cargo",
        "rustc",
        "host",
        "workspace",
        "config",
        "temp",
        "git",
        "targets",
        "environment",
        "cache",
        "disk",
        "snapshots",
        "llvm-tools",
    ] {
        assert!(
            names.contains(&needed),
            "{needed} is not asked about: {names:?}"
        );
    }
}

#[test]
fn a_package_with_nothing_to_run_is_a_warning_that_names_it() {
    let fixture = Fixture::copy("fixture-macros");
    let asked = against(&fixture, &["doctor"]);
    let text = stdout(&asked);
    assert!(text.contains("WARN targets"), "{text}");
    assert!(text.contains("fixture-macros-derive"), "{text}");
    assert_eq!(
        asked.status.code(),
        Some(0),
        "a warning is not a reason not to run: {text}"
    );
}

#[test]
fn a_root_that_is_a_member_of_a_workspace_fails_and_names_the_root_to_use() {
    let fixture = Fixture::copy("fixture-workspace");
    let member = fixture.root().join("crates/core");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["doctor", "--root", njutest_devkit::paths::utf8(&member)])
            .map(OsString::from),
        &environment(&fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let asked = njutest_devkit::process::answered(code, out, err);
    let text = njutest_devkit::process::strict_utf8(&asked.stdout).into_owned();
    assert!(text.contains("FAIL workspace"), "{text}");
    assert!(
        text.contains(&fixture.root().display().to_string()),
        "the root to use is named: {text}"
    );
    assert_eq!(asked.status.code(), Some(2), "{text}");
}

#[test]
fn a_snapshot_an_earlier_run_left_behind_is_a_warning_that_says_what_removes_it() {
    let fixture = Fixture::copy("fixture-simple");
    std::fs::create_dir_all(fixture.temp().join("rust-mutants-snap-abandoned"))
        .expect("a leftover snapshot");
    let asked = against(&fixture, &["doctor"]);
    let text = stdout(&asked);
    assert!(text.contains("WARN snapshots"), "{text}");
    assert!(text.contains("cache --gc"), "{text}");
}

#[test]
fn a_project_that_moved_its_reports_is_still_told_what_a_run_kept() {
    let fixture = Fixture::copy("fixture-simple");
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        "[reports]\ndirectory = \"target/mutation\"\n",
    )
    .expect("a configuration");
    let ran = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--ui",
            "quiet",
            "--keep-temp",
        ],
    );
    assert!(ran.status.code().is_some_and(|code| code <= 1), "{ran:?}");
    let asked = against(&fixture, &["doctor"]);
    let text = stdout(&asked);
    assert!(
        text.contains("kept on purpose"),
        "the ledger lives under the report directory the configuration names: {text}"
    );
    assert!(
        !text.contains("0 kept on purpose"),
        "a project that moved its reports would otherwise be told nothing was kept: {text}"
    );
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
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
