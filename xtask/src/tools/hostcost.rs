// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Keyed actual command and host-wait observations from repository gates.

use std::io::{self, Write as _};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::environment::Environment;
use crate::observation::WaitNote;
use crate::work::{Ended, WorkError};

#[derive(Debug, Serialize)]
pub(super) struct Invocation {
    identity: String,
    encoding: &'static str,
    role: Role,
    argv: Vec<String>,
    directory: Option<String>,
}

impl Invocation {
    pub(super) fn of(command: &Command) -> Self {
        let argv: Vec<_> = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|argument| hex::encode(argument.as_encoded_bytes()))
            .collect();
        let directory = command
            .get_current_dir()
            .map(|path| hex::encode(path.as_os_str().as_encoded_bytes()));
        let identity = hex::encode(Sha256::digest(format!("{command:?}")));
        Self {
            identity,
            encoding: "os-str-encoded-bytes-hex",
            role: Role::of(command),
            argv,
            directory,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Role {
    CargoBuild,
    CargoTest,
    CargoDocTest,
    CargoMetadata,
    CargoProbe,
    CargoUnclassified,
    RustcProbe,
    RustcBuild,
    Program,
}

impl Role {
    fn of(command: &Command) -> Self {
        let program = std::path::Path::new(command.get_program())
            .file_stem()
            .and_then(std::ffi::OsStr::to_str);
        let args: Vec<_> = command.get_args().collect();
        match program {
            Some("cargo")
                if args
                    .iter()
                    .any(|arg| matches!(arg.to_str(), Some("--version" | "-V" | "-vV"))) =>
            {
                Self::CargoProbe
            }
            Some("cargo") => match args
                .iter()
                .find(|arg| !arg.as_encoded_bytes().starts_with(b"+"))
                .and_then(|arg| arg.to_str())
            {
                Some("build" | "check" | "rustc") => Self::CargoBuild,
                Some("test") if args.contains(&std::ffi::OsStr::new("--no-run")) => {
                    Self::CargoBuild
                }
                Some("test") if args.contains(&std::ffi::OsStr::new("--doc")) => Self::CargoDocTest,
                Some("test") => Self::CargoTest,
                Some("metadata") => Self::CargoMetadata,
                Some(_) | None => Self::CargoUnclassified,
            },
            Some("rustc")
                if args.iter().any(|arg| {
                    matches!(arg.to_str(), Some("--version" | "-V" | "-vV" | "--print"))
                }) =>
            {
                Self::RustcProbe
            }
            Some("rustc") => Self::RustcBuild,
            _ => Self::Program,
        }
    }
}

#[derive(Debug)]
pub(super) struct Measured<'a> {
    pub(super) invocation: Invocation,
    pub(super) leader: Option<u32>,
    pub(super) began: Instant,
    pub(super) outcome: &'a Result<Ended, WorkError>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Origin {
    Suite {
        binary: String,
        test: String,
    },
    Product {
        program: &'static str,
        command: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Launch {
    Observed { leader: u32 },
    NotStarted,
    Unobserved { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize)]
struct Machine {
    os: &'static str,
    arch: &'static str,
    cpus: usize,
}

#[derive(Debug, Serialize)]
struct Record {
    schema: &'static str,
    origin: Origin,
    machine: Machine,
    invocation: Invocation,
    launch: Launch,
    duration_ns: u64,
    waits: Vec<WaitNote>,
    outcome: String,
}

/// Publishes the actual measured command on both success and refusal paths.
pub(super) fn publish(
    measured: Measured<'_>,
    waits: Vec<WaitNote>,
    environment: &Environment,
) -> io::Result<()> {
    let Some(directory) = environment.value("NJUTEST_TEST_COST_DIR") else {
        return Ok(());
    };
    let Measured {
        invocation,
        leader,
        began,
        outcome,
    } = measured;
    let launch = match leader {
        Some(leader) => Launch::Observed { leader },
        None => match outcome {
            Err(WorkError::Start { .. }) => Launch::NotStarted,
            Ok(
                Ended::Exited(_)
                | Ended::Interrupted { .. }
                | Ended::OverBudget { .. }
                | Ended::Quiet { .. },
            )
            | Err(WorkError::Watch { .. } | WorkError::Signals { .. }) => Launch::Unobserved {
                reason: "the owned runner did not publish a leader identity".to_owned(),
            },
            #[cfg(unix)]
            Err(WorkError::Outlived) => Launch::Unobserved {
                reason: "the producer outlived the owned completion observation".to_owned(),
            },
        },
    };
    let record = Record {
        schema: "njutest-host-work-v1",
        origin: origin(environment, &invocation)?,
        machine: Machine {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cpus: std::thread::available_parallelism()?.get(),
        },
        invocation,
        launch,
        duration_ns: u64::try_from(began.elapsed().as_nanos()).map_err(io::Error::other)?,
        waits,
        outcome: format!("{outcome:?}"),
    };
    let mut output = tempfile::Builder::new()
        .prefix("host-work-")
        .suffix(".json")
        .tempfile_in(directory)?;
    serde_json::to_writer(output.as_file_mut(), &record).map_err(io::Error::other)?;
    output.as_file_mut().write_all(b"\n")?;
    let (file, path) = output.keep().map_err(io::Error::other)?;
    drop(file);
    drop(path);
    Ok(())
}

fn origin(environment: &Environment, invocation: &Invocation) -> io::Result<Origin> {
    match (
        environment.value("NEXTEST_BINARY_ID"),
        environment.value("NEXTEST_TEST_NAME"),
    ) {
        (Some(binary), Some(test)) => Ok(Origin::Suite {
            binary: binary
                .to_str()
                .ok_or_else(|| io::Error::other("the actual nextest binary is not UTF-8"))?
                .to_owned(),
            test: test
                .to_str()
                .ok_or_else(|| io::Error::other("the actual nextest test is not UTF-8"))?
                .to_owned(),
        }),
        (None, None) => Ok(Origin::Product {
            program: "xtask",
            command: invocation.argv.clone(),
        }),
        (Some(_), None) | (None, Some(_)) => {
            Err(io::Error::other("the actual nextest origin is incomplete"))
        }
    }
}
