// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest verify`, end to end, against a real workspace.

#![expect(
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
#![cfg_attr(
    unix,
    expect(
        clippy::indexing_slicing,
        clippy::panic,
        clippy::too_many_lines,
        reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table, and what these permit is what a test that reads a published report needs, which this platform cannot publish: those tests are behind cfg(unix) one by one, so what their shapes permit is behind it too"
    )
)]

use njutest_devkit::fixture::copy_tree;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
#[cfg(unix)]
use njutest::report::Verdict;
use rust_mutants::runner::Cancel;

/// A throwaway copy of a fixture, so the run writes its reports somewhere nothing else is reading.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-verify-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
    args.extend_from_slice(extra);
    asked(&of(&fixture.root, &[]), &args)
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
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
        terminal: njutest::presentation::Terminal::default(),
    }
}

/// The environment of a fixture, with the cache and the scratch beside its root.
fn of(root: &Path, named: &[(&str, &str)]) -> Environment {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}

#[cfg(unix)]
#[test]
fn a_run_says_where_it_wrote_as_a_path_from_the_project_it_is_about() {
    let fixture = fixture("fixture-assured");
    let output = asked(
        &of(&fixture.root, &[]),
        &[
            "verify",
            "--offline",
            "--locked",
            "--directory",
            &fixture.root.display().to_string(),
        ],
    );
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    let written = said
        .lines()
        .find_map(|line| line.strip_prefix("REPORT\t"))
        .expect("a run that kept a report says where it kept it");
    assert!(
        !Path::new(written).is_absolute(),
        "a run told where to work says where it wrote from there, because a reader \
         joining it onto the project would otherwise get the path twice: {written}"
    );
    assert!(
        fixture.root.join(written).is_file(),
        "and the path it says is one the project's own root reaches: {written}"
    );
}

/// Where the latest run wrote its report: the run directories themselves, because a shard is a merge input and deliberately points no latest-complete index at itself.
#[cfg(unix)]
fn latest(fixture: &Fixture) -> PathBuf {
    let runs = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs");
    let run = std::fs::read_dir(&runs)
        .unwrap_or_else(|error| panic!("{}: {error}", runs.display()))
        .map(|entry| entry.expect("every run entry is readable"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .max_by_key(std::fs::DirEntry::file_name)
        .expect("the runs directory names a run");
    runs.join(run.file_name())
        .join(njutest::app::reports::DOCUMENT_NAME)
}

/// The durable document of the latest run, envelope and all, as text.
#[cfg(unix)]
fn envelope(fixture: &Fixture) -> String {
    let path = latest(fixture);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(unix)]
fn document(fixture: &Fixture) -> serde_json::Value {
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&envelope(fixture)).expect("the report is JSON");
    assert!(
        document["document_type"] == "complete" || document["document_type"] == "shard",
        "a run writes a tagged envelope naming which document is under it: {document}"
    );
    document["report"].clone()
}

/// The latest run's document read back the way a consumer reads one, with the verdict that reader derives.
#[cfg(unix)]
fn parsed(fixture: &Fixture) -> njutest::report::ReportDocument {
    njutest::report::json::parse_any(&envelope(fixture)).expect("the document reads back")
}

#[cfg(unix)]
fn engine_trace(recording: &Path, ordinal: u32) -> Vec<rust_mutants::trace::Event> {
    let stream = recording
        .join(njutest::app::trace::BUILDS_DIRECTORY)
        .join(format!("{ordinal:010}"))
        .join(njutest::app::trace::ENGINE_DIRECTORY)
        .join(rust_mutants::trace::FILE_NAME);
    rust_mutants::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&stream)
            .unwrap_or_else(|error| panic!("{}: {error}", stream.display())),
    ))
    .unwrap_or_else(|error| panic!("{}: {error}", stream.display()))
}

#[cfg(unix)]
#[test]
fn a_target_put_to_mutations_that_noticed_none_is_named_with_how_many() {
    let fixture = fixture("fixture-hollow");
    let output = verify(&fixture, &[]);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    let document = document(&fixture);
    let hollow: Vec<&serde_json::Value> = document["builds"][0]["parts"][0]["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("the report lists findings: {document}\n{stderr}"))
        .iter()
        .filter(|one| one["kind"] == "hollow-target")
        .collect();

    assert_eq!(
        hollow.len(),
        1,
        "`smoke` runs `double` and asserts nothing about what came back, so every \
         mutation of `double` survives it; the library target noticed its own, so \
         only one target is hollow here: {document}\n{stderr}"
    );
    let named = hollow[0]["subject"]
        .as_str()
        .unwrap_or_else(|| panic!("a finding names its subject: {}", hollow[0]));
    assert!(
        named.contains("smoke"),
        "and it is the one that asserts nothing, not the one that does: {named}"
    );
    let detail = hollow[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains('2'),
        "the count is the claim, and it is what was asked rather than what the \
         catalog holds: four mutations of `double` survive the engine on its own, \
         and a proof discharged two of them here without an execution, so `smoke` \
         was put to two. A finding that said only `noticed nothing` would read the \
         same whether it was asked once or a hundred times: {detail}"
    );
}

/// Every finding of `kind` the one whole part of a report raised.
fn findings_of<'a>(document: &'a serde_json::Value, kind: &str) -> Vec<&'a serde_json::Value> {
    document["builds"][0]["parts"][0]["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("the report lists findings: {document}"))
        .iter()
        .filter(|one| one["kind"] == kind)
        .collect()
}

