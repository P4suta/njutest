// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A resource provider and a generation provider that answer what a test told them to answer.

#![expect(
    clippy::print_stderr,
    reason = "printing on the standard streams is what this program is: it stands in for a \
              provider whose whole answer is what it prints"
)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::process::ExitCode;

/// The exit code a role nobody wrote leaves, with its own argument on stderr.
const UNKNOWN_ROLE_EXIT: u8 = 99;

fn main() -> ExitCode {
    let role = match std::env::args().nth(1) {
        Some(role) => role,
        None => String::new(),
    };
    match role.as_str() {
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
    for line in input.lock().lines() {
        let Ok(line) = line else {
            return ExitCode::FAILURE;
        };
        if line.contains(r#""action":"start""#) && !silent {
            let ready = match told("FAKE_PROVIDER_READY") {
                Ok(ready) => ready,
                Err(error) => {
                    eprintln!("fake-provider: ready is not exact text: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = say(&ready) {
                eprintln!("fake-provider: cannot say ready: {error}");
                return ExitCode::FAILURE;
            }
        } else if line.contains(r#""action":"stop""#) {
            let stopped = match told("FAKE_PROVIDER_STOPPED") {
                Ok(stopped) => stopped,
                Err(error) => {
                    eprintln!("fake-provider: stopped answer is not exact text: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = say(&stopped) {
                eprintln!("fake-provider: cannot say stopped: {error}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
    }
    ExitCode::SUCCESS
}

/// Answers a run that asks for a candidate, keeping the question where the test can read it.
fn generation() -> ExitCode {
    let mut asked = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut asked) {
        eprintln!("fake-provider: cannot read the generation question: {error}");
        return ExitCode::FAILURE;
    }
    if let Some(name) = std::env::var_os("FAKE_GENERATOR_ASKED") {
        let path = std::path::PathBuf::from(name);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!("fake-provider: cannot create {}: {error}", parent.display());
            return ExitCode::FAILURE;
        }
        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) => {
                eprintln!("fake-provider: cannot open {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = file.write_all(asked.as_bytes()) {
            eprintln!("fake-provider: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        if let Err(error) = file.write_all(b"\n") {
            eprintln!("fake-provider: cannot finish {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    let offer = match told("FAKE_GENERATOR_OFFERS") {
        Ok(offer) => offer,
        Err(error) => {
            eprintln!("fake-provider: offer is not exact text: {error}");
            return ExitCode::FAILURE;
        }
    };
    match say(&offer) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fake-provider: cannot say its offer: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What the environment told this process to say under `name`.
fn told(name: &str) -> Result<String, std::env::VarError> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(String::new()),
        Err(error @ std::env::VarError::NotUnicode(_)) => Err(error),
    }
}

/// Says one line and lets the run read it now: a provider a run waits on says nothing while its output sits in a buffer.
fn say(line: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush()
}
