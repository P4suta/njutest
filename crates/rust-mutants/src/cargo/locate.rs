// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Finding cargo and the rustc it drives, and naming them.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use super::version::{VersionInfo, parse_version};
use super::{CargoError, CargoErrorKind};
use crate::runner::{Bound, Cancel, PROBE, PROBE_OUTPUT_LIMIT, Spec, run};

/// Configures [`Toolchain::locate`].
#[derive(Debug, Clone, Default)]
pub struct LocateOptions {
    /// The cargo to use: a path, or a bare name to find on `search_path`. `None` means the bare name `cargo`.
    pub cargo: Option<PathBuf>,
    /// The `PATH` a bare name is searched on. The composition root reads the process environment; this module never does. `None` refuses every bare name.
    pub search_path: Option<OsString>,
    /// The complete environment every cargo command runs with. `None` inherits this process's environment.
    pub env: Option<Vec<(OsString, OsString)>>,
}

/// A located cargo and the rustc beside it, named by their banners.
#[derive(Debug, Clone)]
pub struct Toolchain {
    cargo: PathBuf,
    rustc: PathBuf,
    sysroot: Option<PathBuf>,
    cargo_version: VersionInfo,
    rustc_version: VersionInfo,
    env: Option<Vec<(OsString, OsString)>>,
}

impl Toolchain {
    /// Finds cargo, then runs `cargo -vV` and `rustc -vV` inside `dir`, so that a rustup toolchain file there is what answers.
    ///
    /// # Errors
    /// [`CargoErrorKind::ToolchainNotFound`] when an executable is missing,
    /// [`CargoErrorKind::CommandFailed`] when a banner could not be read, and
    /// [`CargoErrorKind::VersionUnreadable`] when it could not be parsed.
    pub fn locate(
        options: &LocateOptions,
        dir: &Path,
        cancel: &Cancel,
    ) -> Result<Self, CargoError> {
        let name = options
            .cargo
            .clone()
            .unwrap_or_else(|| PathBuf::from("cargo"));
        let cargo = resolve_executable(&name, options.search_path.as_deref())?;
        let rustc = sibling(&cargo, "rustc").map_or_else(
            || resolve_executable(Path::new("rustc"), options.search_path.as_deref()),
            Ok,
        )?;
        let banner = |program: &Path| -> Result<VersionInfo, CargoError> {
            let mut spec = Spec::new(
                [program.as_os_str(), OsStr::new("-vV")],
                Bound::After(PROBE),
            );
            spec.dir = Some(dir.to_path_buf());
            spec.env.clone_from(&options.env);
            spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
            let result = run(&spec, cancel);
            if !result.ok() {
                return Err(command_failed(&spec, &result));
            }
            parse_version(&String::from_utf8_lossy(&result.stdout))
        };
        let cargo_version = banner(&cargo)?;
        let rustc_version = banner(&rustc)?;
        let sysroot = sysroot_of(&rustc, dir, options.env.as_deref(), cancel);
        Ok(Self {
            cargo,
            rustc,
            sysroot,
            cargo_version,
            rustc_version,
            env: options.env.clone(),
        })
    }

    /// The cargo executable.
    #[must_use]
    pub fn cargo(&self) -> &Path {
        &self.cargo
    }

    /// The rustc executable.
    #[must_use]
    pub fn rustc(&self) -> &Path {
        &self.rustc
    }

    /// The toolchain directory rustc names as its own, when it would say.
    #[must_use]
    pub fn sysroot(&self) -> Option<&Path> {
        self.sysroot.as_deref()
    }

    /// What `cargo -vV` said.
    #[must_use]
    pub const fn cargo_version(&self) -> &VersionInfo {
        &self.cargo_version
    }

    /// What `rustc -vV` said.
    #[must_use]
    pub const fn rustc_version(&self) -> &VersionInfo {
        &self.rustc_version
    }

