// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A watch against a real workspace: the round it runs before anything changes, and the verdict it carries out of it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helper that copies a fixture is not itself a test, and a copy that fails is a setup failure to report by panicking"
)]

use std::ffi::OsString;
use std::io::Write;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use njutest_cli::cli::{EXIT_ERROR, Environment};
use njutest_devkit::fixture::copy_tree;
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
        self.said.push_str(&String::from_utf8_lossy(buffer));
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
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
    let (done, waited) = channel::<()>();
    let bound = std::thread::spawn(move || {
        let expired = waited.recv_timeout(LONGEST) == Err(RecvTimeoutError::Timeout);
        if expired {
            watchdog.cancel();
        }
        expired
    });

    let mut output = Stopping::watching(&environment.cancel);
    let mut complaints = Stopping::complaining(&environment.cancel);
    let code = njutest_cli::run_from(
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars: Vec::new(),
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--trace"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );

    let complained = String::from_utf8_lossy(&complaints);
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
        !String::from_utf8_lossy(&said).contains("VERDICT"),
        "a run that stopped here reached no verdict, and printing one would be a claim \
         about a workspace it never opened"
    );

    let recording = std::fs::read_dir(root.path().join(".njutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path().join(njutest_cli::trace::FILE_NAME))
        .next()
        .expect("one recording");
    let events = njutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back");
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
            _ => None,
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

#[test]
fn a_run_told_where_to_look_for_a_toolchain_looks_there_and_nowhere_else() {
    let root = tempfile::Builder::new()
        .prefix("njutest-nocargo-")
        .tempdir()
        .expect("a temporary directory");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\n",
    )
    .expect("a manifest");
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).expect("a directory with no toolchain in it");
    let scratch = root.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let environment = Environment {
        cache_directory: root.path().to_owned(),
        working_directory: root.path().to_owned(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars: vec![(
            OsString::from("PATH"),
            OsString::from(empty.display().to_string()),
        )],
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );

    let complained = String::from_utf8_lossy(&complaints);
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars: std::env::vars_os()
            .filter(|(name, _)| {
                njutest_devkit::paths::same_name(name, std::ffi::OsStr::new("PATH"))
            })
            .collect(),
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let _code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked"].map(OsString::from),
        &told,
        &mut said,
        &mut complaints,
    );

    let complained = String::from_utf8_lossy(&complaints);
    assert!(
        !complained.contains("no search path"),
        "and a run whose environment does name a place to look is told that place: \
         reading some other variable, or reading none, would have it report that nobody \
         said where to look while somebody had: {complained}"
    );
}

/// Every stage a whole run goes through names itself, to a person and to a recording.
fn stages_of(complained: &str, root: &std::path::Path) {
    let said: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let events = events_of(root);
    let problems = njutest_cli::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let recorded: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
            _ => None,
        })
        .collect();

    let routed: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::Route { route } => Some(route.mutant.as_str()),
            _ => None,
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
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::Progress { progress } => Some(progress.subject.as_str()),
            _ => None,
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
fn judged(events: &[njutest_cli::trace::Event]) {
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        phases.contains(&"mutation-judge"),
        "the phase that takes the time names itself in the recording, or the minutes \
         between two events belong to nothing anybody can name: {phases:?}"
    );

    let routes: Vec<&njutest_cli::trace::RouteRecord> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::Route { route } => Some(route),
            _ => None,
        })
        .collect();
    let placed = routes
        .iter()
        .find(|route| route.granularity == "block")
        .expect("a mutation the measurement placed");
    assert!(
        placed.fallback.is_none() && !placed.reaching.is_empty(),
        "a route the measurement decided says so by naming the targets it decided on \
         and nothing that widened it: a route that named neither is one a reader cannot \
         tell from a measurement that said nothing: {placed:?}"
    );

    let probes: Vec<&njutest_cli::trace::ProbeExecRecord> = events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::ProbeExec { probe } => Some(probe),
            _ => None,
        })
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
    let directory = njutest_cli::app::reports::Store::read(root)
        .run_of(njutest_cli::app::reports::Index::Any)
        .expect("the index names a run");
    serde_json::from_str(
        &std::fs::read_to_string(directory.join(njutest_cli::app::reports::DOCUMENT_NAME))
            .expect("the report"),
    )
    .expect("the report is JSON")
}

