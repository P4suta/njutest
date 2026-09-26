// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest select`: say which test targets can notice what changed since the tree was measured, and which are proved unable to.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use rust_mutants::execute::TargetKind;
use rust_mutants::id::HexDigest;
use rust_mutants::select::{
    Changed, Decided, Difference, Everything, Measurement, Now, Selection, Unheld, Why, decide,
    differences,
};

use crate::assure::measure::{self, Measuring};
use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Select as Arguments, SelectFormat};
use crate::config::Config;
use crate::error::RunnerError;

/// Decides what a change can be noticed by and says it in the format asked for.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let config = match Config::load(root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let measuring = Measuring {
        root,
        environment,
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        config: &config,
    };
    match selected(&measuring) {
        Ok((selection, measured)) => {
            for line in told(&selection, &measured, arguments.format) {
                super::say(stdout, &line)?;
            }
            if arguments.format == SelectFormat::Nextest {
                for line in doctests(&selection, &measured) {
                    super::say(stderr, &line)?;
                }
            }
            Ok(EXIT_ASSURED)
        }
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            Ok(EXIT_ERROR)
        }
    }
}

/// What the measurement kept for this tree decides about the tree as it is now.
///
/// # Errors
/// No measurement to read, a toolchain or workspace that cannot be asked, and a selected variable that is not UTF-8.
pub fn selected(measuring: &Measuring<'_>) -> Result<(Selection, Measurement), RunnerError> {
    let (root, environment, config) = (measuring.root, measuring.environment, measuring.config);
    let directory = crate::reach::directory(&root.join(config.reports.directory.as_str()));
    let document = crate::reach::read(&directory)?;
    let measured = &document.measurement;
    let cancel = &environment.cancel;
    let (toolchain, metadata) = super::plan::locate(root, environment, measuring.cargo, cancel)
        .map_err(rust_mutants::EngineError::from)?;
    let members: Vec<&rust_mutants::cargo::Package> = metadata.members().collect();
    let now = rust_mutants::execute::declared_targets(&members);
    let survey =
        rust_mutants::workspace::Workspace::survey_at(root, &measure::opening(measuring), cancel)?;
    let vars: BTreeMap<String, String> = environment
        .vars
        .for_process()
        .filter_map(|(name, value)| Some((name.to_str()?.to_owned(), value.to_str()?.to_owned())))
        .collect();
    let found = differences(
        measured,
        &Now {
            toolchain: &toolchain.to_string(),
            survey: &survey,
            environment: &measure::environment(environment, config)?,
            settings: &measure::settings(measuring),
            vars: &vars,
        },
    );
    let selection = match found {
        Err(everything) => Selection::everything(&now, &everything),
        Ok(found) => {
            let read = revisions(&found, (root, &directory));
            let changes: Vec<Changed<'_>> = read.iter().map(Revised::changed).collect();
            decide(measured, &now, &changes)
        }
    };
    Ok((selection, document.measurement))
}

/// One file that differs, with its digests and its measured and current bytes where there are any.
enum Revised {
    /// In both trees.
    Edited {
        path: String,
        digests: (HexDigest, HexDigest),
        old: Option<String>,
        new: Option<String>,
    },
    /// In one of them.
    Whole { path: String },
}

impl Revised {
    fn changed(&self) -> Changed<'_> {
        match self {
            Self::Edited {
                path,
                digests,
                old,
                new,
            } => Changed::read(
                path,
                (&digests.0, &digests.1),
                old.as_deref(),
                new.as_deref(),
            ),
            Self::Whole { path } => Changed::Whole { path },
        }
    }
}

/// The measured and current bytes of every file that differs.
fn revisions(found: &[Difference], (root, directory): (&Path, &Path)) -> Vec<Revised> {
    found
        .iter()
        .map(|difference| match difference {
            Difference::Edited {
                path,
                measured,
                now,
            } => Revised::Edited {
                old: crate::reach::measured(directory, &measured.sha256),
                new: match std::fs::read_to_string(root.join(path)) {
                    Ok(text) => Some(text),
                    Err(_gone_or_not_text) => None,
                },
                path: path.clone(),
                digests: (measured.sha256.clone(), now.sha256.clone()),
            },
            Difference::Whole { path } => Revised::Whole { path: path.clone() },
        })
        .collect()
}

/// Every doc target that runs, and why, which a nextest filterset cannot say because nextest does not run documentation.
fn doctests(selection: &Selection, measured: &Measurement) -> Vec<String> {
    selection
        .decided()
        .iter()
        .filter(|(target, _)| binary_id(target).is_none())
        .filter_map(|(target, decided)| match decided {
            Decided::Run(why) => Some(format!("DOCTESTS\t{target}\t{}", because(why, measured))),
            Decided::Skip => None,
        })
        .collect()
}

