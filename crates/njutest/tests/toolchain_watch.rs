// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A watch against a real workspace: the round it runs before anything changes, and the verdict it carries out of it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
#![cfg_attr(
    unix,
    expect(
        clippy::disallowed_methods,
        clippy::panic,
        clippy::too_many_lines,
        reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table, and what these permit is what a test that reads a published report needs, which this platform cannot publish: those tests are behind cfg(unix) one by one, so what their shapes permit is behind it too"
    )
)]

use std::ffi::OsString;
use std::io::Write;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::Duration;

use njutest::cli::{EXIT_ERROR, Environment};
use njutest_devkit::fixture::copy_tree;
use njutest_devkit::thread::JoinedThread;
use rust_mutants::runner::Cancel;

/// The longest this test will wait for a round that is not coming.
const LONGEST: Duration = Duration::from_secs(10);

/// One of the watch's two streams, stopping it once the round has been.
struct Stopping<'a> {
    cancel: &'a Cancel,
    stops: bool,
    said: String,
}

impl<'a> Stopping<'a> {
    const fn watching(cancel: &'a Cancel) -> Self {
        Self {
            cancel,
            stops: false,
            said: String::new(),
        }
    }

    const fn complaining(cancel: &'a Cancel) -> Self {
        Self {
            cancel,
            stops: true,
            said: String::new(),
        }
    }
}

impl Write for Stopping<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.said
            .push_str(&njutest_devkit::process::strict_utf8(buffer));
        if self.stops || self.said.contains("waiting\tfor the next change") {
            self.cancel.cancel();
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The machine a watch is given: this tree, that scratch, and nothing of the outside.
fn working_in(root: &std::path::Path, scratch: std::path::PathBuf) -> Environment {
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.to_owned(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

#[test]
fn a_watch_verifies_the_tree_as_it_stands_and_carries_that_round_s_verdict() {
    let root = tempfile::Builder::new()
        .prefix("njutest-watch-")
        .tempdir()
        .expect("a temporary directory");
    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname = \"\"\n")
        .expect("a manifest cargo will refuse");
    let scratch = root.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a scratch directory");

    let environment = working_in(root.path(), scratch);

    let watchdog = environment.cancel.clone();
    let (done, waited) = sync_channel::<()>(1);
    let bound = JoinedThread::launch(move || {
        let expired = waited.recv_timeout(LONGEST) == Err(RecvTimeoutError::Timeout);
        if expired {
            watchdog.cancel();
        }
        expired
    });

    let mut output = Stopping::watching(&environment.cancel);
    let mut complaints = Stopping::complaining(&environment.cancel);
    let code = njutest::run_from(
        [
            "njutest",
            "watch",
            "--poll-ms",
            "20",
            "--offline",
            "--locked",
        ]
        .map(OsString::from),
        &environment,
        &mut output,
        &mut complaints,
    );
    drop(done);
    let expired = bound.join().expect("the bound");

    let said = output.said;
    let complained = complaints.said;
    assert!(
        !expired,
        "the round never wrote a second line, so this was stopped by its own clock \
         rather than by the watch: {said}\n{complained}"
    );
    let stages: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    assert_eq!(
        stages.first(),
        Some(&"open"),
        "the round names each stage as it starts, and this test is one that drives the \
         runner in this process rather than starting it in another, so it is the one a \
         measurement can attribute the rule to: {complained}"
    );
    assert!(
        said.starts_with("watching\t"),
        "a person who starts a watch is told what it is on before it does anything: \
         {said}"
    );
    assert!(
        said.contains("waiting\tfor the next change"),
        "a watch that has answered for the tree in front of it says it is waiting, \
         because a round that ends in silence reads as one that is still running: \
         {said}\n{complained}"
    );
    assert_eq!(
        code, EXIT_ERROR,
        "and the verdict it carries out is the round's own: this tree is one cargo \
         refuses, so the round is an error, and a watch that reported success on it \
         would be a green terminal for a workspace nobody could measure: \
         {said}\n{complained}"
    );
}

#[test]
fn a_run_with_nowhere_to_work_stops_before_it_says_it_looked() {
    let root = tempfile::Builder::new()
        .prefix("njutest-nowhere-")
        .tempdir()
        .expect("a temporary directory");
    let occupied = root.path().join("occupied");
    std::fs::write(&occupied, "not a directory").expect("a file where a scratch goes");

    let environment = Environment {
        cache_directory: root.path().to_owned(),
        working_directory: root.path().to_owned(),
        temp_directory: occupied,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: njutest_devkit::paths::environment_for_a_run(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--trace"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );

    let complained = njutest_devkit::process::strict_utf8(&complaints);
    assert_eq!(
        code, EXIT_ERROR,
        "a run with nowhere to put what it builds has not verified anything, and \
         carrying on would put every later phase in a directory nobody owns: \
         {complained}"
    );
    assert!(
        complained.contains("occupied"),
        "and it names the place it could not use, because that is the one thing a \
         person can change: {complained}"
    );
    assert!(
        !njutest_devkit::process::strict_utf8(&said).contains("VERDICT"),
        "a run that stopped here reached no verdict, and printing one would be a claim \
         about a workspace it never opened"
    );

    let recording = std::fs::read_dir(root.path().join(".njutest/trace"))
        .expect("the trace directory")
        .map(|entry| entry.expect("every trace entry is readable"))
        .map(|entry| entry.path().join(njutest::trace::FILE_NAME))
        .next()
        .expect("one recording");
    let events = njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back");
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
        "and the recording says where it got to. A run that stopped early is exactly \
         when somebody reads one, and a recording whose first stage is missing leaves \
         them no way to say how far it went: {phases:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_told_where_to_look_for_a_toolchain_looks_there_and_nowhere_else() {
    let root = tempfile::Builder::new()
        .prefix("njutest-nocargo-")
        .tempdir()
        .expect("a temporary directory");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"told\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("a manifest");
    std::fs::create_dir_all(root.path().join("src")).expect("a source directory");
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn one() -> u8 {\n    1\n}\n",
    )
    .expect("a library the second run can verify");
    std::fs::write(
        root.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"told\"\nversion = \"0.0.0\"\n",
    )
    .expect("a lock a locked run may use");
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).expect("a directory with no toolchain in it");
    let scratch = root.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let environment = Environment {
        cache_directory: root.path().to_owned(),
        working_directory: root.path().to_owned(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: vec![(
            OsString::from("PATH"),
            OsString::from(empty.display().to_string()),
        )],
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );

    let complained = njutest_devkit::process::strict_utf8(&complaints);
    assert_eq!(
        code, EXIT_ERROR,
        "the only place this run was told to look for a toolchain has none in it, and \
         finding one somewhere else would build the workspace with a compiler nobody \
         named: {complained}"
    );
    assert!(
        complained.contains("cargo"),
        "and it says what it could not find: {complained}"
    );

    let scratch = root.path().join("scratch-again");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let told = Environment {
        cache_directory: root.path().to_owned(),
        working_directory: root.path().to_owned(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: std::env::vars_os()
            .filter(|(name, _)| {
                njutest_devkit::paths::same_name(name, std::ffi::OsStr::new("PATH"))
            })
            .collect(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked"].map(OsString::from),
        &told,
        &mut said,
        &mut complaints,
    );
    assert_ne!(
        code, EXIT_ERROR,
        "the configured search path contains the toolchain"
    );

    let complained = njutest_devkit::process::strict_utf8(&complaints);
    assert!(
        !complained.contains("no search path"),
        "and a run whose environment does name a place to look is told that place: \
         reading some other variable, or reading none, would have it report that nobody \
         said where to look while somebody had: {complained}"
    );
}

/// Every stage a whole run goes through names itself, to a person and to a recording.
#[cfg(unix)]
fn stages_of(complained: &str, root: &std::path::Path) {
    let said: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let events = events_of(root);
    let problems = njutest::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let recorded: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .phase_start()
                .map(|phase| phase.name.as_str())
        })
        .collect();

    let routed: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .route()
                .map(|route| route.mutant.as_str())
        })
        .collect();
    assert!(
        !routed.is_empty(),
        "every mutation a run judged says which targets could have noticed it and which \
         a proof removed, or the layer that removed them left no trace of having worked"
    );
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.payload.type_name())
        .collect();
    assert!(
        kinds.contains(&"mutant-exec"),
        "a run says what it started, or the minutes it spent belong to nothing a reader \
         can name: {kinds:?}"
    );
    let progressed: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .progress()
                .map(|progress| progress.subject.as_str())
        })
        .collect();
    assert!(
        routed.iter().any(|mutant| progressed.contains(mutant)),
        "and it says how far it has got through the phase that takes the time, to the \
         recording and not only to the terminal in front of somebody: a run judging \
         mutations for half an hour whose recording says nothing between the phase \
         starting and the phase ending cannot be told from one that stopped. Progress \
         reached {progressed:?}, mutations {routed:?}"
    );

    judged(&events);

    for named in ["open", "soundness", "baseline", "mutation"] {
        assert!(
            said.contains(&named),
            "every stage a whole run goes through names itself as it starts, or the \
             minutes between two lines belong to nothing anybody can name: {said:?}"
        );
        assert!(
            recorded.contains(&named),
            "and says the same to a recording, which is what somebody reads when the \
             run is not in front of them: {recorded:?}"
        );
    }
}

