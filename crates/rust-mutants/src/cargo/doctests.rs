// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Building one library's doctests for the sealed target, with every binary rustdoc would run handed to a capture instead (ADR 0046).

use std::ffi::OsString;
use std::path::Path;

use super::compile::{CompileOptions, Exited};
use super::locate::command_failed;
use super::{CargoError, CargoErrorKind, Driver};
use crate::runner::{Termination, run};
use crate::trace::ExecRecord;

/// How much of rustdoc's report is kept.
const REPORT_LIMIT: usize = 64 << 20;

/// Configures [`capture_doctests`].
#[derive(Debug, Clone, Copy)]
pub struct DoctestCapture<'a> {
    /// The package whose doctests are built.
    pub package: &'a str,
    /// The program rustdoc runs each binary through, and the directory it keeps them in.
    pub capture: (&'a Path, &'a Path),
    /// The rest of the compilation, as the sealed build of the tests asked for it.
    pub compile: &'a CompileOptions,
}

/// The command line that builds `capture.package`'s doctests with the capture for their runner.
///
/// It gives rustdoc none of the native run's harness arguments: which doctests the suite runs is what the native baseline says.
///
/// # Errors
/// [`CargoErrorKind::CommandFailed`] where a path the runner is given is not text, which cargo's configuration cannot carry.
pub fn capture_arguments(capture: &DoctestCapture<'_>) -> Result<Vec<OsString>, CargoError> {
    let options = capture.compile;
    let mut args: Vec<OsString> = ["test", "--doc", "--package", capture.package]
        .into_iter()
        .map(OsString::from)
        .collect();
    if options.locked {
        args.push(OsString::from("--locked"));
    }
    if options.offline {
        args.push(OsString::from("--offline"));
    }
    args.push(OsString::from("--target-dir"));
    args.push(options.target_dir.path().as_os_str().to_owned());
    args.extend(
        options
            .build
            .cargo_arguments()
            .into_iter()
            .map(OsString::from),
    );
    let text = |path: &Path| match path.to_str() {
        Some(text) => Ok(toml::Value::String(text.to_owned())),
        None => Err(CargoError::new(
            CargoErrorKind::CommandFailed,
            format!(
                "{} is not text, so cargo's configuration cannot name it as a runner",
                path.display()
            ),
        )),
    };
    let (program, directory) = capture.capture;
    let runner = toml::Value::Array(vec![text(program)?, text(directory)?]);
    let target = match &options.build.target {
        Some(target) => target.as_str(),
        None => crate::sealed::TARGET,
    };
    args.push(OsString::from("--config"));
    args.push(OsString::from(format!("target.{target}.runner={runner}")));
    args.push(OsString::from("--"));
    args.push(OsString::from("--test-threads=1"));
    Ok(args)
}

/// Builds the doctests `capture` names, handing every binary to the capture, and returns what rustdoc printed.
///
/// What cargo exited with is not asked: the capture fails every binary on purpose.
///
/// # Errors
/// [`CargoErrorKind::BuildLedger`] when the target directory cannot be settled, [`CargoErrorKind::Cancelled`] when the run was cancelled, and [`CargoErrorKind::CommandFailed`] when cargo could not run, timed out, or printed more than is kept.
pub fn capture_doctests(
    driver: &Driver<'_>,
    capture: &DoctestCapture<'_>,
) -> Result<Vec<u8>, CargoError> {
    let options = capture.compile;
    options.target_dir.settle()?;
    let mut spec = driver
        .toolchain
        .command(driver.dir, capture_arguments(capture)?);
    if !options.env.is_empty() {
        let Some(mut env) = spec.env.clone() else {
            return Err(CargoError::new(
                CargoErrorKind::CommandFailed,
                "the doctests' build adds variables to the toolchain's environment, and the \
                 toolchain was given none: it inherits this process's, which only the \
                 composition root reads, so there is nothing to add them to",
            ));
        };
        env.overlay(&options.env);
        spec.env = Some(env);
    }
    spec.structured_stdout = Some(REPORT_LIMIT);
    spec.timeout = options.timeout;
    let result = run(&spec, driver.cancel);
    driver.trace.exec_result(ExecRecord::of(&spec, &result));
    let cancelled = matches!(&result.termination, Termination::Cancelled { .. });
    if driver.cancel.is_cancelled() || cancelled {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the doctests' build was cancelled",
        ));
    }
    if Exited::of(&result.termination).is_none() {
        return Err(command_failed(&spec, &result));
    }
    if result.stdout_truncated {
        return Err(CargoError::new(
            CargoErrorKind::CommandFailed,
            "rustdoc printed more of a report of doctests than the engine keeps",
        ));
    }
    Ok(result.stdout)
}