#[cfg(unix)]
#[test]
fn a_target_whose_reach_moved_between_its_baseline_and_a_control_is_an_unstable_baseline() {
    let fixture = fixture("fixture-drifts");
    let output = verify(&fixture, &[]);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    let document = document(&fixture);
    let target = "fixture-drifts/lib/fixture_drifts";
    let unstable = findings_of(&document, "unstable-baseline");
    assert_eq!(
        unstable.len(),
        1,
        "the baseline was the first process of the run to look and every control after it          was not, so the one target reached one function on its baseline and another on the          control that confirmed a kill, over the same passing test: {document}\n{stderr}"
    );
    assert_eq!(unstable[0]["subject"], target, "{}", unstable[0]);
    let detail = unstable[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("2 mutations no test reached"),
        "the weight of the finding is what rests on the moved record: both mutations of \
         `return_visit` are unreached on its word, and nothing was discharged: {detail}"
    );
    let drift = &document["builds"][0]["parts"][0]["drift"];
    assert_eq!(drift[0]["state"], "moved", "{drift}");
    assert_eq!(drift[0]["target"], target, "{drift}");
    assert_eq!(
        output.status.code(),
        Some(2),
        "a moved measurement is a gap in what the run established, not a fault in the code: \
         {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_target_no_kill_was_confirmed_on_is_one_whose_drift_was_not_measured() {
    let fixture = fixture("fixture-hollow");
    let output = verify(&fixture, &[]);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    let document = document(&fixture);
    let part = &document["builds"][0]["parts"][0];
    let states: Vec<(String, String)> = part["drift"]
        .as_array()
        .unwrap_or_else(|| panic!("every part records drift: {document}\n{stderr}"))
        .iter()
        .map(|one| {
            (
                one["target"].as_str().unwrap_or_default().to_owned(),
                one["state"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            (
                "fixture-hollow/lib/fixture_hollow".to_owned(),
                "held".to_owned()
            ),
            (
                "fixture-hollow/test/smoke".to_owned(),
                "not-measured".to_owned()
            ),
        ],
        "the library's kills were confirmed by a control that reached what its baseline did, \
         and nothing was ever confirmed on `smoke`, so nothing compared it"
    );
    let limitation: Vec<&serde_json::Value> = part["limitations"]
        .as_array()
        .unwrap_or_else(|| panic!("the report lists limitations: {document}"))
        .iter()
        .filter(|one| one["name"] == njutest::limitation::DRIFT_NOT_MEASURED)
        .collect();
    assert_eq!(limitation.len(), 1, "{document}");
    let detail = limitation[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.ends_with("(fixture-hollow/test/smoke)") && detail.contains("1 target"),
        "the limitation names the one target it is about and says how many: {detail}"
    );
    assert!(
        findings_of(&document, "unstable-baseline").is_empty(),
        "a target whose drift was not measured is not one that moved: {document}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutation_only_one_of_the_builds_notices_is_a_survivor_that_names_the_other() {
    let fixture = fixture("fixture-features");
    std::fs::write(
        fixture.root.join(njutest::config::FILE_NAME),
        "[[configuration]]\nname = \"imperial\"\nfeatures = [\"imperial\"]\n",
    )
    .expect("the configuration is written");

    let output = verify(&fixture, &[]);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    let document = document(&fixture);
    let mutants = document["builds"][0]["parts"][0]["mutants"]
        .as_array()
        .unwrap_or_else(|| panic!("the report lists mutations: {document}"));

    let feet: Vec<&serde_json::Value> = mutants
        .iter()
        .filter(|one| {
            one["item"]
                .as_str()
                .is_some_and(|item| item.contains("feet"))
        })
        .collect();
    assert!(
        !feet.is_empty(),
        "the fixture mutates `feet`, and a run that catalogued none of them is \
         measuring something else: {document}\n{stderr}"
    );
    let concluded = njutest::report::json::parse(&envelope(&fixture))
        .expect("the report reads back")
        .conclusion()
        .expect("the report adds up");
    let feet_ids: Vec<String> = feet
        .iter()
        .filter_map(|one| one["display_id"].as_str().map(str::to_owned))
        .collect();
    for mutation in concluded
        .mutants
        .iter()
        .filter(|one| feet_ids.contains(&one.display_id().to_owned()))
    {
        assert_eq!(
            mutation.decision(),
            njutest::report::Decision::Unreached,
            "the default build compiles no test for `feet`, so nothing there even \
             runs it; the run may not let the build that does outvote the one that \
             does not: {:?}",
            mutation.by_build()
        );
        assert_eq!(
            mutation.blind_in(),
            &[njutest::report::BlindIn {
                build: njutest::report::BuildName::try_from("default")
                    .expect("a canonical build name"),
                decision: njutest::report::Blind::Unreached,
            }],
            "and the run names the build and what that build established, because a \
             gap everywhere and a gap under the defaults are different things to act \
             on — and so are a build whose tests noticed nothing and one where nothing \
             ran it at all"
        );
    }

    let metres: Vec<&serde_json::Value> = mutants
        .iter()
        .filter(|one| {
            one["item"]
                .as_str()
                .is_some_and(|item| item.contains("metres"))
        })
        .collect();
    assert!(!metres.is_empty(), "{document}");
    let metres_ids: Vec<String> = metres
        .iter()
        .filter_map(|one| one["display_id"].as_str().map(str::to_owned))
        .collect();
    for mutation in concluded
        .mutants
        .iter()
        .filter(|one| metres_ids.contains(&one.display_id().to_owned()))
    {
        assert!(
            mutation.blind_in().is_empty(),
            "a mutation both builds noticed names no build: {:?}",
            mutation.by_build()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_suite_with_a_gap_it_cannot_see_is_insufficient() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = njutest_devkit::process::strict_utf8(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {stdout}\nstderr: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "the verdict is the last record: {stdout}"
    );
    assert!(
        stdout.contains("FINDING\tsurviving-mutant"),
        "and the report names what nobody noticed: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn the_report_is_written_where_a_reader_will_look_and_validates_against_the_schema() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let report = document(&fixture);
    let schema_path =
        njutest_devkit::paths::workspace_root().join("schema/njutest-assurance-report-v1.json");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(schema_path).expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let written: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&envelope(&fixture)).expect("the report is JSON");
    let problems: Vec<String> = validator
        .iter_errors(&written)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(parsed(&fixture).verdict(), Verdict::Insufficient);
    assert_eq!(
        report["builds"][0]["parts"][0]["accounting"]["targets"]["selected"],
        2
    );
    assert_eq!(
        report["builds"][0]["parts"][0]["accounting"]["targets"]["passed"],
        2
    );
    assert_eq!(
        report["builds"][0]["parts"][0]["accounting"]["targets"]["skipped"],
        0
    );
    assert_eq!(
        report["builds"][0]["parts"][0]["findings"]
            .as_array()
            .expect("findings")
            .len(),
        4
    );
    assert_eq!(
        report["builds"][0]["parts"][0]["toolchain"]["target"]
            .as_str()
            .unwrap_or_default(),
        report["builds"][0]["parts"][0]["toolchain"]["target"]
            .as_str()
            .unwrap_or("x"),
        "the triple is recorded"
    );
    assert!(
        report["repository"]["configuration_digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the effective configuration is identified: {report}"
    );
}

#[cfg(unix)]
#[test]
fn the_targets_are_named_and_ordered_slowest_first() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let report = document(&fixture);
    let targets = report["builds"][0]["parts"][0]["targets"]
        .as_array()
        .expect("targets");
    assert_eq!(
        targets.len(),
        2,
        "one row per test binary: the library's own tests and the integration test. \
         A row is a binary because a run measures a binary, and which of its tests a \
         mutation is put to is what a route says: {targets:?}"
    );

    let durations: Vec<u64> = targets
        .iter()
        .map(|target| target["duration_ms"].as_u64().unwrap_or_default())
        .collect();
    let mut sorted = durations.clone();
    sorted.sort_unstable();
    sorted.reverse();
    assert_eq!(durations, sorted, "slowest first: {durations:?}");

    let names: Vec<&str> = targets
        .iter()
        .filter_map(|target| target["name"].as_str())
        .collect();
    assert!(
        names.contains(&"fixture-baseline/lib/fixture_baseline"),
        "a target is named the way the engine names the binary it is — package, kind, \
         and the binary's own name: {names:?}"
    );
    assert!(
        names.contains(&"fixture-baseline/test/doubling"),
        "{names:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_that_asked_for_a_trace_leaves_one_that_reads_back() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(2));

    let traces = fixture.root.join(".njutest/trace");
    let recording = std::fs::read_dir(&traces)
        .expect("the trace directory")
        .map(|entry| entry.expect("every trace entry is readable"))
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let stream = recording.join(njutest::trace::FILE_NAME);
    let events = njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&stream).expect("the stream"),
    ))
    .expect("the events read back");

    let problems = njutest::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.payload.type_name())
        .collect();
    assert_eq!(kinds.first(), Some(&"run-start"));
    assert_eq!(kinds.last(), Some(&"run-end"));
    assert!(kinds.contains(&"exec"), "{kinds:?}");
    assert!(kinds.contains(&"phase-end"), "{kinds:?}");

    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .phase_start()
                .map(|phase| phase.name.as_str())
        })
        .collect();
    assert_eq!(
        phases.first(),
        Some(&"open"),
        "a recording is read to find out where a run spent its time and where it \
         stopped, so the first thing it says is the first thing the run did: a phase \
         nobody opened leaves every event before the next one filed under nothing: \
         {phases:?}"
    );
    for named in ["open", "baseline", "mutation"] {
        assert!(
            phases.contains(&named),
            "every stage a run goes through names itself, or a reader counting the \
             seconds between two events cannot say what happened in them: {phases:?}"
        );
    }

    let njutest::trace::Payload::RunStart { start } = &events[0].payload else {
        panic!("the checked outer stream starts with its run binding")
    };
    let nested = engine_trace(&recording, 0);
    let rust_mutants::trace::Payload::RunStart { context, .. } = &nested[0].payload else {
        panic!("the checked engine stream starts with its build binding")
    };
    let rust_mutants::trace::TraceContext::Njutest { build } = context else {
        panic!("an engine trace under an njutest run cannot claim to be standalone")
    };
    assert_eq!(build.final_run_id().as_str(), start.run_id);
    assert_eq!(
        build.internal_run_id().as_str(),
        format!("{}-b0000000000", start.run_id)
    );
    assert_ne!(build.internal_run_id(), build.final_run_id());
    assert_eq!(build.ordinal(), 0);
    assert_eq!(build.name(), njutest::config::DEFAULT_CONFIGURATION);
    assert_eq!(
        build.build_selection(),
        rust_mutants::cargo::BuildConfig::default()
            .selection()
            .digest()
    );
}

#[cfg(unix)]
#[test]
fn every_configured_build_owns_one_ordinal_trace_bound_to_its_selection() {
    let fixture = fixture("fixture-features");
    std::fs::write(
        fixture.root.join(njutest::config::FILE_NAME),
        "[[configuration]]\nname = \"imperial\"\nfeatures = [\"imperial\"]\n",
    )
    .expect("the configuration is written");
    let output = verify(&fixture, &["--trace"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let trace_root = fixture.root.join(".njutest/trace");
    let recording = std::fs::read_dir(&trace_root)
        .expect("the trace root")
        .next()
        .expect("one outer namespace")
        .expect("the namespace is readable")
        .path();
    let outer = njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(recording.join(njutest::trace::FILE_NAME)).expect("the outer stream"),
    ))
    .expect("the outer events");
    let njutest::trace::Payload::RunStart { start } = &outer[0].payload else {
        panic!("the checked outer stream starts with its run binding")
    };
    let selections = [
        rust_mutants::cargo::BuildConfig::default().selection(),
        njutest::config::Configuration {
            name: "imperial".to_owned(),
            features: vec!["imperial".to_owned()],
            ..njutest::config::Configuration::default()
        }
        .build()
        .selection(),
    ];
    for (ordinal, (name, selection)) in ["default", "imperial"]
        .into_iter()
        .zip(selections.iter())
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).expect("two configured builds fit u32");
        let events = engine_trace(&recording, ordinal);
        assert!(rust_mutants::trace::check(&events).is_empty(), "{events:?}");
        let rust_mutants::trace::Payload::RunStart { context, .. } = &events[0].payload else {
            panic!("the checked engine stream starts with its build binding")
        };
        let rust_mutants::trace::TraceContext::Njutest { build } = context else {
            panic!("an njutest-owned engine namespace cannot claim to be standalone")
        };
        assert_eq!(build.final_run_id().as_str(), start.run_id);
        assert_eq!(build.ordinal(), ordinal);
        assert_eq!(build.name(), name);
        assert_eq!(build.build_selection(), selection.digest());
        assert_eq!(
            build.internal_run_id().as_str(),
            format!("{}-b{ordinal:010}", start.run_id)
        );
        assert_ne!(build.internal_run_id(), build.final_run_id());
    }
    assert!(
        !recording
            .join(njutest::app::trace::BUILDS_DIRECTORY)
            .join(njutest::app::trace::ENGINE_DIRECTORY)
            .exists(),
        "two builds must never share an unqualified fallback namespace"
    );
}