    /// The target triple the toolchain runs on.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.rustc_version.host
    }

    /// The environment cargo commands run with, when frozen.
    #[must_use]
    pub fn env(&self) -> Option<&[(OsString, OsString)]> {
        self.env.as_deref()
    }

    /// A spec that runs `cargo <args>` inside `dir` with the toolchain's environment, unbounded until the caller says otherwise. The length of a build or a test run is the project's, so the caller assigns [`Spec::timeout`] with the number that applies to it; the caller adds an output limit or a structured stdout as the command warrants.
    pub fn command<I, S>(&self, dir: &Path, args: I) -> Spec
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut command_line = vec![self.cargo.clone().into_os_string()];
        command_line.extend(args.into_iter().map(Into::into));
        let mut spec = Spec::new(command_line, Bound::Unbounded);
        spec.dir = Some(dir.to_path_buf());
        spec.env.clone_from(&self.env);
        spec
    }
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} / {}",
            self.cargo_version.summary, self.rustc_version.summary
        )
    }
}

/// The failure of a cargo command, with the tail of what it said.
#[must_use]
pub fn command_failed(spec: &Spec, result: &crate::runner::RunResult) -> CargoError {
    let argv: Vec<String> = spec
        .argv
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let said = String::from_utf8_lossy(&result.output);
    let said = said.trim();
    let mut message = match &result.error {
        Some(error) => format!("{} could not run: {error}", argv.join(" ")),
        None if result.timed_out => format!("{} timed out", argv.join(" ")),
        None => format!("{} exited with {}", argv.join(" "), result.exit_code),
    };
    if !said.is_empty() {
        message.push_str(": ");
        message.push_str(said);
    }
    CargoError::new(CargoErrorKind::CommandFailed, message)
}

/// The executable `name` beside `program`, if there is one.
fn sibling(program: &Path, name: &str) -> Option<PathBuf> {
    let dir = program.parent()?;
    executable_variants(dir, Path::new(name))
        .into_iter()
        .find(|p| p.is_file())
}

/// Resolves an executable the way a shell would, without consulting this process's environment.
///
/// # Errors
/// [`CargoErrorKind::ToolchainNotFound`].
pub fn resolve_executable(name: &Path, search_path: Option<&OsStr>) -> Result<PathBuf, CargoError> {
    let not_found = |detail: String| CargoError::new(CargoErrorKind::ToolchainNotFound, detail);
    if name.components().count() > 1 || name.is_absolute() {
        return if name.is_file() {
            Ok(name.to_path_buf())
        } else {
            Err(not_found(format!("{} is not a file", name.display())))
        };
    }
    let Some(search_path) = search_path else {
        return Err(not_found(format!(
            "{} is a bare name and no search path was given",
            name.display()
        )));
    };
    for dir in std::env::split_paths(search_path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if let Some(found) = executable_variants(&dir, name)
            .into_iter()
            .find(|p| p.is_file())
        {
            return Ok(found);
        }
    }
    Err(not_found(format!(
        "{} was not found on the search path",
        name.display()
    )))
}

/// The spellings an executable named `name` may have in `dir`.
fn executable_variants(dir: &Path, name: &Path) -> Vec<PathBuf> {
    let plain = dir.join(name);
    if cfg!(windows) {
        let mut variants = vec![plain.clone()];
        for extension in ["exe", "cmd", "bat"] {
            let mut with = plain.clone().into_os_string();
            with.push(".");
            with.push(extension);
            variants.push(PathBuf::from(with));
        }
        variants
    } else {
        vec![plain]
    }
}

/// What `rustc --print sysroot` says, when it will say anything: a path this run may use and never one it needs.
fn sysroot_of(
    rustc: &Path,
    dir: &Path,
    env: Option<&[(OsString, OsString)]>,
    cancel: &Cancel,
) -> Option<PathBuf> {
    let mut spec = Spec::new(
        [
            rustc.as_os_str(),
            OsStr::new("--print"),
            OsStr::new("sysroot"),
        ],
        Bound::After(PROBE),
    );
    spec.dir = Some(dir.to_path_buf());
    spec.env = env.map(<[(OsString, OsString)]>::to_vec);
    spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
    let result = run(&spec, cancel);
    if !result.ok() {
        return None;
    }
    let said = String::from_utf8_lossy(&result.stdout);
    let line = said.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}