/// The capture program in `directory`, compiled there with the toolchain's own rustc unless the program its source names is already there.
///
/// The source is written last, as the mark that the program beside it is the one it names, so a run that stopped halfway leaves nothing a later run takes for finished.
///
/// # Errors
/// [`CargoErrorKind::CommandFailed`] when its source cannot be written or rustc refuses it, and [`CargoErrorKind::Cancelled`] when the run was cancelled.
pub fn build_capture(
    driver: &Driver<'_>,
    directory: &Path,
) -> Result<std::path::PathBuf, CargoError> {
    let failed = |path: &Path, error: std::io::Error| {
        CargoError::new(
            CargoErrorKind::CommandFailed,
            format!("{}: {error}", path.display()),
        )
    };
    let source = directory.join("capture.rs");
    let program = directory.join(format!("capture{}", std::env::consts::EXE_SUFFIX));
    let marked = match std::fs::read(&source) {
        Ok(bytes) => bytes == crate::sealed::doctest::CAPTURE_SOURCE.as_bytes(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(failed(&source, error)),
    };
    let built = match std::fs::metadata(&program) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(failed(&program, error)),
    };
    if marked && built {
        return Ok(program);
    }
    std::fs::create_dir_all(directory).map_err(|error| failed(directory, error))?;
    let staged_source = directory.join(format!("{}.rs", crate::replace::staging(&source)));
    let staged_program = directory.join(crate::replace::staging(&program));
    std::fs::write(&staged_source, crate::sealed::doctest::CAPTURE_SOURCE)
        .map_err(|error| failed(&staged_source, error))?;
    let mut spec = crate::runner::Spec::new(
        [
            driver.toolchain.rustc().as_os_str().to_owned(),
            OsString::from("--edition=2021"),
            OsString::from("--crate-name=capture"),
            OsString::from("-o"),
            staged_program.as_os_str().to_owned(),
            staged_source.as_os_str().to_owned(),
        ],
        crate::runner::Bound::After(crate::runner::PROBE),
    );
    spec.dir = Some(directory.to_path_buf());
    spec.env = driver.toolchain.env().cloned();
    let result = run(&spec, driver.cancel);
    driver.trace.exec_result(ExecRecord::of(&spec, &result));
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the doctests' capture was cancelled",
        ));
    }
    if !result.succeeded() {
        return Err(command_failed(&spec, &result));
    }
    std::fs::rename(&staged_program, &program).map_err(|error| failed(&program, error))?;
    std::fs::remove_file(&staged_source).map_err(|error| failed(&staged_source, error))?;
    crate::replace::file(&source, crate::sealed::doctest::CAPTURE_SOURCE.as_bytes())
        .map_err(|failure| failed(&failure.path, failure.source))?;
    Ok(program)
}

/// Empties `directory`, or makes it, so that every claim a capture gives out there is one this build made.
///
/// # Errors
/// [`CargoErrorKind::BuildLedger`] when it cannot be emptied or made.
pub fn empty_capture(directory: &Path) -> Result<(), CargoError> {
    let refused = |error: std::io::Error| {
        CargoError::new(
            CargoErrorKind::BuildLedger,
            format!("{}: {error}", directory.display()),
        )
    };
    crate::tempowner::remove_tree(directory).map_err(refused)?;
    std::fs::create_dir_all(directory).map_err(refused)
}