/// What a run records about each mutation it judged, which is the only account of that phase a person has afterwards.
#[cfg(unix)]
fn judged(events: &[njutest::trace::Event]) {
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .phase_start()
                .map(|phase| phase.name.as_str())
        })
        .collect();
    assert!(
        phases.contains(&"mutation-judge"),
        "the phase that takes the time names itself in the recording, or the minutes \
         between two events belong to nothing anybody can name: {phases:?}"
    );

    let routes: Vec<&njutest::trace::RouteRecord> = events
        .iter()
        .filter_map(|event| njutest::testkit::payload::of(&event.payload).route())
        .collect();
    let placed = routes
        .iter()
        .find(|route| route.granularity == rust_mutants::session::Granularity::Block)
        .expect("a mutation the measurement placed");
    assert!(
        placed.fallback.is_none() && !placed.reaching.is_empty(),
        "a route the measurement decided says so by naming the targets it decided on \
         and nothing that widened it: a route that named neither is one a reader cannot \
         tell from a measurement that said nothing: {placed:?}"
    );

    let probes: Vec<&njutest::trace::ProbeExecRecord> = events
        .iter()
        .filter_map(|event| njutest::testkit::payload::of(&event.payload).probe_exec())
        .collect();
    let seen = probes
        .iter()
        .find(|probe| probe.outcome == "measured")
        .expect("a target the infection layer measured");
    assert!(
        seen.infected.is_some() && !seen.target.is_empty(),
        "a target the layer measured carries a count, because none is not zero: a \
         reader who cannot tell \"infected nothing\" from \"was never asked\" cannot tell \
         a discharge resting on evidence from one resting on silence: {seen:?}"
    );

    let execs = executions(events);
    let started = execs.first().expect("a mutation this run started");
    assert!(
        !started.mutant.is_empty() && !started.target.is_empty() && !started.outcome.is_empty(),
        "every execution says which mutation it was, what it ran against, and what came \
         of it, or the count of executions is a number about nothing: {started:?}"
    );
    assert!(
        !execs.iter().any(|exec| exec.alone),
        "and none of them was given the machine to itself, because none of them ran out \
         of time: a run that says it did that without a budget expiring is one whose \
         account of where the time went is wrong: {execs:?}"
    );
    let against: std::collections::BTreeSet<&str> =
        execs.iter().map(|exec| exec.target.as_str()).collect();
    assert!(
        !against.contains("package-suite"),
        "and a mutation the measurement placed is put to the targets it placed rather \
         than to the package, which is the whole of what routing buys: {against:?}"
    );
}

/// The report the latest run of `root` wrote, found the way a person finds it.
fn report_of(root: &std::path::Path) -> serde_json::Value {
    let run = njutest::app::reports::pointed_at(root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let directory = root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str());
    njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(directory.join(njutest::app::reports::DOCUMENT_NAME))
            .expect("the report"),
    )
    .expect("the report is JSON")
}

/// The document the latest run of `root` wrote, found without the index: a run that judged one part of a catalog writes a document and no index naming it.
#[cfg(unix)]
fn latest_report_of(root: &std::path::Path) -> serde_json::Value {
    let mut runs: Vec<std::path::PathBuf> = std::fs::read_dir(
        root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs"),
    )
    .expect("the runs directory")
    .map(|entry| entry.expect("every run entry is readable"))
    .map(|entry| entry.path())
    .collect();
    runs.sort();
    njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            runs.last()
                .expect("one run")
                .join(njutest::app::reports::DOCUMENT_NAME),
        )
        .expect("the report"),
    )
    .expect("the report is JSON")
}

/// The one part a single-build run measured, where its counts, rows, and timings live.
fn part_of(report: &serde_json::Value) -> &serde_json::Value {
    &report["report"]["builds"][0]["parts"][0]
}

