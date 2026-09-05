// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind when it is interrupted: an exit code that says so, no descendant process, and no temporary tree.

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

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

/// Every process on this machine whose process group is `group`, which is the tree the engine puts a test process in.
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
        let Some(after) = stat.rsplit_once(american_parenthesis()) else {
            continue;
        };
        let fields: Vec<&str> = after.1.split_whitespace().collect();
        if fields.get(2).and_then(|value| value.parse::<u32>().ok()) == Some(group)
            && fields.first() != Some(&"Z")
        {
            found.push(pid);
        }
    }
    found
}

const fn american_parenthesis() -> char {
    ')'
}

/// An interrupted run still writes what it did establish, and says what it never reached.
fn what_it_established(root: &Path) {
    let directory = root.join("reports/mutation");
    let pointer = directory.join("latest.json");
    assert!(
        pointer.is_file(),
        "an interrupted run still writes what it did establish"
    );
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pointer).expect("the pointer"))
            .expect("the pointer is a document");
    let relative = value["document"].as_str().expect("a document path");
    let document: serde_json::Value = serde_json::from_str(
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
fn a_run_that_is_interrupted_exits_130_and_leaves_no_process_and_no_snapshot_behind() {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-interrupt-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join("fixture-simple");
    copy_dir(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        &root,
    );
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(["run", "--offline", "--locked", "--tier", "all"])
        .args(["--root", &root.to_string_lossy()])
        .env("NO_COLOR", "1")
        .env("TMPDIR", &temp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rust-mutants starts");
    let pid = child.id();

    let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read");
        assert!(read > 0, "the run ended before it executed a mutant");
        if line.starts_with("[1/") {
            break;
        }
        assert!(Instant::now() < deadline, "the run never reached a mutant");
    }

    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");

    let status = child.wait().expect("the run ends");
    assert_eq!(
        status.code(),
        Some(130),
        "an interrupted run says so in its exit code rather than in a crash"
    );

    what_it_established(&root);

    let stragglers = in_group(pid);
    assert!(
        stragglers.is_empty(),
        "the run left {stragglers:?} behind in its own process group"
    );

    let left: Vec<PathBuf> = std::fs::read_dir(&temp)
        .expect("the temporary directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("rust-mutants-snap-"))
        })
        .collect();
    assert!(
        left.is_empty(),
        "an interrupted run removes its snapshot like any other: {left:?}"
    );
}
