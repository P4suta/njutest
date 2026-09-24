// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `deep-v1` soundness phase: interpreting the tests rather than counting the `unsafe`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

use crate::error::RunnerError;
use crate::report::{Finding, FindingKind, Limitation};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// What Miri says when it will not interpret something.
const UNSUPPORTED: [&str; 3] = [
    "unsupported operation",
    "can't call foreign function",
    "unsupported target",
];

/// What Miri says when it has found unsoundness.
const UNDEFINED: &str = "Undefined Behavior";

/// What a toolchain says when there is nothing to interpret with.
const ABSENT: [&str; 3] = ["no such command", "no such subcommand", "is not installed"];

/// What the phase is asked to interpret, and how it is bounded.
#[derive(Debug, Clone)]
pub struct Interpreting<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The cargo to drive, which must be one that understands `+nightly`.
    pub cargo: &'a Path,
    /// The environment it runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The packages to interpret.
    /// Empty is the whole workspace.
    pub packages: &'a [String],
    /// What the configuration passes to Miri itself.
    pub flags: &'a [String],
    /// How long the interpretation may take.
    pub timeout: Option<Duration>,
    /// Whether cargo may reach the network, which Miri's own setup may need.
    pub offline: bool,
    /// Whether cargo may change the lock file.
    /// A phase that let it would measure a dependency set the baseline never saw, and would write into the tree under measurement.
    pub locked: bool,
    /// What a toolchain with no interpreter means to the contract the run answers.
    pub absent: Absent,
}

/// What a toolchain with no interpreter means to a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absent {
    /// The contract promises interpretation, so a run that cannot interpret cannot answer it.
    Refused,
    /// The contract names each thing it could not establish, so the soundness nothing interpreted is a hole it names.
    Hole,
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
/// [`RunnerError::MiriMissing`] when the toolchain has no Miri: a contract that promises interpretation and a run that could not interpret is an error, never a pass.
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
    let mut spec = Spec::new(
        argv,
        interpreting.timeout.map_or(
            rust_mutants::runner::Bound::Unbounded,
            rust_mutants::runner::Bound::After,
        ),
    );
    spec.dir = Some(interpreting.root.to_path_buf());
    spec.env = Some(environment(interpreting));

    let ran = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &ran));
    let said = std::str::from_utf8(&ran.output).map_err(|source| RunnerError::PhaseOutput {
        phase: "miri",
        source,
    })?;
    if ran.error().is_some() || absent(said) {
        return unavailable(interpreting.absent, absence(said, ran.error()));
    }
    if !ran.timed_out()
        && ran.conventional_exit_code() != 0
        && let Some(absence) = missing(interpreting, watch)?
    {
        return unavailable(interpreting.absent, absence);
    }
    let ending = if ran.timed_out() {
        Ending::TimedOut
    } else if ran.conventional_exit_code() == 0 {
        Ending::Passed
    } else {
        Ending::Failed
    };
    Ok(read(said, ending))
}

/// What a toolchain with no interpreter, which said `message`, comes to under a contract that treats it as `absent` says.
fn unavailable(absent: Absent, message: String) -> Result<Interpreted, RunnerError> {
    match absent {
        Absent::Refused => Err(RunnerError::MiriMissing { message }),
        Absent::Hole => Ok(Interpreted {
            executed: false,
            findings: vec![Finding {
                kind: FindingKind::NotMeasured,
                subject: "soundness".to_owned(),
                detail: "nothing interpreted the suite, so nothing is claimed about the unsafe it \
                         holds"
                    .to_owned(),
                origin: crate::report::FindingOrigin::Global,
                path: None,
                position: None,
            }],
            limitations: vec![Limitation::new(
                crate::limitation::MIRI_UNAVAILABLE,
                &format!("the toolchain has no interpreter: {message}"),
            )],
        }),
    }
}

/// What the toolchain says when it has no interpreter, asked once the run it was given has failed.
fn missing(
    interpreting: &Interpreting<'_>,
    watch: Watch<'_>,
) -> Result<Option<String>, RunnerError> {
    let mut spec = Spec::new(
        [
            interpreting.cargo.as_os_str().to_owned(),
            OsString::from("+nightly"),
            OsString::from("miri"),
            OsString::from("--version"),
        ],
        rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
    );
    spec.dir = Some(interpreting.root.to_path_buf());
    spec.env = Some(environment(interpreting));
    let asked = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &asked));
    if asked.error().is_none() && asked.conventional_exit_code() == 0 {
        return Ok(None);
    }
    let said = std::str::from_utf8(&asked.output).map_err(|source| RunnerError::PhaseOutput {
        phase: "miri version probe",
        source,
    })?;
    Ok(Some(absence(said, asked.error())))
}

/// What Miri's own environment is: the run's, plus what the configuration passes to the interpreter.
fn environment(interpreting: &Interpreting<'_>) -> Vec<(OsString, OsString)> {
    let mut env: BTreeMap<OsString, OsString> = interpreting.env.iter().cloned().collect();
    if !interpreting.flags.is_empty() {
        env.extend(std::iter::once((
            OsString::from("MIRIFLAGS"),
            OsString::from(interpreting.flags.join(" ")),
        )));
    }
    env.into_iter().collect()
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
            crate::limitation::MIRI_TIMED_OUT,
            "the interpreter ran out of time, so the suite was not interpreted whole",
        ));
        return interpreted;
    }
    if let Some(line) = first_line(said, UNDEFINED) {
        interpreted.findings.push(Finding {
            kind: FindingKind::UndefinedBehaviour,
            subject: "soundness".to_owned(),
            detail: line,
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
        return interpreted;
    }
    if let Some(unsupported) = UNSUPPORTED
        .iter()
        .find_map(|marker| first_line(said, marker))
    {
        interpreted.limitations.push(Limitation::new(
            crate::limitation::MIRI_UNSUPPORTED,
            &format!("the interpreter could not interpret the suite whole: {unsupported}"),
        ));
        interpreted.findings.push(Finding {
            kind: FindingKind::NotMeasured,
            subject: "soundness".to_owned(),
            detail: "the suite was not interpreted whole, so nothing is claimed about the \
                     unsafe it holds"
                .to_owned(),
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
        return interpreted;
    }
    if ending == Ending::Failed {
        interpreted.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: "soundness".to_owned(),
            detail: "a test fails under the interpreter that passes without it".to_owned(),
            origin: crate::report::FindingOrigin::Global,
            path: None,
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
fn absence(said: &str, error: Option<rust_mutants::runner::RunFailure<'_>>) -> String {
    error.map_or_else(
        || first_line(said, "error").unwrap_or_else(|| "the toolchain has no miri".to_owned()),
        |failure| failure.to_string(),
    )
}
