// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest plan`: what a run would do, without doing it.

use std::io::Write;

use crate::build::{BuildOptions, Cargo, Flavour, Selection, build};
use crate::cli::{Environment, Plan as Arguments};
use crate::targets::{Target, UnitKind, enumerate};
use crate::trace::Recorder;
use crate::watch::Watch;

use super::Completion;

/// Why a plan could not be constructed from the requested workspace.
#[derive(Debug, thiserror::Error)]
enum PlanError {
    /// The project configuration is invalid or unreadable.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    /// Cargo or its metadata boundary could not be used.
    #[error(transparent)]
    Cargo(#[from] rust_mutants::cargo::CargoError),
    /// The private planning workspace could not be created.
    #[error(transparent)]
    Scratch(#[from] crate::scratch::ScratchError),
    /// The generated run name did not satisfy the filesystem-component invariant.
    #[error(transparent)]
    RunId(#[from] rust_mutants::id::RunIdError),
    /// The selected workspace could not be built.
    #[error(transparent)]
    Build(#[from] crate::build::BuildError),
    /// A built target could not enumerate its tests.
    #[error(transparent)]
    Target(#[from] crate::targets::TargetError),
    /// A target identity could not be framed by the stable recipe.
    #[error(transparent)]
    TargetIdentity(#[from] crate::targets::TargetIdError),
    /// A requested package is not a workspace member.
    #[error(transparent)]
    Discovery(#[from] rust_mutants::discover::DiscoverError),
    /// Cargo completed normally but reported a compilation failure.
    #[error(
        "{}: the workspace does not compile, so there is nothing to plan:\n{failure}",
        crate::error::BUILD_FAILED.code
    )]
    Compilation { failure: String },
}

/// Says what a run would measure.
pub(super) fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<Completion> {
    match planned(arguments, environment) {
        Ok(lines) => {
            for line in lines {
                super::say(stdout, &line)?;
            }
            Ok(Completion::Assured)
        }
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            Ok(Completion::Error)
        }
    }
}

/// Writes a complete plan or returns the one reason no plan can be made.
fn planned(arguments: &Arguments, environment: &Environment) -> Result<Vec<String>, PlanError> {
    let root = environment.rooted(arguments.directory.as_deref());
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let watch = Watch::new(&cancel, &trace);
    let cargo = Cargo {
        offline: arguments.offline,
        locked: arguments.locked,
    };

    let selection = compiled(&root, arguments)?;
    let packages = selection.packages.clone();

    let (toolchain, metadata) = locate(&root, environment, cargo, &cancel)?;

    if let Some(refusal) = unknown_package(&packages, &metadata.packages) {
        return Err(refusal.into());
    }

    let scratch = workplace(environment)?;
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
    let built = build(&toolchain, &metadata.packages, &options, watch)?;
    if let Some(failure) = &built.failure {
        return Err(PlanError::Compilation {
            failure: failure.clone(),
        });
    }

    let selected = selected(&built.units, watch)?;
    let mut lines = Vec::new();
    if arguments.why {
        lines.push(format!("SCOPE\t{}", scope(arguments, &packages)));
    }
    for target in &selected {
        lines.push(line(target, arguments.why));
    }
    lines.push(format!("TARGETS\t{}", selected.len()));
    Ok(lines)
}

/// Every binary a run would measure, and how many tests each of them holds.
fn selected(units: &[crate::targets::Unit], watch: Watch<'_>) -> Result<Vec<Planned>, PlanError> {
    let mut selected = Vec::new();
    for unit in units {
        let held = enumerate(unit, watch)?;
        selected.push(Planned {
            target: crate::targets::whole_binary(unit)?,
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
fn workplace(environment: &Environment) -> Result<crate::scratch::Scratch, PlanError> {
    let now = jiff::Timestamp::now();
    crate::scratch::Scratch::create(
        &environment.temp_directory,
        &crate::run_id::mint(now, std::process::id())?,
        now,
    )
    .map_err(PlanError::from)
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
    rust_mutants::cargo::CargoError,
> {
    let toolchain = rust_mutants::cargo::Toolchain::locate(
        &rust_mutants::cargo::LocateOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: Some(environment.vars.clone()),
        },
        root,
        cancel,
    )?;
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
    )?;
    Ok((toolchain, metadata))
}

/// One target, and — when asked — what put it in scope.
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
/// # Errors
/// Returns the configuration's own refusal, rendered.
pub fn compiled(
    root: &std::path::Path,
    arguments: &Arguments,
) -> Result<Selection, crate::config::ConfigError> {
    let config = crate::config::Config::load(root)?;
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
fn unknown_package(
    named: &[String],
    members: &[rust_mutants::cargo::Package],
) -> Option<rust_mutants::discover::DiscoverError> {
    named
        .iter()
        .find(|name| !members.iter().any(|member| member.name == **name))
        .map(|name| rust_mutants::discover::DiscoverError::UnknownPackage { name: name.clone() })
}
