// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest doctor`: what this machine can and cannot do.

use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::cargo::{LocateOptions, Toolchain};
use rust_mutants::runner::{Cancel, Spec, run as run_process};

use crate::cli::{Doctor, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::coverage::Tools;
use crate::trace::Recorder;
use crate::watch::Watch;

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

/// One thing that is either there or not.
struct Finding {
    tool: &'static str,
    need: Need,
    detail: Option<String>,
}

impl Finding {
    fn line(&self) -> String {
        let (state, detail) = self.detail.as_ref().map_or_else(
            || ("missing", String::new()),
            |detail| ("ok", format!("  {detail}")),
        );
        format!("{:<8} {:<14} {state}{detail}", self.need.name(), self.tool)
    }
}

/// Reports the toolchain and the tools, and exits 3 when a required one is missing.
pub fn run(
    _arguments: Doctor,
    environment: &Environment,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> u8 {
    let findings = examine(environment);
    for finding in &findings {
        super::say(stdout, &finding.line());
    }
    let missing: Vec<&str> = findings
        .iter()
        .filter(|finding| finding.need == Need::Required && finding.detail.is_none())
        .map(|finding| finding.tool)
        .collect();
    super::say(stdout, "");
    if missing.is_empty() {
        super::say(stdout, "a standard-v1 run can go ahead on this machine");
        return EXIT_ASSURED;
    }
    super::say(
        stdout,
        &format!("a run cannot go ahead: {} is missing", missing.join(", ")),
    );
    EXIT_ERROR
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

    let required = |tool, detail| Finding {
        tool,
        need: Need::Required,
        detail,
    };
    let optional = |tool, detail| Finding {
        tool,
        need: Need::Optional,
        detail,
    };
    vec![
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
        spec.structured_stdout = Some(64 * 1024);
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
