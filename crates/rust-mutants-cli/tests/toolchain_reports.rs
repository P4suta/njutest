// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a stored run becomes for the readers a team already has: a CI test report, a code-scanning document, a page, and a paragraph in a pull request.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

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

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
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

fn stdout(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
}

/// As much of a page as a failing assertion is worth printing.
fn head(text: &str) -> &str {
    text.char_indices()
        .nth(400)
        .map_or(text, |(at, _)| text.get(..at).unwrap_or(text))
}

fn measured() -> Fixture {
    ran("fixture-simple")
}

/// A run whose report holds a gap no proof can close: `earlier` compares two values the compiler will not vouch for, so nothing removes the mutation and a test has to notice it or not.
fn with_a_survivor() -> Fixture {
    ran("fixture-coverage")
}

fn ran(name: &str) -> Fixture {
    let fixture = Fixture::copy(name);
    let ran = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--ui",
            "quiet",
            "--jobs",
            "1",
        ],
    );
    assert_eq!(ran.status.code(), Some(1), "{ran:?}");
    fixture
}

fn projected(fixture: &Fixture, format: &str) -> String {
    let output = against(fixture, &["report", "--format", format]);
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "reading a report back answers with the run's own exit code: {output:?}"
    );
    stdout(&output)
}

/// The text with everything that changes between two runs of the same tree taken out.
fn steady(text: &str, fixture: &Fixture) -> String {
    let mut out = text.replace(njutest_devkit::paths::utf8(fixture.root()), "<root>");
    out = out.replace(env!("CARGO_PKG_VERSION"), "<version>");
    out = blanked(&out, "time=\"", '"', "0.000");
    out = blanked(&out, "<run>", '<', "");
    let run = out
        .match_indices("20")
        .find_map(|(at, _)| {
            let candidate = out.get(at..at.checked_add(19)?)?;
            let shaped = candidate.len() == 19
                && matches!(candidate.get(8..9), Some("T" | "t"))
                && candidate.ends_with(['Z', 'z'])
                && candidate.chars().all(|one| {
                    one.is_ascii_digit() || one == 'T' || one == 't' || one == 'Z' || one == 'z'
                });
            shaped.then(|| candidate.to_owned())
        })
        .unwrap_or_default();
    if !run.is_empty() {
        out = out.replace(&run, "<run>");
    }
    out
}

/// Every value between `after` and `until` replaced with `with`.
fn blanked(text: &str, after: &str, until: char, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(after) {
        let (head, tail) = rest.split_at(at.saturating_add(after.len()));
        out.push_str(head);
        out.push_str(with);
        let Some(end) = tail.find(until) else {
            rest = "";
            break;
        };
        rest = tail.get(end..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name);
    njutest_devkit::golden::golden(&path, text.as_bytes()).expect("the recorded projection");
}

#[test]
fn a_junit_report_makes_a_survivor_a_failure_and_a_mutant_nothing_ran_a_skip() {
    let fixture = with_a_survivor();
    let text = projected(&fixture, "junit");
    assert!(text.starts_with("<?xml version=\"1.0\""), "{text}");
    assert!(text.contains("<testsuites"), "{text}");
    assert!(
        text.contains("<failure message=\"survived\""),
        "a mutant the tests did not notice is a failing test case: {text}"
    );
    golden("junit.golden", &steady(&text, &fixture));
}

#[test]
fn a_finding_no_mutant_row_carries_is_a_failing_case_of_its_own() {
    let fixture = measured();
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        concat!(
            "[[mutation.expect]]\n",
            "id = \"0000000000000000000000000000\"\n",
            "reason = \"a claim about a mutant that is not there\"\n",
            "outcome = \"survived\"\n"
        ),
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
            "--jobs",
            "1",
        ],
    );
    assert_eq!(ran.status.code(), Some(1), "{ran:?}");
    let text = projected(&fixture, "junit");
    assert!(
        text.contains("<testsuite name=\"findings\""),
        "a finding nobody can see in the view they read is one that does not exist: {text}"
    );
    assert!(text.contains("unmatched-expectation"), "{text}");
}

