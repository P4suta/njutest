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
    dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-crashes-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    Fixture { root, dir }
}

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
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
    let events = root
        .parent()
        .expect("the fixture parent")
        .join("clock-events");
    std::fs::create_dir_all(&events).expect("clock events");
    let mut vars: rust_mutants::vars::Variables =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect();
    vars.set("NJUTEST_TEST_CLOCK", events.as_os_str());
    Environment {
        cache_directory: root.join(".cache"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new().with_clock(rust_mutants::runner::Clock::events(events)),
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

fn decisions(part: &serde_json::Value) -> Vec<(u64, String)> {
    let mut decided: Vec<(u64, String)> = part["crashes"]
        .as_array()
        .expect("a list of crashes")
        .iter()
        .map(|crash| {
            (
                crash["position"]["line"]
                    .as_u64()
                    .expect("every site, put or not, says where it is"),
                crash["decision"]["decision"]
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
fn a_count_written_in_pieces_is_torn_by_a_stop_and_one_moved_into_place_is_not() {
    let fixture = fixture("fixture-durable");
    let output = verify(&fixture, &["--crashes"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![
            (31, "corrupt".to_owned()),
            (32, "corrupt".to_owned()),
            (33, "restarted".to_owned()),
            (43, "restarted".to_owned()),
            (44, "restarted".to_owned()),
        ],
        "a truncated file or a bare `count=` is one the next run cannot read, and a whole count \
         or one moved into place always is: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        named(&part, "findings", "kind", "corrupt-after-crash").len(),
        2,
        "each torn write is a finding at its call: {part}"
    );
    assert_eq!(
        output.status.code(),
        Some(njutest::cli::EXIT_DEFECT.into()),
        "a program that cannot start over what it wrote has a defect: {part}"
    );
}

#[test]
fn a_run_not_asked_for_crashes_stops_nothing() {
    let fixture = fixture("fixture-durable");
    let output = verify(&fixture, &[]);
    let part = part(&fixture);
    assert_eq!(
        part["crashes"],
        serde_json::json!([]),
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
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

/// Every phase the recording starts inside the stage `stage`, in order.
fn phases_inside(events: &[njutest::trace::Event], stage: &str) -> Vec<String> {
    let mut inside = false;
    let mut phases = Vec::new();
    for event in events {
        if let njutest::trace::Payload::PhaseStart { phase } = &event.payload {
            if phase.name == stage {
                inside = true;
            } else if inside {
                phases.push(phase.name.clone());
            }
        }
        if let njutest::trace::Payload::PhaseEnd { phase } = &event.payload
            && phase.name == stage
        {
            inside = false;
        }
    }
    phases
}

#[test]
fn a_tree_that_writes_nothing_is_known_to_from_discovery_before_anything_is_built() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--crashes", "--trace"]);
    let part = part(&fixture);
    assert_eq!(
        named(&part, "limitations", "name", "crash-no-site").len(),
        1,
        "no measured file of it calls anything that writes: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        named(&part, "findings", "subject", "crash-baseline-not-measured").len(),
        0,
        "a tree with nothing to ask cannot leave the crashes unmeasured: {part}"
    );
    assert_eq!(
        phases_inside(&recording(&fixture), "crashes"),
        Vec::<String>::new(),
        "discovery alone says there is nothing to stop after, so no crash build is made and no \
         baseline runs"
    );
}

/// A test that passes only where the home holds the marker the given home was made with, and then keeps a setting under the home and reads it back.
const WRITES_THE_GIVEN_HOME: &str = "// SPDX-FileCopyrightText: 2026 njutest contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n\n//! Keeps a setting beside a marker only the given home holds.\n\n#[test]\nfn a_setting_kept_beside_the_marker_is_the_setting_recalled() {\n    let path = fixture_home::setting_path().expect(\"a home\");\n    assert!(path.with_file_name(\"marker\").exists(), \"only the given home holds the marker\");\n    fixture_home::remember(\"kept\").expect(\"the setting is written\");\n    assert_eq!(fixture_home::recall().expect(\"the setting is read\"), \"kept\");\n}\n";

#[test]
fn a_crash_that_writes_only_to_the_home_the_run_was_given_is_not_read_as_unshared() {
    let fixture = fixture("fixture-home");
    std::fs::remove_file(fixture.root.join("tests/writes.rs")).expect("the confined test goes");
    std::fs::write(fixture.root.join("tests/given.rs"), WRITES_THE_GIVEN_HOME)
        .expect("a test that writes under the given home");
    let home = fixture.dir.path().join("given-home");
    std::fs::create_dir_all(home.join(".fixture-home")).expect("the given home");
    std::fs::write(home.join(".fixture-home/marker"), "").expect("the marker");
    let mut given = environment(&fixture.root);
    for change in njutest_devkit::paths::given_home(&home) {
        match change {
            njutest_devkit::paths::Given::Set(name, value) => given.vars.set(name, value),
            njutest_devkit::paths::Given::Removed(name) => given.vars.remove(name),
        }
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--crashes"]
            .into_iter()
            .map(OsString::from),
        &given,
        &mut out,
        &mut err,
    );
    let output = njutest_devkit::process::answered(code, out, err);
    let part = part(&fixture);
    assert!(
        part["crashes"]
            .as_array()
            .expect("a list of crashes")
            .iter()
            .all(|crash| crash["decision"]["decision"] != "unshared"),
        "what the stop wrote is under the home the next run is given too, so it is not a stop \
         that left nothing for the next run: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        decisions(&part),
        vec![(21, "undecided".to_owned()), (23, "undecided".to_owned())],
        "a target that runs with the given home is not asked which of its tests reaches a call, \
         so each stop is undecided on it: {part}"
    );
    for crash in part["crashes"].as_array().expect("a list of crashes") {
        assert_eq!(
            (
                crash["decision"]["on"].as_str(),
                crash["decision"]["why"]
                    .as_str()
                    .is_some_and(|why| why.starts_with("which of their tests reaches the call"))
            ),
            (Some("fixture-home/test/given"), true),
            "the stop is undecided because the target's reach is not measured in the home it \
             runs with, not because anything ran: {crash}"
        );
    }
}

#[test]
fn a_program_that_ends_with_the_stop_status_itself_is_not_decided_on_it() {
    let fixture = fixture("fixture-stop-status");
    let output = verify(&fixture, &["--crashes"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![(16, "undecided".to_owned())],
        "the status is the stop's, and no notice says the runtime made it, so the next run is \
         never asked: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}

/// Every call that writes in fixture-durable-calls, by its line, the call, and what a crash just after it comes to.
const CALLS: [(u64, &str, &str); 15] = [
    (12, "fs::write of the copy beside", "restarted"),
    (13, "fs::copy into place", "restarted"),
    (14, "fs::remove_file of the copy beside", "restarted"),
    (19, "File::create", "corrupt"),
    (20, "write_all", "restarted"),
    (21, "sync_data", "restarted"),
    (22, "sync_all", "restarted"),
    (33, "set_len", "corrupt"),
    (34, "write_all after set_len", "restarted"),
    (39, "File::create under a buffer", "corrupt"),
    (40, "write_all into the buffer", "corrupt"),
    (41, "flush", "restarted"),
    (47, "fs::write beside a guard", "restarted"),
    (55, "the guard's own fs::write", "restarted"),
    (61, "fs::write a child process reaches", "undecided"),
];

/// The line of the one call no test of its own process reaches, which only a process a test starts does.
const REACHED_BY_A_CHILD: u64 = 61;

/// Each crash site of `part` by its line: the decision, the record's `sealed`, and the decision's `why`, `left` and `on`.
fn crashed(part: &serde_json::Value) -> Vec<(u64, String, bool, serde_json::Value)> {
    let mut sites: Vec<(u64, String, bool, serde_json::Value)> = part["crashes"]
        .as_array()
        .expect("a list of crashes")
        .iter()
        .map(|crash| {
            (
                crash["position"]["line"]
                    .as_u64()
                    .expect("every site says where it is"),
                crash["decision"]["decision"]
                    .as_str()
                    .expect("a decision")
                    .to_owned(),
                crash["sealed"]
                    .as_bool()
                    .expect("every site says whether it was sealed"),
                crash["decision"].clone(),
            )
        })
        .collect();
    sites.sort_by_key(|(line, ..)| *line);
    sites
}

/// Every crash run the recording holds, as its crash, stage and whether it was sealed.
fn crash_runs(events: &[njutest::trace::Event]) -> Vec<(String, String, bool)> {
    events
        .iter()
        .filter_map(|event| {
            if let njutest::trace::Payload::CrashExec { crash } = &event.payload {
                Some((crash.crash.clone(), crash.stage.clone(), crash.sealed))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_crash_after_every_call_that_writes_is_decided_in_one_sealed_round() {
    let fixture = fixture("fixture-durable-calls");
    let output = verify(&fixture, &["--crashes", "--trace"]);
    let part = part(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    let sites = crashed(&part);
    for ((line, call, expected), (at, decision, sealed, recorded)) in CALLS.iter().zip(&sites) {
        assert_eq!(
            (at, decision.as_str(), *sealed),
            (line, *expected, *line != REACHED_BY_A_CHILD),
            "{call}: a crash just after it is {expected}, decided sealed wherever the test's \
             control reached it sealed, and natively where only a process the test started \
             reaches it: {recorded}\n{said}"
        );
    }
    assert_eq!(sites.len(), CALLS.len(), "one site per call: {part}");
    let child = sites
        .iter()
        .find(|(line, ..)| *line == REACHED_BY_A_CHILD)
        .map(|(.., recorded)| recorded["why"].as_str().unwrap_or_default().to_owned());
    assert!(
        child
            .as_deref()
            .is_some_and(|why| why.starts_with("a process the test started stopped at the call")),
        "a stop in the process the test started is not one the next run's test made: {child:?}"
    );
    let runs = crash_runs(&recording(&fixture));
    assert!(
        !runs
            .iter()
            .any(|(_, stage, sealed)| *sealed && stage == "fresh"),
        "a sealed crash is decided in one round, with no fresh run to confirm it: {runs:?}"
    );
    for (crash, ..) in runs.iter().filter(|(_, _, sealed)| *sealed) {
        let stops = runs
            .iter()
            .filter(|(held, stage, _)| held == crash && stage == "crash")
            .count();
        assert_eq!(
            stops, 1,
            "{crash}: one sealed stop decides it, since the same instance comes out the same"
        );
    }
}

#[test]
fn a_sealed_crash_leaves_nothing_the_program_would_have_written_after_the_call() {
    let fixture = fixture("fixture-durable-calls");
    let output = verify(&fixture, &["--crashes"]);
    let part = part(&fixture);
    let left = |line: u64| {
        crashed(&part)
            .into_iter()
            .find(|(at, _, sealed, _)| *at == line && *sealed)
            .map(|(.., recorded)| recorded["left"].clone())
    };
    assert_eq!(
        left(47),
        Some(serde_json::json!([
            "fixture-durable-calls/",
            "fixture-durable-calls/guarded"
        ])),
        "the host halted the instance in the call that published the stop, so the guard that \
         says it was dropped never ran: what the next instance starts over is what the call \
         wrote and nothing after it\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        left(55),
        Some(serde_json::json!([
            "fixture-durable-calls/",
            "fixture-durable-calls/guarded",
            "fixture-durable-calls/guarded.dropped"
        ])),
        "a stop just after the guard's own write left what the guard wrote"
    );
    let buffered = crashed(&part)
        .into_iter()
        .find(|(at, ..)| *at == 40)
        .map(|(_, decision, sealed, _)| (decision, sealed));
    assert_eq!(
        buffered,
        Some(("corrupt".to_owned(), true)),
        "bytes a buffer held when the call returned never reach the file: no destructor \
         flushes them after the halt, and the next instance reads an empty count"
    );
}

#[test]
fn a_crash_after_every_call_that_writes_is_decided_natively_where_nothing_is_sealed() {
    let fixture = fixture("fixture-durable-calls");
    let output = verify(&fixture, &["--crashes", "--no-seal"]);
    let part = part(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    let sites = crashed(&part);
    for ((line, call, expected), (at, decision, sealed, recorded)) in CALLS.iter().zip(&sites) {
        assert_eq!(
            (at, decision.as_str(), *sealed),
            (line, *expected, false),
            "{call}: a native run comes to what a sealed one does, in three rounds: \
             {recorded}\n{said}"
        );
    }
    assert_eq!(sites.len(), CALLS.len(), "one site per call: {part}");
}
