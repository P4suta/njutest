// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest plan`: what a run would do, without doing it.

use std::io::Write;

use crate::build::{BuildOptions, Cargo, Flavour, Selection, build};
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Plan as Arguments};
use crate::targets::{Target, UnitKind, enumerate};
use crate::trace::Recorder;
use crate::watch::Watch;

/// Says what a run would measure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = environment.rooted(arguments.directory.as_deref());
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let watch = Watch::new(&cancel, &trace);
    let cargo = Cargo {
        offline: arguments.offline,
        locked: arguments.locked,
    };

    let selection = match compiled(&root, arguments) {
        Ok(selection) => selection,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };
    let packages = selection.packages.clone();

    let located = locate(&root, environment, cargo, &cancel);
    let (toolchain, metadata) = match located {
        Ok(pair) => pair,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };

    if let Some(refusal) = unknown_package(&packages, &metadata.packages) {
        super::diagnose(stderr, &refusal);
        return EXIT_ERROR;
    }

    let scratch = match workplace(environment) {
        Ok(scratch) => scratch,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let options = BuildOptions {
        root,
        selection,
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
        super::say(stdout, &format!("SCOPE\t{}", scope(arguments, &packages)));
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

/// Every binary a run would measure, and how many tests each of them holds.
///
/// A run measures a binary and a route names which of its tests a mutation is
/// put to, so a plan that listed tests would name a thing a run never reports.
/// What a reader wants from the count is still there, one column along.
fn selected(
    units: &[crate::targets::Unit],
    watch: Watch<'_>,
) -> Result<Vec<Planned>, crate::targets::TargetError> {
    let mut selected = Vec::new();
    for unit in units {
        let held = enumerate(unit, watch)?;
        selected.push(Planned {
            target: crate::targets::whole_binary(unit),
            tests: held.iter().filter(|one| !one.ignored).count(),
            ignored: held.iter().filter(|one| one.ignored).count(),
        });
    }
    Ok(selected)
}

/// One binary a run would measure, and what it holds.
#[derive(Debug, Clone)]
pub struct Planned {
    /// The binary, as a report names it.
    pub target: Target,
    /// How many tests it holds that a run would start.
    pub tests: usize,
    /// How many of them are `#[ignore]`d, which a run does not start and a plan still counts.
    pub ignored: usize,
}

/// Where a plan builds.
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
///
/// The three reasons are the three kinds of thing a run measures, and each of
/// them is a different thing for a reader to do: a library's examples are a
/// target a route cannot narrow, a binary that answers by exiting is one a
/// route cannot narrow either but for another reason, and everything else is a
/// count they can compare with what the suite says it has.
#[must_use]
pub fn line(planned: &Planned, why: bool) -> String {
    let head = format!("TARGET\t{}\t{}", planned.target.id, planned.target.name());
    if !why {
        return head;
    }
    let reason = if planned.target.unit == UnitKind::Doc {
        "a library's documented examples, which cargo runs and rustdoc compiles".to_owned()
    } else if planned.tests == 0 && planned.ignored == 0 {
        "the binary answers by exiting, so it is measured whole".to_owned()
    } else {
        format!(
            "{} tests, {} of them ignored; a route names which of them a mutation is put to",
            planned.tests.saturating_add(planned.ignored),
            planned.ignored
        )
    };
    format!("{head}\t{reason}")
}

/// What a run of this tree would compile: the packages the reader or the configuration named, and the features the configuration turns on.
///
/// A plan says what a run would measure, so it answers to the same
/// configuration the run answers to. One that read none of it would describe a
/// run nobody asked for, and be believed, because describing is all it does.
///
/// # Errors
/// Returns the configuration's own refusal, rendered.
fn compiled(root: &std::path::Path, arguments: &Arguments) -> Result<Selection, String> {
    let config = crate::config::Config::load(root).map_err(|error| error.to_string())?;
    Ok(Selection {
        packages: if arguments.packages.is_empty() {
            config.project.packages
        } else {
            arguments.packages.clone()
        },
        features: config.execution.features,
        all_features: config.execution.all_features,
        default_features: !config.execution.no_default_features,
    })
}

/// What put the targets in scope, in the words the reader would recognise.
fn scope(arguments: &Arguments, packages: &[String]) -> String {
    if packages.is_empty() {
        return "every workspace member".to_owned();
    }
    let from = if arguments.packages.is_empty() {
        "the packages the configuration names"
    } else {
        "the packages asked for"
    };
    format!("{from}: {}", packages.join(", "))
}

/// The first package named that is no member of the workspace, if one is.
///
/// A verification refuses this and says so; a plan used to answer `TARGETS 0`
/// and exit zero, which reads as a package with nothing to run in it. A plan is
/// what a person asks before a run to find out what will happen, so the two
/// must give the same answer to the same mistake.
fn unknown_package(named: &[String], members: &[rust_mutants::cargo::Package]) -> Option<String> {
    named
        .iter()
        .find(|name| !members.iter().any(|member| member.name == **name))
        .map(|name| {
            rust_mutants::discover::DiscoverError::UnknownPackage { name: name.clone() }.to_string()
        })
}