/// The verdict the latest run of `root` reached, read back the way a reader does: the document no longer carries it.
#[cfg(unix)]
fn verdict_of(root: &std::path::Path) -> njutest::report::Verdict {
    let run = njutest::app::reports::pointed_at(root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let text = std::fs::read_to_string(
        root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs")
            .join(run.as_str())
            .join(njutest::app::reports::DOCUMENT_NAME),
    )
    .expect("the report");
    njutest::report::json::parse(&text)
        .expect("the report reads back")
        .verdict()
}

/// What a run whose every measurement was slow exactly once concludes.
#[cfg(unix)]
fn once_slow(report: &serde_json::Value) {
    assert_eq!(
        (
            part_of(report)["accounting"]["mutants"]["killed"].as_u64(),
            part_of(report)["accounting"]["mutants"]["survived"].as_u64(),
        ),
        (Some(9), Some(1)),
        "every mutation but one is noticed here, and by the second measurement rather \
         than the first. The one nothing noticed ran and was not noticed, which is a \
         different fact from one nothing could decide: {report}"
    );
    assert_eq!(
        (
            part_of(report)["accounting"]["mutants"]["waited"].as_u64(),
            part_of(report)["accounting"]["mutants"]["step_limit_reached"].as_u64(),
        ),
        (Some(0), Some(0)),
        "a bound reached once and not again is not a mutation this machine stopped \
         waiting for, and no verified step boundary was crossed: every measurement was slow the first \
         time and quick the second. A run that reported them either way would hand a \
         person findings caused by whatever else the machine was doing: {report}"
    );
}

/// Every stage the latest recording under `root` names, in the order it named them.
#[cfg(unix)]
fn recorded_stages(root: &std::path::Path) -> Vec<String> {
    events_of(root)
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .phase_start()
                .map(|phase| phase.name.clone())
        })
        .collect()
}

/// What each route of the latest recording said about the answer an earlier run had left: the run it took, and why it took none.
#[cfg(unix)]
fn consulted(root: &std::path::Path) -> Vec<(Option<String>, Option<String>)> {
    events_of(root)
        .iter()
        .filter_map(|event| {
            njutest::testkit::payload::of(&event.payload)
                .route()
                .map(|route| (route.reused.clone(), route.refused.clone()))
        })
        .collect()
}

/// Every mutation execution a recording holds.
#[cfg(unix)]
fn executions(events: &[njutest::trace::Event]) -> Vec<&njutest::trace::MutantExecRecord> {
    events
        .iter()
        .filter_map(|event| njutest::testkit::payload::of(&event.payload).mutant_exec())
        .collect()
}

/// Every event the latest recording under `root` holds.
#[cfg(unix)]
fn events_of(root: &std::path::Path) -> Vec<njutest::trace::Event> {
    let mut recordings: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(".njutest/trace"))
        .expect("the trace directory")
        .map(|entry| entry.expect("every trace entry is readable"))
        .map(|entry| entry.path())
        .collect();
    recordings.sort();
    let recording = recordings
        .last()
        .expect("one recording")
        .join(njutest::trace::FILE_NAME);
    njutest::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back")
}

#[cfg(unix)]
#[test]
fn a_second_run_of_one_tree_reads_back_what_the_first_established_and_says_whose_it_is() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-again-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let once = || {
        let (mut said, mut complaints) = (Vec::new(), Vec::new());
        let code = njutest::run_from(
            [
                "njutest",
                "verify",
                "--offline",
                "--locked",
                "--trace",
                "--ui=plain",
            ]
            .map(OsString::from),
            &environment,
            &mut said,
            &mut complaints,
        );
        (
            code,
            njutest_devkit::process::strict_utf8(&complaints).into_owned(),
        )
    };

    let (first, complained) = once();
    assert_eq!(first, 2, "the first run establishes it: {complained}");
    let asked = consulted(&root);
    assert!(
        !asked.is_empty()
            && asked.iter().all(|(reused, refused)| reused.is_none()
                && refused.as_deref() == Some("nothing-recorded")),
        "a believed record is an execution that did not happen, so the recording says \
         what the store answered for every mutation and not only for the ones it \
         answered. Here there is nothing in it yet, and saying so is what parts a cold \
         store from one that has quietly stopped answering: told only how long they \
         took, the two runs look the same. It said {asked:?}"
    );
    let established = report_of(&root);
    assert_eq!(
        part_of(&established)["accounting"]["mutants"]["reused_killed"].as_u64(),
        Some(0),
        "and establishes all of it itself, because there was nothing to read back yet"
    );
    let run_id = part_of(&established)["run_id"]
        .as_str()
        .expect("the run that established it")
        .to_owned();

    std::fs::write(
        root.join("NOTES.md"),
        "a file beside the code that no test reads\n",
    )
    .expect("a change to the tree that is not a change to the package");

    let (second, complained) = once();
    assert_eq!(
        second, 2,
        "and the second reaches the same verdict: {complained}"
    );
    read_back(&root, &run_id);
}

/// What the second run of one tree says about the answers the first one left.
#[cfg(unix)]
fn read_back(root: &std::path::Path, run_id: &str) {
    let report = report_of(root);
    assert_eq!(
        report["report"]["provenance"]["cached"],
        serde_json::Value::Bool(false),
        "a tree that changed is a tree this run answered for itself, whatever the file \
         that changed was: {report}"
    );
    assert!(
        part_of(&report)["accounting"]["mutants"]["reused_killed"]
            .as_u64()
            .is_some_and(|counted| counted > 0),
        "reading back what an earlier run of this exact tree established is the whole \
         of why a second run is cheap, and a run that established it all again would be \
         doing the work twice while reporting that it had not: {report}"
    );
    let again = consulted(root);
    assert!(
        again
            .iter()
            .any(|(reused, refused)| reused.as_deref() == Some(run_id) && refused.is_none()),
        "and the second names the run whose answer it took, with nothing to say against \
         it: a route that carried both would be a run that believed a record and \
         recorded a reason for not believing it. It said {again:?}"
    );
    let sources: Vec<&str> = part_of(&report)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|one| one["reuse"]["source_run_id"].as_str())
        .collect();
    assert!(
        !sources.is_empty() && sources.iter().all(|source| *source == run_id),
        "and each answer it read back names the run that established it, or a person \
         reading a kill has no way to find the execution behind it: this run says \
         {sources:?} and the first was {run_id}"
    );
}

#[cfg(unix)]
#[test]
fn fuzz_targets_a_run_was_not_asked_to_drive_are_a_gap_it_states_rather_than_passes_over() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-fuzzing-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let targets = root.join("fuzz/fuzz_targets");
    std::fs::create_dir_all(&targets).expect("a fuzz directory");
    for named in ["parses", "renders"] {
        std::fs::write(
            targets.join(format!("{named}.rs")),
            "#![no_main]\nlibfuzzer_sys::fuzz_target!(|_data: &[u8]| {});\n",
        )
        .expect("a fuzz target");
    }
    std::fs::write(targets.join("README.md"), "not a target\n").expect("a file beside them");

    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        2,
        "{}",
        njutest_devkit::process::strict_utf8(&complaints)
    );

    let report = report_of(&root);
    let stated = part_of(&report)["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .find(|one| one["name"] == "fuzz-not-executed")
        .expect("what the run did not drive");
    let detail = stated["detail"].as_str().unwrap_or_default();
    assert!(
        detail.starts_with("2 fuzz targets are here and were not driven"),
        "a tree that holds fuzz targets and a run that was not asked to drive them is a \
         gap, and passing over it silently reads as a workspace with no fuzzing in it: \
         {stated}"
    );
    assert!(
        detail.ends_with("parses, renders"),
        "and it names them, because which ones were not driven is what a person would \
         go and drive: {stated}"
    );
}

