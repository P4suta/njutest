// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one run established about one mutant, from what the run stored rather than from a tree it prepares again.

use serde::{Deserialize, Serialize};

use super::catalog::{CatalogDocument, MutantDocument};
use super::run::{RouteDocument, RunDocument, RunMutantDocument};

/// The document type an explanation carries.
pub const DOCUMENT_TYPE: &str = "rust-mutants/explain";

/// Everything known about one mutant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainDocument {
    /// Names the shape, so a reader can tell versions apart.
    pub document_type: String,
    /// The version of that shape.
    pub schema_version: u32,
    /// The engine that produced it.
    pub tool_version: String,
    /// The mutant, as the catalog holds it.
    pub mutant: MutantDocument,
    /// The run this is about, when a stored run answered for the mutant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// What the compiler refused it with, when it refused it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// What the tests made of it, when a run executed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// The target that ran it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Every test that failed with it active, which is what noticed it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub killed_by: Vec<String>,
    /// The mutation as a change to the file, when the file is the one it was taken from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Why there is no diff, when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Which targets could have noticed it, and which of them ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteDocument>,
    /// How long every execution of it took together, when a run executed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retried: bool,
    /// The command that puts this one mutation back to the tests.
    pub reproduce: String,
    /// The configuration block that records this mutation as one a reason is written for.
    ///
    /// A survivor has two readings a reader has to tell apart: a gap in the
    /// tests, and a claim about the code that somebody should write down.
    /// Nothing said so here. One caller read ninety survivors, decided five of
    /// them were the second kind, went looking for how to record them, and
    /// found the form only after reading the configuration page — having
    /// already known the feature existed. A block they can paste costs a
    /// reader nothing and says the choice is theirs to make.
    pub accept: String,
}

/// Why one mutant could not be explained.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ExplainError {
    /// No mutant of this catalog answers to the prefix.
    #[error("no mutant of this catalog answers to {prefix:?}")]
    Nothing {
        /// What was asked for.
        prefix: String,
    },
    /// More than one does.
    #[error("{} mutants of this catalog answer to {prefix:?}: {}", matches.len(), matches.join(", "))]
    Several {
        /// What was asked for.
        prefix: String,
        /// What it could have meant.
        matches: Vec<String>,
    },
}

/// What an explanation is built from.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    /// The catalog the run stored.
    pub catalog: &'a CatalogDocument,
    /// The run's own report, when there is one to read.
    pub run: Option<&'a RunDocument>,
    /// The identity, or a prefix that names exactly one mutant.
    pub prefix: &'a str,
    /// The file as it is now, when the tree is at hand.
    pub source: Option<&'a str>,
}

/// Everything known about the one mutant `asked` names.
///
/// It is built from what a run stored, so an explanation costs nothing but
/// reading two documents: no snapshot, no build, no instrumented tree. What it
/// cannot say without the tree is what the mutation looks like as a change,
/// and it says so rather than guessing.
///
/// # Errors
/// [`ExplainError::Nothing`] and [`ExplainError::Several`] for a prefix that
/// does not name exactly one mutant.
pub fn explain(asked: &Asked<'_>) -> Result<ExplainDocument, ExplainError> {
    let matching: Vec<&MutantDocument> = asked
        .catalog
        .mutants
        .iter()
        .filter(|one| one.id.starts_with(asked.prefix))
        .collect();
    let mutant = match matching.as_slice() {
        [] => {
            return Err(ExplainError::Nothing {
                prefix: asked.prefix.to_owned(),
            });
        }
        [one] => (*one).clone(),
        several => {
            return Err(ExplainError::Several {
                prefix: asked.prefix.to_owned(),
                matches: several.iter().map(|one| one.display_id.clone()).collect(),
            });
        }
    };
    let row: Option<&RunMutantDocument> = asked
        .run
        .and_then(|run| run.mutants.iter().find(|one| one.id == mutant.id));
    let refused = asked.catalog.rejections.iter().find_map(|one| {
        (one.rule == mutant.rule && one.path == mutant.path).then(|| one.diagnostic.clone())
    });
    let (diff, source) = changed(&mutant, asked.source);
    Ok(ExplainDocument {
        document_type: DOCUMENT_TYPE.to_owned(),
        schema_version: 1,
        tool_version: crate::VERSION.to_owned(),
        run_id: asked.run.map(|run| run.run.id.clone()),
        refused,
        outcome: row.map(|one| one.outcome.clone()),
        target: row
            .map(|one| one.target.clone())
            .filter(|target| !target.is_empty()),
        killed_by: row.map(|one| one.killed_by.clone()).unwrap_or_default(),
        diff,
        source,
        route: row.and_then(|one| one.route.clone()),
        duration_ms: row.map(|one| one.duration_ms),
        retried: row.is_some_and(|one| one.retried),
        reproduce: reproduce(&mutant, row),
        accept: accept(&mutant),
        mutant,
    })
}

/// The mutation as a change to the file, or why there is none to show.
fn changed(mutant: &MutantDocument, source: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(source) = source else {
        return (None, Some("the tree was not read".to_owned()));
    };
    if crate::id::digest(source.as_bytes()) != mutant.source_digest {
        return (None, Some("the file has changed since the run".to_owned()));
    }
    let Some(after) = super::diff::mutated(
        source,
        mutant.start_byte,
        mutant.end_byte,
        &mutant.replacement,
    ) else {
        return (
            None,
            Some("the edit is not a range of this file".to_owned()),
        );
    };
    (
        Some(super::diff::unified(&mutant.path, source, &after)),
        None,
    )
}

/// The command that puts one mutation back to the tests.
/// How a reader names this mutation again, which has to hold after they have changed the file.
///
/// The identity is a function of the file's bytes, so the edit that fixes a
/// survivor re-mints it — and in Rust that edit is usually a test added to the
/// `#[cfg(test)] mod tests` at the bottom of the same file. A command printed
/// with an identity in it stops working the moment it is followed. A locator
/// says where the mutation is and what it edits, so it holds.
#[must_use]
pub fn names(mutant: &MutantDocument) -> String {
    if mutant.item.is_empty() || mutant.path.is_empty() {
        return mutant.display_id.clone();
    }
    format!(
        "{}:{}:{}@{}",
        mutant.path, mutant.item, mutant.rule, mutant.line
    )
}

/// The `[[mutation.expect]]` a reader pastes to record this mutation with a reason.
///
/// It is written as a locator rather than as an identity for the same reason
/// the reproduce command is: the identity is re-minted by any edit to the
/// file, so a recorded one stops naming anything as soon as somebody touches
/// the file it is about.
fn accept(mutant: &MutantDocument) -> String {
    if mutant.item.is_empty() || mutant.path.is_empty() {
        return format!(
            "[[mutation.expect]]\nid = {:?}\nreason = \"\"  # why this is not a gap in the tests\n",
            mutant.display_id
        );
    }
    format!(
        "[[mutation.expect]]\npath = {:?}\nitem = {:?}\nrule = {:?}\noriginal = {:?}\nline = {}\nreason = \"\"  # why this is not a gap in the tests\n",
        mutant.path, mutant.item, mutant.rule, mutant.original, mutant.line
    )
}

fn reproduce(mutant: &MutantDocument, row: Option<&RunMutantDocument>) -> String {
    let mut text = format!("rust-mutants run --mutant {}", names(mutant));
    if let Some(one) = row {
        if !one.target.is_empty() {
            text.push_str(" --target ");
            text.push_str(&one.target);
        }
        if let Some(test) = one.killed_by.first() {
            text.push_str(" --test ");
            text.push_str(test);
        }
    }
    text
}
