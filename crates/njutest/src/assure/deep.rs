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

/// What Miri's diagnostic says next when it has found unsoundness.
const UNDEFINED: &str = "Undefined Behavior:";

/// What starts a line in which the interpreter or the toolchain speaks, rather than a test.
const DIAGNOSTIC: &str = "error: ";

/// What starts the line cargo prints before it starts each test binary.
const BINARY: [&str; 2] = ["Running ", "Doc-tests "];

/// What opens a test's captured output, which the harness prints only for a test that failed; the name sits between this and a [`CAPTURED`] ending.
const CAPTURE_OPEN: &str = "---- ";

/// How the header of a test's captured output ends.
const CAPTURED: [&str; 2] = [" stdout ----", " stderr ----"];

/// The line that ends the captured output of every failing test.
const CAPTURE_CLOSE: &str = "failures:";

/// What libtest prints before a test's outcome, around its name, on the line a diagnostic may follow.
const TEST_PREFIX: (&str, &str) = ("test ", " ... ");

/// What a toolchain says when there is nothing to interpret with.
const ABSENT: [&str; 3] = ["no such command", "no such subcommand", "is not installed"];

/// What starts each line in which libtest says how one test binary ended.
const RESULT: &str = "test result: ";

/// What such a line says next when a test of the binary failed.
const FAILED: &str = "FAILED";

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
        return Err(RunnerError::MiriMissing {
            message: absence(said, ran.error()),
        });
    }
    if !ran.timed_out()
        && ran.conventional_exit_code() != 0
        && let Some(absence) = missing(interpreting, watch)?
    {
        return Err(RunnerError::MiriMissing { message: absence });
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

/// Records that the interpreter ended without a test result, which says nothing about the suite.
fn ran_no_test(interpreted: &mut Interpreted, said: &str) {
    interpreted.executed = false;
    let last = said
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map_or_else(
            || "it said nothing".to_owned(),
            |line| format!("it last said: {line}"),
        );
    interpreted.limitations.push(Limitation::new(
        crate::limitation::MIRI_RAN_NO_TEST,
        &format!(
            "the interpreter ended without a test result that says a test failed or every one \
             passed, so its status is its own trouble and not the suite's; {last}"
        ),
    ));
    interpreted.findings.push(Finding {
        kind: FindingKind::NotMeasured,
        subject: "soundness".to_owned(),
        detail: "the interpreter ran no test to a result, so nothing is claimed about the unsafe \
                 the suite holds"
            .to_owned(),
        origin: crate::report::FindingOrigin::Global,
        path: None,
        position: None,
    });
}

/// How one test binary the interpreter started came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// It gave no result.
    Unfinished,
    /// Every test of it passed.
    Passed,
    /// A test of it failed.
    Failed,
}

/// What one interpreter run said, read by the structure of its output rather than a phrase anywhere in it.
#[derive(Debug, Default)]
struct Reading {
    /// Every binary it started, in order, and how each ended.
    binaries: Vec<Ended>,
    /// The first diagnostic that found undefined behaviour.
    undefined: Option<String>,
    /// The first diagnostic that could not interpret something.
    unsupported: Option<String>,
    /// Whether a diagnostic said there is nothing to interpret with.
    absent: bool,
}

/// `said` read line by line: a test's captured output skipped, each binary held to the one result that follows it, and diagnostics taken only where the interpreter or the toolchain speaks.
fn reading(said: &str) -> Reading {
    let mut read = Reading::default();
    let mut captured = false;
    let lines: Vec<&str> = said.lines().map(str::trim_end).collect();
    for (at, line) in lines.iter().copied().enumerate() {
        if line.starts_with(CAPTURE_OPEN) && CAPTURED.iter().any(|end| line.ends_with(end)) {
            captured = true;
            continue;
        }
        if captured {
            captured = !closes_capture(&lines, at);
            continue;
        }
        let spoken = line.trim_start();
        if BINARY.iter().any(|start| spoken.starts_with(start)) {
            read.binaries.push(Ended::Unfinished);
            continue;
        }
        if let Some(ended) = spoken.strip_prefix(RESULT).and_then(summary) {
            if let Some(last) = read.binaries.last_mut()
                && *last == Ended::Unfinished
            {
                *last = ended;
            }
            continue;
        }
        let after_test = spoken
            .strip_prefix(TEST_PREFIX.0)
            .and_then(|rest| rest.split_once(TEST_PREFIX.1))
            .map_or(spoken, |(_name, after)| after);
        let Some(diagnostic) = after_test.strip_prefix(DIAGNOSTIC) else {
            continue;
        };
        if diagnostic.starts_with(UNDEFINED) && read.undefined.is_none() {
            read.undefined = Some(after_test.to_owned());
        }
        if UNSUPPORTED.iter().any(|marker| diagnostic.contains(marker))
            && read.unsupported.is_none()
        {
            read.unsupported = Some(after_test.to_owned());
        }
        read.absent |= ABSENT.iter().any(|marker| diagnostic.contains(marker));
    }
    read
}

