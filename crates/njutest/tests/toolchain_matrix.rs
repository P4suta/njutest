// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every dimension of a run is one column, and `whole-v1` is not assured while any of them is a hole (ADR 0033).

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Output;

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;
use sha2::{Digest as _, Sha256};

struct Fixture {
    root: PathBuf,
    dir: tempfile::TempDir,
}

fn fixture(name: &str, config: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-matrix-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    std::fs::write(root.join(".njutest.toml"), config).expect("the configuration");
    Fixture { root, dir }
}

fn verify(fixture: &Fixture) -> Output {
    verify_with(fixture, &[])
}

fn verify_with(fixture: &Fixture, extra: &[&str]) -> Output {
    let events = fixture.dir.path().join("clock-events");
    std::fs::create_dir_all(&events).expect("clock events");
    let mut vars: rust_mutants::vars::Variables =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect();
    vars.set("NJUTEST_TEST_CLOCK", events.as_os_str());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--format",
            "lines",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(OsString::from),
        &Environment {
            cache_directory: fixture.root.join(".cache"),
            working_directory: fixture.root.clone(),
            temp_directory: njutest_devkit::paths::temp_beside(&fixture.root)
                .expect("a temporary directory"),
            program: PathBuf::from("this test never runs it"),
            vars,
            cancel: Cancel::new().with_clock(rust_mutants::runner::Clock::events(events)),
            terminal: njutest::presentation::Terminal::default(),
        },
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn dimensions(output: &Output) -> Vec<String> {
    njutest_devkit::process::strict_utf8(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("DIMENSION\t"))
        .map(|line| {
            line.split('\t')
                .skip(1)
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn a_whole_run_that_names_no_contract_asks_every_dimension_and_is_not_assured_while_one_is_a_hole()
{
    let fixture = fixture("fixture-faulted", "version = 1\n");
    let output = verify(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        said.contains("contract=whole-v1"),
        "a run that names no contract gets the strictest answer, which is the default (ADR 0033): {said}"
    );
    assert_eq!(
        dimensions(&output),
        vec![
            "mutation measured",
            "repeatable measured",
            "fault measured",
            "schedule measured",
            "wire nothing-to-ask",
            "durable nothing-to-ask",
        ],
        "one whole run asks every dimension it can, and no row of it is not-asked: {said}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        said.contains("VERDICT\tINSUFFICIENT"),
        "one binary is not proven to run one thread and no schedule of it was established, which is a hole: {said}"
    );
    assert!(
        said.contains("FINDING\tdimension-not-measured\tschedule\t")
            && !said.contains("FINDING\tdimension-not-measured\tdurable\t"),
        "and a finding names it, while durability, which found no call that writes, is not: {said}"
    );
}

#[test]
fn a_standard_run_shows_the_matrix_and_is_decided_as_it_was() {
    let fixture = fixture(
        "fixture-faulted",
        "version = 1\ncontract = \"standard-v1\"\n",
    );
    let output = verify(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert_eq!(
        dimensions(&output),
        vec![
            "mutation measured",
            "repeatable not-asked",
            "fault not-asked",
            "schedule measured",
            "wire nothing-to-ask",
            "durable not-asked",
        ],
        "{said}"
    );
    assert!(
        !said.contains("dimension-not-measured"),
        "a contract that does not ask every dimension raises nothing about one it did not ask: {said}"
    );
}

#[test]
fn a_whole_run_that_seals_nothing_is_held_by_a_retained_recording_the_audit_re_decides() {
    let root = committed();
    if std::env::var_os(UPDATE).is_some() {
        record(&root);
        return;
    }
    let text = std::fs::read_to_string(root.join("binding.json")).expect("the retained recording");
    let binding = njutest_devkit::strictjson::decode_str::<Binding>(&text)
        .expect("the retained recording's binding document");
    assert_eq!(
        (
            binding.schema.as_str(),
            binding.fixture.as_str(),
            binding.config.as_str(),
            binding.claim.as_str()
        ),
        (
            SCHEMA,
            "fixture-simple",
            WHOLE,
            "a whole run that seals nothing leaves every answer a native lead, so the mutation \
             column is a hole the runner names and the audit re-decides with nothing to say \
             against it",
        ),
        "the recording is bound to the fixture, the contract and the claim it holds"
    );
    assert_eq!(
        binding.arguments,
        ["--no-seal", "--no-cache"],
        "the recording is bound to the options its run was asked with"
    );
    let source = njutest_devkit::paths::fixtures_dir().join("fixture-simple");
    assert_eq!(
        source_digest(&source),
        binding.source_digest,
        "the recording is bound to the source it was run of: re-record it with {UPDATE}=1"
    );
    for (retained, original) in [
        ("Cargo.toml", source.join("Cargo.toml")),
        ("Cargo.lock", source.join("Cargo.lock")),
    ] {
        assert_eq!(
            std::fs::read(root.join("fixture").join(retained)).expect("the retained manifest"),
            std::fs::read(&original).expect("today's manifest"),
            "the retained {retained} is the manifest the run verified"
        );
    }
    assert_eq!(
        std::fs::read(root.join("fixture").join(".njutest.toml")).expect("the retained contract"),
        WHOLE.as_bytes(),
        "the retained contract is the one the run verified"
    );
    let run = root.join("run").join(&binding.run_id);
    let report = njutest_devkit::strictjson::decode_str::<serde_json::Value>(
        &std::fs::read_to_string(run.join(njutest::app::reports::DOCUMENT_NAME))
            .expect("the retained report"),
    )
    .expect("the retained report is JSON");
    assert_eq!(
        report["report"]["contract"].as_str(),
        Some("whole-v1"),
        "the retained report is of a whole contract: {report}"
    );
    let holes = dimension_holes(&report);
    assert!(
        holes.iter().any(|(subject, detail)| subject == "mutation"
            && detail.contains("no sealed execution established")),
        "with nothing sealed, every answer is a native lead and the mutation column is a hole, \
         which the report names: {holes:?}"
    );
    let audit = audited(&run, &root.join("trace"));
    let said = njutest_devkit::process::strict_utf8(&audit.stdout);
    assert!(
        audit.status.success()
            && said.contains("layer: dimensions: re-decided")
            && said.contains("; 0 violations, 0 unaudited"),
        "the runner and the audit each count a lead as a hole in the mutation column, and every \
         other column the same way, so a whole run that seals nothing is re-decided with nothing \
         to say against it: {said}\n{}",
        njutest_devkit::process::strict_utf8(&audit.stderr)
    );
    let tampered = tempfile::Builder::new()
        .prefix("njutest-matrix-tampered-")
        .tempdir()
        .expect("a temporary directory");
    let tampered_run = tampered.path().join("run");
    let tampered_trace = tampered.path().join("trace");
    copy_tree(&run, &tampered_run);
    copy_tree(&root.join("trace"), &tampered_trace);
    let document = tampered
        .path()
        .join("run")
        .join(njutest::app::reports::DOCUMENT_NAME);
    let mut corrupted = report.clone();
    corrupted["report"]["builds"][0]["parts"][0]["findings"]
        .as_array_mut()
        .expect("the retained report's findings")
        .retain(|finding| {
            finding["kind"]
                .as_str()
                .is_some_and(|kind| kind != "dimension-not-measured")
                || finding["subject"]
                    .as_str()
                    .is_some_and(|subject| subject != "mutation")
        });
    std::fs::write(
        &document,
        serde_json::to_string(&corrupted).expect("the tampered report"),
    )
    .expect("the tampered report");
    let refused = audited(&tampered_run, &tampered_trace);
    let answer = njutest_devkit::process::strict_utf8(&refused.stdout);
    assert!(
        !(refused.status.success() && answer.contains("; 0 violations, 0 unaudited")),
        "a recording whose report drops the hole it held is refused rather than re-decided: {answer}"
    );
}

#[test]
fn a_fabricated_recording_is_never_audit_clean() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-matrix-fabricated-")
        .tempdir()
        .expect("a temporary directory");
    let run = dir.path().join("runs").join("fabricated");
    std::fs::create_dir_all(&run).expect("the fabricated run directory");
    std::fs::write(
        run.join(njutest::app::reports::DOCUMENT_NAME),
        serde_json::to_string(&serde_json::json!({
            "document_type": "njutest-assurance-report",
            "report": {
                "schema": "njutest-assurance-report-v1",
                "schema_version": 3,
                "run_id": "fabricated",
                "run_kind": "full",
                "contract": "whole-v1",
                "builds": [{
                    "name": "default",
                    "parts": [{
                        "part": {"kind": "whole"},
                        "findings": [{
                            "kind": "dimension-not-measured",
                            "subject": "mutation",
                            "detail": "fabricated: a native lead of killed no sealed execution established",
                            "origin": {"scope": "source", "build": "default", "run_id": "fabricated-b0000000000", "part": {"kind": "whole"}},
                            "path": null,
                            "position": null
                        }]
                    }]
                }]
            }
        }))
        .expect("the fabricated report"),
    )
    .expect("the fabricated report");
    let trace = dir.path().join("trace");
    std::fs::create_dir_all(&trace).expect("the fabricated trace directory");
    std::fs::write(trace.join("trace.jsonl"), "").expect("the fabricated recording");
    let audit = audited(&run, &trace);
    let said = njutest_devkit::process::strict_utf8(&audit.stdout);
    assert!(
        !(audit.status.success() && said.contains("; 0 violations, 0 unaudited")),
        "the audit never certifies a hand-written specimen in place of a real recording: {said}\n{}",
        njutest_devkit::process::strict_utf8(&audit.stderr)
    );
}

/// The schema of the one recording this test keeps.
const SCHEMA: &str = "njutest-matrix-recording-v1";

/// The variable that records the committed recording again from a real whole run of today's runner.
const UPDATE: &str = "UPDATE_MATRIX_RECORDING";

/// The configuration the recorded run verified, whose bytes the retained contract is held to.
const WHOLE: &str = "version = 1\ncontract = \"whole-v1\"\n";

/// Where the committed recording is kept, beside the repository's other committed recordings.
fn committed() -> PathBuf {
    njutest_devkit::paths::workspace_root()
        .join("xtask/tests/testdata")
        .join("matrix-run-unsealed")
}

/// Every binding field a retained recording states, with none of them defaulted.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: String,
    fixture: String,
    config: String,
    arguments: Vec<String>,
    source_digest: String,
    run_id: String,
    claim: String,
}

/// The audit of one run directory with its recording, as the retained one is asked for.
fn audited(run: &std::path::Path, trace: &std::path::Path) -> Output {
    njutest_devkit::paths::command(&njutest_devkit::paths::cargo_binary())
        .args(["xtask", "proofaudit"])
        .arg(run)
        .arg("--trace")
        .arg(trace)
        .current_dir(njutest_devkit::paths::workspace_root())
        .output()
        .expect("the audit starts")
}

/// Every dimension hole the report names, by its subject and detail.
fn dimension_holes(report: &serde_json::Value) -> Vec<(String, String)> {
    report["report"]["builds"]
        .as_array()
        .expect("the report's builds")
        .iter()
        .flat_map(|build| build["parts"].as_array().expect("the build's parts"))
        .flat_map(|part| part["findings"].as_array().expect("the part's findings"))
        .filter(|finding| finding["kind"].as_str() == Some("dimension-not-measured"))
        .map(|finding| {
            (
                finding["subject"]
                    .as_str()
                    .expect("a dimension finding's subject")
                    .to_owned(),
                finding["detail"]
                    .as_str()
                    .expect("a dimension finding's detail")
                    .to_owned(),
            )
        })
        .collect()
}

/// Every file under `root`, by its `/`-separated path relative to it.
fn walked(root: &std::path::Path, prefix: &str, into: &mut Vec<String>) {
    for entry in std::fs::read_dir(root).expect("a directory of the source tree") {
        let entry = entry.expect("an entry of the source tree");
        let name = entry
            .file_name()
            .into_string()
            .expect("a file of the source tree is named in UTF-8");
        let at = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().expect("the entry's kind").is_dir() {
            walked(&entry.path(), &at, into);
        } else {
            into.push(at);
        }
    }
}

/// The content digest of a source tree: every file's path and bytes, sorted and length-prefixed.
fn source_digest(root: &std::path::Path) -> String {
    let mut files = Vec::new();
    walked(root, "", &mut files);
    files.sort();
    let mut digest = Sha256::new();
    for file in files {
        let bytes = std::fs::read(root.join(&file)).expect("a file of the source tree");
        let size = u64::try_from(bytes.len()).expect("a readable file size");
        digest.update(file.as_bytes());
        digest.update(size.to_be_bytes());
        digest.update(&bytes);
    }
    hex::encode(digest.finalize())
}

/// Records the committed recording from one real whole run of `fixture-simple` that seals nothing.
fn record(into: &PathBuf) {
    let fixture = fixture("fixture-simple", WHOLE);
    let trace = fixture.root.join("recorded");
    let traced = format!("--trace={}", trace.display());
    let output = verify_with(&fixture, &["--no-seal", "--no-cache", &traced]);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        said.contains("FINDING\tdimension-not-measured\tmutation\t"),
        "a recording is kept from a run whose mutation column is a hole of native leads: {said}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let runs: Vec<PathBuf> = std::fs::read_dir(
        fixture
            .root
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs"),
    )
    .expect("the run wrote its report")
    .map(|entry| entry.expect("a stored run").path())
    .filter(|path| path.is_dir())
    .collect();
    let [run] = runs.as_slice() else {
        panic!("one run, one report: {runs:?}");
    };
    let report = njutest_devkit::strictjson::decode_str::<serde_json::Value>(
        &std::fs::read_to_string(run.join(njutest::app::reports::DOCUMENT_NAME))
            .expect("the run's report"),
    )
    .expect("the run's report is JSON");
    let run_id = report["report"]["run_id"]
        .as_str()
        .expect("the run's identity")
        .to_owned();
    match std::fs::remove_dir_all(into) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("the committed recording could not be replaced: {error}"),
    }
    std::fs::create_dir_all(into.join("fixture")).expect("the retained fixture directory");
    for retained in ["Cargo.toml", "Cargo.lock", ".njutest.toml"] {
        std::fs::copy(
            fixture.root.join(retained),
            into.join("fixture").join(retained),
        )
        .expect("the retained fixture file");
    }
    let recorded_run = into.join("run").join(&run_id);
    let recorded_trace = into.join("trace");
    copy_tree(run, &recorded_run);
    copy_tree(&trace, &recorded_trace);
    std::fs::write(
        into.join("binding.json"),
        format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "schema": SCHEMA,
                "fixture": "fixture-simple",
                "config": WHOLE,
                "arguments": ["--no-seal", "--no-cache"],
                "source_digest": source_digest(
                    &njutest_devkit::paths::fixtures_dir().join("fixture-simple")
                ),
                "run_id": run_id,
                "claim": "a whole run that seals nothing leaves every answer a native lead, so the \
                          mutation column is a hole the runner names and the audit re-decides with \
                          nothing to say against it",
            }))
            .expect("the binding document")
        ),
    )
    .expect("the binding document");
}
