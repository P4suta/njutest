// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run asked for faults fails the calls a `?` asks about, and says which failures the suite noticed.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a published report as a table"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-faults-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    Fixture { root, _dir: dir }
}

/// A run of `njutest verify` whose mutations are put natively, since what these tests are about is how a fault is put, which only a native run does, and what it says of a mutation is a lead.
fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked", "--no-seal"];
    args.extend_from_slice(extra);
    asked(fixture, &args)
}

fn asked(fixture: &Fixture, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(&fixture.root),
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: root.join(".cache"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect::<rust_mutants::vars::Variables>(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn part(fixture: &Fixture) -> serde_json::Value {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    let text = std::fs::read_to_string(path).expect("the document");
    let whole: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    whole["report"]["builds"][0]["parts"][0].clone()
}

/// Re-decides the latest faulted run from its own report, source tree and recording.
fn audited(fixture: &Fixture) {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let output = njutest_devkit::paths::command(&njutest_devkit::paths::cargo_binary())
        .args(["xtask", "proofaudit"])
        .arg(
            fixture
                .root
                .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
                .join("runs")
                .join(run.as_str()),
        )
        .arg("--trace")
        .arg(fixture.root.join(".njutest/trace").join(run.as_str()))
        .arg("--root")
        .arg(&fixture.root)
        .current_dir(njutest_devkit::paths::workspace_root())
        .output()
        .expect("the audit starts");
    let audit = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        output.status.success() && audit.contains("; 0 violations"),
        "the proof audit re-decides the faulted run: {audit}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        !audit.contains("violation:") && !audit.contains("unaudited:"),
        "every proof layer is held, including the faults and the evidence beside them: {audit}"
    );
}

/// The runner's recording of the run the index points at, read back.
fn recording(fixture: &Fixture) -> Vec<njutest::trace::Event> {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let stream = fixture
        .root
        .join(".njutest/trace")
        .join(run.as_str())
        .join(njutest::trace::FILE_NAME);
    njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&stream).expect("the recording"),
    ))
    .expect("the recording reads back")
}