/// Whether the line at `at` is the `failures:` libtest closes a binary's captured output with: one or more names indented four spaces, a blank line, and the binary's exact summary; a `failures:` a test printed is followed by anything else.
fn closes_capture(lines: &[&str], at: usize) -> bool {
    if lines.get(at) != Some(&CAPTURE_CLOSE) {
        return false;
    }
    let names = lines
        .iter()
        .skip(at.saturating_add(1))
        .take_while(|line| line.starts_with("    ") && !line.trim().is_empty())
        .count();
    let after = at.saturating_add(1).saturating_add(names);
    names > 0
        && lines.get(after).is_some_and(|line| line.is_empty())
        && lines
            .get(after.saturating_add(1))
            .and_then(|line| line.trim_start().strip_prefix(RESULT))
            .and_then(summary)
            .is_some()
}

/// How a binary ended where `rest` is exactly libtest's summary after its [`RESULT`], and nothing where it is not.
fn summary(rest: &str) -> Option<Ended> {
    let (status, counts) = rest.split_once(". ")?;
    let ended = match status {
        "ok" => Ended::Passed,
        FAILED => Ended::Failed,
        _ => return None,
    };
    let parts: Vec<&str> = counts.split("; ").collect();
    let [passed, failed, ignored, measured, filtered, finished] = parts.as_slice() else {
        return None;
    };
    let counted = |part: &str, what: &str| {
        part.strip_suffix(what).is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    let seconds = finished
        .strip_prefix("finished in ")
        .and_then(|time| time.strip_suffix('s'))
        .is_some_and(|time| {
            !time.is_empty()
                && time
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && time.bytes().filter(|byte| *byte == b'.').count() <= 1
        });
    (counted(passed, " passed")
        && counted(failed, " failed")
        && counted(ignored, " ignored")
        && counted(measured, " measured")
        && counted(filtered, " filtered out")
        && seconds)
        .then_some(ended)
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
    let read = reading(said);
    if let Some(line) = read.undefined {
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
    if let Some(unsupported) = read.unsupported {
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
    let failed = read.binaries.contains(&Ended::Failed);
    let passed =
        !read.binaries.is_empty() && read.binaries.iter().all(|ended| *ended == Ended::Passed);
    match ending {
        Ending::Failed if failed => interpreted.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: "soundness".to_owned(),
            detail: "a test fails under the interpreter that passes without it".to_owned(),
            origin: crate::report::FindingOrigin::Global,
            path: None,
            position: None,
        }),
        Ending::Passed if passed => {}
        Ending::Failed | Ending::Passed | Ending::TimedOut => ran_no_test(&mut interpreted, said),
    }
    interpreted
}

/// The first line that carries `marker`, trimmed.
fn first_line(said: &str, marker: &str) -> Option<String> {
    said.lines()
        .find(|line| line.contains(marker))
        .map(|line| line.trim().to_owned())
}

/// Whether the toolchain has no Miri, as a diagnostic says.
fn absent(said: &str) -> bool {
    reading(said).absent
}

/// What to say about a Miri that is not there.
fn absence(said: &str, error: Option<rust_mutants::runner::RunFailure<'_>>) -> String {
    error.map_or_else(
        || first_line(said, "error").unwrap_or_else(|| "the toolchain has no miri".to_owned()),
        |failure| failure.to_string(),
    )
}
