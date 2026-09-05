// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest plan`: what a run would do, without doing it.
//!
//! It builds — there is no way to know what tests exist without asking the
//! binaries that hold them — but it runs none of them and writes no report.
//! The build is the plain one rather than the instrumented one, because a
//! plan does not measure coverage and a plan that warmed the wrong layer
//! would make the run after it slower rather than faster.
//!
//! `--why` is the part that earns the command. A reader who disagrees with
//! what a run measured needs to see the reason, not the result.

use std::io::Write;

use crate::build::{BuildOptions, Cargo, Flavour, Selection, build};
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Plan as Arguments};
use crate::targets::enumerate;
use crate::trace::Recorder;
use crate::watch::Watch;

/// Says what a run would measure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = arguments
        .directory
        .clone()
        .unwrap_or_else(|| environment.working_directory.clone());
    let cancel = rust_mutants::runner::Cancel::new();
    let trace = Recorder::disabled();
    let watch = Watch::new(&cancel, &trace);
    let cargo = Cargo {
        offline: arguments.offline,
        locked: arguments.locked,
    };

    let located = locate(&root, environment, cargo, &cancel);
    let (toolchain, metadata) = match located {
        Ok(pair) => pair,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };

    let scratch = match workplace(environment) {
        Ok(scratch) => scratch,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let options = BuildOptions {
        root,
        selection: Selection {
            packages: arguments.packages.clone(),
            ..Selection::default()
        },
        flavour: Flavour::Native,
        target_dir: scratch.dir().join("layer"),
        scratch_build_dir: scratch.build_dir(),
        env: environment.vars.clone(),
        cargo,
        timeout: None,
    };
    let built = match build(&toolchain, &metadata.packages, &options, watch) {
        Ok(built) if built.failure.is_none() => built,
        Ok(built) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: the workspace does not compile, so there is nothing to plan:\n{}",
                    crate::error::BUILD_FAILED.code,
                    built.failure.unwrap_or_default()
                ),
            );
            return EXIT_ERROR;
        }
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };

    if arguments.why {
        let scope = if arguments.packages.is_empty() {
            "every workspace member".to_owned()
        } else {
            format!("the packages asked for: {}", arguments.packages.join(", "))
        };
        super::say(stdout, &format!("SCOPE\t{scope}"));
    }

    let selected = match selected(&built.units, watch) {
        Ok(selected) => selected,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    for target in &selected {
        super::say(stdout, &line(target, arguments.why));
    }
    super::say(stdout, &format!("TARGETS\t{}", selected.len()));
    drop(scratch.close());
    EXIT_ASSURED
}

/// Every test the built binaries hold.
fn selected(
    units: &[crate::targets::Unit],
    watch: Watch<'_>,
) -> Result<Vec<crate::targets::Target>, crate::targets::TargetError> {
    let mut selected = Vec::new();
    for unit in units {
        selected.extend(enumerate(unit, watch)?);
    }
    Ok(selected)
}

/// Where a plan builds.
///
/// A plan builds, so it needs a directory, and a directory this program
/// makes has an owner like every other (ADR 0006). It is removed when the
/// plan ends, which is what makes a plan cheap to run twice.
fn workplace(
    environment: &Environment,
) -> Result<crate::scratch::Scratch, crate::scratch::ScratchError> {
    let now = jiff::Timestamp::now();
    crate::scratch::Scratch::create(
        &environment.temp_directory,
        &crate::run_id::mint(now, std::process::id()),
        now,
    )
}

/// The toolchain and what it says the workspace holds.
fn locate(
    root: &std::path::Path,
    environment: &Environment,
    cargo: Cargo,
    cancel: &rust_mutants::runner::Cancel,
) -> Result<
    (
        rust_mutants::cargo::Toolchain,
        rust_mutants::cargo::Metadata,
    ),
    String,
> {
    let toolchain = rust_mutants::cargo::Toolchain::locate(
        &rust_mutants::cargo::LocateOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: Some(environment.vars.clone()),
        },
        root,
        cancel,
    )
    .map_err(|error| error.to_string())?;
    let metadata = rust_mutants::cargo::Metadata::load(
        &rust_mutants::cargo::Driver {
            toolchain: &toolchain,
            dir: root,
            cancel,
            trace: &rust_mutants::trace::Recorder::disabled(),
        },
        rust_mutants::cargo::MetadataOptions {
            locked: cargo.locked,
            offline: cargo.offline,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok((toolchain, metadata))
}

/// One target, and — when asked — what put it in scope.
fn line(target: &crate::targets::Target, why: bool) -> String {
    let head = format!("TARGET\t{}\t{}", target.id, target.name());
    if !why {
        return head;
    }
    let reason = if target.ignored {
        "libtest will not run it unless asked: ignored"
    } else if target.is_whole_binary() {
        "one target per binary: it has a harness of its own"
    } else {
        "one target per test, so what each one reaches can be told apart"
    };
    format!("{head}\t{reason}")
}
