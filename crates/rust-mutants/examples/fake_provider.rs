// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A resource provider and a generation provider that answer what a test told them to answer.
//!
//! Both were shell scripts run as `/bin/sh <path>`, which names a program only
//! on a platform that has one there. A suite that spelled it that way was
//! asking a unix question of every machine, and the answer on the others is a
//! run that never started the provider it is a test of.
//!
//! It is an example for the same reason `fake_cargo` is: only an example is
//! built beside the test binaries by `cargo test`, `cargo nextest run`, and
//! `cargo llvm-cov` alike, on every platform the product is tested on.
//!
//! The first argument is the role. What each role says is what the environment
//! told it to say, so a test writes the answer and reads back what the run
//! made of it.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "printing on the standard streams is what this program is: it stands in for a \
              provider whose whole answer is what it prints"
)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::process::ExitCode;

/// The exit code a role nobody wrote leaves, with its own argument on stderr.
const UNKNOWN_ROLE_EXIT: u8 = 99;

fn main() -> ExitCode {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "resource" => resource(),
        "generation" => generation(),
        other => {
            eprintln!("fake-provider: {other:?} is neither a resource nor a generation provider");
            ExitCode::from(UNKNOWN_ROLE_EXIT)
        }
    }
}

/// Answers a run that starts and stops a resource: ready when it is asked to start, stopped when it is asked to stop.
fn resource() -> ExitCode {
    let silent = std::env::var_os("FAKE_PROVIDER_SILENT").is_some();
    let input = std::io::stdin();
    for line in input.lock().lines().map_while(Result::ok) {
        if line.contains(r#""action":"start""#) {
            if !silent {
                say(&told("FAKE_PROVIDER_READY"));
            }
        } else if line.contains(r#""action":"stop""#) {
            say(&told("FAKE_PROVIDER_STOPPED"));
            return ExitCode::SUCCESS;
        }
    }
    ExitCode::SUCCESS
}

/// Answers a run that asks for a candidate, keeping the question where the test can read it.
fn generation() -> ExitCode {
    let mut asked = String::new();
    let read = std::io::stdin().read_to_string(&mut asked);
    drop(read);
    if let Some(name) = std::env::var_os("FAKE_GENERATOR_ASKED") {
        let path = std::path::PathBuf::from(name);
        if let Some(parent) = path.parent() {
            drop(std::fs::create_dir_all(parent));
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            drop(file.write_all(asked.as_bytes()));
            drop(file.write_all(b"\n"));
        }
    }
    say(&told("FAKE_GENERATOR_OFFERS"));
    ExitCode::SUCCESS
}

/// What the environment told this process to say under `name`.
fn told(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Says one line and lets the run read it now: a provider a run waits on says nothing while its output sits in a buffer.
fn say(line: &str) {
    println!("{line}");
    drop(std::io::stdout().flush());
}
