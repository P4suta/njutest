// SPDX-FileCopyrightText: 2026 mjutest contributors
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

use mjutest_cli::cli::{EXIT_ERROR, Environment};
use rust_mutants::runner::Cancel;

/// The longest this test will wait for a round that is not coming.
///
/// The round here answers in well under a second, so this is an order of
/// magnitude of headroom and never a bound the work runs into. It is here
/// because the alternative bound is a line the round prints, and a test whose
/// only bound is something the code under test says stops terminating the
/// moment that code loses the line. A test that hangs when a rule goes missing
/// is not a test that holds the rule: it is one something outside has to kill,
/// and a killed test reports nothing.
///
/// It is ten seconds and not sixty because a bound only helps while it is
/// shorter than whatever else would stop the process first. At sixty a
/// measurement's own patience ran out before this did, and four rules of the
/// watch loop came back as a mutation that timed out rather than one a test
/// caught — the same finding to a reader counting survivors, and nothing at
/// all to one asking which rule is held.
const LONGEST: Duration = Duration::from_secs(10);

/// One of the watch's two streams, stopping it once the round has been.
///
/// Both streams share one flag, and the round here is one the workspace makes
/// fail, so its complaint on the error stream is the signal that it has run.
/// Stopping on *that* rather than only on the line being asserted is what
/// keeps every later assertion an assertion: a round that printed the wrong
/// thing on the other stream, or nothing at all, still ends the loop and still
/// fails here rather than running out the clock.
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

