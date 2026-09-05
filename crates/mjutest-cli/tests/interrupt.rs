// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a verification leaves behind when the process is asked to stop: the exit code the contract names, and no descendant process.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let kind = entry.file_type().expect("a file type");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

/// Every live process on this machine whose process group is `group`.
fn in_group(group: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some((_name, after)) = stat.rsplit_once(')') else {
            continue;
        };
        let fields: Vec<&str> = after.split_whitespace().collect();
        if fields.get(2).and_then(|value| value.parse::<u32>().ok()) == Some(group)
            && fields.first() != Some(&"Z")
        {
            found.push(pid);
        }
    }
    found
}

fn interrupted_by(signal: rustix::process::Signal, expected: i32) {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-interrupt-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(["verify", "--offline", "--locked", "--ui=plain"])
        .current_dir(&root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            mjutest_devkit::paths::cache_beside(&root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            mjutest_devkit::paths::temp_beside(&root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mjutest starts");
    let pid = child.id();

    std::thread::sleep(Duration::from_millis(400));
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        signal,
    )
    .expect("the signal is delivered");

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .expect("a deadline five minutes out");
    let status = loop {
        match child.try_wait().expect("the child is ours") {
            Some(status) => break status,
            None => assert!(
                Instant::now() < deadline,
                "the run did not stop when it was asked to"
            ),
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "a run that was asked to stop says so in its exit code rather than in a crash"
    );
    let stragglers = in_group(pid);
    assert!(
        stragglers.is_empty(),
        "the run left {stragglers:?} behind in its own process group"
    );
}

#[test]
fn an_interrupted_verification_exits_130_and_leaves_no_process_behind() {
    interrupted_by(rustix::process::Signal::INT, 130);
}

#[test]
fn a_terminated_verification_exits_143_and_leaves_no_process_behind() {
    interrupted_by(rustix::process::Signal::TERM, 143);
}

fn verify_in(root: &Path, extra: &[&str]) -> std::process::Child {
    let mut args = vec!["verify", "--offline", "--locked", "--ui=plain"];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            mjutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            mjutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mjutest starts")
}

#[test]
fn an_interrupted_run_leaves_scheduling_state_for_the_next_one() {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-resume-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-assured"),
        &root,
    );

    let mut first = verify_in(&root, &[]);
    let pid = first.id();
    let mut reader = BufReader::new(first.stderr.take().expect("stderr is piped"));
    let mut line = String::new();
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .expect("a deadline five minutes out");
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read");
        assert!(read > 0, "the run ended before it measured anything");
        if line.contains("[2/") && line.contains("fixture-assured") {
            break;
        }
        assert!(Instant::now() < deadline, "the run never measured a target");
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");
    assert_eq!(first.wait().expect("the run ends").code(), Some(130));

    let checkpoints = mjutest_devkit::paths::cache_beside(&root)
        .expect("a cache directory")
        .join("mjutest/outcomes-v1/checkpoints");
    let states = written_states(&checkpoints);
    assert_eq!(
        states.len(),
        1,
        "an interrupted run leaves what it established for the next one: {states:?}"
    );
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&states[0]).expect("the state"))
            .expect("the state is a document");
    assert_eq!(state["schema"], "mjutest-assurance-checkpoint-v1");
    assert!(
        !state["targets"].as_array().expect("targets").is_empty(),
        "the target it had already measured is in it: {state}"
    );
}

/// Every checkpoint file under `root`.
fn written_states(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path().join("checkpoint-v1.json");
        if path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    found
}