#[cfg(unix)]
#[test]
fn a_mutation_the_compiler_renders_identically_is_only_equivalent_where_the_tests_ran_it() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-equivalent-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-equivalent");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-equivalent"),
        &root,
    );
    std::fs::write(
        root.join(".njutest.toml"),
        "version = 1\n\n[mutation]\nequivalence = true\n",
    )
    .expect("a configuration that asks the compiler");
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--trace",
            "--ui=plain",
        ]
        .map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    let complained = njutest_devkit::process::strict_utf8(&complaints).into_owned();
    assert_eq!(code, 2, "{complained}");

    let stages: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let problems = njutest::trace::check(&events_of(&root));
    assert!(
        problems.is_empty(),
        "a recording of a run that put its survivors to the compiler is a recording a \
         reader can add up: {problems:?}"
    );
    let recorded = recorded_stages(&root);
    assert!(
        stages.contains(&"equivalence") && recorded.iter().any(|name| name == "equivalence"),
        "a run asked to put its survivors to the compiler names that stage as it starts \
         it, to the person watching and to the recording alike, because it is two builds \
         a survivor and somebody watching the minutes go by has to know which of them \
         are these: {stages:?} and {recorded:?}"
    );

    proved_equivalent(&root);
}

/// What the equivalence layer decided, and the one survivor it may not take.
#[cfg(unix)]
fn proved_equivalent(root: &std::path::Path) {
    let report = report_of(root);
    if !njutest_devkit::reproducible::builds_the_same_twice() {
        assert_eq!(
            part_of(&report)["accounting"]["mutants"]["equivalent"].as_u64(),
            Some(0),
            "a machine that renders one unchanged tree two ways establishes nothing here, \
             and a run that took its own difference for the mutation's would remove a \
             finding nobody proved: {report}"
        );
        return;
    }
    assert_eq!(
        part_of(&report)["accounting"]["mutants"]["equivalent"].as_u64(),
        Some(1),
        "the compiler renders `n + 0` and `n - 0` identically at this fixture's \
         optimisation level, and the tests run it, so no test could have noticed: that \
         is a finding removed rather than a survivor reported: {report}"
    );
    let surviving: Vec<&str> = part_of(&report)["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|one| one["kind"] == "surviving-mutant" || one["kind"] == "unreached-mutant")
        .filter_map(|one| one["detail"].as_str())
        .collect();
    assert!(
        surviving.iter().any(|detail| detail.contains("div-to-mul")),
        "while the mutation of a function nothing calls also comes out identical, \
         because the linker dropped it, and that is the finding itself rather than a \
         proof: identical bytes mean no test can tell them apart, which is reassuring \
         when the tests run the code and is the whole problem when they do not. It said \
         {surviving:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_that_held_something_says_what_it_held_and_lets_go_of_it() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-holding-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let provider = njutest_devkit::fake_cargo::example_in("fake_provider", dir.path());
    std::fs::write(
        root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[resources.postgres]\ncommand = [{:?}, \"resource\"]\n\
             timeout = \"10s\"\nenvironment = [\"FAKE_PROVIDER_READY\", \"FAKE_PROVIDER_STOPPED\"]\n",
            provider.to_str().expect("test protocol paths are UTF-8")
        ),
    )
    .expect("a configuration that names a resource");
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let mut vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    vars.push((
        OsString::from("FAKE_PROVIDER_READY"),
        OsString::from(
            r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#,
        ),
    ));
    vars.push((
        OsString::from("FAKE_PROVIDER_STOPPED"),
        OsString::from(r#"{"version":1,"status":"stopped","instance":"pg-1"}"#),
    ));
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--trace",
        ]
        .map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    let complained = njutest_devkit::process::strict_utf8(&complaints).into_owned();
    assert_eq!(code, 2, "{complained}");
    let stages: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let recorded = recorded_stages(&root);
    assert!(
        stages.contains(&"resources") && recorded.iter().any(|name| name == "resources"),
        "a run that waits for somebody else's program to become ready names that stage \
         as it starts it, to the person watching and to the recording alike: {stages:?} \
         and {recorded:?}"
    );

    leased(&report_of(&root));
}

/// What a report says about the one resource a run was told to hold.
#[cfg(unix)]
fn leased(report: &serde_json::Value) {
    let held = part_of(report)["resources"]
        .as_array()
        .expect("resources")
        .first()
        .expect("the one it was told to hold");
    assert_eq!(
        (held["capability"].as_str(), held["instance"].as_str()),
        (Some("postgres"), Some("pg-1")),
        "a run that was given something to hold says what it held and which one it \
         got, because a test that fails against a resource is a different question \
         from one that fails without it: {report}"
    );
    assert_eq!(
        held["environment"],
        serde_json::json!(["DATABASE_URL"]),
        "and the variables it told the tests about by name, never their values: what a \
         provider hands over is a credential as often as not, and a report is a \
         document people put in front of each other: {report}"
    );
    assert!(
        !part_of(report)["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "resource-not-stopped"),
        "and it lets go of it: a resource nobody released is one the next run waits \
         for, and a run that stopped one without saying so leaves a person no way to \
         tell that from one that never held it: {report}"
    );
}

/// The test a generation provider offers to close the gap the ignored test left.
#[cfg(unix)]
const OFFERED: &str = "Ly8gU1BEWC1GaWxlQ29weXJpZ2h0VGV4dDogMjAyNiBtanV0ZXN0IGNvbnRyaWJ1dG9ycwovLyBTUERYLUxpY2Vuc2UtSWRlbnRpZmllcjogTUlUIE9SIEFwYWNoZS0yLjAKCi8vISBPZmZlcmVkIGJ5IGEgZ2VuZXJhdGlvbiBwcm92aWRlciB0byBjbG9zZSB0aGUgZ2FwIHRoZSBpZ25vcmVkIHRlc3QgbGVmdC4KCiNbdGVzdF0KZm4gemVyb19oYXNfYV9zaWduX29mX2l0c19vd24oKSB7CiAgICBhc3NlcnRfZXEhKGZpeHR1cmVfYmFzZWxpbmU6OnNpZ24oMCksICJ6ZXJvIik7Cn0K";

#[cfg(unix)]
#[test]
fn a_candidate_offered_for_a_gap_is_put_to_the_tests_before_it_is_recorded() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-offering-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let provider = njutest_devkit::fake_cargo::example_in("fake_provider", dir.path());
    std::fs::write(
        root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[generation]\ncommand = [{:?}, \"generation\"]\n\
             environment = [\"FAKE_GENERATOR_OFFERS\", \"FAKE_GENERATOR_ASKED\"]\n",
            provider.to_str().expect("test protocol paths are UTF-8")
        ),
    )
    .expect("a configuration that names a generator");
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let asked = dir.path().join("asked.jsonl");
    let mut vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    vars.push((
        OsString::from("FAKE_GENERATOR_ASKED"),
        OsString::from(asked.display().to_string()),
    ));
    vars.push((
        OsString::from("FAKE_GENERATOR_OFFERS"),
        OsString::from(format!(
            r#"{{"version":1,"candidates":[{{"kind":"patch","path":"tests/zero.rs","preimage_sha256":null,"content_base64":"{OFFERED}"}}]}}"#
        )),
    ));
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--trace",
        ]
        .map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        2,
        "{}",
        njutest_devkit::process::strict_utf8(&complaints)
    );

    let complained = njutest_devkit::process::strict_utf8(&complaints).into_owned();
    let stages: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let recorded = recorded_stages(&root);
    assert!(
        stages.contains(&"generation") && recorded.iter().any(|name| name == "generation"),
        "a run that asks somebody else for a candidate names that stage as it starts \
         it, to the person watching and to the recording alike, because the minutes it \
         spends waiting are somebody else's program running: {stages:?} and {recorded:?}"
    );

    questioned(&asked);

    let report = report_of(&root);
    offered_and_checked(&report, &root);
    unkeepable(dir.path(), &root, environment);
}

