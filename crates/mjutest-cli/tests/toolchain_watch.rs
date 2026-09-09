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
/// The round here answers in milliseconds, so this is three orders of
/// magnitude of headroom and never a bound the work runs into. It is here
/// because the alternative bound is a line the round prints, and a test whose
/// only bound is something the code under test says stops terminating the
/// moment that code loses the line. A test that hangs when a rule goes missing
/// is not a test that holds the rule: it is one something outside has to kill,
/// and a killed test reports nothing.
const LONGEST: Duration = Duration::from_secs(60);

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
    let recording = std::fs::read_dir(root.join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path().join(mjutest_cli::trace::FILE_NAME))
        .next()
        .expect("one recording");
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&recording).expect("the stream"),
    ))
    .expect("the events read back");
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

    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("reports/latest-any.json")).expect("the latest index"),
    )
    .expect("the index is JSON");
    let directory = index["directory"].as_str().expect("the run's directory");
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join(directory)
                .join("mjutest-assurance-report-v1.json"),
        )
        .expect("the report"),
    )
    .expect("the report is JSON");

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
}
