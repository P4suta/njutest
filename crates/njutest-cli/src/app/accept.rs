// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest accept`: record that a reviewer looked at a surviving mutant.

use std::io::Write;

use crate::app::runs;
use crate::cli::{Accept as Arguments, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::config;

/// Appends an acceptance to the configuration, keeping its comments.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let identity = match resolve(root, arguments) {
        Ok(identity) => identity,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };

    let path = root.join(config::FILE_NAME);
    let existing =
        std::fs::read_to_string(&path).unwrap_or_else(|_error| String::from("version = 1\n"));
    let mut document = match existing.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: {}: {error}",
                    crate::error::CONFIG_UNPARSABLE.code,
                    path.display()
                ),
            );
            return EXIT_ERROR;
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
        );
        return EXIT_ERROR;
    };
    if array
        .iter()
        .any(|table| table.get("id").and_then(toml_edit::Item::as_str) == Some(identity.as_str()))
    {
        super::say(stdout, &format!("{identity} was already accepted"));
        return EXIT_ASSURED;
    }

    let mut table = toml_edit::Table::new();
    table.insert("id", toml_edit::value(identity.clone()));
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
    array.push(table);

    if let Err(error) = std::fs::write(&path, document.to_string()) {
        super::diagnose(
            stderr,
            &format!(
                "{}: writing {}: {error}",
                crate::error::CONFIG_UNREADABLE.code,
                path.display()
            ),
        );
        return EXIT_ERROR;
    }
    super::say(
        stdout,
        &format!("accepted {identity} in {}", config::FILE_NAME),
    );
    EXIT_ASSURED
}

/// The full identity a prefix names, from the run that measured it.
fn resolve(root: &std::path::Path, arguments: &Arguments) -> Result<String, String> {
    let run = runs::resolve(root, arguments.run.as_deref()).map_err(|error| error.to_string())?;
    let report = runs::report(root, &run).map_err(|error| error.to_string())?;
    let matching: Vec<&crate::report::MutantRecord> = report
        .mutants
        .iter()
        .filter(|mutant| {
            mutant.id.starts_with(&arguments.mutant)
                || mutant.display_id.starts_with(&arguments.mutant)
        })
        .collect();
    match matching.as_slice() {
        [only] if matches!(only.outcome.as_str(), "survived" | "unreached") => Ok(only.id.clone()),
        [only] => Err(format!(
            "{}: {} is {}, and only a mutation nothing noticed is a decision to accept",
            crate::error::CONFIG_INVALID.code,
            only.display_id,
            only.outcome
        )),
        [] => Err(format!(
            "{}: no mutant of {run} starts with {}",
            crate::error::RUN_NOT_FOUND.code,
            arguments.mutant
        )),
        several => Err(format!(
            "{}: {} names {} mutants: {}",
            crate::error::RUN_NOT_FOUND.code,
            arguments.mutant,
            several.len(),
            several
                .iter()
                .map(|one| format!("{} at {}:{}", one.display_id, one.path, one.position.line))
                .collect::<Vec<String>>()
                .join(", ")
        )),
    }
}
