// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A watch against a real workspace: the round it runs before anything changes, and the verdict it carries out of it.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use mjutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

/// Everything the watch said, stopping it once it says it is waiting or once a second round has answered.
///
/// Both bounds are what makes this a test rather than a hang: a watch whose
/// loop lost its stopping rule would otherwise keep this process alive until
/// something outside killed it, and a killed test reports nothing.
struct Stopping<'a> {
    cancel: &'a Cancel,
    said: String,
}

impl Write for Stopping<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.said.push_str(&String::from_utf8_lossy(buffer));
        if self.said.contains("waiting\tfor the next change")
            || self.said.matches("VERDICT").count() > 1
        {
            self.cancel.cancel();
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn copy(from: &Path, to: &Path) {
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
fn a_watch_verifies_the_tree_as_it_stands_and_carries_that_round_s_verdict() {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-watch-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let scratch = dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("a scratch directory");

    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let environment = Environment {
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: root,
        temp_directory: scratch,
        vars,
        cancel: Cancel::new(),
    };

    let watchdog = environment.cancel.clone();
    let (done, waited) = channel::<()>();
    let bound = std::thread::spawn(move || {
        if waited.recv_timeout(Duration::from_secs(240)) == Err(RecvTimeoutError::Timeout) {
            watchdog.cancel();
        }
    });

    let mut output = Stopping {
        cancel: &environment.cancel,
        said: String::new(),
    };
    let mut complaints = Vec::new();
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
    bound.join().expect("the bound");

    let said = output.said;
    assert!(
        said.contains("waiting\tfor the next change"),
        "a watch that has answered for the tree in front of it says it is waiting, \
         because a round that ends in silence reads as one that is still running: \
         {said}\n{}",
        String::from_utf8_lossy(&complaints)
    );
    assert_eq!(
        code, 2,
        "and the verdict it carries out is the round's own: this fixture is \
         INSUFFICIENT, and a watch that reported success on it would be a green \
         terminal for a suite with a gap in it: {said}"
    );
}