#[test]
fn an_explicit_trace_that_cannot_be_claimed_refuses_before_verification() {
    let fixture = fixture("fixture-baseline");
    let blocked = fixture.root.join("blocked-trace");
    std::fs::write(&blocked, "not a directory").expect("the path-blocking file");
    let output = verify(&fixture, &[&format!("--trace={}", blocked.display())]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        stderr.contains("requested trace directory") && stderr.contains("blocked-trace"),
        "{stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "no verification answer is produced after trace setup refusal"
    );
    assert!(
        !fixture.root.join(".njutest").exists(),
        "trace setup is settled before report, checkpoint, or engine work"
    );
}

#[test]
fn progress_goes_to_the_error_stream_so_a_redirected_report_is_a_report() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = njutest_devkit::process::strict_utf8(&output.stdout);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);

    let stages: Vec<&str> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    assert_eq!(
        stages.first(),
        Some(&"open"),
        "the first thing a person watching is told is the first thing the run is doing, \
         and a run that says nothing until it has built the workspace looks like one \
         that has hung: {stderr}"
    );
    for named in ["open", "baseline", "mutation"] {
        assert!(
            stages.contains(&named),
            "and every stage names itself as it starts: {stages:?}"
        );
    }
    assert!(stderr.contains("== baseline"), "{stderr}");
    assert!(
        !stdout.contains("== baseline"),
        "the output stream carries the report alone: {stdout}"
    );
    for line in stdout.lines() {
        let kind = line.split('\t').next().unwrap_or_default();
        assert_eq!(kind, kind.to_uppercase(), "not a record: {line:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_jsonl_interface_writes_one_object_per_line() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &["--ui", "jsonl"]);
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(!stderr.trim().is_empty(), "it said something");
    for line in stderr.lines() {
        let value: serde_json::Value = njutest_devkit::strictjson::decode_str(line)
            .unwrap_or_else(|error| panic!("{line:?}: {error}"));
        assert!(value.get("type").is_some(), "{line}");
    }
}

#[cfg(unix)]
#[test]
fn a_workspace_that_does_not_compile_is_a_defect_that_names_itself() {
    let fixture = fixture("fixture-baseline");
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        b"// SPDX-FileCopyrightText: 2026 njutest contributors\n\
          // SPDX-License-Identifier: MIT OR Apache-2.0\n\
          //! Broken on purpose.\npub fn sign() -> i32 { \"not an integer\" }\n",
    )
    .expect("break it");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(parsed(&fixture).verdict(), Verdict::Defect);
    let findings = report["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["kind"], "build-failure");
    assert!(
        findings[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("mismatched types")),
        "the compiler's own words: {findings:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_report_of_a_known_workspace_is_the_recorded_one() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let normalized = njutest_devkit::report::normalize(&document(&fixture));
    let mut text = serde_json::to_string_pretty(&normalized).expect("one document");
    text.push('\n');
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/verify.golden.json");
    njutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded report");
}

#[cfg(unix)]
#[test]
fn a_workspace_with_no_tests_at_all_observed_nothing_and_says_so() {
    let repo = njutest_devkit::repo::Repo::new();
    repo.package("silent")
        .lib("/// Nothing tests this.\npub const fn one() -> i32 {\n    1\n}\n");
    let fixture = Fixture {
        root: repo.root().to_path_buf(),
        _dir: tempfile::Builder::new()
            .prefix("njutest-unused-")
            .tempdir()
            .expect("a temporary directory"),
    };
    let output = verify(&fixture, &[]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let stdout = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        stdout.contains("TARGETS\tselected=1\tpassed=0"),
        "the library's test binary exists and was run: hiding it would say the run \
         found nothing to look at, when what it found was a binary with nothing in \
         it: {stdout}"
    );
    assert!(
        stdout.contains("FINDING\ttarget-missing"),
        "and that it executed no test is the finding: {stdout}"
    );
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "a suite with nothing in it assures nothing: {stdout}"
    );
    drop(repo);
}

#[cfg(unix)]
#[test]
fn a_second_run_of_the_same_work_reads_the_first_run_back_rather_than_doing_it_again() {
    let fixture = fixture("fixture-assured");
    let first = verify(&fixture, &[]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&first.stderr)
    );
    let established = document(&fixture);
    let established_verdict = parsed(&fixture).verdict();
    assert_eq!(established["provenance"]["cached"], false);
    assert_eq!(
        established["provenance"]["source_run_id"],
        serde_json::Value::Null
    );
    let identity = established["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();
    assert_eq!(identity.len(), 64, "{identity}");
    assert!(
        !established["builds"][0]["parts"][0]["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "workspace-digest-not-computed"),
        "a run that measured the tree does not say it could not: {established}"
    );

    let second = verify(&fixture, &[]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&second.stderr)
    );
    assert!(
        njutest_devkit::process::strict_utf8(&second.stderr).is_empty(),
        "{}",
        njutest_devkit::process::strict_utf8(&second.stderr)
    );
    let reused = document(&fixture);
    assert_eq!(reused["provenance"]["cached"], true, "{reused}");
    assert_eq!(reused["provenance"]["identity"], identity);
    assert_eq!(
        reused["provenance"]["source_run_id"], established["run_id"],
        "a reused answer names the run that established it"
    );
    assert_ne!(reused["run_id"], established["run_id"]);
    assert_eq!(parsed(&fixture).verdict(), established_verdict);
    assert_eq!(
        reused["builds"][0]["parts"][0]["accounting"],
        established["builds"][0]["parts"][0]["accounting"]
    );

    let afresh = verify(&fixture, &["--no-cache"]);
    assert_eq!(afresh.status.code(), Some(0));
    assert_eq!(
        document(&fixture)["provenance"]["cached"],
        false,
        "a run told to establish everything afresh does"
    );
}

#[cfg(unix)]
#[test]
fn a_tree_that_changed_is_a_different_question_and_is_answered_again() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let before = document(&fixture)["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();

    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("{source}\n// one more line\n")).expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let after = document(&fixture);
    assert_ne!(
        after["provenance"]["identity"], before,
        "a byte of the tree is part of what the run is about"
    );
    assert_eq!(
        after["provenance"]["cached"], false,
        "nothing was stored for this question yet"
    );
}