/// What a run tells a generation provider about the gap it is asking to close.
#[cfg(unix)]
fn questioned(asked: &std::path::Path) {
    let put = std::fs::read_to_string(asked).expect("what the generator was asked");
    let question: serde_json::Value =
        njutest_devkit::strictjson::decode_str(put.lines().next().expect("one question"))
            .expect("it is JSON");
    assert_eq!(
        question["finding"]["kind"], "surviving-mutant",
        "and it says what kind of gap it is asking about, because what closes a \
         mutation nothing noticed is a different thing from what closes a test that \
         fails: {question}"
    );
    assert!(
        question["finding"]["line"]
            .as_u64()
            .is_some_and(|line| line > 0)
            && question["finding"]["path"] == "src/lib.rs",
        "and where it is, which is what a generator needs to write anything at all: \
         {question}"
    );
    assert!(
        question["finding"]["replay"]
            .as_str()
            .is_some_and(|said| said.starts_with("njutest replay ")),
        "and how to see it again, so whoever reads the answer can check it: {question}"
    );
}

/// What a run records about a candidate it put to the tests.
#[cfg(unix)]
fn offered_and_checked(report: &serde_json::Value, root: &std::path::Path) {
    let offered = part_of(report)["candidates"]
        .as_array()
        .expect("candidates")
        .first()
        .expect("what the generator offered");
    assert_eq!(
        (
            offered["path"].as_str(),
            offered["accepted"].as_bool(),
            offered["stability_runs"].as_u64(),
            offered["kill_runs"].as_u64(),
        ),
        (Some("tests/zero.rs"), Some(true), Some(3), Some(2)),
        "a candidate is put to the tests before it is written down: the patched tree \
         has to pass on its own and catch the mutation twice, and a run that recorded \
         one without asking would offer somebody a test that does not do what it says: \
         {report}"
    );
    assert!(
        !root.join("tests/zero.rs").exists(),
        "and it stays a proposal: verifying is reading, and the only thing that writes \
         into somebody's tree is being asked to"
    );
    let kept = std::fs::read_dir(root.join(njutest::repair::STORE))
        .expect("the candidates this run kept")
        .collect::<Result<Vec<_>, _>>()
        .expect("every kept-candidate entry is readable");
    assert!(
        !kept.is_empty(),
        "and what it holds up is kept, because `fix --apply` writes what was checked \
         rather than asking the generator again for something nobody put to the tests"
    );
}

/// A candidate that holds up and has nowhere to be kept.
#[cfg(unix)]
fn unkeepable(dir: &std::path::Path, from: &std::path::Path, environment: Environment) {
    let root = from;
    let blocked = dir.join("fixture-blocked");
    copy_tree(root, &blocked);
    njutest_devkit::fixture::pin_contract(&blocked, "standard-v1");
    for gone in [".njutest", "reports"] {
        match std::fs::remove_dir_all(blocked.join(gone)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("removing {gone} from the blocked fixture: {error}"),
        }
    }
    std::fs::create_dir_all(blocked.join(".njutest")).expect("the directory it works in");
    std::fs::write(
        blocked.join(njutest::repair::STORE),
        "a file where the candidates go",
    )
    .expect("a file where a directory belongs");
    let scratch = dir.join("scratch-again");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let elsewhere = Environment {
        working_directory: blocked.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        cache_directory: dir.join("cache-again"),
        ..environment
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &elsewhere,
        &mut said,
        &mut complaints,
    );
    assert_ne!(
        code, EXIT_ERROR,
        "failure to retain an optional proposal does not erase the completed run"
    );
    let report = report_of(&blocked);
    assert!(
        part_of(&report)["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "generation-candidate-not-kept"),
        "a candidate that held up and could not be kept is one nothing can apply \
         afterwards, so the run says so rather than recording an offer whose content is \
         gone: {report}"
    );
    assert!(
        part_of(&report)["candidates"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "and it does not record it, because a candidate a reader cannot get back is an \
         offer that cannot be taken up: {report}"
    );
}

/// A stream that raises `cancel` once a run has judged a mutation and started saying so about the next.
#[cfg(unix)]
struct Interrupting<'a> {
    cancel: &'a Cancel,
    said: String,
}

#[cfg(unix)]
impl Write for Interrupting<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.said
            .push_str(&njutest_devkit::process::strict_utf8(buffer));
        if self
            .said
            .split_once("== mutation")
            .is_some_and(|(_before, after)| after.contains("[2/"))
        {
            self.cancel.cancel();
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn a_run_that_was_stopped_leaves_what_it_established_for_the_next_one() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-stopped-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let mut said = Vec::new();
    let mut complaints = Interrupting {
        cancel: &environment.cancel,
        said: String::new(),
    };
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--ui=plain"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code, 2,
        "the cancel lands once the phase has measured everything and started saying so, \
         so the run carries its verdict out rather than dying between rows: what it \
         must not do is clear what it established on the way out: {}",
        complaints.said
    );

    left_behind(&environment.cache_directory);

    resumed(&root, environment);
}

/// What a run that was stopped wrote where its successor will look.
#[cfg(unix)]
fn left_behind(cache: &std::path::Path) {
    let kept: Vec<serde_json::Value> =
        std::fs::read_dir(cache.join("njutest/outcomes-v1/checkpoints"))
            .expect("the checkpoints directory")
            .map(|entry| entry.expect("every checkpoint entry is readable"))
            .map(|entry| {
                std::fs::read_to_string(entry.path().join(njutest::checkpoint::FILE_NAME))
                    .expect("every checkpoint document is readable")
            })
            .map(|text| {
                njutest_devkit::strictjson::decode_str(&text)
                    .expect("every checkpoint is valid JSON")
            })
            .collect();
    let state = kept.first().expect("what the stopped run established");
    assert!(
        state["mutants"]
            .as_array()
            .is_some_and(|mutants| !mutants.is_empty()),
        "a run that was stopped keeps what it established up to there, or an interrupt \
         costs the whole run, which is the opposite of what a checkpoint is for: {state}"
    );
    assert!(
        state["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["disposition"]["by"]
                .as_str()
                .is_some_and(|by| !by.is_empty())),
        "and each of them names the target that decided it, because a kill nobody can \
         attribute is not one the next run may carry: the report it ends in has to say \
         which test noticed: {state}"
    );
    assert_eq!(
        state["attempts"].as_u64(),
        Some(1),
        "and it counts this as one attempt, which is how a run that keeps being \
         interrupted is told from one that ran once: {state}"
    );
}

