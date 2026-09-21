// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest doctor`: what this machine can and cannot do.

use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::cargo::{LocateOptions, Toolchain};
use rust_mutants::runner::{Cancel, PROBE_OUTPUT_LIMIT, Spec, run as run_process};

use crate::cli::{Doctor, Environment};
use crate::coverage::Tools;
use crate::trace::Recorder;
use crate::watch::Watch;

use super::Completion;

/// What a missing tool costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// A standard-v1 run cannot go ahead without it.
    Required,
    /// A later phase or a deeper contract wants it; a run says so as a limitation rather than failing.
    Optional,
}

impl Need {
    const fn name(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
        }
    }
}

/// What was found out about one thing a run needs.
enum State {
    /// It is there, and this is what it is.
    Found(String),
    /// It is not there at all.
    Missing,
    /// It is there and a run will not have it: the reason, in the words the run would use.
    Refused(String),
}

impl State {
    const fn word(&self) -> &'static str {
        match self {
            Self::Found(_) => "ok",
            Self::Missing => "missing",
            Self::Refused(_) => "refused",
        }
    }

    const fn held(&self) -> bool {
        matches!(self, Self::Found(_))
    }

    fn detail(&self) -> String {
        match self {
            Self::Found(said) | Self::Refused(said) => format!("  {said}"),
            Self::Missing => String::new(),
        }
    }
}

/// One thing a run needs, named as a closed set so that adding one is adding its remedy.
///
/// A `&'static str` here let `remedy` end in a `_` arm, so a check added without advice was a check that said nothing about what to do — silently,
/// and only at the moment somebody's machine was missing it (ADR 0023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Needed {
    /// The project's own configuration file.
    Configuration,
    /// The cargo this project pins.
    Cargo,
    /// The rustc this project pins.
    Rustc,
    /// The coverage reader.
    Profdata,
    /// The coverage summariser.
    Cov,
    /// Version control, which is what `--changed` asks.
    Git,
    /// The nightly toolchain, which miri needs.
    Nightly,
    /// The interpreter a deep contract runs.
    Miri,
    /// The fuzzing driver.
    CargoFuzz,
    /// What it costs this machine to run a file it has just written.
    Exec,
}

impl Needed {
    /// What a reader calls it, which is what the report prints.
    const fn named(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Cargo => "cargo",
            Self::Rustc => "rustc",
            Self::Profdata => "llvm-profdata",
            Self::Cov => "llvm-cov",
            Self::Git => "git",
            Self::Nightly => "nightly",
            Self::Miri => "miri",
            Self::CargoFuzz => "cargo-fuzz",
            Self::Exec => "exec",
        }
    }

    /// What to do about it when it is not there, which every one of them has.
    const fn remedy(self) -> &'static str {
        match self {
            Self::Configuration => {
                "njutest init writes one; a run without it uses the defaults and says so"
            }
            Self::Cargo | Self::Rustc => {
                "install the toolchain this project pins, or put it on the PATH this \
                 process was given"
            }
            Self::Profdata | Self::Cov => "rustup component add llvm-tools-preview",
            Self::Nightly => "rustup toolchain install nightly",
            Self::Miri => {
                "rustup +nightly component add miri; without it a deep-v1 contract cannot \
                 interpret, and a standard-v1 run does not need it"
            }
            Self::CargoFuzz => {
                "cargo install cargo-fuzz; without it [fuzz] run = true finds targets and \
                 drives none of them"
            }
            Self::Git => {
                "install git; without it --changed cannot say what changed and the report \
                 records the repository as unavailable"
            }
            Self::Exec => {
                "this machine is evaluating new executables; a run started now measures \
                 that and not your tests, so wait until the first number is under a second"
            }
        }
    }
}

/// One thing a run needs, and what was found out about it.
struct Finding {
    /// Which of the things a run needs this is.
    named: Needed,
    need: Need,
    detail: State,
}

impl Finding {
    fn line(&self) -> String {
        let mut text = format!(
            "{:<8} {:<14} {}{}",
            self.need.name(),
            self.named.named(),
            self.detail.word(),
            self.detail.detail()
        );
        if !self.detail.held() {
            text.push_str("\n         try: ");
            text.push_str(self.remedy());
        }
        text
    }