#[cfg(unix)]
#[test]
fn a_run_leaves_nothing_in_the_tree_it_verified_but_its_own_reports() {
    let fixture = fixture("fixture-assured");
    let before = listing(&fixture.root);
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let after = listing(&fixture.root);
    let added: Vec<&String> = after
        .iter()
        .filter(|path| !before.contains(*path))
        .collect();
    assert!(
        added.iter().all(|path| {
            path.starts_with(&format!(
                "{}/",
                njutest::config::Config::default()
                    .reports
                    .directory
                    .as_str()
            ))
        }),
        "an instrumented build writes its own coverage profiles, and they belong in the \
         directory the run works in rather than in the tree it is about: {added:?}"
    );
}

/// Every file under `root`, as slash-separated relative paths.
#[cfg(unix)]
fn listing(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.map(|entry| entry.expect("every trace entry is readable")) {
            let name = entry
                .file_name()
                .to_str()
                .expect("test protocol paths are UTF-8")
                .to_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push((entry.path(), relative));
            } else {
                found.insert(relative);
            }
        }
    }
    found
}

#[cfg(unix)]
#[test]
fn a_workspace_that_steps_outside_what_the_compiler_guarantees_says_so() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let safe = document(&fixture);
    let safe_part = &safe["builds"][0]["parts"][0];
    assert_eq!(safe_part["accounting"]["soundness"]["unsafe_items"], 0);
    assert_eq!(
        safe_part["accounting"]["soundness"]["packages_with_unsafe"],
        0
    );
    assert_eq!(safe_part["accounting"]["soundness"]["executed"], false);
    assert!(
        !names(&safe).contains(&"soundness-not-executed".to_owned()),
        "a workspace the compiler vouches for has nothing to say here: {safe}"
    );

    let path = fixture.root.join("tests/doubling.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(
        &path,
        format!(
            "{source}\n\
             /// Never called. The contract counts where the compiler stops vouching; it does \
             not execute it.\n\
             pub fn peek() -> u8 {{\n\
             \x20   unsafe {{ core::ptr::null::<u8>().read() }}\n\
             }}\n"
        ),
    )
    .expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let unsound = document(&fixture);
    assert_eq!(
        unsound["builds"][0]["parts"][0]["accounting"]["soundness"]["unsafe_items"],
        1
    );
    assert_eq!(
        unsound["builds"][0]["parts"][0]["accounting"]["soundness"]["packages_with_unsafe"],
        1
    );
    assert_eq!(
        unsound["builds"][0]["parts"][0]["accounting"]["soundness"]["executed"], false,
        "this contract counts them rather than executing them"
    );
    assert!(
        names(&unsound).contains(&"soundness-not-executed".to_owned()),
        "a non-empty inventory is a limitation the report states: {unsound}"
    );
}