/// What the next run of the same tree does with it.
#[cfg(unix)]
fn resumed(root: &std::path::Path, environment: Environment) {
    let environment = Environment {
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
        ..environment
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--ui=plain"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        2,
        "{}",
        njutest_devkit::process::strict_utf8(&complaints)
    );
    let report = report_of(root);
    assert_eq!(
        report["report"]["provenance"]["cached"],
        serde_json::Value::Bool(false),
        "and the next run of the same tree establishes it rather than reading back what \
         the stopped one reached. A run that was told to stop stopped: its answer is \
         what it got through, and storing that under the tree's identity would hand the \
         next run a partial measurement wearing a whole one's name: {report}"
    );
    assert!(
        part_of(&report)["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "resumed-from-checkpoint"),
        "while what it did establish is carried forward, and the run that continues says \
         so: a restored target keeps reaching its whole file rather than the regions \
         inside it, and a reader comparing two reports has to know which one that was: \
         {report}"
    );
}

/// One run of `fixture` in this process under `configured`, and the report it wrote.
fn once(
    fixture: &str,
    dir: &std::path::Path,
    name: &str,
    configured: Option<&str>,
) -> serde_json::Value {
    let root = dir.join(name);
    copy_tree(&njutest_devkit::paths::fixtures_dir().join(fixture), &root);
    if let Some(text) = configured {
        std::fs::write(root.join(".njutest.toml"), text).expect("a configuration");
    }
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.join(format!("{name}-scratch"));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.join(format!("{name}-cache")),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert!(
        matches!(code, 0..=2),
        "{name} reached a verdict rather than a failure: {}",
        njutest_devkit::process::strict_utf8(&complaints)
    );
    report_of(&root)
}

#[cfg(unix)]
#[test]
fn a_run_of_one_tree_says_the_same_thing_however_many_times_and_however_widely_it_is_run() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-again-and-again-")
        .tempdir()
        .expect("a temporary directory");

    let first = once("fixture-baseline", dir.path(), "first", None);
    let again = once("fixture-baseline", dir.path(), "again", None);
    assert_eq!(
        njutest_devkit::report::normalize(&first),
        njutest_devkit::report::normalize(&again),
        "a run of one tree twice says one thing twice, or every number in it is about          the afternoon it was produced rather than about the workspace. Everything that          may differ between two runs — when they started, what they were called — is          what a normalised report takes out; what is left is the answer"
    );

    let alone = once(
        "fixture-baseline",
        dir.path(),
        "alone",
        Some("version = 1\n\n[execution]\njobs = 1\n"),
    );
    let together = once(
        "fixture-baseline",
        dir.path(),
        "together",
        Some("version = 1\n\n[execution]\njobs = 8\n"),
    );
    assert_eq!(
        njutest_devkit::report::normalize(&alone),
        njutest_devkit::report::normalize(&together),
        "and measuring eight mutations at once rather than one changes which processes          overlap and nothing a report says: a verdict that moved with the machine's load          would be a verdict about the machine"
    );
    let unsaid = once(
        "fixture-baseline",
        dir.path(),
        "unsaid",
        Some("version = 1\n"),
    );
    assert_eq!(
        njutest_devkit::report::normalize(&alone),
        njutest_devkit::report::normalize(&unsaid),
        "and neither does saying nothing about how many"
    );
}

/// One shard document, read back from a run that judged part of a catalog.
#[cfg(unix)]
fn shard_of(document: &serde_json::Value) -> njutest::report::ShardReport {
    match njutest::report::json::parse_any(&document.to_string()).expect("a part reads back") {
        njutest::report::ReportDocument::Shard(shard) => shard,
        njutest::report::ReportDocument::Complete(_) => {
            panic!("a sharded run writes one shard document")
        }
    }
}

/// One part of `fixture`'s catalog, judged in this process.
#[cfg(unix)]
fn part(root: &std::path::Path, dir: &std::path::Path, shard: &str) -> serde_json::Value {
    let scratch = dir.join(format!("part-{}-scratch", shard.replace('/', "-")));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.join("parts-cache"),
        working_directory: root.to_path_buf(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--shard",
            shard,
        ]
        .map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        0,
        "{shard}: {}",
        njutest_devkit::process::strict_utf8(&complaints)
    );
    latest_report_of(root)
}

#[cfg(unix)]
#[test]
fn a_catalog_cut_into_parts_and_put_back_together_says_what_the_whole_would_have() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-in-parts-")
        .tempdir()
        .expect("a temporary directory");

    let whole = once("fixture-assured", dir.path(), "whole", None);

    let root = dir.path().join("parts");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-assured"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let one = part(&root, dir.path(), "1/2");
    let two = part(&root, dir.path(), "2/2");
    assert_ne!(
        one["report"]["run_id"], two["report"]["run_id"],
        "two runs, two reports"
    );

    let parts: Vec<njutest::report::ShardReport> = [&one, &two].into_iter().map(shard_of).collect();
    let final_run =
        rust_mutants::id::RunId::try_from("the-whole-of-the-parts").expect("a canonical run id");
    let merged = njutest::report::merge::merge(&final_run, &parts)
        .expect("two parts of one catalog")
        .complete_without_models()
        .expect("the whole needs no model phase");
    let whole_report =
        njutest::report::json::parse(&whole.to_string()).expect("the whole reads back");

    let answer = |report: &njutest::report::Report| {
        let concluded = report.conclusion().expect("the report adds up");
        (
            report.verdict(),
            concluded.accounting.mutants,
            concluded
                .mutants
                .iter()
                .map(|one| {
                    (
                        one.display_id().to_owned(),
                        (
                            one.decision(),
                            one.by_build()
                                .first()
                                .and_then(|fact| fact.outcome().decided_by().map(str::to_owned)),
                        ),
                    )
                })
                .collect::<std::collections::BTreeMap<String, _>>(),
        )
    };
    assert_eq!(
        answer(&merged),
        answer(&whole_report),
        "dividing the work is not a budget only if the pieces add back up to it. A run \
         cut in two and put back together has to answer what one run of the same tree \
         answers — the same verdict, the same counts, the same row for every mutation — \
         or --shard is a way of getting a different answer cheaply. A merged report \
         keeps one part per shard, so the answer is compared where a reader reads it, \
         not as bytes"
    );
}

