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
    /// The cargo to use: a path, or a bare name to find on `search_path`.
    /// `None` means the bare name `cargo`.
    pub cargo: Option<PathBuf>,
    /// The `PATH` a bare name is searched on.
    /// The composition root reads the process environment; this module never does.
    /// `None` refuses every bare name.
    pub search_path: Option<OsString>,
    /// The complete environment every cargo command runs with.
    /// `None` inherits this process's environment.
    pub env: Option<crate::vars::Variables>,
}

/// A located cargo and the rustc beside it, named by their banners.
#[derive(Debug, Clone)]
pub struct Toolchain {
    cargo: PathBuf,
    chosen: PathBuf,
    rustc: PathBuf,
    sysroot: Option<PathBuf>,
    cargo_version: VersionInfo,
    rustc_version: VersionInfo,
    env: Option<crate::vars::Variables>,
}

impl Toolchain {
    /// Finds cargo, then runs `cargo -vV` and `rustc -vV` inside `dir`, so that a rustup toolchain file there is what answers.
    ///
    /// # Errors
    /// [`CargoErrorKind::ToolchainNotFound`] when an executable is missing,
    /// [`CargoErrorKind::CommandFailed`] when a banner could not be read, and [`CargoErrorKind::VersionUnreadable`] when it could not be parsed.
    pub fn locate(
        options: &LocateOptions,
        dir: &Path,
        cancel: &Cancel,
    ) -> Result<Self, CargoError> {
        let name = match &options.cargo {
            Some(cargo) => cargo.clone(),
            None => PathBuf::from("cargo"),
        };
        let cargo = resolve_executable(&name, options.search_path.as_deref())?;
        let rustc = match sibling(&cargo, "rustc")? {
            Some(rustc) => rustc,
            None => resolve_executable(Path::new("rustc"), options.search_path.as_deref())?,
        };
        let banner = |program: &Path| -> Result<VersionInfo, CargoError> {
            let mut spec = Spec::new(
                [program.as_os_str(), OsStr::new("-vV")],
                Bound::After(PROBE),
            );
            spec.dir = Some(dir.to_path_buf());
            spec.env.clone_from(&options.env);
            spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
            let result = run(&spec, cancel);
            if !result.succeeded() {
                return Err(command_failed(&spec, &result));
            }
            let banner = std::str::from_utf8(&result.stdout).map_err(|source| {
                CargoError::new(
                    CargoErrorKind::VersionUnreadable,
                    format!("{} printed a non-UTF-8 version banner", program.display()),
                )
                .with_source(source)
            })?;
            parse_version(banner)
        };
        let cargo_version = banner(&cargo)?;
        let rustc_version = banner(&rustc)?;
        let sysroot = sysroot_of(&rustc, dir, options.env.as_ref(), cancel)?;
        let chosen_by_path = name.components().count() == 1 && !name.is_absolute();
        let toolchain = if chosen_by_path {
            sysroot.as_deref()
        } else {
            None
        };
        let pinned_cargo = pinned((&cargo, "cargo"), toolchain, &cargo_version, banner)?;
        let pinned_rustc = pinned((&rustc, "rustc"), toolchain, &rustc_version, banner)?;
        let env = match options.env.clone() {
            Some(env) if pinned_rustc != rustc => {
                Some(with_toolchain(env, &pinned_rustc, sysroot.as_deref())?)
            }
            unpinned => unpinned,
        };
        Ok(Self {
            chosen: cargo,
            cargo: pinned_cargo,
            rustc: pinned_rustc,
            sysroot,
            cargo_version,
            rustc_version,
            env,
        })
    }

    /// What `rustc --print cfg` says of `target`, or of the host where there is none, kept to the names a target alone decides (ADR 0042).
    ///
    /// # Errors
    /// [`CargoErrorKind::CommandFailed`] when rustc could not say, and [`CargoErrorKind::VersionUnreadable`] when what it said is not text.
    pub fn target_facts(
        &self,
        dir: &Path,
        target: Option<&str>,
        cancel: &Cancel,
    ) -> Result<crate::facts::Facts, CargoError> {
        let mut args = vec![
            self.rustc.as_os_str().to_owned(),
            OsString::from("--print"),
            OsString::from("cfg"),
        ];
        if let Some(target) = target {
            args.push(OsString::from("--target"));
            args.push(OsString::from(target));
        }
        let mut spec = Spec::new(args, Bound::After(PROBE));
        spec.dir = Some(dir.to_path_buf());
        spec.env.clone_from(&self.env);
        spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
        let result = run(&spec, cancel);
        if !result.succeeded() {
            return Err(command_failed(&spec, &result));
        }
        let said = std::str::from_utf8(&result.stdout).map_err(|source| {
            CargoError::new(
                CargoErrorKind::VersionUnreadable,
                format!("{} printed a non-UTF-8 cfg", self.rustc.display()),
            )
            .with_source(source)
        })?;
        Ok(crate::facts::Facts::printed(said))
    }
    /// The cargo executable.
    #[must_use]
    pub fn cargo(&self) -> &Path {
        &self.cargo
    }

