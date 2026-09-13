// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a verification leaves behind when the process is asked to stop: the exit code the contract names, and no descendant process.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
        .prefix("njutest-interrupt-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_njutest"))
        .args(["verify", "--offline", "--locked", "--ui=plain"])
        .current_dir(&root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            njutest_devkit::paths::cache_beside(&root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            njutest_devkit::paths::temp_beside(&root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME"
            )
        }))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("njutest starts");
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
    Command::new(env!("CARGO_BIN_EXE_njutest"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME"
            )
        }))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("njutest starts")
}

#[test]
fn an_interrupted_run_leaves_what_an_earlier_one_established_rather_than_clearing_it() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-resume-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-assured"),
        &root,
    );

    let checkpoints = njutest_devkit::paths::cache_beside(&root)
        .expect("a cache directory")
        .join("njutest/outcomes-v1/checkpoints");
    let seeded = checkpoints.join("an-earlier-run");
    std::fs::create_dir_all(&seeded).expect("mkdir");
    std::fs::write(
        seeded.join("checkpoint-v1.json"),
        serde_json::to_string(&serde_json::json!({
            "schema": "njutest-assurance-checkpoint-v1",
            "identity": "an-earlier-run",
            "attempts": 1,
            "targets": [],
            "mutants": [{
                "id": "0f7b4d7472329894e9b3",
                "disposition": "killed",
                "killed_by": "fixture-assured/lib/fixture_assured",
                "duration_ms": 1,
            }],
        }))
        .expect("the state renders"),
    )
    .expect("write");

    let mut interrupted = verify_in(&root, &[]);
    let pid = interrupted.id();
    let mut reader = BufReader::new(interrupted.stderr.take().expect("stderr is piped"));
    let mut line = String::new();
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .expect("a deadline five minutes out");
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read");
        assert!(read > 0, "the run ended before it measured anything");
        if line.contains("== baseline") {
            break;
        }
        assert!(Instant::now() < deadline, "the run never began measuring");
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");
    assert_eq!(interrupted.wait().expect("the run ends").code(), Some(130));

    let states = written_states(&checkpoints);
    assert!(
        !states.is_empty(),
        "a run that stopped is not a run that finished, and clearing what an earlier \
         one established would make an interrupt cost the whole run — which is the \
         opposite of what a checkpoint is for: {states:?}"
    );
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&states[0]).expect("the state"))
            .expect("the state is a document");
    assert_eq!(state["schema"], "njutest-assurance-checkpoint-v1");
    assert!(
        !state["mutants"].as_array().expect("mutants").is_empty(),
        "the mutants an earlier run judged are still there: {state}"
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