    /// What to do about this one, which is the half a reader acts on.
    const fn remedy(&self) -> &'static str {
        self.named.remedy()
    }
}

/// Reports the toolchain and the tools, succeeding only when every required one is present.
pub(super) fn run(
    _arguments: Doctor,
    environment: &Environment,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> std::io::Result<Completion> {
    let findings = examine(environment);
    for finding in &findings {
        super::say(stdout, &finding.line())?;
    }
    let wanting: Vec<String> = findings
        .iter()
        .filter(|finding| finding.need == Need::Required && !finding.detail.held())
        .map(|finding| format!("{} is {}", finding.named.named(), finding.detail.word()))
        .collect();
    super::say(stdout, "")?;
    if wanting.is_empty() {
        super::say(
            stdout,
            &format!(
                "a standard-v1 run can go ahead: {} were examined and every required one \
                 answered",
                findings.len()
            ),
        )?;
        Ok(Completion::Assured)
    } else {
        super::say(
            stdout,
            &format!("a run cannot go ahead: {}", wanting.join(", ")),
        )?;
        Ok(Completion::Error)
    }
}

/// Everything this machine was asked about, in reading order: what a run needs first, then what it would only like.
fn examine(environment: &Environment) -> Vec<Finding> {
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let dir = environment.working_directory.clone();
    let toolchain = match Toolchain::locate(
        &LocateOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: Some(environment.vars.clone()),
        },
        &dir,
        &cancel,
    ) {
        Ok(toolchain) => Some(toolchain),
        Err(_) => None,
    };
    let tools = toolchain.as_ref().and_then(|located| {
        match Tools::locate(located, &dir, &Watch::new(&cancel, &trace)) {
            Ok(tools) => Some(tools),
            Err(_) => None,
        }
    });
    let probe = Probe {
        environment,
        dir: &dir,
        cancel: &cancel,
    };

    vec![
        Finding {
            named: Needed::Configuration,
            need: Need::Required,
            detail: configuration(&dir),
        },
        required(
            Needed::Cargo,
            toolchain
                .as_ref()
                .map(|located| located.cargo().display().to_string()),
        ),
        required(
            Needed::Rustc,
            toolchain
                .as_ref()
                .map(|located| located.rustc().display().to_string()),
        ),
        required(
            Needed::Profdata,
            tools
                .as_ref()
                .map(|found| found.profdata.display().to_string()),
        ),
        required(
            Needed::Cov,
            tools.as_ref().map(|found| found.cov.display().to_string()),
        ),
        optional(Needed::Git, probe.version_of("git", &["--version"])),
        optional(
            Needed::Nightly,
            probe.version_of("rustup", &["run", "nightly", "rustc", "--version"]),
        ),
        optional(
            Needed::Miri,
            probe.version_of("cargo", &["+nightly", "miri", "--version"]),
        ),
        optional(
            Needed::CargoFuzz,
            probe.version_of("cargo", &["fuzz", "--version"]),
        ),
        Finding {
            named: Needed::Exec,
            need: Need::Required,
            detail: exec_cost(&environment.temp_directory, &environment.program),
        },
    ]
}

fn required(named: Needed, detail: Option<String>) -> Finding {
    Finding {
        named,
        need: Need::Required,
        detail: match detail {
            Some(detail) => State::Found(detail),
            None => State::Missing,
        },
    }
}

const fn optional(named: Needed, detail: State) -> Finding {
    Finding {
        named,
        need: Need::Optional,
        detail,
    }
}

