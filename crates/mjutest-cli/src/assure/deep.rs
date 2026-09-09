// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `deep-v1` soundness phase: interpreting the tests rather than counting the `unsafe`.
//!
//! Safe Rust has no data races for a race detector to find, so the fault
//! class this phase is about is unsoundness in `unsafe`. Miri executes the
//! suite under an interpreter that notices it. What Miri cannot interpret is
//! stated as a limitation, never as a pass: a suite that was not interpreted
//! is not a suite that was interpreted and found sound.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

use crate::error::RunnerError;
use crate::report::{Finding, FindingKind, Limitation};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// The limitation a run states when Miri could not interpret something the suite does.
pub const UNSUPPORTED_LIMITATION: &str = "miri-unsupported";

/// The limitation a run states when the interpreter ran out of time.
pub const TIMEOUT_LIMITATION: &str = "miri-timed-out";

/// What Miri says when it will not interpret something.
const UNSUPPORTED: [&str; 3] = [
    "unsupported operation",
    "can't call foreign function",
    "unsupported target",
];

/// What Miri says when it has found unsoundness.
const UNDEFINED: &str = "Undefined Behavior";

/// What Miri says when it is not installed.
const ABSENT: [&str; 3] = [
    "no such command",
    "no such subcommand",
    "is not installed for the toolchain",
];

/// What the phase is asked to interpret, and how it is bounded.
#[derive(Debug, Clone)]
pub struct Interpreting<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The cargo to drive, which must be one that understands `+nightly`.
    pub cargo: &'a Path,
    /// The environment it runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The packages to interpret. Empty is the whole workspace.
    pub packages: &'a [String],
    /// What the configuration passes to Miri itself.
    pub flags: &'a [String],
    /// How long the interpretation may take.
    pub timeout: Option<Duration>,
    /// Whether cargo may reach the network, which Miri's own setup may need.
    pub offline: bool,
    /// Whether cargo may change the lock file. A phase that let it would measure a dependency set the baseline never saw, and would write into the tree under measurement.
    pub locked: bool,
}

/// What interpreting the suite established.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interpreted {
    /// Whether Miri ran at all.
    pub executed: bool,
    /// What it found wrong.
    pub findings: Vec<Finding>,
    /// What it could not say.
    pub limitations: Vec<Limitation>,
}

/// Interprets the suite under Miri.
///
/// # Errors
/// [`RunnerError::MiriMissing`] when the toolchain has no Miri: a contract
/// that promises interpretation and a run that could not interpret is an
/// error, never a pass.
pub fn interpret(
    interpreting: &Interpreting<'_>,
    watch: Watch<'_>,
) -> Result<Interpreted, RunnerError> {
    let mut argv: Vec<OsString> = vec![
        interpreting.cargo.as_os_str().to_owned(),
        OsString::from("+nightly"),
        OsString::from("miri"),
        OsString::from("test"),
    ];
    if interpreting.packages.is_empty() {
        argv.push(OsString::from("--workspace"));
    } else {
        for package in interpreting.packages {
            argv.push(OsString::from("--package"));
            argv.push(OsString::from(package));
        }
    }
    if interpreting.offline {
        argv.push(OsString::from("--offline"));
    }
    if interpreting.locked {
        argv.push(OsString::from("--locked"));
    }
    let mut spec = Spec::new(argv);
    spec.dir = Some(interpreting.root.to_path_buf());
    spec.timeout = interpreting.timeout;
    spec.env = Some(environment(interpreting));

    let ran = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &ran));
    let said = String::from_utf8_lossy(&ran.output).into_owned();
    if ran.error.is_some() || absent(&said) {
        return Err(RunnerError::MiriMissing {
            message: absence(&said, ran.error.as_ref()),
        });
    }
    let ending = if ran.timed_out {
        Ending::TimedOut
    } else if ran.exit_code == 0 {
        Ending::Passed
    } else {
        Ending::Failed
    };
    Ok(read(&said, ending))
}

/// What Miri's own environment is: the run's, plus what the configuration passes to the interpreter.
fn environment(interpreting: &Interpreting<'_>) -> Vec<(OsString, OsString)> {
    let mut env = interpreting.env.clone();
    if !interpreting.flags.is_empty() {
        let name = OsString::from("MIRIFLAGS");
        env.retain(|(other, _)| *other != name);
        env.push((name, OsString::from(interpreting.flags.join(" "))));
    }
    env
}

/// How one Miri run ended, apart from what it said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// It finished and every test passed.
    Passed,
    /// It finished and something failed.
    Failed,
    /// It ran out of time.
    TimedOut,
}

/// What one Miri run means.
fn read(said: &str, ending: Ending) -> Interpreted {
    let mut interpreted = Interpreted {
        executed: true,
        ..Interpreted::default()
    };
    if ending == Ending::TimedOut {
        interpreted.executed = false;
        interpreted.limitations.push(Limitation::new(
            TIMEOUT_LIMITATION,
            "the interpreter ran out of time, so the suite was not interpreted whole",
        ));
        return interpreted;
    }
    if said.contains(UNDEFINED) {
        interpreted.findings.push(Finding {
            kind: FindingKind::UndefinedBehaviour,
            subject: "soundness".to_owned(),
            detail: first_line(said, UNDEFINED).unwrap_or_else(|| {
                "the interpreter found undefined behaviour in the suite".to_owned()
            }),
            position: None,
        });
        return interpreted;
    }
    if let Some(unsupported) = UNSUPPORTED
        .iter()
        .find_map(|marker| first_line(said, marker))
    {
        interpreted.limitations.push(Limitation::new(
            UNSUPPORTED_LIMITATION,
            &format!("the interpreter could not interpret the suite whole: {unsupported}"),
        ));
        interpreted.findings.push(Finding {
            kind: FindingKind::NotMeasured,
            subject: "soundness".to_owned(),
            detail: "the suite was not interpreted whole, so nothing is claimed about the \
                     unsafe it holds"
                .to_owned(),
            position: None,
        });
        return interpreted;
    }
    if ending == Ending::Failed {
        interpreted.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: "soundness".to_owned(),
            detail: "a test fails under the interpreter that passes without it".to_owned(),
            position: None,
        });
    }
    interpreted
}

/// The first line that carries `marker`, trimmed.
fn first_line(said: &str, marker: &str) -> Option<String> {
    said.lines()
        .find(|line| line.contains(marker))
        .map(|line| line.trim().to_owned())
}

/// Whether the toolchain has no Miri.
fn absent(said: &str) -> bool {
    ABSENT.iter().any(|marker| said.contains(marker))
}

/// What to say about a Miri that is not there.
fn absence(said: &str, error: Option<&rust_mutants::runner::RunnerError>) -> String {
    error.map_or_else(
        || first_line(said, "error").unwrap_or_else(|| "the toolchain has no miri".to_owned()),
        ToString::to_string,
    )
}