/// One run of `fixture` in this process under `configured`, and what it exited with rather than the report it did not write.
fn refused(fixture: &str, dir: &std::path::Path, name: &str, configured: &str) -> (u8, String) {
    let root = dir.join(name);
    copy_tree(&njutest_devkit::paths::fixtures_dir().join(fixture), &root);
    std::fs::write(root.join(".njutest.toml"), configured).expect("a configuration");
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.join(format!("{name}-scratch"));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let environment = Environment {
        cache_directory: dir.join(format!("{name}-cache")),
        working_directory: root,
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: njutest_devkit::paths::environment_for_a_run(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    (
        code,
        njutest_devkit::process::strict_utf8(&complaints).into_owned(),
    )
}

/// Whether this machine has the interpreter the `deep-v1` contract promises.
fn interpreter() -> bool {
    std::process::Command::new("cargo")
        .args(["+nightly", "miri", "--version"])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn the_contract_that_promises_the_suite_is_interpreted_interprets_it() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-deep-")
        .tempdir()
        .expect("a temporary directory");

    if !interpreter() {
        let (code, complaints) = refused(
            "fixture-baseline",
            dir.path(),
            "without",
            "version = 1\ncontract = \"deep-v1\"\n",
        );
        assert_eq!(
            code, EXIT_ERROR,
            "a contract that promises the suite is interpreted cannot be answered by a \
             toolchain that cannot interpret it, and a machine without the interpreter \
             is where that has to hold: {complaints}"
        );
        assert!(
            complaints.contains("NJ7001"),
            "and it says which of the things a run needs is the one that is not here, \
             because a reader who is told only that the run failed has to guess: \
             {complaints}"
        );
        return;
    }

    let report = once(
        "fixture-baseline",
        dir.path(),
        "interpreted",
        Some("version = 1\ncontract = \"deep-v1\"\n"),
    );
    assert_eq!(
        report["report"]["contract"], "deep-v1",
        "the run answers to the contract it was given: {report}"
    );
    assert_eq!(
        part_of(&report)["accounting"]["soundness"]["executed"],
        serde_json::Value::Bool(true),
        "and deep-v1 promises the suite is interpreted, so a run of it that did not \
         interpret anything and still reached a verdict would be the promise unkept in \
         the one place nobody looks: {report}"
    );
    let stated: Vec<&str> = part_of(&report)["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        !stated.contains(&"miri-unsupported"),
        "and what the interpreter could not follow is stated rather than passed over; \
         this fixture holds nothing it cannot: {stated:?}"
    );
}

/// A tree holding one fuzz target, which `builds` says whether cargo could build.
#[cfg(unix)]
fn with_fuzz_target(root: &std::path::Path, builds: bool) {
    let targets = root.join("fuzz/fuzz_targets");
    std::fs::create_dir_all(&targets).expect("a fuzz directory");
    std::fs::write(
        targets.join("parses.rs"),
        if builds {
            "#![no_main]\nfn main() {}\n"
        } else {
            "this is not rust at all\n"
        },
    )
    .expect("a fuzz target");
    std::fs::write(
        root.join("fuzz/Cargo.toml"),
        "[package]\nname = \"a-fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         publish = false\n\n[[bin]]\nname = \"parses\"\npath = \"fuzz_targets/parses.rs\"\n\
         test = false\ndoc = false\nbench = false\n",
    )
    .expect("a fuzz crate");
}

#[cfg(unix)]
#[test]
fn a_target_the_fuzzer_could_not_drive_is_a_gap_and_never_a_target_that_found_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-undriven-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    with_fuzz_target(&root, false);
    std::fs::write(
        root.join(".njutest.toml"),
        "version = 1\n\n[fuzz]\nrun = true\nmax_total_time = \"3s\"\n",
    )
    .expect("a configuration that asks for the targets to be driven");
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");

    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        2,
        "{}",
        njutest_devkit::process::strict_utf8(&complaints)
    );

    let report = report_of(&root);
    let stated: Vec<&str> = part_of(&report)["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        stated.contains(&"cargo-fuzz-unavailable"),
        "a target the fuzzer was asked to drive and could not is a gap this run says it \
         has. Counting it as driven is the worst answer a fuzzing phase can give: \
         nothing ran, and the report reads as though something did and found nothing: \
         {stated:?}"
    );
    assert!(
        part_of(&report)["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|one| one["kind"] == "not-measured"
                && one["subject"]
                    .as_str()
                    .is_some_and(|at| at.contains("parses"))),
        "and it is a finding, naming the target, because what was asked for and not \
         done is what a person acts on: {report}"
    );
}

/// A run in this process against `fixture`, and what it left behind.
#[cfg(unix)]
fn verified_in_process(
    fixture: &str,
    dir: &std::path::Path,
    extra: &[&str],
    carrying: &[(&str, &str)],
) -> (u8, String, std::path::PathBuf) {
    let root = dir.join(fixture);
    copy_tree(&njutest_devkit::paths::fixtures_dir().join(fixture), &root);
    if !carrying.is_empty() {
        let named: Vec<String> = carrying
            .iter()
            .map(|(name, _value)| format!("{name:?}"))
            .collect();
        std::fs::write(
            root.join(".njutest.toml"),
            format!(
                "version = 1\n\n[execution]\ntimeout = \"2s\"\nenvironment = [{}]\n",
                named.join(", ")
            ),
        )
        .expect("a configuration this run reads");
        njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    }
    let scratch = dir.join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let mut vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    vars.extend(
        carrying
            .iter()
            .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
    );
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let mut args: Vec<OsString> = ["njutest", "verify", "--offline", "--locked", "--no-cache"]
        .map(OsString::from)
        .to_vec();
    args.extend(extra.iter().map(|one| OsString::from(*one)));

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(args, &environment, &mut said, &mut complaints);
    assert!(
        std::str::from_utf8(&said).is_ok(),
        "command output is a UTF-8 protocol"
    );
    (
        code,
        njutest_devkit::process::strict_utf8(&complaints).into_owned(),
        root,
    )
}

