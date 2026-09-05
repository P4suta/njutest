// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The infection log: which mutants one test process reached with a state the mutation would have changed.
//!
//! The runtime appends to it and never reads it back, so the file may hold one
//! process's records or several, and a process killed part-way may leave a line
//! that was never finished. The reader is fail-closed: a truncated line, a
//! header that names another catalog, or an index no mutant answers to yields
//! no facts at all. A smaller wrong answer is exactly what a partially-read log
//! looks like, and a run that acted on one would discharge a test that could
//! have killed something.

use std::collections::BTreeSet;

/// The first token of a log's header line, which carries the recipe version.
pub const SCHEMA: &str = "rust-mutants-infection-v1";

/// Why a log said nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LogError {
    /// A line is not a header and not an index.
    #[error("line {line}: {what}")]
    Malformed {
        /// The 1-based line.
        line: usize,
        /// What is wrong with it.
        what: String,
    },
    /// A header names a catalog this run is not about.
    #[error("line {line}: the log is about catalog {found}, and this run is about {expected}")]
    OtherCatalog {
        /// The 1-based line.
        line: usize,
        /// What the log says.
        found: String,
        /// What was asked for.
        expected: String,
    },
    /// An index no mutant of the catalog answers to.
    #[error("line {line}: mutant {index} is beyond the {count} the catalog holds")]
    BeyondCatalog {
        /// The 1-based line.
        line: usize,
        /// The index the log named.
        index: u32,
        /// How many mutants there are.
        count: u32,
    },
    /// An index appeared before any header did.
    #[error("line {line}: an index before any header, so nothing says which catalog it is about")]
    Headless {
        /// The 1-based line.
        line: usize,
    },
}

/// Every mutant the log says was infected, ascending.
///
/// `catalog` is the digest this run is about and `count` is how many mutants it
/// holds; both are what makes a stale log say nothing rather than something
/// wrong.
///
/// # Errors
/// See [`LogError`]. Every failure yields no facts at all, never the prefix
/// that parsed.
pub fn read(text: &str, catalog: &str, count: u32) -> Result<BTreeSet<u32>, LogError> {
    let mut infected = BTreeSet::new();
    let mut seen_header = false;
    for (position, line) in text.lines().enumerate() {
        let line_number = position.saturating_add(1);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(SCHEMA) {
            seen_header = true;
            header(rest, catalog, count, line_number)?;
            continue;
        }
        if !seen_header {
            return Err(LogError::Headless { line: line_number });
        }
        let index: u32 = line.parse().map_err(|_error| LogError::Malformed {
            line: line_number,
            what: format!("{line:?} is not a mutant index"),
        })?;
        if index >= count {
            return Err(LogError::BeyondCatalog {
                line: line_number,
                index,
                count,
            });
        }
        infected.insert(index);
    }
    Ok(infected)
}

/// The rest of a header line: the catalog digest it is about and how many mutants that catalog held.
fn header(rest: &str, catalog: &str, count: u32, line: usize) -> Result<(), LogError> {
    let mut fields = rest.split_whitespace();
    let (Some(found), Some(held), None) = (fields.next(), fields.next(), fields.next()) else {
        return Err(LogError::Malformed {
            line,
            what: "a header names one catalog and one count".to_owned(),
        });
    };
    if found != catalog {
        return Err(LogError::OtherCatalog {
            line,
            found: found.to_owned(),
            expected: catalog.to_owned(),
        });
    }
    let held: u32 = held.parse().map_err(|_error| LogError::Malformed {
        line,
        what: format!("{held:?} is not a count of mutants"),
    })?;
    if held != count {
        return Err(LogError::Malformed {
            line,
            what: format!("the log was written against {held} mutants and this run has {count}"),
        });
    }
    Ok(())
}

/// The header one process writes before its first index.
#[must_use]
pub fn header_line(catalog: &str, count: u32) -> String {
    format!("{SCHEMA} {catalog} {count}\n")
}
