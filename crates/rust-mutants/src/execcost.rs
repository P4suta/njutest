// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What it costs to run a file that has just been written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runner::{MonitorFailure, ProcessExit, RunnerError};

/// Why the cost of executing a newly written program could not be measured.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExecCostError {
    /// The directory that holds the fresh copy could not be made.
    #[error("{} could not be made: {source}", path.display())]
    CreateDirectory {
        /// The directory that was requested.
        path: PathBuf,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// The program could not be copied to its fresh name.
    #[error("{} could not be copied to {}: {source}", from.display(), to.display())]
    CopyProgram {
        /// The program being copied.
        from: PathBuf,
        /// Its fresh name.
        to: PathBuf,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// The probe completed, but its temporary directory could not be removed.
    #[error("{} could not be removed after the probe: {source}", path.display())]
    Cleanup {
        /// The temporary directory.
        path: PathBuf,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// Both the probe and the cleanup failed; neither failure is hidden.
    #[error(
        "the probe failed ({measurement}); removing {} also failed ({cleanup})",
        path.display()
    )]
    CleanupAfterFailure {
        /// The temporary directory.
        path: PathBuf,
        /// What stopped the probe.
        #[source]
        measurement: Box<Self>,
        /// What stopped cleanup.
        cleanup: std::io::Error,
    },
    /// The runner could not start or collect the program.
    #[error("{} could not be run: {source}", path.display())]
    Runner {
        /// The program being measured.
        path: PathBuf,
        /// The runner failure.
        #[source]
        source: RunnerError,
    },
    /// A monitor failed even though this probe did not configure one.
    #[error("{} had an unexpected monitor failure: {source}", path.display())]
    Monitor {
        /// The program being measured.
        path: PathBuf,
        /// The monitor failure.
        #[source]
        source: MonitorFailure,
    },
    /// The program exceeded the deliberately generous probe bound.
    #[error("{} did not finish within {seconds} seconds, which is itself the answer", path.display())]
    TimedOut {
        /// The program being measured.
        path: PathBuf,
        /// The probe bound in seconds.
        seconds: u64,
    },
    /// An execution monitor stopped a probe that has no monitor.
    #[error("{} was stopped by an unexpected execution monitor", path.display())]
    StoppedByMonitor {
        /// The program being measured.
        path: PathBuf,
    },
    /// The caller cancelled the probe.
    #[error("{} was cancelled", path.display())]
    Cancelled {
        /// The program being measured.
        path: PathBuf,
    },
    /// The program ran but did not return success.
    #[error("{} exited {exit:?} rather than doing nothing successfully", path.display())]
    Unsuccessful {
        /// The program being measured.
        path: PathBuf,
        /// How it ended.
        exit: ProcessExit,
    },
}

/// How long a probe may take before the measurement is abandoned.
///
/// The bound is generous rather than tight: the phenomenon this measures is minutes long, so a deadline that fired at the ordinary case would report nothing on the machine that needed the answer.
/// What it prevents is a probe on a wedged filesystem hanging instead of reporting the slow execution it was there to find.
///
/// macOS evaluates a newly written Mach-O before it may run and Windows scans a newly written executable, so both are machines this measures; what varies is only the name the copy has to have.
pub const PROBE_LIMIT: Duration = Duration::from_secs(600);

/// Copies a program nobody has run from this path before, runs it twice, and hands back what each run took.
///
/// The pair is the evidence and neither number means anything alone: one slow run could be a slow disk, and a slow run beside a fast run of the same file cannot be anything else.
///
/// # Errors
/// What stopped the measurement, which is itself a thing to be told: a silence here reads as a machine that is well.
pub fn exec_twice(temp: &Path, program: &Path) -> Result<(Duration, Duration), ExecCostError> {
    let dir = temp.join(format!(
        "{}exec-{}",
        crate::workspace::SCRATCH_DIR_PREFIX,
        std::process::id()
    ));
    let measured = made(&dir, program);
    let cleaned = crate::tempowner::remove_tree(&dir);
    match (measured, cleaned) {
        (answer, Ok(())) => answer,
        (answer, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => answer,
        (Ok(_answer), Err(source)) => Err(ExecCostError::Cleanup { path: dir, source }),
        (Err(measurement), Err(cleanup)) => Err(ExecCostError::CleanupAfterFailure {
            path: dir,
            measurement: Box::new(measurement),
            cleanup,
        }),
    }
}

/// The two runs, or what stopped them, which is a thing to be told rather than a silence.
fn made(dir: &Path, program: &Path) -> Result<(Duration, Duration), ExecCostError> {
    std::fs::create_dir_all(dir).map_err(|source| ExecCostError::CreateDirectory {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    std::fs::copy(program, &path).map_err(|source| ExecCostError::CopyProgram {
        from: program.to_path_buf(),
        to: path.clone(),
        source,
    })?;
    Ok((timed(&path)?, timed(&path)?))
}

/// How long one run of `path` took, or nothing when it could not be started or would not finish.
fn timed(path: &Path) -> Result<Duration, ExecCostError> {
    let spec = crate::runner::Spec::new(
        [path.as_os_str().to_owned(), "--version".into()],
        crate::runner::Bound::After(PROBE_LIMIT),
    );
    let result = crate::runner::run(&spec, &crate::runner::Cancel::new());
    let duration = result.duration;
    match result.termination {
        crate::runner::Termination::NotStarted { error }
        | crate::runner::Termination::WaitFailed { error } => {
            return Err(ExecCostError::Runner {
                path: path.to_path_buf(),
                source: error,
            });
        }
        crate::runner::Termination::MonitorFailed { failure } => {
            return Err(ExecCostError::Monitor {
                path: path.to_path_buf(),
                source: failure,
            });
        }
        crate::runner::Termination::TimedOut => {
            return Err(ExecCostError::TimedOut {
                path: path.to_path_buf(),
                seconds: PROBE_LIMIT.as_secs(),
            });
        }
        crate::runner::Termination::StoppedByMonitor | crate::runner::Termination::Stalled => {
            return Err(ExecCostError::StoppedByMonitor {
                path: path.to_path_buf(),
            });
        }
        crate::runner::Termination::Cancelled { .. } => {
            return Err(ExecCostError::Cancelled {
                path: path.to_path_buf(),
            });
        }
        crate::runner::Termination::Exited(ProcessExit::Code(0)) => {}
        crate::runner::Termination::Exited(exit) => {
            return Err(ExecCostError::Unsuccessful {
                path: path.to_path_buf(),
                exit,
            });
        }
    }
    Ok(duration)
}