    /// The cargo the search path chose, before it was pinned, which is the one a command naming another toolchain than this run's (`+nightly`) runs.
    #[must_use]
    pub fn selecting(&self) -> Selecting<'_> {
        Selecting(&self.chosen)
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
    pub const fn env(&self) -> Option<&crate::vars::Variables> {
        self.env.as_ref()
    }

    /// A spec that runs `cargo <args>` inside `dir` with the toolchain's environment, unbounded until the caller says otherwise.
    /// The length of a build or a test run is the project's, so the caller assigns [`Spec::timeout`] with the number that applies to it; the caller adds an output limit or a structured stdout as the command warrants.
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

/// A cargo that may select a toolchain by `+name`: the one the search path chose, which is rustup's proxy where rustup is installed, and never the toolchain's own cargo, which knows no `+name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selecting<'a>(&'a Path);

impl<'a> Selecting<'a> {
    /// A cargo somebody named by its path, which is run as named.
    #[must_use]
    pub const fn named(path: &'a Path) -> Self {
        Self(path)
    }

    /// The program to run.
    #[must_use]
    pub const fn path(self) -> &'a Path {
        self.0
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
    let argv: Vec<String> = spec.argv.iter().map(|arg| diagnostic_os(arg)).collect();
    let said = diagnostic_bytes(&result.output);
    let mut message = match &result.termination {
        crate::runner::Termination::NotStarted { error }
        | crate::runner::Termination::WaitFailed { error } => {
            format!("{} could not run: {error}", argv.join(" "))
        }
        crate::runner::Termination::MonitorFailed { failure } => {
            format!("{} monitor failed: {failure}", argv.join(" "))
        }
        crate::runner::Termination::TimedOut => format!("{} timed out", argv.join(" ")),
        crate::runner::Termination::Stalled => {
            format!("{} made no progress for its quiet window", argv.join(" "))
        }
        crate::runner::Termination::StoppedByMonitor => {
            format!("{} was stopped by its execution monitor", argv.join(" "))
        }
        crate::runner::Termination::Answered => {
            format!(
                "{} was stopped at the first test it said failed",
                argv.join(" ")
            )
        }
        crate::runner::Termination::Cancelled { .. } => {
            format!("{} was cancelled", argv.join(" "))
        }
        crate::runner::Termination::Exited(exit) => {
            let status = match exit.conventional_code() {
                Some(code) => code.to_string(),
                None => String::from("an unknown status"),
            };
            format!("{} exited with {status}", argv.join(" "))
        }
    };
    if !said.is_empty() {
        message.push_str(": ");
        message.push_str(&said);
    }
    CargoError::new(CargoErrorKind::CommandFailed, message)
}

fn diagnostic_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.trim().to_owned(),
        Err(_invalid_utf8) => format!("non-UTF-8 output (hex): {}", hex::encode(bytes)),
    }
}

fn diagnostic_os(value: &OsStr) -> String {
    match value.to_str() {
        Some(text) => text.to_owned(),
        None => format!(
            "non-UTF-8 argument (hex): {}",
            hex::encode(value.as_encoded_bytes())
        ),
    }
}

/// The executable `name` in the toolchain directory `sysroot` where it says exactly what `located` said, which is the same program without whatever chose it, and otherwise `located`.
///
/// A shim that chooses a toolchain by the directory it runs in, as mise and direnv do, is asked once, in the directory the run was asked in; every later command runs in a snapshot, which such a shim may refuse or answer differently.
/// Only a cargo found by its bare name is pinned: one named by its path is somebody's choice, a wrapper perhaps, and is run as named.
fn pinned(
    (located, name): (&Path, &str),
    sysroot: Option<&Path>,
    said: &VersionInfo,
    banner: impl Fn(&Path) -> Result<VersionInfo, CargoError>,
) -> Result<PathBuf, CargoError> {
    let Some(sysroot) = sysroot else {
        return Ok(located.to_path_buf());
    };
    let Some(candidate) =
        first_executable(executable_variants(&sysroot.join("bin"), Path::new(name)))?
    else {
        return Ok(located.to_path_buf());
    };
    if candidate == located {
        return Ok(candidate);
    }
    match banner(&candidate) {
        Ok(candidate_said) if candidate_said == *said => Ok(candidate),
        Ok(_another_toolchain) => Ok(located.to_path_buf()),
        Err(_unrunnable) => Ok(located.to_path_buf()),
    }
}

