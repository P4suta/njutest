// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test whose count a process it starts keeps: the test binary again, running the same test, so a stop at the call happens in that process and not in the test's.

use std::path::{Path, PathBuf};

/// Set in the process a test starts, which then keeps the count rather than starting another.
const CHILD: &str = "FIXTURE_DURABLE_CALLS_CHILD";

fn kept(name: &str) -> PathBuf {
    let directory = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .expect("every run gives its tests a temporary directory")
        .join("fixture-durable-calls");
    std::fs::create_dir_all(&directory).expect("a directory to keep the count in");
    directory.join(name)
}

fn load(path: &Path) -> u32 {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .strip_prefix("count=")
            .expect("the file holds a count")
            .parse()
            .expect("the count is a number"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("the count reads: {error}"),
    }
}

#[test]
fn a_count_kept_by_a_child_goes_up() {
    let path = kept("child");
    let count = load(&path);
    if std::env::var_os(CHILD).is_some() {
        fixture_durable_calls::save_from_a_child(&path, &format!("count={}", count + 1));
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "a_count_kept_by_a_child_goes_up",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .status()
        .expect("the child starts");
    assert!(status.success(), "the child keeps the count: {status}");
    assert_eq!(load(&path), count + 1);
}

#[test]
fn a_count_nobody_kept_reads_as_none() {
    assert_eq!(load(&kept("never")), 0);
}
