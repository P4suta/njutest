// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `cargo xtask remote-check` tells each machine, and what it reads back.

use xtask::remote::{
    BUNDLE, Fleet, Machine, RemoteError, Shell, base64, failures, fleet, invocation, known, script,
};

fn posix() -> Machine {
    Machine {
        name: "linux".to_owned(),
        host: "linux".to_owned(),
        shell: Shell::Posix,
        repository: "~/projects/njutest".to_owned(),
        worktree: "~/projects/njutest-gate".to_owned(),
        target_dir: Some("~/projects/njutest/target".to_owned()),
        prelude: None,
    }
}

fn powershell() -> Machine {
    Machine {
        name: "windows".to_owned(),
        host: "win".to_owned(),
        shell: Shell::Powershell,
        repository: r"C:\Users\someone\projects\njutest".to_owned(),
        worktree: r"C:\Users\someone\projects\njutest-gate".to_owned(),
        target_dir: None,
        prelude: Some("$env:PATH = 'C:\\Program Files\\Git\\bin;' + $env:PATH".to_owned()),
    }
}

fn file() -> Fleet {
    Fleet {
        command: "mise run test".to_owned(),
        members: vec![posix(), powershell()],
    }
}

#[test]
fn base64_is_the_standard_alphabet_with_padding() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"ab"), "YWI=");
    assert_eq!(base64(b"abc"), "YWJj");
    assert_eq!(base64(b"abcdef"), "YWJjZGVm");
    assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
}

#[test]
fn a_posix_machine_checks_out_exactly_the_commit_and_runs_the_command_last() {
    let said = script(&file(), &posix(), "0123456789abcdef");
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.first(), Some(&"set -e"), "{said}");
    assert!(
        !said.contains("https://"),
        "a machine is never asked to reach the network: {said}"
    );
    assert!(
        said.contains(&format!("git fetch -q ~/{BUNDLE} HEAD")),
        "{said}"
    );
    assert!(
        said.contains("git checkout -q --detach 0123456789abcdef"),
        "{said}"
    );
    assert!(
        said.contains("export CARGO_TARGET_DIR=~/projects/njutest/target"),
        "{said}"
    );
    assert_eq!(lines.last(), Some(&"mise run test"), "{said}");
}

#[test]
fn a_powershell_machine_stops_on_the_first_native_failure() {
    let said = script(&file(), &powershell(), "0123456789abcdef");
    assert!(
        said.starts_with("$ErrorActionPreference = 'Stop'"),
        "{said}"
    );
    assert!(
        said.contains("$PSNativeCommandUseErrorActionPreference = $true"),
        "{said}"
    );
    assert!(
        said.contains("git checkout -q --detach 0123456789abcdef"),
        "{said}"
    );
    assert!(
        !said.contains("CARGO_TARGET_DIR"),
        "no target was asked for: {said}"
    );
    assert_eq!(said.lines().last(), Some("mise run test"), "{said}");
}

#[test]
fn the_command_line_carries_the_script_in_a_form_no_shell_reinterprets() {
    let posix_line = invocation(Shell::Posix, "echo 'a b' \"$HOME\"\n");
    assert!(posix_line.starts_with("echo "), "{posix_line}");
    assert!(
        posix_line.ends_with(" | base64 -d | bash -l"),
        "{posix_line}"
    );
    assert!(!posix_line.contains('\''), "{posix_line}");
    assert!(!posix_line.contains('$'), "{posix_line}");
    let windows_line = invocation(Shell::Powershell, "Write-Output 'a'");
    assert!(
        windows_line.starts_with("pwsh -NoProfile -NonInteractive -EncodedCommand "),
        "{windows_line}"
    );
    let encoded = windows_line.rsplit(' ').next().unwrap_or_default();
    assert_eq!(
        encoded,
        base64(&[
            b'W', 0, b'r', 0, b'i', 0, b't', 0, b'e', 0, b'-', 0, b'O', 0, b'u', 0, b't', 0, b'p',
            0, b'u', 0, b't', 0, b' ', 0, b'\'', 0, b'a', 0, b'\'', 0
        ])
    );
}

#[test]
fn only_the_lines_that_say_what_failed_are_read_back_each_once() {
    let log = "   Compiling njutest\n        FAIL [   8.1s] (1/2) njutest::toolchain_edits every_edit\n    thread 'every_edit' panicked at src/lib.rs:1:1:\n        FAIL [   8.1s] (1/2) njutest::toolchain_edits every_edit\nerror: test run failed\n     Summary [ 9s] 2 tests run: 1 passed, 1 failed\n";
    assert_eq!(
        failures(log),
        vec![
            "FAIL [   8.1s] (1/2) njutest::toolchain_edits every_edit".to_owned(),
            "thread 'every_edit' panicked at src/lib.rs:1:1:".to_owned(),
            "error: test run failed".to_owned(),
        ]
    );
    assert!(failures("     Summary [ 9s] 2 tests run: 2 passed\n").is_empty());
}

#[test]
fn a_machines_file_is_read_strictly_and_must_name_a_machine() {
    let directory = tempfile::tempdir().expect("a directory");
    let good = directory.path().join("machines.toml");
    std::fs::write(
        &good,
        "command = \"mise run test\"\n\n[[machine]]\nname = \"linux\"\nhost = \"linux\"\nshell = \"posix\"\nrepository = \"~/r\"\nworktree = \"~/r-gate\"\n",
    )
    .expect("the good file");
    let read = fleet(&good).expect("a fleet");
    assert_eq!(read.members.len(), 1);
    assert_eq!(
        read.members.first().map(|one| one.shell),
        Some(Shell::Posix)
    );

    let empty = directory.path().join("empty.toml");
    std::fs::write(&empty, "command = \"y\"\nmachine = []\n").expect("the empty file");
    assert!(matches!(fleet(&empty), Err(RemoteError::NoMachine { .. })));

    let unknown = directory.path().join("unknown.toml");
    std::fs::write(&unknown, "command = \"y\"\nsurprise = 1\nmachine = []\n")
        .expect("the unknown file");
    assert!(matches!(fleet(&unknown), Err(RemoteError::Parse { .. })));
}

#[test]
fn a_machine_is_asked_only_which_commits_its_clone_has() {
    assert_eq!(
        known(&posix()),
        "git -C ~/projects/njutest for-each-ref --format='%(objectname)'\n"
    );
    assert!(
        known(&powershell())
            .starts_with(r"git -C 'C:\Users\someone\projects\njutest' for-each-ref")
    );
}