/// `env` with `RUSTC` naming the pinned `rustc` and `RUSTDOC` the `rustdoc` beside it, unless the environment already names them, so cargo never asks a shim again.
fn with_toolchain(
    mut env: crate::vars::Variables,
    rustc: &Path,
    sysroot: Option<&Path>,
) -> Result<crate::vars::Variables, CargoError> {
    if !env.holds("RUSTC") {
        env.set("RUSTC", rustc.as_os_str());
    }
    if let Some(sysroot) = sysroot
        && !env.holds("RUSTDOC")
        && let Some(rustdoc) = first_executable(executable_variants(
            &sysroot.join("bin"),
            Path::new("rustdoc"),
        ))?
    {
        env.set("RUSTDOC", rustdoc);
    }
    Ok(env)
}

/// The executable `name` beside `program`, if there is one.
fn sibling(program: &Path, name: &str) -> Result<Option<PathBuf>, CargoError> {
    let Some(dir) = program.parent() else {
        return Ok(None);
    };
    first_executable(executable_variants(dir, Path::new(name)))
}

/// Resolves an executable the way a shell would, without consulting this process's environment.
///
/// # Errors
/// [`CargoErrorKind::ToolchainNotFound`].
pub fn resolve_executable(name: &Path, search_path: Option<&OsStr>) -> Result<PathBuf, CargoError> {
    let not_found = |detail: String| CargoError::new(CargoErrorKind::ToolchainNotFound, detail);
    if name.components().count() > 1 || name.is_absolute() {
        return if executable_file(name)? {
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
    let mut unreadable = Vec::new();
    for dir in std::env::split_paths(search_path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for candidate in executable_variants(&dir, name) {
            match std::fs::metadata(&candidate) {
                Ok(metadata) if metadata.file_type().is_file() => return Ok(candidate),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if passed_over(&error) => {
                    unreadable.push(format!("{}: {error}", candidate.display()));
                }
                Err(source) => {
                    return Err(CargoError::new(
                        CargoErrorKind::ToolchainNotFound,
                        format!(
                            "cannot inspect executable candidate {}",
                            candidate.display()
                        ),
                    )
                    .with_source(source));
                }
            }
        }
    }
    if unreadable.is_empty() {
        return Err(not_found(format!(
            "{} was not found on the search path",
            name.display()
        )));
    }
    Err(not_found(format!(
        "{} was not found on the search path, which holds candidates that could not be read: {}",
        name.display(),
        unreadable.join("; ")
    )))
}

/// Windows' refusal to traverse a mount point the process does not trust, such as a junction a user made.
const ERROR_UNTRUSTED_MOUNT_POINT: i32 = 448;

/// Whether a shell searching its path passes over a candidate whose metadata fails with `error`, as `execvp` does over an entry that is not a directory or that it may not enter, and Windows does over a path through a mount point it does not trust.
fn passed_over(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::PermissionDenied
    ) || (cfg!(windows) && error.raw_os_error() == Some(ERROR_UNTRUSTED_MOUNT_POINT))
}

fn first_executable(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<Option<PathBuf>, CargoError> {
    for candidate in candidates {
        if executable_file(&candidate)? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Whether the shell-compatible candidate resolves to a regular file.
/// Metadata failures other than absence remain failures rather than becoming an apparently clean search miss.
/// Following a tool symlink is deliberate:
/// that is the executable the shell would run too.
fn executable_file(path: &Path) -> Result<bool, CargoError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(CargoError::new(
            CargoErrorKind::ToolchainNotFound,
            format!("cannot inspect executable candidate {}", path.display()),
        )
        .with_source(source)),
    }
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
    env: Option<&crate::vars::Variables>,
    cancel: &Cancel,
) -> Result<Option<PathBuf>, CargoError> {
    let mut spec = Spec::new(
        [
            rustc.as_os_str(),
            OsStr::new("--print"),
            OsStr::new("sysroot"),
        ],
        Bound::After(PROBE),
    );
    spec.dir = Some(dir.to_path_buf());
    spec.env = env.cloned();
    spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
    let result = run(&spec, cancel);
    if !result.succeeded() {
        return Ok(None);
    }
    let said = std::str::from_utf8(&result.stdout).map_err(|source| {
        CargoError::new(
            CargoErrorKind::VersionUnreadable,
            format!("{} printed a non-UTF-8 sysroot", rustc.display()),
        )
        .with_source(source)
    })?;
    let Some(line) = said.lines().next() else {
        return Ok(None);
    };
    let line = line.trim();
    if line.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(line)))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_path_through_an_untrusted_mount_point_is_passed_over_where_windows_refuses_it() {
        let untrusted = std::io::Error::from_raw_os_error(super::ERROR_UNTRUSTED_MOUNT_POINT);
        assert_eq!(
            super::passed_over(&untrusted),
            cfg!(windows),
            "Windows refuses to traverse a junction a user made, as scoop's `current` is, and \
             its own command lookup moves past it: {untrusted}"
        );
    }
}