/// What it costs to run a file that has just been written, which a run does per target it builds.
///
/// A system that evaluates an executable before it may run pays that cost once per file, and where the evaluation has a backlog it is seconds or minutes.
/// A run builds a binary per target and runs each once, so it pays that per target and measures the evaluation instead of the tests.
/// Nothing else a caller can see reports it: not load, not free processors, not free memory.
/// The pair is the evidence — one slow run could be a slow disk, and a slow one beside a fast one of the same file cannot be anything else.
fn exec_cost(temp: &Path, program: &Path) -> State {
    let (first, second) = match rust_mutants::execcost::exec_twice(temp, program) {
        Ok(measured) => measured,
        Err(why) => return State::Found(format!("not measured: {why}")),
    };

    let (first, second) = (first.as_secs_f64(), second.as_secs_f64());
    let said = format!(
        "a newly written file took {first:.2}s to run the first time and {second:.2}s the \
         second"
    );
    if first >= 5.0 && first >= second * 10.0 {
        return State::Refused(said);
    }
    State::Found(said)
}

/// What a run in this directory would make of the configuration beside it.
fn configuration(root: &Path) -> State {
    let path = root.join(crate::config::FILE_NAME);
    match crate::config::Config::load(root) {
        Ok(_read) => match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                State::Found(path.display().to_string())
            }
            Ok(_) => State::Refused(format!("{} is not a regular file", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::Found(format!(
                "none; the defaults apply. `njutest init` writes {}",
                crate::config::FILE_NAME
            )),
            Err(error) => State::Refused(format!("cannot inspect {}: {error}", path.display())),
        },
        Err(error) => State::Refused(error.to_string()),
    }
}

/// Asking one tool whether it is there, in the machine the run was given.
struct Probe<'a> {
    environment: &'a Environment,
    dir: &'a Path,
    cancel: &'a Cancel,
}

impl Probe<'_> {
    /// The first line the tool prints, or why it did not print one.
    ///
    /// A tool that is not installed and a tool that is installed and answered badly are different things to be told: the second sends somebody to install what they already have.
    /// The exit status and what it printed are in hand, so they are what is said.
    fn version_of(&self, program: &str, arguments: &[&str]) -> State {
        let program = match self.resolve(program) {
            Ok(Some(program)) => program,
            Ok(None) => return State::Missing,
            Err(error) => return State::Refused(format!("cannot inspect PATH: {error}")),
        };
        let mut spec = Spec::new(
            std::iter::once(program.into_os_string())
                .chain(arguments.iter().map(std::ffi::OsString::from)),
            rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
        );
        spec.dir = Some(self.dir.to_path_buf());
        spec.env = Some(self.environment.vars.clone());
        spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
        let result = run_process(&spec, self.cancel);
        if let Some(error) = result.error() {
            return State::Refused(format!("it is installed and would not start: {error}"));
        }
        if result.timed_out() {
            return State::Refused(format!(
                "it is installed and did not answer within {} seconds, which a run \
                 would have waited for too",
                rust_mutants::runner::PROBE.as_secs()
            ));
        }
        if result.conventional_exit_code() != 0 {
            let output = match std::str::from_utf8(&result.output) {
                Ok(output) => output,
                Err(error) => {
                    return State::Refused(format!(
                        "it is installed, exited {}, and printed output that is not valid UTF-8: {error}",
                        result.conventional_exit_code()
                    ));
                }
            };
            let said = match output.lines().next() {
                Some(line) => line.trim().to_owned(),
                None => String::new(),
            };
            return State::Refused(format!(
                "it is installed and exited {}{}",
                result.conventional_exit_code(),
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {said}")
                }
            ));
        }
        let stdout = match std::str::from_utf8(&result.stdout) {
            Ok(stdout) => stdout,
            Err(error) => {
                return State::Refused(format!(
                    "it is installed and printed a version that is not valid UTF-8: {error}"
                ));
            }
        };
        stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map_or_else(
                || State::Refused(String::from("it is installed and printed no version")),
                |line| State::Found(line.to_owned()),
            )
    }

    /// Where `program` is on the environment's `PATH`, if it is anywhere.
    fn resolve(&self, program: &str) -> std::io::Result<Option<PathBuf>> {
        let Some(path) = self.environment.var("PATH") else {
            return Ok(None);
        };
        for directory in std::env::split_paths(path) {
            for candidate in [
                directory.join(program),
                directory.join(format!("{program}.exe")),
            ] {
                match std::fs::metadata(&candidate) {
                    Ok(metadata) if metadata.file_type().is_file() => return Ok(Some(candidate)),
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(None)
    }
}
