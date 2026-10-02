// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `cargo xtask remote-check` tells each machine, and what it reads back.

use xtask::remote::{
    BUNDLE, Fleet, Invocation, Machine, RemoteError, Shell, failures, file_invocation, fleet,
    known, script, script_name,
};

/// The finite executable and argument vector that carries `script` unchanged.
#[must_use]
fn invocation(shell: Shell, script: &str) -> Invocation {
    match shell {
        Shell::Posix => Invocation {
            program: "bash",
            arguments: vec![
                "-lc".to_owned(),
                format!("echo {} | base64 -d | bash -l", base64(script.as_bytes())),
            ],
        },
        Shell::Powershell => {
            let wide: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            Invocation {
                program: "pwsh",
                arguments: vec![
                    "-NoProfile".to_owned(),
                    "-NonInteractive".to_owned(),
                    "-EncodedCommand".to_owned(),
                    base64(&wide),
                ],
            }
        }
    }
}

/// The base64 digit for the low six bits of `value`.
fn digit(value: u32) -> char {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let at = match usize::try_from(value & 0x3f) {
        Ok(at) => at,
        Err(_six_bits_always_fit) => return '=',
    };
    match DIGITS.get(at) {
        Some(byte) => char::from(*byte),
        None => '=',
    }
}

/// Standard base64 with padding.
#[must_use]
fn base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let (joined, kept) = match *chunk {
            [first, second, third] => (
                (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third),
                4,
            ),
            [first, second] => ((u32::from(first) << 16) | (u32::from(second) << 8), 3),
            [first] => (u32::from(first) << 16, 2),
            _ => continue,
        };
        for (index, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            encoded.push(if index < kept {
                digit(joined >> shift)
            } else {
                '='
            });
        }
    }
    encoded
}

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
        said.contains("git fetch -q \"$bundle_path\" HEAD")
            && said.contains(&format!("bundle_path=\"$PWD/{BUNDLE}\"")),
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
    assert_eq!(posix_line.program, "bash");
    assert_eq!(
        posix_line.arguments.first().map(String::as_str),
        Some("-lc")
    );
    let encoded_script = posix_line.arguments.last().expect("the encoded script");
    assert!(encoded_script.starts_with("echo "));
    assert!(encoded_script.ends_with(" | base64 -d | bash -l"));
    assert!(!encoded_script.contains('\''));
    assert!(!encoded_script.contains('$'));
    let windows_line = invocation(Shell::Powershell, "Write-Output 'a'");
    assert_eq!(windows_line.program, "pwsh");
    assert_eq!(
        windows_line.arguments.get(..3),
        Some(
            [
                "-NoProfile".to_owned(),
                "-NonInteractive".to_owned(),
                "-EncodedCommand".to_owned(),
            ]
            .as_slice()
        )
    );
    let encoded = windows_line
        .arguments
        .last()
        .expect("the encoded Windows script");
    assert_eq!(
        encoded.as_str(),
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

#[test]
fn native_checks_dispatch_through_the_owned_domyjob_transport() {
    let source = include_str!("../src/remote.rs");
    assert!(
        source.contains("\"domyjob\"")
            && source.contains("\"--submission\"")
            && source.contains("\"--wait\""),
        "native proof must run through domyjob with an owned source landing"
    );
    assert!(
        !source.contains("run(\"scp\"") && !source.contains("\"ssh\",\n"),
        "native checks must not start untracked SSH or shared home bundle uploads"
    );
    assert!(
        !source.contains("\"--fresh\"") && !source.contains("\"--root\""),
        "native dispatch must use the installed CLI's immutable snapshot submission contract"
    );
}

#[test]
fn the_native_invocation_executes_exact_script_bytes_and_exit() {
    let (shell, script) = if cfg!(windows) {
        (
            Shell::Powershell,
            "[Console]::Out.Write('native $ input; quoted'); [Console]::Error.Write('actual stderr'); exit 7",
        )
    } else {
        (
            Shell::Posix,
            "printf '%s' 'native $ input; quoted'\nprintf '%s' 'actual stderr' >&2\nexit 7\n",
        )
    };
    let invoked = invocation(shell, script);
    let mut command = std::process::Command::new(invoked.program);
    command.args(invoked.arguments);
    let actual = command.output().expect("the native invocation starts");
    assert_eq!(actual.stdout, b"native $ input; quoted");
    assert_eq!(actual.stderr, b"actual stderr");
    assert_eq!(actual.status.code(), Some(7));
}

#[test]
fn a_complete_native_script_larger_than_a_transport_word_is_run_from_its_snapshot() {
    let directory = tempfile::tempdir().expect("an owned native input snapshot");
    let value = "actual".repeat(2000);
    let (shell, script) = if cfg!(windows) {
        (
            Shell::Powershell,
            format!(
                "[Console]::Out.Write('{value}'); [Console]::Error.Write('actual stderr'); exit 7"
            ),
        )
    } else {
        (
            Shell::Posix,
            format!("printf '%s' '{value}'\nprintf '%s' 'actual stderr' >&2\nexit 7\n"),
        )
    };
    assert!(script.len() > 8192, "the genuine failing per-word shape");
    std::fs::write(directory.path().join(script_name(shell)), script)
        .expect("the complete original script bytes");
    let invoked = file_invocation(shell);
    assert!(invoked.argv().all(|word| word.len() <= 8192));
    let output = std::process::Command::new(invoked.program)
        .args(invoked.arguments)
        .current_dir(directory.path())
        .output()
        .expect("the real file invocation starts");
    assert_eq!(output.stdout, value.as_bytes());
    assert_eq!(output.stderr, b"actual stderr");
    assert_eq!(output.status.code(), Some(7));
}
