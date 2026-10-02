// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run held to the ledger of survivors somebody accepted.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use super::{Audit, Decided, Layer, MET, Notes, Report, SURVIVING_MUTANT, UNREACHED_MUTANT};
use crate::layers::Closed;

/// The ledger layer's subject, closed from the report's own claim list: a run that claims no acceptance — whose `expectations` the report itself carries complete and empty — has no acceptance a ledger could answer or go stale over, so no ledger is owed one; a run that claims any is owed the ledger its claims answer to.
const fn accepted_scope(report: &Report) -> Closed {
    if report.expectations.is_empty() {
        return Closed::NothingOwed("the run claims no acceptance, so no ledger is owed an answer");
    }
    Closed::Missing
}

pub(super) fn ledger(report: &Report, ledger: Option<&Ledger>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Ledger);
    let Some(ledger) = ledger else {
        match accepted_scope(report) {
            Closed::NothingOwed(why) => return notes.absent(why),
            Closed::Missing => {
                notes.unaudited(
                    "ledger",
                    "no ledger was given, so whether every survivor is one somebody accepted \
                     cannot be re-decided"
                        .to_owned(),
                );
                return notes.looked();
            }
        }
    };
    for finding in &report.findings {
        if finding.kind == SURVIVING_MUTANT || finding.kind == UNREACHED_MUTANT {
            let Some(subject) = finding.mutant.as_deref() else {
                notes.violated(
                    finding.kind.as_str(),
                    "a survivor finding names no mutant, so no acceptance can answer it".to_owned(),
                );
                continue;
            };
            notes.violated(
                subject,
                "no test noticed this mutation and the ledger does not accept it; a survivor \
                 is either killed or accepted with a reason"
                    .to_owned(),
            );
        }
    }
    let standing: BTreeSet<&Named> = report
        .expectations
        .iter()
        .map(|claim| &claim.named)
        .collect();
    for entry in &ledger.accepted {
        if !standing.contains(entry) {
            notes.violated(
                &entry.to_string(),
                "the ledger accepts this mutant and the run does not hold it; an acceptance \
                 nothing answers is one nobody will notice going stale"
                    .to_owned(),
            );
        }
    }
    for claim in report.expectations.iter().filter(|it| it.standing == MET) {
        if !ledger.accepted.contains(&claim.named) {
            notes.violated(
                &claim.id,
                "the run met a claim the ledger does not carry; an acceptance a reviewer \
                 cannot find is not one they made"
                    .to_owned(),
            );
        }
    }
    notes.looked()
}

/// Every mutant a ledger accepts, each named in the form its entry was written in.
#[derive(Debug)]
pub(super) struct Ledger {
    accepted: BTreeSet<Named>,
}

impl Ledger {
    /// The acceptances `document` holds under `[[mutation.expect]]`, every one of them or an error.
    pub(super) fn read(document: &toml::Table) -> Result<Self, toml::de::Error> {
        let expect = match document.get("mutation") {
            None => None,
            Some(toml::Value::Table(mutation)) => mutation.get("expect"),
            Some(other) => {
                return Err(serde::de::Error::custom(format!(
                    "`mutation` is {} where the engine reads a table",
                    other.type_str()
                )));
            }
        };
        let entries: Vec<Entry> = match expect {
            None => Vec::new(),
            Some(expect) => expect.clone().try_into()?,
        };
        let mut accepted = BTreeSet::new();
        for entry in entries {
            let named = entry.named()?;
            if accepted.contains(&named) {
                return Err(serde::de::Error::custom(format!(
                    "two acceptances name {named}, which the engine refuses: a mutant has one \
                     reason"
                )));
            }
            accepted.insert(named);
        }
        Ok(Self { accepted })
    }
}

/// A mutant as a claim names it, which is the form the engine reads it in and the form a report writes it back in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Named {
    /// An identity, or a prefix of one.
    Identity(String),
    /// Where the mutation is and what it edits, with the line and the count when the claim states them.
    Place {
        path: String,
        item: String,
        rule: String,
        original: String,
        line: Option<u64>,
        count: Option<u64>,
    },
}

impl fmt::Display for Named {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(id) => f.write_str(id),
            Self::Place {
                path,
                item,
                rule,
                original,
                line: Some(line),
                count: _,
            } => write!(f, "{path} {item} {rule} {original:?} @{line}"),
            Self::Place {
                path,
                item,
                rule,
                original,
                line: None,
                count: _,
            } => write!(f, "{path} {item} {rule} {original:?}"),
        }
    }
}

/// One `[[mutation.expect]]` entry in every field the engine's own `Expect` reads, so an entry holding a field this audit does not know refuses the ledger rather than dropping out of it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: Option<String>,
    path: Option<String>,
    item: Option<String>,
    rule: Option<String>,
    original: Option<String>,
    line: Option<u64>,
    count: Option<u64>,
    #[serde(rename = "reason")]
    _reason: String,
    #[serde(rename = "outcome")]
    _outcome: Option<String>,
    #[serde(rename = "where")]
    _holds: Option<toml::Table>,
}

impl Entry {
    /// The claim this entry makes, in the one form of the two the engine reads that it is written in.
    fn named(self) -> Result<Named, toml::de::Error> {
        let Self {
            id,
            path,
            item,
            rule,
            original,
            line,
            count,
            _reason: _,
            _outcome: _,
            _holds: _,
        } = self;
        match (id, path, item, rule, original, line, count) {
            (Some(id), None, None, None, None, None, None) => Ok(Named::Identity(id)),
            (None, Some(path), Some(item), Some(rule), Some(original), line, count) => {
                Ok(Named::Place {
                    path,
                    item,
                    rule,
                    original,
                    line,
                    count,
                })
            }
            (Some(id), ..) => Err(serde::de::Error::custom(format!(
                "the acceptance of {id:?} names a mutant twice, by identity and by where it is, \
                 which the engine refuses"
            ))),
            (None, ..) => Err(serde::de::Error::custom(
                "an acceptance names no mutant: neither an identity nor a path, an item, a rule \
                 and the bytes the edit replaces, which the engine refuses",
            )),
        }
    }
}
