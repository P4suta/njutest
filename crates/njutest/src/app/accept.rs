// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest accept`: record that a reviewer looked at a surviving mutant.

use std::io::Write;
use std::str::FromStr;

use crate::app::runs;
use crate::cli::{Accept as Arguments, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::config;

/// Why the acceptance ledger cannot be opened for a checked edit.
#[derive(Debug, thiserror::Error)]
enum LedgerError {
    /// The existing configuration could not be read.
    #[error(
        "{}: reading {}: {source}",
        crate::error::CONFIG_UNREADABLE.code,
        path.display()
    )]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The existing configuration is not TOML.
    #[error(
        "{}: {}: {source}",
        crate::error::CONFIG_UNPARSABLE.code,
        path.display()
    )]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
}

/// Why the requested acceptance does not resolve to one acceptable mutation.
#[derive(Debug, thiserror::Error)]
enum ResolveError {
    /// The stored run or its report could not be read.
    #[error(transparent)]
    Run(#[from] runs::RunError),
    /// The completed report could not reproduce its exact projection.
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
    /// The prefix names one mutation, but its outcome is not accept-able.
    #[error(
        "{}: {mutant} is {outcome}, and only a mutation nothing noticed is a decision to accept",
        crate::error::CONFIG_INVALID.code
    )]
    Outcome { mutant: String, outcome: String },
    /// No mutation has the requested prefix.
    #[error(
        "{}: no mutant of {run} starts with {prefix}",
        crate::error::RUN_NOT_FOUND.code
    )]
    Missing { run: String, prefix: String },
    /// More than one mutation has the requested prefix.
    #[error(
        "{}: {prefix} names {count} mutants: {matches}",
        crate::error::RUN_NOT_FOUND.code
    )]
    Ambiguous {
        prefix: String,
        count: usize,
        matches: String,
    },
}

/// Appends an acceptance to the configuration, keeping its comments.
/// The locator an acceptance is written as, so it holds through the next edit to the file.
struct Written {
    path: String,
    item: String,
    rule: String,
    original: String,
    line: u32,
}

/// Whether this table is the same acceptance, however it was written.
///
/// One written as a locator and one written as an identity name the same mutation, and a second line about it is a second reason the run would have to choose between.
fn already(table: &toml_edit::Table, identity: &str, locator: Option<&Written>) -> bool {
    let said = |key: &str| {
        table
            .get(key)
            .and_then(toml_edit::Item::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if said("id") == identity {
        return true;
    }
    locator.is_some_and(|one| {
        said("path") == one.path && said("item") == one.item && said("rule") == one.rule
    })
}

/// The table an acceptance is written as: a locator where there is one, an identity otherwise.
fn written(arguments: &Arguments, locator: Option<&Written>, identity: &str) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    if let Some(one) = locator {
        table.insert("path", toml_edit::value(one.path.clone()));
        table.insert("item", toml_edit::value(one.item.clone()));
        table.insert("rule", toml_edit::value(one.rule.clone()));
        table.insert("original", toml_edit::value(one.original.clone()));
        table.insert("line", toml_edit::value(i64::from(one.line)));
    } else {
        table.insert("id", toml_edit::value(identity.to_owned()));
    }
    table.insert("reason", toml_edit::value(arguments.reason.clone()));
    if let Some(owner) = &arguments.owner {
        table.insert("owner", toml_edit::value(owner.clone()));
    }
    if let Some(ticket) = &arguments.ticket {
        table.insert("ticket", toml_edit::value(ticket.clone()));
    }
    if let Some(expires) = &arguments.expires {
        table.insert("expires", toml_edit::value(expires.to_string()));
    }
    table
}

/// Records one surviving mutation as accepted, with the reason a reader gave.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let found = match resolve(root, arguments) {
        Ok(found) => found,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let identity = found.id().to_owned();
    let locator = (!found.item().is_empty() && !found.path().is_empty()).then(|| Written {
        path: found.path().to_owned(),
        item: found.item().to_owned(),
        rule: found.rule().to_owned(),
        original: found.original().to_owned(),
        line: found.position().line,
    });

    let path = root.join(config::FILE_NAME);
    let mut document = match ledger(&path) {
        Ok(document) => document,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };

    let acceptances = document
        .entry("acceptance")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    let Some(array) = acceptances.as_array_of_tables_mut() else {
        super::diagnose(
            stderr,
            &format!(
                "{}: the configuration's acceptance is not a list of tables",
                crate::error::CONFIG_INVALID.code
            ),
        )?;
        return Ok(EXIT_ERROR);
    };
    if array
        .iter()
        .any(|table| already(table, &identity, locator.as_ref()))
    {
        super::say(stdout, &format!("{identity} was already accepted"))?;
        return Ok(EXIT_ASSURED);
    }

    array.push(written(arguments, locator.as_ref(), &identity));

    if let Err(error) = std::fs::write(&path, document.to_string()) {
        super::diagnose(
            stderr,
            &format!(
                "{}: writing {}: {error}",
                crate::error::CONFIG_UNREADABLE.code,
                path.display()
            ),
        )?;
        return Ok(EXIT_ERROR);
    }
    super::say(
        stdout,
        &format!("accepted {identity} in {}", config::FILE_NAME),
    )?;
    Ok(EXIT_ASSURED)
}

fn ledger(path: &std::path::Path) -> Result<toml_edit::DocumentMut, LedgerError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::from("version = 1\n"),
        Err(source) => {
            return Err(LedgerError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    toml_edit::DocumentMut::from_str(&existing).map_err(|source| LedgerError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// The full identity a prefix names, from the run that measured it.
fn resolve(
    root: &std::path::Path,
    arguments: &Arguments,
) -> Result<crate::report::ProjectedMutant, ResolveError> {
    let run = runs::resolve(root, arguments.run.as_deref())?;
    let report = runs::report(&run)?;
    let conclusion = report.conclusion()?;
    let every: Vec<&crate::report::ProjectedMutant> = conclusion.mutants.iter().collect();
    let matching = crate::naming::matching(&every, &arguments.mutant);
    match matching.as_slice() {
        [only]
            if matches!(
                only.decision(),
                crate::report::Decision::Unnoticed | crate::report::Decision::Unreached
            ) =>
        {
            Ok((*only).clone())
        }
        [only] => Err(ResolveError::Outcome {
            mutant: only.display_id().to_owned(),
            outcome: only.decision().name().to_owned(),
        }),
        [] => Err(ResolveError::Missing {
            run: run.id().to_string(),
            prefix: arguments.mutant.clone(),
        }),
        several => Err(ResolveError::Ambiguous {
            prefix: arguments.mutant.clone(),
            count: several.len(),
            matches: several
                .iter()
                .map(|one| {
                    format!(
                        "{} at {}:{}",
                        one.display_id(),
                        one.path(),
                        one.position().line
                    )
                })
                .collect::<Vec<String>>()
                .join(", "),
        }),
    }
}
