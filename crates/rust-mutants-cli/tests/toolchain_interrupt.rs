// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind when it is interrupted: an exit code that says so, a forcefully-signalled inherited process group, and no temporary tree.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

include!("support/metadata.rs");
include!("support/missing.rs");
include!("support/regular_file.rs");

/// A test process backed by the shared non-detachable child owner.
struct RunningChild(njutest_devkit::process::SupervisedChild);

impl RunningChild {
    fn launch(command: &mut Command) -> Self {
        Self(
            njutest_devkit::process::SupervisedChild::launch(command).expect("rust-mutants starts"),
        )
    }

    fn id(&self) -> u32 {
        self.0.id().expect("the running child is still owned")
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, njutest_devkit::process::ChildError> {
        self.0.try_wait()
    }

    fn wait_with_output(self) -> Result<Output, njutest_devkit::process::ChildError> {
        self.0.wait_with_output()
    }
}

/// Every observable process on Linux whose process group is `group`; other POSIX kernels expose no `/proc` assertion surface here.
fn in_group(group: u32) -> Option<Vec<u32>> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return None;
    };
    for entry in entries.map(|entry| entry.expect("read a /proc entry")) {
        let name = njutest_devkit::paths::owned_utf8(entry.file_name());
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(after) = stat.rsplit_once(american_parenthesis()) else {
            continue;
        };
        let fields: Vec<&str> = after.1.split_whitespace().collect();
        let parsed_group = fields.get(2).and_then(|value| match value.parse::<u32>() {
            Ok(group) => Some(group),
            Err(_) => None,
        });
        if parsed_group == Some(group) && fields.first() != Some(&"Z") {
            found.push(pid);
        }
    }
    Some(found)
}

const fn american_parenthesis() -> char {
    ')'
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerWait {
    Ready,
    ChildExited,
}

fn await_marker_or_child_exit(
    child: &mut RunningChild,
    marker: &Path,
    deadline: Instant,
) -> std::io::Result<MarkerWait> {
    loop {
        if test_regular_file(marker) {
            return Ok(MarkerWait::Ready);
        }
        if child.try_wait().map_err(std::io::Error::other)?.is_some() {
            return Ok(MarkerWait::ChildExited);
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the run never reached a mutant test",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// An interrupted run still writes what it did establish, and says what it never reached.
fn what_it_established(root: &Path) {
    let directory = rust_mutants_cli::app::stored::Store::read(root).root();
    let pointer = directory.join("latest.json");
    assert!(
        test_metadata(&pointer).is_file(),
        "an interrupted run still writes what it did establish"
    );
    let value: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&pointer).expect("the pointer"),
    )
    .expect("the pointer is a document");
    let relative = value["document"].as_str().expect("a document path");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(directory.join(relative)).expect("the report"),
    )
    .expect("the report is a document");
    assert_eq!(
        document["run"]["interrupted"].as_bool(),
        Some(true),
        "{document}"
    );
    assert_eq!(document["run"]["exit_code"], 130, "{document}");
    assert!(
        document["accounting"]["not_run"].as_u64().expect("a count") > 0,
        "an interrupted run says which mutants it never reached: {document}"
    );
}

#[test]
fn a_run_that_is_interrupted_exits_130_and_releases_its_snapshot() {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-interrupt-")
        .tempdir()
        .expect("tempdir");
    let directory = std::fs::canonicalize(dir.path()).expect("the physical temporary directory");
    let root = directory.join("fixture-simple");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        &root,
    );
    let temp = directory.join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");
    let cache = directory.join("cache");
    std::fs::create_dir_all(&cache).expect("mkdir");
    let mutant_started = directory.join("mutant-started");

    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .args(["run", "--offline", "--locked", "--tier", "all"])
        .args(["--root", njutest_devkit::paths::utf8(&root)])
        .env("NO_COLOR", "1")
        .env("TMPDIR", &temp)
        .env("XDG_CACHE_HOME", &cache)
        .env("FIXTURE_SIMPLE_PAUSE_MS", "400")
        .env("FIXTURE_SIMPLE_MUTANT_STARTED", &mutant_started)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = RunningChild::launch(&mut command);
    let pid = child.id();

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .expect("a deadline five minutes out");
    match await_marker_or_child_exit(&mut child, &mutant_started, deadline)
        .expect("the run reaches a mutant test before the deadline")
    {
        MarkerWait::Ready => {}
        MarkerWait::ChildExited => {
            let output = child.wait_with_output().expect("collect the ended command");
            assert!(
                test_regular_file(&mutant_started),
                "the run ended before a mutant test began: status={:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                njutest_devkit::process::strict_utf8(&output.stdout),
                njutest_devkit::process::strict_utf8(&output.stderr)
            );
            return;
        }
    }

    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");

    let output = child.wait_with_output().expect("the run ends");
    assert_eq!(
        output.status.code(),
        Some(130),
        "an interrupted run says so in its exit code rather than in a crash: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    what_it_established(&root);

    if let Some(stragglers) = in_group(pid) {
        assert!(
            stragglers.is_empty(),
            "the run left observable members {stragglers:?} in its inherited process group"
        );
    }

    let left: Vec<PathBuf> = std::fs::read_dir(&temp)
        .expect("the temporary directory")
        .map(|entry| entry.expect("read a temporary-directory entry"))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                njutest_devkit::paths::utf8(Path::new(name)).starts_with("rust-mutants-snap-")
            })
        })
        .collect();
    assert!(
        left.is_empty(),
        "an interrupted run removes its snapshot like any other: {left:?}"
    );
}

#[test]
fn ctrl_c_during_a_compilation_exits_130_and_writes_no_rejection() {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-interrupt-build-")
        .tempdir()
        .expect("tempdir");
    let directory = std::fs::canonicalize(dir.path()).expect("the physical temporary directory");
    let root = directory.join("fixture-build-script");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-build-script"),
        &root,
    );
    let temp = directory.join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");
    let cache = directory.join("cache");
    std::fs::create_dir_all(&cache).expect("mkdir");
    let marker = directory.join("compiling");

    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .args(["run", "--offline", "--locked", "--tier", "all"])
        .args(["--root", njutest_devkit::paths::utf8(&root)])
        .env("NO_COLOR", "1")
        .env("TMPDIR", &temp)
        .env("XDG_CACHE_HOME", &cache)
        .env("FIXTURE_BUILD_SCRIPT_PAUSE_MS", "20000")
        .env("FIXTURE_BUILD_SCRIPT_MARKER", &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = RunningChild::launch(&mut command);
    let pid = child.id();

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .expect("a deadline five minutes out");
    while !test_regular_file(&marker) {
        assert!(Instant::now() < deadline, "the run never reached a build");
        std::thread::sleep(Duration::from_millis(50));
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");

    let output = child.wait_with_output().expect("the run ends");
    assert_eq!(
        output.status.code(),
        Some(130),
        "a build nobody waited for is a cancellation, not a tree that does not compile: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        said.contains("RM0001"),
        "the cancellation says so by its own code: {said}"
    );
    assert!(
        test_missing(&rust_mutants_cli::app::stored::Store::read(&root).root()),
        "a run that never got a session establishes nothing and writes nothing"
    );

    if let Some(stragglers) = in_group(pid) {
        assert!(
            stragglers.is_empty(),
            "the run left observable members {stragglers:?} in its inherited process group"
        );
    }
}