#[test]
fn a_watch_verifies_the_tree_as_it_stands_and_carries_that_round_s_verdict() {
    let root = tempfile::Builder::new()
        .prefix("mjutest-watch-")
        .tempdir()
        .expect("a temporary directory");
    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname = \"\"\n")
        .expect("a manifest cargo will refuse");
    let scratch = root.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a scratch directory");

    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.path().to_owned(),
        temp_directory: scratch,
        vars,
        cancel: Cancel::new(),
    };

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
    let code = mjutest_cli::run_from(
        [
            "mjutest",
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
        .prefix("mjutest-nowhere-")
        .tempdir()
        .expect("a temporary directory");
    let occupied = root.path().join("occupied");
    std::fs::write(&occupied, "not a directory").expect("a file where a scratch goes");

    let environment = Environment {
        cache_directory: root.path().to_owned(),
        working_directory: root.path().to_owned(),
        temp_directory: occupied,
        vars: Vec::new(),
        cancel: Cancel::new(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        ["mjutest", "verify", "--offline", "--locked", "--trace"].map(OsString::from),
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

    let recording = std::fs::read_dir(root.path().join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path().join(mjutest_cli::trace::FILE_NAME))
        .next()
        .expect("one recording");
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back");
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
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
        .prefix("mjutest-nocargo-")
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
        vars: vec![(
            OsString::from("PATH"),
            OsString::from(empty.display().to_string()),
        )],
        cancel: Cancel::new(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        ["mjutest", "verify", "--offline", "--locked"].map(OsString::from),
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
        vars: std::env::vars_os()
            .filter(|(name, _value)| name == "PATH")
            .collect(),
        cancel: Cancel::new(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let _code = mjutest_cli::run_from(
        ["mjutest", "verify", "--offline", "--locked"].map(OsString::from),
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
    let problems = mjutest_cli::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let recorded: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
            _ => None,
        })
        .collect();

    let routed: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Route { route } => Some(route.mutant.as_str()),
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
            mjutest_cli::trace::Payload::Progress { progress } => Some(progress.message.as_str()),
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
fn judged(events: &[mjutest_cli::trace::Event]) {
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::PhaseStart { phase } => Some(phase.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        phases.contains(&"mutation-judge"),
        "the phase that takes the time names itself in the recording, or the minutes \
         between two events belong to nothing anybody can name: {phases:?}"
    );

    let routes: Vec<&mjutest_cli::trace::RouteRecord> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Route { route } => Some(route),
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

    let probes: Vec<&mjutest_cli::trace::ProbeExecRecord> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::ProbeExec { probe } => Some(probe),
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
    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("reports/latest-any.json")).expect("the latest index"),
    )
    .expect("the index is JSON");
    let directory = index["directory"].as_str().expect("the run's directory");
    serde_json::from_str(
        &std::fs::read_to_string(
            root.join(directory)
                .join("mjutest-assurance-report-v1.json"),
        )
        .expect("the report"),
    )
    .expect("the report is JSON")
}

/// What a run whose every measurement was slow exactly once concludes.
fn once_slow(report: &serde_json::Value) {
    assert_eq!(
        report["accounting"]["mutants"]["killed"].as_u64(),
        Some(6),
        "every mutation but one is noticed here, and by the second measurement rather \
         than the first: {report}"
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

/// Every mutation execution a recording holds.
fn executions(events: &[mjutest_cli::trace::Event]) -> Vec<&mjutest_cli::trace::MutantExecRecord> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::MutantExec { mutant } => Some(mutant),
            _ => None,
        })
        .collect()
}

/// Every event the latest recording under `root` holds.
fn events_of(root: &std::path::Path) -> Vec<mjutest_cli::trace::Event> {
    let recording = std::fs::read_dir(root.join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path().join(mjutest_cli::trace::FILE_NAME))
        .next()
        .expect("one recording");
    mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back")
}

#[test]
fn a_second_run_of_one_tree_reads_back_what_the_first_established_and_says_whose_it_is() {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-again-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let environment = Environment {
        cache_directory: dir.path().join("cache"),
        working_directory: root.clone(),
        temp_directory: scratch,
        vars,
        cancel: Cancel::new(),
    };
    let once = || {
        let (mut said, mut complaints) = (Vec::new(), Vec::new());
        let code = mjutest_cli::run_from(
            ["mjutest", "verify", "--offline", "--locked", "--ui=plain"].map(OsString::from),
            &environment,
            &mut said,
            &mut complaints,
        );
        (code, String::from_utf8_lossy(&complaints).into_owned())
    };

    let (first, complained) = once();
    assert_eq!(first, 2, "the first run establishes it: {complained}");
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
    let read_back = report_of(&root);
    assert_eq!(
        read_back["provenance"]["cached"],
        serde_json::Value::Bool(false),
        "a tree that changed is a tree this run answered for itself, whatever the file \
         that changed was: {read_back}"
    );
    assert!(
        read_back["accounting"]["mutants"]["reused_killed"]
            .as_u64()
            .is_some_and(|counted| counted > 0),
        "reading back what an earlier run of this exact tree established is the whole \
         of why a second run is cheap, and a run that established it all again would be \
         doing the work twice while reporting that it had not: {read_back}"
    );
    let sources: Vec<&str> = read_back["mutants"]
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

/// A run in this process against `fixture`, and what it left behind.
fn verified_in_process(
    fixture: &str,
    dir: &std::path::Path,
    extra: &[&str],
    carrying: &[(&str, &str)],
) -> (u8, String, std::path::PathBuf) {
    let root = dir.join(fixture);
    copy(&mjutest_devkit::paths::fixtures_dir().join(fixture), &root);
    if !carrying.is_empty() {
        let named: Vec<String> = carrying
            .iter()
            .map(|(name, _value)| format!("{name:?}"))
            .collect();
        std::fs::write(
            root.join(".mjutest.toml"),
            format!(
                "version = 1\n\n[execution]\ntimeout = \"1s\"\nenvironment = [{}]\n",
                named.join(", ")
            ),
        )
        .expect("a configuration this run reads");
    }
    let scratch = dir.join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    vars.extend(
        carrying
            .iter()
            .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
    );
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.clone(),
        temp_directory: scratch,
        vars,
        cancel: Cancel::new(),
    };
    let mut args: Vec<OsString> = ["mjutest", "verify", "--offline", "--locked", "--no-cache"]
        .map(OsString::from)
        .to_vec();
    args.extend(extra.iter().map(|one| OsString::from(*one)));

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(args, &environment, &mut said, &mut complaints);
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
        .prefix("mjutest-hang-")
        .tempdir()
        .expect("a temporary directory");
    let paused = dir.path().join("paused");
    let (code, complained, root) = verified_in_process(
        "fixture-hang",
        dir.path(),
        &["--trace", "--ui=plain"],
        &[
            ("FIXTURE_HANG_MARKER", &paused.display().to_string()),
            ("FIXTURE_HANG_PAUSE_MS", "4000"),
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

fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            let _bytes = std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

#[test]
fn a_run_in_this_process_writes_what_it_learned_before_it_compiled_anything() {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-inprocess-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");

    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root.clone(),
        temp_directory: scratch,
        vars,
        cancel: Cancel::new(),
    };

    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        [
            "mjutest",
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
            .is_some_and(|detail| detail.starts_with("1 places were not mutated")),
        "and how much of the tree it did not mutate, with the number: a run that says \
         it skipped something without saying how much reads as a footnote, and the \
         difference between one place and four hundred is the difference between a \
         report about this workspace and a report about a corner of it: {skipped}"
    );

    stages_of(&String::from_utf8_lossy(&complaints), &root);

    accounted(&report);
}

/// What a whole run's report says about the targets and the mutations it judged.
fn accounted(report: &serde_json::Value) {
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
    assert_eq!(report["verdict"], "INSUFFICIENT");
    assert_eq!(
        report["accounting"]["mutants"]["killed"].as_u64(),
        Some(7),
        "and what the tests did notice is counted: a phase that reported every \
         mutation as unnoticed would reach the same verdict on this fixture by a route \
         that says nothing about the suite: {report}"
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
}