/// The fault records these tests read, with every other trace variant left explicit.
enum FaultEvent<'a> {
    Baseline(&'a njutest::trace::FaultBaselineRecord),
    Route(&'a njutest::trace::FaultRouteRecord),
    Fate(&'a njutest::trace::FaultFateRecord),
    Writes(&'a njutest::trace::FaultWritesRecord),
    Other,
}

const fn fault_event(payload: &njutest::trace::Payload) -> FaultEvent<'_> {
    use njutest::trace::Payload;
    match payload {
        Payload::FaultBaseline { baseline } => FaultEvent::Baseline(baseline),
        Payload::FaultRoute { route } => FaultEvent::Route(route),
        Payload::FaultFate { fate } => FaultEvent::Fate(fate),
        Payload::FaultWrites { writes } => FaultEvent::Writes(writes),
        Payload::RunStart { .. }
        | Payload::PhaseStart { .. }
        | Payload::PhaseEnd { .. }
        | Payload::Exec { .. }
        | Payload::Progress { .. }
        | Payload::Artifact { .. }
        | Payload::Route { .. }
        | Payload::MutantExec { .. }
        | Payload::SealedExec { .. }
        | Payload::FaultExec { .. }
        | Payload::FaultRejected { .. }
        | Payload::FaultAttribution { .. }
        | Payload::FaultControl { .. }
        | Payload::Fault { .. }
        | Payload::Beside { .. }
        | Payload::BesideRun { .. }
        | Payload::CrashExec { .. }
        | Payload::CrashStep { .. }
        | Payload::Crash { .. }
        | Payload::ProbeExec { .. }
        | Payload::WireExchange { .. }
        | Payload::WireExec { .. }
        | Payload::Sentinel { .. }
        | Payload::Model { .. }
        | Payload::Drift { .. }
        | Payload::Control { .. }
        | Payload::Confirm { .. }
        | Payload::Resumed { .. }
        | Payload::Repair { .. }
        | Payload::Knob { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => FaultEvent::Other,
    }
}

fn decisions(part: &serde_json::Value) -> Vec<(u64, String)> {
    let mut decided: Vec<(u64, String)> = part["faults"]
        .as_array()
        .expect("a list of faults")
        .iter()
        .map(|fault| {
            (
                fault["position"]["line"]
                    .as_u64()
                    .expect("every site, put or not, says where it is"),
                fault["decision"]["decision"]
                    .as_str()
                    .expect("a decision")
                    .to_owned(),
            )
        })
        .collect();
    decided.sort();
    decided
}

fn named<'a>(
    part: &'a serde_json::Value,
    list: &str,
    field: &str,
    name: &str,
) -> Vec<&'a serde_json::Value> {
    part[list]
        .as_array()
        .expect("a list")
        .iter()
        .filter(|one| one[field] == name)
        .collect()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the evidence is checked where it is drawn, one drawing at a time"
)]
fn a_surviving_ignore_question_statement_is_told_apart_by_the_call_failing_beside_it() {
    let fixture = fixture("fixture-faulted-ignore");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let survivors: Vec<&serde_json::Value> = part["mutants"]
        .as_array()
        .expect("the mutants")
        .iter()
        .filter(|mutant| {
            mutant["rule"] == "ignore-question-statement"
                && mutant["decision"]["outcome"] == "survived"
        })
        .collect();
    assert_eq!(
        survivors.len(),
        1,
        "deleting the `?` of `leave` leaves the failure unread and the answer `Ok`, so nothing \
         notices: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let survivor = survivors[0]["display_id"].as_str().expect("a survivor");
    let evidence: Vec<&serde_json::Value> = part["beside"]
        .as_array()
        .expect("the evidence beside a fault")
        .iter()
        .filter(|beside| beside["mutant"] == survivor)
        .collect();
    assert_eq!(
        evidence.len(),
        1,
        "the survivor gains its evidence from the call at its own site failing, which is what \
         makes it no equivalence (ADR 0032 decision 6): {part}"
    );
    assert_eq!(
        (
            evidence[0]["failed"].as_str(),
            part["faults"]
                .as_array()
                .expect("the fault records")
                .iter()
                .find(|fault| fault["display_id"] == evidence[0]["fault"])
                .and_then(|fault| fault["decision"]["decision"].as_str())
        ),
        (Some("alone"), Some("noticed")),
        "the fault alone fails the write and the `?` answers it, and beside the deletion nobody \
         reads the failure: {part}"
    );
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let said = |name: &str| {
        std::fs::read_to_string(
            fixture
                .root
                .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
                .join("runs")
                .join(run.as_str())
                .join(name),
        )
        .expect("the run publishes every drawing")
    };
    let fault = evidence[0]["fault"].as_str().expect("the fault");
    let lines = said(njutest::report::lines::FILE_NAME);
    assert!(
        lines.contains("observable_under_fault=") && lines.contains(fault),
        "the lines drawing states the evidence under the survivor it belongs to: {lines}"
    );
    let html = said(njutest::app::reports::HTML_NAME);
    assert!(
        html.contains("Evidence under a fault") && html.contains(fault),
        "the HTML drawing states it too: {html}"
    );
    let sarif = said(njutest::app::reports::SARIF_NAME);
    assert!(
        sarif.contains("observable-under-fault") && sarif.contains(survivor),
        "the SARIF drawing carries it as a note a code-scanning reader sees: {sarif}"
    );
    let junit = said(njutest::app::reports::JUNIT_NAME);
    assert!(
        junit.contains("evidence-under-fault") && junit.contains(fault),
        "the JUnit drawing carries it as a passing testcase beside the findings: {junit}"
    );
    audited(&fixture);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the fixture's fault decisions, identity fields and multi-target evidence are held together"
)]
fn a_run_asked_for_faults_says_which_failed_calls_the_suite_noticed() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![
            (13, "noticed".to_owned()),
            (22, "noticed".to_owned()),
            (33, "absorbed".to_owned()),
            (46, "not-put".to_owned()),
            (57, "not-put".to_owned()),
            (66, "unreached".to_owned()),
            (75, "waited".to_owned()),
            (84, "undecided".to_owned()),
        ],
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let unnoticed = named(&part, "findings", "kind", "unnoticed-fault");
    assert_eq!(
        unnoticed.len(),
        1,
        "one finding names the failure nothing noticed: {part}"
    );
    assert_eq!(unnoticed[0]["path"], "src/lib.rs", "{part}");
    assert!(
        unnoticed[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("dropped without anything reading it")),
        "the finding says the failure went nowhere, which is what `length` does with it: {part}"
    );
    assert!(
        njutest_devkit::process::strict_utf8(&output.stdout).contains(
            "FAULTS\tsites=8\tnoticed=2\tunnoticed=0\tabsorbed=1\tunreached=1\twaited=1\tundecided=1\tnot_put=2"
        ),
        "the run says what the faults came to where it says what the mutations did: {}",
        njutest_devkit::process::strict_utf8(&output.stdout)
    );
    assert_eq!(
        named(&part, "limitations", "name", "fault-not-put").len(),
        1,
        "the two faults no run could put are stated once, as one class: {part}"
    );
    assert_eq!(
        part["accounting"]["faults"],
        serde_json::json!({
            "sites": 8, "noticed": 2, "unnoticed": 0, "absorbed": 1, "unreached": 1,
            "waited": 1, "undecided": 1, "not_put": 2
        }),
        "{part}"
    );
    for fault in part["faults"].as_array().expect("the fault records") {
        let path = fault["path"].as_str().expect("a path");
        assert_eq!(
            (
                fault["rule"].as_str(),
                fault["rule_version"].as_u64(),
                fault["span"]["start"].is_u64(),
                fault["span"]["end"].is_u64(),
                fault["original"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                fault["replacement"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            ),
            (Some("inject-error"), Some(1), true, true, true, true),
            "a fault record carries every field its identity is minted from, as a mutation's \
             does: {fault}"
        );
        let said = part["sources"]
            .as_array()
            .expect("the run's source digests")
            .iter()
            .find(|entry| entry["path"].as_str() == Some(path));
        assert_eq!(
            fault["source_digest"],
            said.expect("its file is among the run's sources")["digest"],
            "and the digest of the whole file it is in, as the run read it: {fault}"
        );
    }
    let declined = part["faults"]
        .as_array()
        .expect("the faults")
        .iter()
        .find(|fault| fault["item"] == "refused")
        .expect("the fault no test measured");
    assert!(
        declined["decision"]["why"].as_str().is_some_and(|why| {
            why.contains("every test that reached it declined to measure")
                && why.contains("this machine does not measure a failed read of the manifest")
        }),
        "all-declined is undecided for the test's stated reason: {declined}"
    );
    let measured = part["faults"]
        .as_array()
        .expect("the faults")
        .iter()
        .find(|fault| fault["item"] == "measured")
        .expect("the fault two targets absorb")["display_id"]
        .as_str()
        .expect("its short id");
    let fated: Vec<String> = recording(&fixture)
        .iter()
        .filter_map(|event| match fault_event(&event.payload) {
            FaultEvent::Fate(fate) => (fate.fault == measured).then(|| fate.target.clone()),
            FaultEvent::Baseline(..)
            | FaultEvent::Route(..)
            | FaultEvent::Writes(..)
            | FaultEvent::Other => None,
        })
        .collect();
    assert_eq!(
        fated,
        vec!["fixture-faulted/test/calls", "fixture-faulted/test/second"],
        "an absorbed fault is asked again of every target that reached it"
    );
    audited(&fixture);
}

#[test]
fn a_faulted_session_compares_no_reach_and_runs_nothing_again() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let events = recording(&fixture);
    let faulted = events
        .iter()
        .position(|event| {
            matches!(
                event.payload,
                njutest::trace::Payload::FaultRoute { .. }
                    | njutest::trace::Payload::FaultExec { .. }
            )
        })
        .unwrap_or_else(|| {
            panic!(
                "the run put faults: {}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        });
    let compared: Vec<&str> = events
        .iter()
        .skip(faulted)
        .map(|event| event.payload.type_name())
        .filter(|kind| matches!(*kind, "drift" | "repair"))
        .collect();
    assert!(
        compared.is_empty(),
        "a faulted session judges faults and nothing else: its touch and drift records are \
         never compared with the baseline's, so it owes no target a control of its own and runs \
         no disposition again, and the recording holds none of either after its first fault \
         (ADR 0032 decision 3): {compared:?}"
    );
}

#[test]
fn the_faulted_baseline_s_reach_is_recorded_so_every_route_s_reaching_holds_to_it() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let events = recording(&fixture);
    let index_of = |fault: &str| -> Option<u64> {
        part["faults"]
            .as_array()
            .expect("the fault records")
            .iter()
            .find(|row| row["display_id"].as_str() == Some(fault))?["catalog_index"]
            .as_u64()
    };
    let baselines: Vec<(&str, bool, &Vec<u32>)> = events
        .iter()
        .filter_map(|event| match fault_event(&event.payload) {
            FaultEvent::Baseline(baseline) => {
                Some((baseline.target.as_str(), baseline.doc, &baseline.reached))
            }
            FaultEvent::Route(..)
            | FaultEvent::Fate(..)
            | FaultEvent::Writes(..)
            | FaultEvent::Other => None,
        })
        .collect();
    assert!(
        !baselines.is_empty(),
        "the run records what each target's faulted baseline reached before routing a fault \
         over it, so a route's reaching is held to the baseline and not to the route's own word: \
         {}\n{}",
        events
            .iter()
            .map(|event| event.payload.type_name())
            .collect::<Vec<&str>>()
            .join(", "),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let routed_faults: Vec<(&str, &Vec<String>)> = events
        .iter()
        .filter_map(|event| match fault_event(&event.payload) {
            FaultEvent::Route(route) => Some((route.fault.as_str(), &route.reaching)),
            FaultEvent::Baseline(..)
            | FaultEvent::Fate(..)
            | FaultEvent::Writes(..)
            | FaultEvent::Other => None,
        })
        .collect();
    assert_eq!(
        routed_faults.len(),
        6,
        "every fault the run put was routed first: {routed_faults:?}"
    );
    for (fault, reaching) in routed_faults {
        let index = u32::try_from(index_of(fault).expect("a routed fault is one the report holds"))
            .expect("a catalog index fits");
        for (target, doc, sites) in &baselines {
            let (reached, routed) = (
                sites.contains(&index),
                reaching.iter().any(|one| one == target),
            );
            assert!(
                !reached || routed,
                "the faulted baseline of {target} reached {fault}'s site and the route leaves it \
                 out, which is a discharge or a narrowed route, exactly what a fault may not \
                 rest on (ADR 0032 decision 4)"
            );
            assert!(
                !routed || reached || *doc,
                "the route puts {target} at {fault} and that target's faulted baseline never \
                 reached its site, which only a documentation target may be"
            );
        }
    }
}

#[test]
fn a_run_not_asked_for_faults_puts_none() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &[]);
    let part = part(&fixture);
    assert!(
        !njutest_devkit::process::strict_utf8(&output.stdout).contains("FAULTS"),
        "a run that put no fault says nothing about faults"
    );
    assert_eq!(
        part["faults"],
        serde_json::json!([]),
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        named(&part, "findings", "kind", "unnoticed-fault").is_empty(),
        "no fault was put, so none went unnoticed: {part}"
    );
}

#[test]
fn the_paths_written_before_and_after_the_faults_are_recorded_so_the_unattributed_rests_on_them() {
    let fixture = fixture("fixture-faulted-failure-writes");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let events = recording(&fixture);
    let written: Vec<&njutest::trace::FaultWritesRecord> = events
        .iter()
        .filter_map(|event| match fault_event(&event.payload) {
            FaultEvent::Writes(writes) => Some(writes),
            FaultEvent::Baseline(..)
            | FaultEvent::Route(..)
            | FaultEvent::Fate(..)
            | FaultEvent::Other => None,
        })
        .collect();
    assert_eq!(
        written.len(),
        1,
        "the run states the paths written before the first fault and after the last once, so \
         the audit holds the unattributed finding to the paths the phase left written and not \
         only to the ones attribution asked about: {}\n{}",
        events
            .iter()
            .map(|event| event.payload.type_name())
            .collect::<Vec<&str>>()
            .join(", "),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let before: std::collections::BTreeSet<&str> =
        written[0].before.iter().map(String::as_str).collect();
    let after: std::collections::BTreeSet<&str> =
        written[0].after.iter().map(String::as_str).collect();
    let broke: Vec<&str> = after.difference(&before).copied().collect();
    assert_eq!(
        broke,
        vec!["regressions.txt"],
        "what the phase left written is what the tests wrote as they failed: {part}"
    );
    let unattributed: Vec<&serde_json::Value> = named(&part, "findings", "kind", "not-measured")
        .into_iter()
        .filter(|finding| finding["subject"] == "fault-write-unattributed")
        .collect();
    assert_eq!(
        unattributed.len(),
        1,
        "and the finding names exactly what broke: {part}"
    );
    assert!(
        unattributed[0]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("regressions.txt"),
        "{part}"
    );
    audited(&fixture);
}

#[test]
fn a_write_one_fault_makes_on_its_own_and_its_test_does_not_without_it_is_a_defect() {
    let fixture = fixture("fixture-faulted-writes");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![(13, "absorbed".to_owned())],
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let broke = named(&part, "findings", "kind", "broken-under-fault");
    assert_eq!(
        broke.len(),
        1,
        "the fault, run alone, wrote failed-read.log while its test passed, and the same test \
         run alone without it did not: that write is the program's answer to the failed call, \
         tied to one fault: {part}"
    );
    let detail = broke[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("failed-read.log") && !detail.contains("always.log"),
        "the finding names what the fault wrote and not what every run writes: {detail}"
    );
    assert!(
        named(&part, "findings", "kind", "not-measured")
            .iter()
            .all(|finding| finding["subject"] != "fault-write-unattributed"),
        "and nothing is left unattributed: {part}"
    );
    assert_eq!(
        output.status.code(),
        Some(njutest::cli::EXIT_DEFECT.into()),
        "a defect decides the run: {part}"
    );
}

#[test]
fn a_write_a_test_makes_as_it_fails_under_a_fault_is_not_a_defect() {
    let fixture = fixture("fixture-faulted-failure-writes");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![(13, "noticed".to_owned())],
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        named(&part, "findings", "kind", "broken-under-fault").is_empty(),
        "the test wrote regressions.txt because it noticed the fault and failed, which is the \
         suite doing its job; a property test keeping the input that broke it is not the \
         program writing past where it was asked to work: {part}"
    );
    let unattributed: Vec<&serde_json::Value> = named(&part, "findings", "kind", "not-measured")
        .into_iter()
        .filter(|finding| finding["subject"] == "fault-write-unattributed")
        .collect();
    assert_eq!(unattributed.len(), 1, "{part}");
    assert!(
        unattributed[0]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("regressions.txt"),
        "{part}"
    );
    assert_ne!(
        output.status.code(),
        Some(njutest::cli::EXIT_DEFECT.into()),
        "{part}"
    );
}

#[test]
fn why_follows_a_fault_from_every_target_it_was_put_to_to_what_it_came_to() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let absorbed = part["faults"]
        .as_array()
        .expect("a list of faults")
        .iter()
        .find(|fault| fault["decision"]["decision"] == "absorbed")
        .and_then(|fault| fault["display_id"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "the run holds the absorbed fault: {part}\n{}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        })
        .to_owned();
    let why = asked(&fixture, &["why", "fault", &absorbed]);
    let page = njutest_devkit::process::strict_utf8(&why.stdout);
    assert!(
        page.contains("every test that reached it passed with the call failing")
            && page.contains("asked fixture-faulted/test/calls  survived")
            && page.contains(
                "asked again fixture-faulted/test/calls  survived  1 failure(s) made, 0 read, \
                 1 dropped"
            ),
        "the page names every target the fault was put to, what its failures came to, and what \
         the run concluded: {page}\n{}",
        njutest_devkit::process::strict_utf8(&why.stderr)
    );
}

#[test]
fn a_tree_every_run_writes_into_is_not_broken_by_a_fault() {
    let fixture = fixture("fixture-writes-tree");
    let output = verify(&fixture, &["--faults"]);
    let part = part(&fixture);
    assert!(
        named(&part, "findings", "kind", "broken-under-fault").is_empty(),
        "the tree was written with no fault in place, so no fault wrote it: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        named(&part, "limitations", "name", "fault-no-site").len(),
        1,
        "no measured file of it has a `?`, so there was no call to fail, and the run says so: {part}"
    );
}

#[test]
fn why_names_a_survivor_the_suite_tells_apart_under_a_fault_observable_under_fault() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let survivor = part["beside"][0]["mutant"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "the run holds evidence beside a fault: {part}\n{}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        })
        .to_owned();
    let fault = part["beside"][0]["fault"]
        .as_str()
        .expect("the evidence names its fault")
        .to_owned();
    let why = asked(&fixture, &["why", "mutation", &survivor]);
    let page = njutest_devkit::process::strict_utf8(&why.stdout);
    assert!(
        page.contains("observable-under-fault")
            && page.contains(&fault)
            && page.contains("fixture-faulted/test/calls"),
        "the page of the survivor names the evidence by its name, with the fault at its own \
         call and the target that told it apart, so a reader learns that it is no equivalence \
         and which failure no test makes (ADR 0032 decision 6): {page}\n{}",
        njutest_devkit::process::strict_utf8(&why.stderr)
    );
}

#[test]
fn a_survivor_the_suite_tells_apart_only_under_a_fault_is_evidence_and_never_a_kill() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults"]);
    let part = part(&fixture);
    let beside: Vec<(String, String)> = part["beside"]
        .as_array()
        .unwrap_or_else(|| {
            panic!(
                "a list of what was put beside a fault: {part}\n{}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        })
        .iter()
        .map(|one| {
            (
                one["target"].as_str().unwrap_or_default().to_owned(),
                one["failed"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        beside,
        vec![("fixture-faulted/test/calls".to_owned(), "beside".to_owned())],
        "only `measured` tells `.unwrap()` from `?` once its read fails; `load` and `number` \
         fail either way, and the two sites no fault can be put at are not asked: {part}"
    );
    let unwrapped = part["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["display_id"] == part["beside"][0]["mutant"])
        .expect("the survivor the evidence is about");
    assert_eq!(
        (
            unwrapped["rule"].as_str(),
            unwrapped["item"].as_str(),
            unwrapped["decision"]["outcome"].as_str()
        ),
        (
            Some("question-to-unwrap"),
            Some("measured"),
            Some("survived")
        ),
        "the evidence is attached to a survivor and leaves it one: no test failed that call"
    );
    assert_eq!(
        part["accounting"]["mutants"]["killed"], 6,
        "and it is in no kill count: {part}"
    );
}
