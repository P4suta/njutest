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

/// One thing a run needs, and what was found out about it.
struct Finding {
    /// What it is called, which is a tool's name or the word for what it is.
    named: &'static str,
    need: Need,
    detail: State,
}

impl Finding {
    fn line(&self) -> String {
        format!(
            "{:<8} {:<14} {}{}",
            self.need.name(),
            self.named,
            self.detail.word(),
            self.detail.detail()
        )
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
        super::say(stdout, "a standard-v1 run can go ahead on this machine");
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
    let optional = |named, detail: Option<String>| Finding {
        named,
        need: Need::Optional,
        detail: detail.map_or(State::Missing, State::Found),
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
    ]
}

/// What a run in this directory would make of the configuration beside it.
///
/// A doctor says whether a run can go ahead here, and a run reads this file
/// before it does anything else. One that answered about the tools alone would
/// say a run can go ahead and be contradicted by the next command.
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
    /// The first line the tool prints, or nothing when it is not there. A tool that is absent is not an error here: that is the answer.
    fn version_of(&self, program: &str, arguments: &[&str]) -> Option<String> {
        let program = self.resolve(program)?;
        let mut spec = Spec::new(
            std::iter::once(program.into_os_string())
                .chain(arguments.iter().map(std::ffi::OsString::from)),
        );
        spec.dir = Some(self.dir.to_path_buf());
        spec.env = Some(self.environment.vars.clone());
        spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
        let result = run_process(&spec, self.cancel);
        if result.error.is_some() || result.exit_code != 0 {
            return None;
        }
        String::from_utf8_lossy(&result.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
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