#[test]
fn a_sarif_report_names_every_finding_with_the_place_it_is() {
    let fixture = with_a_survivor();
    let text = projected(&fixture, "sarif");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("SARIF is JSON");
    assert_eq!(document["version"], "2.1.0");
    let run = &document["runs"][0];
    assert_eq!(run["tool"]["driver"]["name"], "rust-mutants");
    let results = run["results"].as_array().expect("the results");
    assert!(!results.is_empty(), "a run with a survivor reports it");
    let report: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&projected(&fixture, "json"))
            .expect("the stored report");
    assert_eq!(
        results.len(),
        report["findings"].as_array().expect("the findings").len(),
        "every finding is a result, including one no mutant row carries"
    );
    for result in results {
        assert!(
            result["level"].as_str().is_some_and(|it| !it.is_empty()),
            "{result}"
        );
        let Some(location) = result["locations"].as_array().and_then(|all| all.first()) else {
            continue;
        };
        let physical = &location["physicalLocation"];
        assert!(
            physical["artifactLocation"]["uri"]
                .as_str()
                .is_some_and(|uri| !uri.is_empty()),
            "{result}"
        );
        assert!(
            physical["region"]["startLine"].as_u64().is_some(),
            "{result}"
        );
        assert!(
            result["ruleId"].as_str().is_some_and(|it| !it.is_empty()),
            "a result that names a place names the operator that made it: {result}"
        );
    }
    golden("sarif.golden", &steady(&text, &fixture));
}

#[test]
fn a_markdown_report_is_the_summary_a_person_puts_in_a_pull_request() {
    let fixture = with_a_survivor();
    let text = projected(&fixture, "markdown");
    assert!(text.contains("# Mutation report"), "{text}");
    assert!(text.contains("| where | rule | change |"), "{text}");
    golden("markdown.golden", &steady(&text, &fixture));
}

#[test]
fn a_stryker_projection_carries_the_tests_that_killed_and_the_configured_thresholds() {
    let fixture = measured();
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        "[reports.stryker]\nhigh = 90\nlow = 70\n",
    )
    .expect("a configuration");
    let text = projected(&fixture, "stryker");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the projection is JSON");
    assert_eq!(document["thresholds"]["high"], 90);
    assert_eq!(document["thresholds"]["low"], 70);
    let killed = document["files"]["src/lib.rs"]["mutants"]
        .as_array()
        .expect("the mutants")
        .iter()
        .find(|mutant| mutant["status"] == "Killed")
        .expect("a killed mutant");
    let by = killed["killedBy"].as_array().expect("what killed it");
    assert!(
        by.iter()
            .any(|one| one.as_str().is_some_and(|name| name.contains("::"))),
        "a test that noticed it, not the binary that held the test: {killed}"
    );
}

#[test]
fn a_source_the_report_names_and_the_root_does_not_hold_is_rm0012() {
    let fixture = measured();
    std::fs::remove_file(fixture.root().join("src/lib.rs")).expect("the source goes away");
    let output = against(&fixture, &["report", "--format", "stryker"]);
    let said = njutest_devkit::process::strict_utf8(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(said.contains("RM0012"), "{said}");
    assert!(said.contains("src/lib.rs"), "{said}");
}

#[test]
fn every_survivor_is_shown_inline_on_the_line_it_is_on() {
    let fixture = with_a_survivor();
    let text = projected(&fixture, "html");
    let report: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&projected(&fixture, "json"))
            .expect("the stored report is JSON");
    let survivors: Vec<&serde_json::Value> = report["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter(|row| row["outcome"] == "survived")
        .collect();
    assert!(!survivors.is_empty(), "the fixture has survivors");
    for survivor in survivors {
        let display = survivor["display_id"].as_str().expect("a short identity");
        assert!(
            text.contains(display),
            "the page does not show {display}: {}",
            head(&text)
        );
    }
    assert!(
        text.contains("<pre class=\"source\""),
        "the page shows the source it measured: {}",
        head(&text)
    );
}

#[test]
fn a_page_shows_a_file_that_changed_since_the_run_as_changed_rather_than_as_source() {
    let fixture = measured();
    std::fs::write(
        fixture.root().join("src/lib.rs"),
        "// SPDX-FileCopyrightText: 2026 njutest contributors\n\
         // SPDX-License-Identifier: MIT OR Apache-2.0\n\npub fn nothing() {}\n",
    )
    .expect("the source changes");
    let text = projected(&fixture, "html");
    assert!(
        text.contains("changed since the run"),
        "a page that showed the new file would be a lie about what was measured: {}",
        head(&text)
    );
    assert!(!text.contains("pub fn nothing"), "{text}");
}

#[test]
fn a_file_the_tests_noticed_every_mutation_in_is_counted_rather_than_printed() {
    let fixture = Fixture::copy("fixture-baseline");
    let ran = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--ui",
            "quiet",
            "--jobs",
            "1",
        ],
    );
    assert!(ran.status.code().is_some_and(|code| code <= 1), "{ran:?}");
    let report: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&projected(&fixture, "json"))
            .expect("the stored report");
    let clean = report["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .all(|row| row["outcome"] == "killed");
    if !clean {
        return;
    }
    let text = projected(&fixture, "html");
    assert!(
        text.contains("The tests noticed every mutation."),
        "a page that shows a thousand lines nobody has to read is one nobody opens: {}",
        head(&text)
    );
    assert!(!text.contains("<pre class=\"source\""), "{}", head(&text));
}
