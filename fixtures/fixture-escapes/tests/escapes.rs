// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starts a process in a process group of its own, which outlives the test, and says which it is where `FIXTURE_ESCAPES_RECORD` names; with `FIXTURE_ESCAPES_HOLDS_OUTPUT` set it keeps the test's output open too.

#[cfg(unix)]
#[test]
fn a_daemon_the_test_starts_keeps_running_after_it() {
    use std::os::unix::process::CommandExt as _;
    if let Some(record) = std::env::var_os("FIXTURE_ESCAPES_RECORD") {
        let daemon = if std::env::var_os("FIXTURE_ESCAPES_HOLDS_OUTPUT").is_some() {
            "sleep 120 & echo $! >> \"$FIXTURE_ESCAPES_RECORD\""
        } else {
            "sleep 120 >/dev/null 2>&1 & echo $! >> \"$FIXTURE_ESCAPES_RECORD\""
        };
        let started = std::process::Command::new("sh")
            .arg("-c")
            .arg(daemon)
            .env("FIXTURE_ESCAPES_RECORD", record)
            .process_group(0)
            .status()
            .expect("the daemon starts");
        assert!(started.success());
    }
    assert_eq!(fixture_escapes::double(3), 6);
}
