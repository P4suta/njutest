// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest doctor`: what this machine can and cannot do.

use std::fmt::Write as _;
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

/// One thing a run needs, and what was found out about it.
struct Finding {
    /// What it is called, which is a tool's name or the word for what it is.
    named: &'static str,
    need: Need,
    detail: State,
}

impl Finding {
    fn line(&self) -> String {
        let mut text = format!(
            "{:<8} {:<14} {}{}",
            self.need.name(),
            self.named,
            self.detail.word(),
            self.detail.detail()
        );
        if !self.detail.held()
            && let Some(remedy) = self.remedy()
        {
            let written = write!(text, "\n         try: {remedy}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        text
    }

    /// What to do about this one, which is the half a reader acts on.
    fn remedy(&self) -> Option<&'static str> {
        Some(match self.named {
            "cargo" | "rustc" => {
                "install the toolchain this project pins, or put it on the PATH this \
                 process was given"
            }
            "llvm-profdata" | "llvm-cov" => "rustup component add llvm-tools-preview",
            "nightly" => "rustup toolchain install nightly",
            "miri" => {
                "rustup +nightly component add miri; without it a deep-v1 contract cannot \
                 interpret, and a standard-v1 run does not need it"
            }
            "cargo-fuzz" => {
                "cargo install cargo-fuzz; without it [fuzz] run = true finds targets and \
                 drives none of them"
            }
            "git" => {
                "install git; without it --changed cannot say what changed and the report \
                 records the repository as unavailable"
            }
            "exec" => {
                "this machine is evaluating new executables; a run started now measures \
                 that and not your tests, so wait until the first number is under a second"
            }
            _ => return None,
        })
    }
}

/// Reports the toolchain and the tools, succeeding only when every required one is present.
pub(super) fn run(
    _arguments: Doctor,
    environment: &Environment,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> Completion {
    let findings = examine(environment);
    for finding in &findings {
        super::say(stdout, &finding.line());
    }
    let wanting: Vec<String> = findings
        .iter()
        .filter(|finding| finding.need == Need::Required && !finding.detail.held())
        .map(|finding| format!("{} is {}", finding.named, finding.detail.word()))
        .collect();
    super::say(stdout, "");
    if wanting.is_empty() {
        super::say(
            stdout,
            &format!(
                "a standard-v1 run can go ahead: {} were examined and every required one \
                 answered",
                findings.len()
            ),
        );
        Completion::Assured
    } else {
        super::say(
            stdout,
            &format!("a run cannot go ahead: {}", wanting.join(", ")),
        );
        Completion::Error
    }
}

/// Everything this machine was asked about, in reading order: what a run needs first, then what it would only like.
fn examine(environment: &Environment) -> Vec<Finding> {
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let dir = environment.working_directory.clone();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: Some(environment.vars.clone()),
        },
        &dir,
        &cancel,
    )
    .ok();
    let tools = toolchain
        .as_ref()
        .and_then(|located| Tools::locate(located, &dir, &Watch::new(&cancel, &trace)).ok());
    let probe = Probe {
        environment,
        dir: &dir,
        cancel: &cancel,
    };

    let required = |named, detail: Option<String>| Finding {
        named,
        need: Need::Required,
        detail: detail.map_or(State::Missing, State::Found),
    };
    let optional = |named, detail: State| Finding {
        named,
        need: Need::Optional,
        detail,
    };
    vec![
        Finding {
            named: "configuration",
            need: Need::Required,
            detail: configuration(&dir),
        },
        required(
            "cargo",
            toolchain
                .as_ref()
                .map(|located| located.cargo().display().to_string()),
        ),
        required(
            "rustc",
            toolchain
                .as_ref()
                .map(|located| located.rustc().display().to_string()),
        ),
        required(
            "llvm-profdata",
            tools
                .as_ref()
                .map(|found| found.profdata.display().to_string()),
        ),
        required(
            "llvm-cov",
            tools.as_ref().map(|found| found.cov.display().to_string()),
        ),
        optional("git", probe.version_of("git", &["--version"])),
        optional(
            "nightly",
            probe.version_of("rustup", &["run", "nightly", "rustc", "--version"]),
        ),
        optional(
            "miri",
            probe.version_of("cargo", &["+nightly", "miri", "--version"]),
        ),
        optional(
            "cargo-fuzz",
            probe.version_of("cargo", &["fuzz", "--version"]),
        ),
        Finding {
            named: "exec",
            need: Need::Required,
            detail: exec_cost(&environment.temp_directory),
        },
    ]
}

/// What it costs to run a file that has just been written, which a run does per target it builds.
///
/// A system that evaluates an executable before it may run pays that cost once
/// per file, and where the evaluation has a backlog it is seconds or minutes.
/// A run builds a binary per target and runs each once, so it pays that per
/// target and measures the evaluation instead of the tests. Nothing else a
/// caller can see reports it: not load, not free processors, not free memory.
/// The pair is the evidence — one slow run could be a slow disk, and a slow one
/// beside a fast one of the same file cannot be anything else.
fn exec_cost(temp: &Path) -> State {
    let Some((first, second)) = rust_mutants::execcost::exec_twice(temp) else {
        return State::Found(String::from(
            "not measured on this platform, so nothing here says what running a fresh \
             binary costs",
        ));
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
        Ok(_read) if path.is_file() => State::Found(path.display().to_string()),
        Ok(_defaults) => State::Found(format!(
            "none; the defaults apply. `njutest init` writes {}",
            crate::config::FILE_NAME
        )),
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
    /// A tool that is not installed and a tool that is installed and answered
    /// badly are different things to be told: the second sends somebody to
    /// install what they already have. The exit status and what it printed are
    /// in hand, so they are what is said.
    fn version_of(&self, program: &str, arguments: &[&str]) -> State {
        let Some(program) = self.resolve(program) else {
            return State::Missing;
        };
        let mut spec = Spec::new(
            std::iter::once(program.into_os_string())
                .chain(arguments.iter().map(std::ffi::OsString::from)),
        );
        spec.dir = Some(self.dir.to_path_buf());
        spec.env = Some(self.environment.vars.clone());
        spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
        let result = run_process(&spec, self.cancel);
        if let Some(error) = &result.error {
            return State::Refused(format!("it is installed and would not start: {error}"));
        }
        if result.exit_code != 0 {
            let said = String::from_utf8_lossy(&result.output)
                .lines()
                .next()
                .map(str::trim)
                .unwrap_or_default()
                .to_owned();
            return State::Refused(format!(
                "it is installed and exited {}{}",
                result.exit_code,
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {said}")
                }
            ));
        }
        String::from_utf8_lossy(&result.stdout)
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
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        let path = self.environment.var("PATH")?;
        std::env::split_paths(path)
            .flat_map(|directory| {
                [
                    directory.join(program),
                    directory.join(format!("{program}.exe")),
                ]
            })
            .find(|candidate| candidate.is_file())
    }
}