/// The names of a report's limitations.
#[cfg(unix)]
fn names(document: &serde_json::Value) -> Vec<String> {
    document["builds"][0]["parts"][0]["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str().map(str::to_owned))
        .collect()
}

#[cfg(unix)]
#[test]
fn a_run_about_a_change_set_mutates_what_changed_and_claims_no_more_than_that() {
    let fixture = fixture("fixture-assured");
    njutest_devkit::repo::commit_tree(&fixture.root);

    let unchanged = verify(&fixture, &["--changed"]);
    assert_eq!(
        unchanged.status.code(),
        Some(2),
        "nothing changed, so nothing was asked of the tests: {}",
        njutest_devkit::process::strict_utf8(&unchanged.stderr)
    );
    let empty = document(&fixture);
    assert_eq!(empty["run_kind"], "changed");
    assert_eq!(
        empty["builds"][0]["parts"][0]["accounting"]["mutants"]["cataloged"],
        0
    );
    assert!(
        empty["repository"]["git"]["available"]
            .as_bool()
            .expect("a flag"),
        "{empty}"
    );

    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("{source}\n// changed\n")).expect("write");

    let changed = verify(&fixture, &["--changed"]);
    assert_eq!(
        changed.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&changed.stderr)
    );
    let document = document(&fixture);
    assert_eq!(document["run_kind"], "changed");
    assert_eq!(
        parsed(&fixture).verdict(),
        Verdict::ChangeAssured,
        "a run that looked at what changed claims no more than that: {document}"
    );
    assert!(
        document["builds"][0]["parts"][0]["accounting"]["mutants"]["cataloged"]
            .as_u64()
            .expect("a count")
            > 0,
        "{document}"
    );
    assert_eq!(
        document["repository"]["git"]["changed_files"],
        serde_json::json!(["src/lib.rs"])
    );
}