/// The lines `selection` is said in.
#[must_use]
pub fn told(selection: &Selection, measured: &Measurement, format: SelectFormat) -> Vec<String> {
    match format {
        SelectFormat::Human => human(selection, measured),
        SelectFormat::Nextest => vec![nextest(selection)],
        SelectFormat::Skippable => selection.skippable().map(str::to_owned).collect(),
    }
}

fn human(selection: &Selection, measured: &Measurement) -> Vec<String> {
    let total = selection.decided().len();
    let skipped = selection.skippable().count();
    let mut lines = vec![format!(
        "SELECT\t{} of {total} targets run\t{skipped} proved unable to notice this change",
        total.saturating_sub(skipped)
    )];
    for (target, decided) in selection.decided() {
        lines.push(match decided {
            Decided::Skip => format!("SKIP\t{target}"),
            Decided::Run(why) => format!("RUN\t{target}\t{}", because(why, measured)),
        });
    }
    lines
}

/// Why a target runs, in the words a reader acts on.
fn because(why: &Why, measured: &Measurement) -> String {
    match why {
        Why::Entered(items) => {
            let named: Vec<String> = items
                .iter()
                .map(|index| {
                    measured
                        .items
                        .iter()
                        .find(|item| item.index == *index)
                        .map_or_else(
                            || format!("item {index}"),
                            |item| format!("{}:{}", item.path, item.name),
                        )
                })
                .collect();
            format!("its tests entered {}", named.join(", "))
        }
        Why::Everything(everything) => everything_because(everything),
        Why::Unestablished(unheld) => match unheld {
            Unheld::Moved => "a second run of it reached something else".to_owned(),
            Unheld::NotMeasured { why } => {
                format!("a second run of it established nothing to compare ({why:?})")
            }
            Unheld::Uncompared => "no second run of it was made".to_owned(),
            Unheld::ReadsTree => {
                "its tests read the tree's source files as data, which no entry records".to_owned()
            }
        },
        Why::Unmeasured => "the measurement holds nothing about it".to_owned(),
    }
}

fn everything_because(everything: &Everything) -> String {
    let said = match everything {
        Everything::Toolchain { measured, now } => {
            format!("the toolchain moved from {measured} to {now}")
        }
        Everything::Rules => "the tree is read by other rules than it was measured by".to_owned(),
        Everything::Setting { name } => format!("the {name} setting changed"),
        Everything::BuildScript { package } => {
            format!("the build script of {package} watches something that changed")
        }
        Everything::Environment { name } => format!("the selected variable {name} changed"),
        Everything::Compiled { name } => format!("the compiler read {name}, which changed"),
        Everything::Outside { path } => format!("{path}, read from outside the tree, changed"),
        Everything::Irregular { path } => format!("{path} is not a regular file and moved"),
        Everything::Build { path } => format!("{path} decides what every target compiles to"),
        Everything::CompileTime { path } => {
            format!(
                "{path} is compiled into code the build runs, a procedural macro or a build script"
            )
        }
        Everything::Unitemized { path } => format!("{path} holds no measured item"),
        Everything::Whole { path } => format!("{path} was added or removed"),
        Everything::Unproven { path } => {
            format!("the measured bytes of {path} were not kept or are not the measured ones")
        }
        Everything::Unparsed { path } => format!("{path} does not parse"),
        Everything::Skeleton { path, line } => {
            format!("{path}:{line} changed outside every measured body")
        }
        Everything::Escapes { item, by } => format!("{item} gained {by}, which reaches past it"),
        Everything::Unmeasurable { item } => format!("{item} has a body no guard records"),
        Everything::Located { path, line } => {
            format!("{path}:{line} moved under a macro that may say where it is")
        }
    };
    format!("everything runs: {said}")
}

/// A nextest filterset leaving out every target proved unable to notice the change; a doc target is not nextest's to leave out.
fn nextest(selection: &Selection) -> String {
    let left_out: BTreeSet<String> = selection
        .skippable()
        .filter_map(binary_id)
        .map(|id| format!("binary_id(={id})"))
        .collect();
    if left_out.is_empty() {
        return "all()".to_owned();
    }
    format!(
        "not ({})",
        left_out.into_iter().collect::<Vec<_>>().join(" | ")
    )
}

/// The nextest binary id of a target id, or nothing for a target nextest does not run.
fn binary_id(target: &str) -> Option<String> {
    let mut parts = target.splitn(3, '/');
    let (package, kind, name) = (parts.next()?, parts.next()?, parts.next()?);
    let kind = TargetKind::ALL.into_iter().find(|one| one.name() == kind)?;
    match kind {
        TargetKind::Lib | TargetKind::ProcMacro => Some(package.to_owned()),
        TargetKind::Bin => Some(format!("{package}::bin/{name}")),
        TargetKind::Test => Some(format!("{package}::{name}")),
        TargetKind::Example => Some(format!("{package}::example/{name}")),
        TargetKind::Doc => None,
    }
}