#[cfg(unix)]
#[test]
fn a_mutation_that_never_returns_is_stopped_measured_alone_and_reported_as_a_wait() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-hang-")
        .tempdir()
        .expect("a temporary directory");
    let paused = dir.path().join("paused");
    let (code, complained, root) = verified_in_process(
        "fixture-hang",
        dir.path(),
        &["--trace", "--ui=plain"],
        &[
            ("FIXTURE_HANG_MARKER", &paused.display().to_string()),
            ("FIXTURE_HANG_PAUSE_MS", "8000"),
        ],
    );
    assert_eq!(
        code, 2,
        "this fixture is slow once per mutation and bounded at a second, so every \
         measurement of it runs out of time and every one is asked again: {complained}"
    );

    let report = report_of(&root);
    once_slow(&report);

    let events = events_of(&root);
    let execs = executions(&events);
    let expired: std::collections::BTreeSet<&str> = execs
        .iter()
        .filter(|exec| exec.outcome == "waited" && !exec.alone)
        .map(|exec| exec.mutant.as_str())
        .collect();
    assert!(
        !expired.is_empty(),
        "and the recording says which measurement ran out of time even where the report \
         does not, because that is the whole account of where the minutes went: \
         {execs:?}"
    );
    let alone: std::collections::BTreeSet<&str> = execs
        .iter()
        .filter(|exec| exec.alone)
        .map(|exec| exec.mutant.as_str())
        .collect();
    assert_eq!(
        alone, expired,
        "a budget that expired is measured once more with the machine to itself and \
         nothing else is, because a bound reached while a dozen measurements shared the \
         processors is a bound about the machine. Giving the machine to a measurement \
         that did not need it spends the run's time on nothing; withholding it from one \
         that did turns the load into a finding: {execs:?}"
    );
    for mutant in &alone {
        assert!(
            execs
                .iter()
                .any(|exec| exec.mutant == *mutant && !exec.alone),
            "and the quiet measurement is a second one rather than the only one: a \
             mutation measured only alone is one the run never put to the tests the \
             ordinary way: {execs:?}"
        );
    }
    assert!(
        part_of(&report)["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|one| expired.contains(one["display_id"].as_str().unwrap_or_default()))
            .all(|one| one["decision"]["outcome"] != "waited"),
        "and what the second measurement said is what the mutation is reported as: the \
         first one is how long it took, not what it established: {report}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_in_this_process_writes_what_it_learned_before_it_compiled_anything() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-inprocess-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--trace",
            "--ui=plain",
        ]
        .map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(
        code,
        2,
        "this fixture has a gap its own tests cannot see: {}\n{}",
        njutest_devkit::process::strict_utf8(&said),
        njutest_devkit::process::strict_utf8(&complaints)
    );

    let report = report_of(&root);
    assert_eq!(
        report["report"]["repository"]["git"]["available"],
        serde_json::Value::Bool(false),
        "a copied fixture is not a repository, and the report says what git answered \
         rather than leaving the question unasked: {report}"
    );
    let stated: Vec<&str> = part_of(&report)["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        stated.contains(&"git-metadata-unavailable"),
        "and states that it could not name the commit it verified, which is the one \
         thing that would let somebody come back to this tree: {stated:?}"
    );
    let skipped = part_of(&report)["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .find(|one| one["name"] == "skipped-test-code")
        .expect("what the run did not mutate");
    assert!(
        skipped["detail"]
            .as_str()
            .is_some_and(|detail| detail.starts_with("1 place was not mutated")),
        "and how much of the tree it did not mutate, with the number: a run that says \
         it skipped something without saying how much reads as a footnote, and the \
         difference between one place and four hundred is the difference between a \
         report about this workspace and a report about a corner of it: {skipped}"
    );

    stages_of(&njutest_devkit::process::strict_utf8(&complaints), &root);

    accounted(&report, &root);
}

/// What a whole run's report says about the targets and the mutations it judged.
#[cfg(unix)]
fn accounted(report: &serde_json::Value, root: &std::path::Path) {
    let targets = part_of(report)["targets"]
        .as_array()
        .expect("target records");
    assert!(
        !targets.is_empty(),
        "a run that measured targets says which ones, or the counts it carries are \
         numbers about nothing a reader can check: {report}"
    );
    let durations: Vec<u64> = targets
        .iter()
        .filter_map(|one| one["duration_ms"].as_u64())
        .collect();
    assert!(
        durations.windows(2).all(|pair| pair[0] >= pair[1]),
        "and they are ordered slowest first, which is the order somebody reading for \
         where the time went needs: {durations:?}"
    );
    let selected = u64::try_from(targets.len()).expect("the fixture target count fits u64");
    assert_eq!(
        part_of(report)["accounting"]["targets"]["selected"].as_u64(),
        Some(selected),
        "the counts and the records are two ways of saying one thing: {report}"
    );
    assert!(
        part_of(report)["accounting"]["mutants"]["cataloged"]
            .as_u64()
            .is_some_and(|counted| counted > 0)
            && !part_of(report)["mutants"]
                .as_array()
                .expect("mutants")
                .is_empty(),
        "and a run that catalogued mutations records what became of each: {report}"
    );
    assert!(
        part_of(report)["timing"]["finished"]
            .as_str()
            .is_some_and(|when| !when.is_empty()),
        "a report that never says when it finished is one nothing can be compared \
         against: {report}"
    );
    provenance(report);
    concluded(report, root);
}

/// What a report says about the thing it is about and what produced it.
#[cfg(unix)]
fn provenance(report: &serde_json::Value) {
    for named in ["rustc", "cargo", "target", "os", "arch"] {
        assert!(
            part_of(report)["toolchain"][named]
                .as_str()
                .is_some_and(|said| !said.is_empty()),
            "and it says what built the thing it is about, because the same suite \
             under two compilers is two answers: {named} is missing from {report}"
        );
    }
    assert_eq!(
        report["report"]["repository"]["packages"],
        serde_json::json!(["fixture-baseline"]),
        "and which packages the workspace holds, which is the scope every count in it \
         is over: {report}"
    );
    assert!(
        part_of(report)["resources"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "a run that was given nothing to hold says so rather than leaving the question \
         unasked, because a resource nobody released is one the next run waits for: \
         {report}"
    );
}

/// What a whole run of the baseline fixture concludes about the mutations it judged.
#[cfg(unix)]
fn concluded(report: &serde_json::Value, root: &std::path::Path) {
    assert_eq!(verdict_of(root), njutest::report::Verdict::Insufficient);
    assert_eq!(
        (
            part_of(report)["accounting"]["mutants"]["killed"].as_u64(),
            part_of(report)["accounting"]["mutants"]["survived"].as_u64(),
            part_of(report)["accounting"]["mutants"]["unreached"].as_u64(),
        ),
        (Some(10), Some(3), Some(1)),
        "and every mutation is in the column it belongs to. A phase that reported them \
         all as unnoticed, or all as something nothing could decide, reaches the same \
         verdict on this fixture by a route that says nothing about the suite: {report}"
    );
    let named: Vec<&str> = part_of(report)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|one| one["decision"]["killed_by"].as_str())
        .collect();
    assert_eq!(
        named.len(),
        10,
        "and each kill names the test that noticed, because the whole of what a kill \
         hands a person is where to look: {named:?}"
    );
    paced(report, root);
}

/// That the phase counted its mutations off one at a time to whoever was watching.
#[cfg(unix)]
fn paced(report: &serde_json::Value, root: &std::path::Path) {
    let counted = part_of(report)["accounting"]["mutants"]["cataloged"]
        .as_u64()
        .expect("how many were judged");
    let said: Vec<(u64, u64)> = events_of(root)
        .iter()
        .filter_map(|event| {
            let progress = njutest::testkit::payload::of(&event.payload).progress()?;
            Some((progress.done?, progress.total?))
        })
        .filter(|(_done, total)| *total == counted)
        .collect();
    assert_eq!(
        said,
        (1..=counted)
            .map(|done| (done, counted))
            .collect::<Vec<(u64, u64)>>(),
        "a phase that says the same number twice, or skips one, is one whose remaining \
         time cannot be read off it, which is the only thing that line is for"
    );
}