#[test]
fn a_run_about_a_change_set_it_cannot_see_refuses_rather_than_verifying_nothing() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &["--changed"]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "a tree git cannot be asked about is not a tree in which nothing changed"
    );
    assert!(
        njutest_devkit::process::strict_utf8(&output.stderr).contains("cannot see what changed"),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn a_checkpoint_never_speaks_for_a_target_this_run_measured_itself() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let established = document(&fixture);
    let identity = established["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();
    let first = &established["builds"][0]["parts"][0]["targets"][0];
    let id = first["id"].as_str().expect("a target id").to_owned();

    let store = njutest_devkit::paths::cache_beside(&fixture.root)
        .expect("a cache directory")
        .join("njutest/outcomes-v1");
    std::fs::remove_file(store.join(format!("{identity}.json")))
        .expect("the answer the first run stored");
    let identity = njutest::evidence::key::continuation_identity(
        &identity,
        &rust_mutants::cargo::BuildConfig::default().selection(),
    );
    let directory = store.join("checkpoints").join(&identity);
    std::fs::create_dir_all(&directory).expect("mkdir");
    std::fs::write(
        directory.join("checkpoint-v1.json"),
        serde_json::to_string(&serde_json::json!({
            "schema": "njutest-assurance-checkpoint-v1",
            "identity": identity,
            "attempts": 1,
            "targets": [{
                "id": id,
                "status": "failed",
                "duration_ms": 1,
                "message": "what the interrupted run observed",
                "files": ["src/lib.rs"],
            }],
            "mutants": [],
        }))
        .expect("the state renders"),
    )
    .expect("write");

    let resumed = verify(&fixture, &[]);
    assert_eq!(
        resumed.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&resumed.stderr)
    );
    let report = document(&fixture);
    assert!(
        names(&report).contains(&"resumed-from-checkpoint".to_owned()),
        "the run says it continued one that was interrupted: {report}"
    );
    let measured = report["builds"][0]["parts"][0]["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| target["id"] == serde_json::Value::String(id.clone()))
        .expect("the target the checkpoint named");
    assert_eq!(
        measured["status"], "passed",
        "a target is measured by the run that routes with it. Preparing runs every \
         target once before anything is judged, so there is no work a restored status \
         could save — and a status this run did not observe is one its routing does \
         not correspond to: {measured}"
    );
    assert_ne!(measured["message"], "what the interrupted run observed");
    assert_eq!(parsed(&fixture).verdict(), Verdict::Assured);
}

#[cfg(unix)]
#[test]
fn a_comparison_an_interrupted_run_made_is_not_one_the_resumed_run_made() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let established = document(&fixture);
    let identity = established["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();
    let part = &established["builds"][0]["parts"][0];
    let mut kills: Vec<serde_json::Value> = part["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|row| row["decision"]["outcome"] == "killed")
        .map(|row| {
            serde_json::json!({
                "id": row["id"],
                "disposition": { "kind": "killed", "by": row["decision"]["killed_by"] },
                "duration_ms": 1,
            })
        })
        .collect();
    kills.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    assert!(
        !kills.is_empty(),
        "the fixture kills something: {established}"
    );
    assert!(
        part["drift"]
            .as_array()
            .expect("drift")
            .iter()
            .all(|one| one["state"] == "held"),
        "the interrupted run compared every target and found each held: {part}"
    );

    let store = njutest_devkit::paths::cache_beside(&fixture.root)
        .expect("a cache directory")
        .join("njutest/outcomes-v1");
    std::fs::remove_file(store.join(format!("{identity}.json")))
        .expect("the answer the first run stored");
    let identity = njutest::evidence::key::continuation_identity(
        &identity,
        &rust_mutants::cargo::BuildConfig::default().selection(),
    );
    let directory = store.join("checkpoints").join(&identity);
    std::fs::create_dir_all(&directory).expect("mkdir");
    let state = serde_json::json!({
        "schema": "njutest-assurance-checkpoint-v1",
        "identity": identity,
        "attempts": 1,
        "targets": [],
        "mutants": kills,
    });
    std::fs::write(
        directory.join("checkpoint-v1.json"),
        serde_json::to_string(&state).expect("the state renders"),
    )
    .expect("write");

    let resumed = verify(&fixture, &[]);
    let stderr = njutest_devkit::process::strict_utf8(&resumed.stderr);
    let report = document(&fixture);
    assert!(
        names(&report).contains(&"resumed-from-checkpoint".to_owned()),
        "the run continued the interrupted one: {report}\n{stderr}"
    );
    let part = &report["builds"][0]["parts"][0];
    let states: Vec<&serde_json::Value> = part["drift"]
        .as_array()
        .expect("drift")
        .iter()
        .map(|one| &one["state"])
        .collect();
    assert!(
        states.iter().all(|state| *state == "not-measured"),
        "every kill was inherited, so no control ran this run, and a comparison the \
         interrupted run made was against a baseline this run measured again: {part}"
    );
    assert!(
        names(&report).contains(&njutest::limitation::DRIFT_NOT_MEASURED.to_owned()),
        "and the run says so rather than claiming a hold it did not observe: {report}"
    );
}

#[cfg(unix)]
#[test]
fn a_target_that_may_behave_differently_establishes_nothing_it_established_before() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let first = document(&fixture);
    assert_eq!(
        first["builds"][0]["parts"][0]["accounting"]["mutants"]["reused_killed"], 0,
        "the first run established everything itself"
    );
    assert!(
        first["builds"][0]["parts"][0]["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["reuse"]["reused"] == false),
        "{first}"
    );

    let differently = verify(&fixture, &["--", "--test-threads=1"]);
    assert_eq!(
        differently.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&differently.stderr)
    );
    let second = document(&fixture);
    assert_ne!(
        second["provenance"]["identity"], first["provenance"]["identity"],
        "what the harness is told is part of what the run is about"
    );
    assert_eq!(second["provenance"]["cached"], false);
    assert_eq!(
        second["builds"][0]["parts"][0]["accounting"]["mutants"]["reused_killed"], 0,
        "the tree is the tree it was, so every record is still there and every mutant still \
         has the identity it had; what the harness is told is not the same, so no target \
         behaves the way those records say it did: {second}"
    );
}

#[cfg(unix)]
#[test]
fn what_changed_outside_a_package_does_not_make_its_own_evidence_stale() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let first = document(&fixture);
    let killed = first["builds"][0]["parts"][0]["accounting"]["mutants"]["killed"]
        .as_u64()
        .expect("a count");
    assert!(killed > 0);

    std::fs::write(
        fixture.root.join("NOTES.md"),
        "nothing to do with the code\n",
    )
    .expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let second = document(&fixture);
    assert_ne!(
        second["provenance"]["identity"], first["provenance"]["identity"],
        "a file no package owns is still part of the tree, so the run is a different run"
    );
    assert_eq!(
        second["builds"][0]["parts"][0]["accounting"]["mutants"]["reused_killed"], killed,
        "every kill was established by a test that still reaches the mutant and still \
         behaves the same: {second}"
    );
    assert!(
        second["builds"][0]["parts"][0]["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|one| one["decision"]["outcome"] == "killed")
            .all(|one| {
                one["reuse"]["reused"] == true
                    && one["reuse"]["source_run_id"] == first["builds"][0]["parts"][0]["run_id"]
            }),
        "a reused verdict names the run that established it: {second}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutation_only_a_documented_example_can_notice_is_noticed_by_it() {
    let fixture = fixture("fixture-doctest");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let killer = |line: u64| -> String {
        report["builds"][0]["parts"][0]["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .find(|mutant| mutant["position"]["line"] == line && mutant["rule"] == "div-to-mul")
            .and_then(|mutant| mutant["decision"]["killed_by"].as_str())
            .unwrap_or("nothing")
            .to_owned()
    };
    assert!(
        killer(21).contains("/doc/"),
        "only the documentation exercises `half`, so only the documentation can notice a \
         mutation of it: {}",
        report["builds"][0]["parts"][0]["mutants"]
    );

    let stated: Vec<&str> = report["builds"][0]["parts"][0]["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        stated.contains(&"doctests-routed-by-file"),
        "rustdoc compiles an example into a binary this run never sees, so what it reaches \
         is known by file and not by region: {stated:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_documented_example_that_does_not_hold_is_a_failing_test() {
    let fixture = fixture("fixture-doctest");
    let library = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&library).expect("the library");
    std::fs::write(
        &library,
        source.replace(
            "/// assert_eq!(fixture_doctest::double(2), 4);",
            "/// assert_eq!(fixture_doctest::double(2), 5);",
        ),
    )
    .expect("an example that does not hold");

    verify(&fixture, &[]);
    let report = document(&fixture);

    assert_eq!(
        parsed(&fixture).verdict(),
        Verdict::Defect,
        "documentation that lies about the library is a defect in the library or in the \
         documentation, and either way it is not something to measure mutations against"
    );
    let failing: Vec<&str> = report["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["kind"] == "failing-test")
        .filter_map(|finding| finding["subject"].as_str())
        .collect();
    assert!(
        failing.iter().any(|subject| subject.contains("/doc/")),
        "the finding names the documentation as the test that failed: {failing:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_library_that_documents_no_example_is_not_a_target_that_ran_nothing() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let names: Vec<&str> = report["builds"][0]["parts"][0]["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter_map(|target| target["name"].as_str())
        .collect();
    assert!(
        !names.iter().any(|name| name.contains("/doc/")),
        "one target per library is one target per library that documents something; \
         reporting a missing target would raise a finding about documentation nobody \
         wrote: {names:?}"
    );
    let stated: Vec<&str> = report["builds"][0]["parts"][0]["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        !stated.iter().any(|name| name.contains("doctests")),
        "and the routing says nothing about documented examples either, because there \
         are none to route: {stated:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutation_only_another_process_reaches_is_settled_by_the_suite_that_reaches_it() {
    let fixture = fixture("fixture-subprocess");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let mutants = report["builds"][0]["parts"][0]["mutants"]
        .as_array()
        .expect("mutants");
    let outcomes: Vec<&str> = mutants
        .iter()
        .filter_map(|mutant| mutant["decision"]["outcome"].as_str())
        .collect();
    assert!(
        !outcomes.contains(&"unreached"),
        "nothing links the library into the test binary, so no region of it is \
         instrumented and the coverage is silent rather than empty: {outcomes:?}"
    );
    assert_eq!(
        outcomes.iter().filter(|one| **one == "killed").count(),
        6,
        "the test runs the binary, the binary calls the library, and every mutation but \
         the boundary makes it say something else: {outcomes:?}"
    );
    assert_eq!(
        parsed(&fixture).verdict(),
        Verdict::Insufficient,
        "one mutation nothing noticed is a gap in the suite, and neither of the other \
         two is a test that fails on the original code"
    );

    let killers: Vec<&str> = mutants
        .iter()
        .filter_map(|mutant| mutant["decision"]["killed_by"].as_str())
        .collect();
    assert!(
        killers
            .iter()
            .all(|by| by.contains("through_the_binary") || by.contains("package-suite")),
        "a kill the suite found still names the target that found it: {killers:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutation_the_compiler_renders_identically_is_not_a_gap_in_the_tests() {
    let fixture = fixture("fixture-equivalent");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "# SPDX-FileCopyrightText: 2026 njutest contributors\n\
         # SPDX-License-Identifier: MIT OR Apache-2.0\n\n\
         [mutation]\n\
         equivalence = true\n",
    )
    .expect("the configuration");

    verify(&fixture, &[]);
    let report = document(&fixture);

    let by_rule = |rule: &str| -> String {
        report["builds"][0]["parts"][0]["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .find(|mutant| mutant["rule"] == rule)
            .and_then(|mutant| mutant["decision"]["outcome"].as_str())
            .unwrap_or("none")
            .to_owned()
    };
    assert_eq!(
        by_rule("mul-to-div"),
        "killed",
        "`n * 2` and `n / 2` are not the same instructions"
    );
    if !njutest_devkit::reproducible::builds_the_same_twice() {
        assert_eq!(
            report["builds"][0]["parts"][0]["accounting"]["mutants"]["equivalent"], 0,
            "a machine that renders one unchanged tree two ways establishes nothing here, \
             and a run that took its own difference for the mutation's would remove a \
             finding nobody proved: {}",
            report["builds"][0]["parts"][0]["mutants"]
        );
        return;
    }
    assert_eq!(
        by_rule("add-to-sub"),
        "equivalent",
        "`n + 0` and `n - 0` are the same instructions at opt-level 2, and the tests ran \
         the position: {}",
        report["builds"][0]["parts"][0]["mutants"]
    );
    assert_eq!(
        report["builds"][0]["parts"][0]["accounting"]["mutants"]["equivalent"], 1,
        "a column of its own, because a reader who cannot tell 'nobody noticed' from \
         'nobody could have' cannot act on either"
    );

    let findings: Vec<&str> = report["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["subject"].as_str())
        .collect();
    assert!(
        !findings.is_empty(),
        "the mutation of `halved` keeps its finding: nothing calls it, the linker drops it, \
         and identical artifacts then say the code is untested: {findings:?}"
    );
}

/// Every mutant the latest run judged, by identity.
#[cfg(unix)]
fn judged(fixture: &Fixture) -> BTreeSet<String> {
    let document = document(fixture);
    let build = &document["builds"][0];
    let sources: Vec<&serde_json::Value> = build["parts"]
        .as_array()
        .map_or_else(|| vec![&build["source"]], |parts| parts.iter().collect());
    sources
        .iter()
        .filter_map(|source| source["mutants"].as_array())
        .flat_map(|mutants| mutants.iter())
        .filter_map(|one| one["id"].as_str().map(ToOwned::to_owned))
        .collect()
}

#[cfg(unix)]
#[test]
fn two_parts_of_one_catalog_judge_every_mutant_between_them_and_none_twice() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let whole = judged(&fixture);
    assert!(whole.len() >= 2, "a catalog worth splitting: {whole:?}");

    let first = verify(&fixture, &["--shard", "1/2"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&first.stderr)
    );
    let one = judged(&fixture);

    let second = verify(&fixture, &["--shard", "2/2"]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&second.stderr)
    );
    let two = judged(&fixture);

    assert!(
        one.is_disjoint(&two),
        "a mutant belongs to one part, so no execution is paid for twice: {:?}",
        one.intersection(&two).collect::<Vec<&String>>()
    );
    assert_eq!(
        one.union(&two).cloned().collect::<BTreeSet<String>>(),
        whole,
        "and between them the parts judge every mutant the whole would: dividing the \
         work is not a budget only if none of it goes missing"
    );
    assert!(!one.is_empty() && !two.is_empty(), "{one:?} {two:?}");
}

#[cfg(unix)]
#[test]
fn a_part_of_a_catalog_does_not_claim_what_the_whole_would() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    assert_eq!(parsed(&fixture).verdict(), Verdict::Assured);

    assert_eq!(verify(&fixture, &["--shard", "1/2"]).status.code(), Some(0));
    let part = document(&fixture);
    assert_eq!(
        parsed(&fixture).verdict(),
        Verdict::Partial,
        "a run that judged half a catalog has assured nothing: the mutations it did not \
         judge are not mutations nothing noticed, they are mutations nobody put to a \
         test: {part}"
    );
    assert_eq!(
        part["shard"],
        serde_json::json!({ "index": 1, "of": 2 }),
        "and it says which part it was: {part}"
    );
}

#[test]
fn a_part_that_is_not_a_part_of_anything_is_refused_before_anything_is_built() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &["--shard", "3/2"]);
    assert_eq!(output.status.code(), Some(3));
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(said.contains("K runs from 1 to N"), "{said}");
    assert!(
        !fixture
            .root
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs")
            .exists(),
        "a refusal before the first build writes no report"
    );
}

#[cfg(unix)]
#[test]
fn the_parts_of_one_catalog_merge_into_the_verdict_neither_of_them_could_say() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &["--shard", "1/2"]).status.code(), Some(0));
    let one = latest(&fixture);
    assert_eq!(verify(&fixture, &["--shard", "2/2"]).status.code(), Some(0));
    let two = latest(&fixture);
    assert_ne!(one, two, "two runs, two reports");

    let merged = asked(
        &of(&fixture.root, &[]),
        &[
            "merge",
            one.to_str().expect("test protocol paths are UTF-8"),
            two.to_str().expect("test protocol paths are UTF-8"),
        ],
    );
    assert_eq!(
        merged.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&merged.stderr)
    );

    let said = njutest_devkit::process::strict_utf8(&merged.stdout);
    let whole: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&said).expect("the whole is JSON");
    assert_eq!(
        njutest::report::json::parse(&said)
            .expect("the whole reads back")
            .verdict(),
        Verdict::Assured,
        "neither part could say this and the two of them together can: {whole}"
    );
    assert_eq!(whole["report"]["scope"]["shard"], serde_json::Value::Null);
    let held = whole["report"]["builds"][0]["parts"]
        .as_array()
        .expect("the merged parts");
    assert_eq!(
        held.iter()
            .map(|part| part["accounting"]["mutants"]["cataloged"]
                .as_u64()
                .unwrap_or_default())
            .sum::<u64>(),
        4,
        "and it holds every mutant the parts judged between them: {whole}"
    );
    assert_eq!(
        held.iter()
            .map(|part| part["accounting"]["mutants"]["killed"]
                .as_u64()
                .unwrap_or_default())
            .sum::<u64>(),
        4
    );
}