/// What a run whose every measurement was slow exactly once concludes.
fn once_slow(report: &serde_json::Value) {
    assert_eq!(
        (
            report["accounting"]["mutants"]["killed"].as_u64(),
            report["accounting"]["mutants"]["survived"].as_u64(),
        ),
        (Some(6), Some(1)),
        "every mutation but one is noticed here, and by the second measurement rather \
         than the first. The one nothing noticed ran and was not noticed, which is a \
         different fact from one nothing could decide: {report}"
    );
    assert_eq!(
        report["accounting"]["mutants"]["timed_out"].as_u64(),
        Some(0),
        "a bound reached once and not again is not a mutation that ran out of time. \
         Every measurement here was slow the first time and quick the second, and a run \
         that reported them as timeouts would hand a person seven findings caused by \
         whatever else the machine was doing: {report}"
    );
}

/// Every stage the latest recording under `root` names, in the order it named them.
fn recorded_stages(root: &std::path::Path) -> Vec<String> {
    events_of(root)
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.clone()),
            _ => None,
        })
        .collect()
}

/// What each route of the latest recording said about the answer an earlier run had left: the run it took, and why it took none.
fn consulted(root: &std::path::Path) -> Vec<(Option<String>, Option<String>)> {
    events_of(root)
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::Route { route } => {
                Some((route.reused.clone(), route.refused.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Every mutation execution a recording holds.
fn executions(events: &[njutest_cli::trace::Event]) -> Vec<&njutest_cli::trace::MutantExecRecord> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::MutantExec { mutant } => Some(mutant),
            _ => None,
        })
        .collect()
}

/// Every event the latest recording under `root` holds.
fn events_of(root: &std::path::Path) -> Vec<njutest_cli::trace::Event> {
    let mut recordings: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(".njutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    recordings.sort();
    let recording = recordings
        .last()
        .expect("one recording")
        .join(njutest_cli::trace::FILE_NAME);
    njutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back")
}

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
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let once = || {
        let (mut said, mut complaints) = (Vec::new(), Vec::new());
        let code = njutest_cli::run_from(
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
        (code, String::from_utf8_lossy(&complaints).into_owned())
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
        established["accounting"]["mutants"]["reused_killed"].as_u64(),
        Some(0),
        "and establishes all of it itself, because there was nothing to read back yet"
    );
    let run_id = established["run_id"]
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
fn read_back(root: &std::path::Path, run_id: &str) {
    let report = report_of(root);
    assert_eq!(
        report["provenance"]["cached"],
        serde_json::Value::Bool(false),
        "a tree that changed is a tree this run answered for itself, whatever the file \
         that changed was: {report}"
    );
    assert!(
        report["accounting"]["mutants"]["reused_killed"]
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
    let sources: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|one| one["source_run_id"].as_str())
        .collect();
    assert!(
        !sources.is_empty() && sources.iter().all(|source| *source == run_id),
        "and each answer it read back names the run that established it, or a person \
         reading a kill has no way to find the execution behind it: this run says \
         {sources:?} and the first was {run_id}"
    );
}

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
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&complaints));

    let report = report_of(&root);
    let stated = report["limitations"]
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
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
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
    let complained = String::from_utf8_lossy(&complaints).into_owned();
    assert_eq!(code, 2, "{complained}");

    let stages: Vec<&str> = complained
        .lines()
        .filter_map(|line| line.strip_prefix("== "))
        .collect();
    let problems = njutest_cli::trace::check(&events_of(&root));
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
fn proved_equivalent(root: &std::path::Path) {
    let report = report_of(root);
    if !njutest_devkit::reproducible::builds_the_same_twice() {
        assert_eq!(
            report["accounting"]["mutants"]["equivalent"].as_u64(),
            Some(0),
            "a machine that renders one unchanged tree two ways establishes nothing here, \
             and a run that took its own difference for the mutation's would remove a \
             finding nobody proved: {report}"
        );
        return;
    }
    assert_eq!(
        report["accounting"]["mutants"]["equivalent"].as_u64(),
        Some(1),
        "the compiler renders `n + 0` and `n - 0` identically at this fixture's \
         optimisation level, and the tests run it, so no test could have noticed: that \
         is a finding removed rather than a survivor reported: {report}"
    );
    let surviving: Vec<&str> = report["findings"]
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
    let provider = njutest_devkit::fake_cargo::example("fake_provider");
    std::fs::write(
        root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[resources.postgres]\ncommand = [{:?}, \"resource\"]\n\
             timeout = \"10s\"\nenvironment = [\"FAKE_PROVIDER_READY\", \"FAKE_PROVIDER_STOPPED\"]\n",
            provider.to_string_lossy()
        ),
    )
    .expect("a configuration that names a resource");
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
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
    let complained = String::from_utf8_lossy(&complaints).into_owned();
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
fn leased(report: &serde_json::Value) {
    let held = report["resources"]
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
        !report["limitations"]
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
const OFFERED: &str = "Ly8gU1BEWC1GaWxlQ29weXJpZ2h0VGV4dDogMjAyNiBtanV0ZXN0IGNvbnRyaWJ1dG9ycwovLyBTUERYLUxpY2Vuc2UtSWRlbnRpZmllcjogTUlUIE9SIEFwYWNoZS0yLjAKCi8vISBPZmZlcmVkIGJ5IGEgZ2VuZXJhdGlvbiBwcm92aWRlciB0byBjbG9zZSB0aGUgZ2FwIHRoZSBpZ25vcmVkIHRlc3QgbGVmdC4KCiNbdGVzdF0KZm4gemVyb19oYXNfYV9zaWduX29mX2l0c19vd24oKSB7CiAgICBhc3NlcnRfZXEhKGZpeHR1cmVfYmFzZWxpbmU6OnNpZ24oMCksICJ6ZXJvIik7Cn0K";

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
    let provider = njutest_devkit::fake_cargo::example("fake_provider");
    std::fs::write(
        root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[generation]\ncommand = [{:?}, \"generation\"]\n\
             environment = [\"FAKE_GENERATOR_OFFERS\", \"FAKE_GENERATOR_ASKED\"]\n",
            provider.to_string_lossy()
        ),
    )
    .expect("a configuration that names a generator");
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
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
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&complaints));

    let complained = String::from_utf8_lossy(&complaints).into_owned();
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
fn questioned(asked: &std::path::Path) {
    let put = std::fs::read_to_string(asked).expect("what the generator was asked");
    let question: serde_json::Value =
        serde_json::from_str(put.lines().next().expect("one question")).expect("it is JSON");
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
fn offered_and_checked(report: &serde_json::Value, root: &std::path::Path) {
    let offered = report["candidates"]
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
    assert!(
        std::fs::read_dir(root.join(njutest_cli::repair::STORE))
            .expect("the candidates this run kept")
            .flatten()
            .count()
            > 0,
        "and what it holds up is kept, because `fix --apply` writes what was checked \
         rather than asking the generator again for something nobody put to the tests"
    );
}

/// A candidate that holds up and has nowhere to be kept.
fn unkeepable(dir: &std::path::Path, from: &std::path::Path, environment: Environment) {
    let root = from;
    let blocked = dir.join("fixture-blocked");
    copy_tree(root, &blocked);
    for gone in [".njutest", "reports"] {
        drop(std::fs::remove_dir_all(blocked.join(gone)));
    }
    std::fs::create_dir_all(blocked.join(".njutest")).expect("the directory it works in");
    std::fs::write(
        blocked.join(njutest_cli::repair::STORE),
        "a file where the candidates go",
    )
    .expect("a file where a directory belongs");
    let scratch = dir.join("scratch-again");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let elsewhere = Environment {
        working_directory: blocked.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: dir.join("cache-again"),
        ..environment
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let _code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &elsewhere,
        &mut said,
        &mut complaints,
    );
    let report = report_of(&blocked);
    assert!(
        report["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "generation-candidate-not-kept"),
        "a candidate that held up and could not be kept is one nothing can apply \
         afterwards, so the run says so rather than recording an offer whose content is \
         gone: {report}"
    );
    assert!(
        report["candidates"].as_array().is_some_and(Vec::is_empty),
        "and it does not record it, because a candidate a reader cannot get back is an \
         offer that cannot be taken up: {report}"
    );
}

/// A stream that raises `cancel` once a run has judged a mutation and started saying so about the next.
struct Interrupting<'a> {
    cancel: &'a Cancel,
    said: String,
}

impl Write for Interrupting<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.said.push_str(&String::from_utf8_lossy(buffer));
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
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let mut said = Vec::new();
    let mut complaints = Interrupting {
        cancel: &environment.cancel,
        said: String::new(),
    };
    let _code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--ui=plain"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );

    left_behind(&environment.cache_directory);

    resumed(&root, environment);
}

/// What a run that was stopped wrote where its successor will look.
fn left_behind(cache: &std::path::Path) {
    let kept: Vec<serde_json::Value> =
        std::fs::read_dir(cache.join("njutest/outcomes-v1/checkpoints"))
            .expect("the checkpoints directory")
            .flatten()
            .filter_map(|entry| {
                std::fs::read_to_string(entry.path().join(njutest_cli::checkpoint::FILE_NAME)).ok()
            })
            .filter_map(|text| serde_json::from_str(&text).ok())
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
            .all(|one| one["killed_by"].as_str().is_some_and(|by| !by.is_empty())),
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
fn resumed(root: &std::path::Path, environment: Environment) {
    let environment = Environment {
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
        ..environment
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--ui=plain"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&complaints));
    let report = report_of(root);
    assert_eq!(
        report["provenance"]["cached"],
        serde_json::Value::Bool(false),
        "and the next run of the same tree establishes it rather than reading back what \
         the stopped one reached. A run that was told to stop stopped: its answer is \
         what it got through, and storing that under the tree's identity would hand the \
         next run a partial measurement wearing a whole one's name: {report}"
    );
    assert!(
        report["limitations"]
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
    let scratch = dir.join(format!("{name}-scratch"));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.join(format!("{name}-cache")),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert!(
        matches!(code, 0..=2),
        "{name} reached a verdict rather than a failure: {}",
        String::from_utf8_lossy(&complaints)
    );
    report_of(&root)
}

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

/// One part of `fixture`'s catalog, judged in this process.
fn part(root: &std::path::Path, dir: &std::path::Path, shard: &str) -> serde_json::Value {
    let scratch = dir.join(format!("part-{}-scratch", shard.replace('/', "-")));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.join("parts-cache"),
        working_directory: root.to_path_buf(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
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
    assert_eq!(code, 0, "{shard}: {}", String::from_utf8_lossy(&complaints));
    report_of(root)
}

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
    let one = part(&root, dir.path(), "1/2");
    let two = part(&root, dir.path(), "2/2");
    assert_ne!(one["run_id"], two["run_id"], "two runs, two reports");

    let parts: Vec<njutest_cli::report::Report> = [&one, &two]
        .into_iter()
        .map(|document| {
            njutest_cli::report::json::parse(&document.to_string()).expect("a part reads back")
        })
        .collect();
    let merged = njutest_cli::report::merge::merge(&parts).expect("two parts of one catalog");
    let combined = serde_json::to_value(&merged).expect("the whole is a document");

    assert_eq!(
        njutest_devkit::report::normalize(&combined),
        njutest_devkit::report::normalize(&whole),
        "dividing the work is not a budget only if the pieces add back up to it. A run \
         cut in two and put back together has to say what one run of the same tree says \
         — the same verdict, the same counts, the same row for every mutation — or \
         --shard is a way of getting a different answer cheaply"
    );
}

/// One run of `fixture` in this process under `configured`, and what it exited with rather than the report it did not write.
fn refused(fixture: &str, dir: &std::path::Path, name: &str, configured: &str) -> (u8, String) {
    let root = dir.join(name);
    copy_tree(&njutest_devkit::paths::fixtures_dir().join(fixture), &root);
    std::fs::write(root.join(".njutest.toml"), configured).expect("a configuration");
    let scratch = dir.join(format!("{name}-scratch"));
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let environment = Environment {
        cache_directory: dir.join(format!("{name}-cache")),
        working_directory: root,
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_run(),
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    (code, String::from_utf8_lossy(&complaints).into_owned())
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
        report["contract"], "deep-v1",
        "the run answers to the contract it was given: {report}"
    );
    assert_eq!(
        report["accounting"]["soundness"]["executed"],
        serde_json::Value::Bool(true),
        "and deep-v1 promises the suite is interpreted, so a run of it that did not \
         interpret anything and still reached a verdict would be the promise unkept in \
         the one place nobody looks: {report}"
    );
    let stated: Vec<&str> = report["limitations"]
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

    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked", "--no-cache"].map(OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    assert_eq!(code, 2, "{}", String::from_utf8_lossy(&complaints));

    let report = report_of(&root);
    let stated: Vec<&str> = report["limitations"]
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
        report["findings"]
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
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };
    let mut args: Vec<OsString> = ["njutest", "verify", "--offline", "--locked", "--no-cache"]
        .map(OsString::from)
        .to_vec();
    args.extend(extra.iter().map(|one| OsString::from(*one)));

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(args, &environment, &mut said, &mut complaints);
    let _kept = String::from_utf8_lossy(&said).into_owned();
    (
        code,
        String::from_utf8_lossy(&complaints).into_owned(),
        root,
    )
}

#[test]
fn a_mutation_that_never_returns_is_stopped_measured_alone_and_reported_as_a_timeout() {
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
        .filter(|exec| exec.outcome == "timed_out" && !exec.alone)
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
        report["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|one| expired.contains(one["display_id"].as_str().unwrap_or_default()))
            .all(|one| one["outcome"] != "timed_out"),
        "and what the second measurement said is what the mutation is reported as: the \
         first one is how long it took, not what it established: {report}"
    );
}

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
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let vars: Vec<(OsString, OsString)> = njutest_devkit::paths::environment_for_a_run();
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.clone(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
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
        String::from_utf8_lossy(&said),
        String::from_utf8_lossy(&complaints)
    );

    let report = report_of(&root);
    assert_eq!(
        report["repository"]["git"]["available"],
        serde_json::Value::Bool(false),
        "a copied fixture is not a repository, and the report says what git answered \
         rather than leaving the question unasked: {report}"
    );
    let stated: Vec<&str> = report["limitations"]
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
    let skipped = report["limitations"]
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

    stages_of(&String::from_utf8_lossy(&complaints), &root);

    accounted(&report, &root);
}

/// What a whole run's report says about the targets and the mutations it judged.
fn accounted(report: &serde_json::Value, root: &std::path::Path) {
    let targets = report["targets"].as_array().expect("target records");
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
    assert_eq!(
        report["accounting"]["targets"]["selected"].as_u64(),
        u64::try_from(targets.len()).ok(),
        "the counts and the records are two ways of saying one thing: {report}"
    );
    assert!(
        report["accounting"]["mutants"]["cataloged"]
            .as_u64()
            .is_some_and(|counted| counted > 0)
            && !report["mutants"].as_array().expect("mutants").is_empty(),
        "and a run that catalogued mutations records what became of each: {report}"
    );
    assert!(
        report["timing"]["finished"]
            .as_str()
            .is_some_and(|when| !when.is_empty()),
        "a report that never says when it finished is one nothing can be compared \
         against: {report}"
    );
    provenance(report);
    concluded(report, root);
}

/// What a report says about the thing it is about and what produced it.
fn provenance(report: &serde_json::Value) {
    for named in ["rustc", "cargo", "target", "os", "arch"] {
        assert!(
            report["toolchain"][named]
                .as_str()
                .is_some_and(|said| !said.is_empty()),
            "and it says what built the thing it is about, because the same suite \
             under two compilers is two answers: {named} is missing from {report}"
        );
    }
    assert_eq!(
        report["repository"]["packages"],
        serde_json::json!(["fixture-baseline"]),
        "and which packages the workspace holds, which is the scope every count in it \
         is over: {report}"
    );
    assert!(
        report["resources"].as_array().is_some_and(Vec::is_empty),
        "a run that was given nothing to hold says so rather than leaving the question \
         unasked, because a resource nobody released is one the next run waits for: \
         {report}"
    );
}

/// What a whole run of the baseline fixture concludes about the mutations it judged.
fn concluded(report: &serde_json::Value, root: &std::path::Path) {
    assert_eq!(report["verdict"], "INSUFFICIENT");
    assert_eq!(
        (
            report["accounting"]["mutants"]["killed"].as_u64(),
            report["accounting"]["mutants"]["survived"].as_u64(),
            report["accounting"]["mutants"]["unreached"].as_u64(),
        ),
        (Some(7), Some(2), Some(1)),
        "and every mutation is in the column it belongs to. A phase that reported them \
         all as unnoticed, or all as something nothing could decide, reaches the same \
         verdict on this fixture by a route that says nothing about the suite: {report}"
    );
    let named: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|one| one["killed_by"].as_str())
        .collect();
    assert_eq!(
        named.len(),
        7,
        "and each kill names the test that noticed, because the whole of what a kill \
         hands a person is where to look: {named:?}"
    );
    paced(report, root);
}

/// That the phase counted its mutations off one at a time to whoever was watching.
fn paced(report: &serde_json::Value, root: &std::path::Path) {
    let counted = report["accounting"]["mutants"]["cataloged"]
        .as_u64()
        .expect("how many were judged");
    let said: Vec<(u64, u64)> = events_of(root)
        .iter()
        .filter_map(|event| match &event.payload {
            njutest_cli::trace::Payload::Progress { progress } => {
                Some((progress.done?, progress.total?))
            }
            _ => None,
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
