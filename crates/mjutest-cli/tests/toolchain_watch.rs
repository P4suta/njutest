// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A watch against a real workspace: the round it runs before anything changes, and the verdict it carries out of it.

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
        ["mjutest", "verify", "--offline", "--locked"].map(OsString::from),
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
}