#[cfg(unix)]
#[test]
fn a_run_that_was_given_a_package_says_it_looked_at_that_one() {
    let fixture = fixture("fixture-workspace");

    let output = verify(&fixture, &["--package", "fixture-core", "--ui=plain"]);
    let stdout = njutest_devkit::process::strict_utf8(&output.stdout);
    let document = document(&fixture);

    assert_eq!(
        document["run_kind"], "scoped",
        "a run that was given one package of two looked at one package of two, and the \
         kind is what the report's own audit reads to decide which assurance it may \
         claim: {stdout}"
    );
    assert_ne!(
        parsed(&fixture).verdict(),
        Verdict::Assured,
        "so the whole-workspace assurance is not one this run can reach. Naming the \
         wider claim after the narrower look is the single sentence a reader would act \
         on hardest and could not check: {stdout}"
    );
    let packages: Vec<&str> = document["builds"][0]["parts"][0]["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter_map(|target| target["package"].as_str())
        .collect();
    assert!(
        packages.iter().all(|package| *package == "fixture-core"),
        "and it ran the package it was given and no other: {packages:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_briefs_whatever_asked_for_it_rather_than_whatever_it_guessed() {
    let fixture = fixture("fixture-baseline");
    let briefed = verify(&fixture, &["--format", "agent"]);
    let text = njutest_devkit::process::strict_utf8(&briefed.stdout);
    assert!(
        text.starts_with("# njutest:"),
        "a thing without a screen runs `njutest verify` like everybody else, and what it \
         gets back is decided by whether stdout happened to be a terminal. The briefing \
         exists and nothing that reads it can ask for it: {text}"
    );
    assert!(
        text.contains("njutest replay"),
        "and the briefing is the one surface that says what to do after the change, which \
         is the whole of what it is for: {text}"
    );

    let streamed = verify(&fixture, &["--format", "lines"]);
    let text = njutest_devkit::process::strict_utf8(&streamed.stdout);
    assert!(
        text.lines().any(|line| line.starts_with("VERDICT\t")),
        "and a program that wants the stream can still say so, whatever it is writing \
         into: {text}"
    );

    let whole = verify(&fixture, &["--format", "json"]);
    let text = njutest_devkit::process::strict_utf8(&whole.stdout);
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(&text)
        .unwrap_or_else(|error| panic!("{error}: {text}"));
    assert_eq!(
        document["report"]["schema"], "njutest-assurance-report-v1",
        "and the document is the one on disk rather than the report serialized a second \
         time, because the second time is a second answer to compare against the first"
    );
}

#[cfg(unix)]
#[test]
fn a_run_that_reads_an_answer_back_says_it_the_way_a_run_that_established_one_does() {
    let fixture = fixture("fixture-baseline");
    let first = verify(&fixture, &["--format", "agent"]);
    let second = verify(&fixture, &["--format", "agent"]);
    let (established, read_back) = (
        njutest_devkit::process::strict_utf8(&first.stdout),
        njutest_devkit::process::strict_utf8(&second.stdout),
    );
    assert!(
        read_back.starts_with("# njutest:"),
        "the second run of the same inputs answers from the store, and until now it \
         answered in the one shape the reading-back path happened to be written with, \
         whatever the reader asked for or the terminal said. A cached answer that is a \
         different answer is a reason not to cache: {read_back}"
    );
    assert_eq!(
        established
            .lines()
            .filter(|line| line.starts_with("### "))
            .count(),
        read_back
            .lines()
            .filter(|line| line.starts_with("### "))
            .count(),
        "and it names the same places:\n{established}\n---\n{read_back}"
    );
}
