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
    Environment {
        cache_directory: root.join(".cache"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect(),
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
