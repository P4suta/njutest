// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The critical decisions and what holds each one at every layer, held to the tree: a cell names what the tree defines, and a hole is one somebody owns.

use std::collections::{BTreeMap, BTreeSet};

/// One layer a critical decision is held at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Layer {
    /// The decision is made in one place, by a type or a single classifier.
    Types,
    /// The running engine verifies its own output and fails closed.
    SelfCheck,
    /// A test that shares no code with the decision derives the answer on its own.
    Oracle,
    /// A defect planted where the decision is made, and the checks shown to catch it.
    Plant,
    /// rust-mutants run over the module that decides.
    Mutation,
    /// Every state the decision can meet, as rows generated from a closed set.
    States,
}

impl Layer {
    /// The column the registry gives this layer, and the word the gaps ledger names it by.
    #[must_use]
    pub const fn column(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::SelfCheck => "self-check",
            Self::Oracle => "oracle",
            Self::Plant => "plant",
            Self::Mutation => "mutation",
            Self::States => "states",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.column() == word)
    }
}

/// What holds one decision at one layer: the items that do, or nothing yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// The items of the tree that hold it, by name.
    Held(Vec<String>),
    /// Nothing holds it yet, which the gaps ledger has to say somebody owns.
    Open,
}

/// One critical decision and what holds it at every layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The decision's name.
    pub decision: String,
    /// What holds it, layer by layer.
    pub cells: BTreeMap<Layer, Cell>,
}

/// One hole in the registry, and who owns closing it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap {
    /// The decision.
    pub decision: String,
    /// The layer nothing holds it at.
    pub layer: Layer,
    /// Who closes it.
    pub owner: String,
}

/// Why the registry and the tree do not hold together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvariantError {
    /// The registry page or the gaps ledger is not in the shape this reads.
    #[error("{source_name}: {detail}")]
    Shape {
        /// The file.
        source_name: String,
        /// What is wrong with it.
        detail: String,
    },
    /// A cell names an item the tree does not define.
    #[error(
        "docs/invariants.md: {decision} is held at {layer} by `{name}`, and the tree defines \
         nothing by that name; a check that is not there holds nothing"
    )]
    Unheld {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// The name.
        name: String,
    },
    /// A cell says nothing holds the decision, and the gaps ledger does not say who owns that.
    #[error(
        "docs/invariants.md: nothing holds {decision} at {layer}, and xtask/invariant_gaps.txt \
         names nobody who closes it"
    )]
    Unowned {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
    },
    /// The gaps ledger lists a hole the registry does not have.
    #[error(
        "xtask/invariant_gaps.txt: {decision} {layer} is listed as open for {owner}, and \
         docs/invariants.md says something holds it or has no such decision; take the line out"
    )]
    Stale {
        /// The decision.
        decision: String,
        /// The layer's column.
        layer: &'static str,
        /// Who the ledger says owns it.
        owner: String,
    },
    /// The gaps ledger holds more holes than its ceiling allows.
    #[error(
        "xtask/invariant_gaps.txt lists {count} hole(s) and xtask/invariant_gap_ceiling.txt \
         allows {most}; the ledger may shrink and never grow, so a new critical decision arrives \
         with what holds it"
    )]
    Grown {
        /// How many it lists.
        count: usize,
        /// How many the ceiling allows.
        most: usize,
    },
}

impl crate::error::Coded for InvariantError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Shape { .. }
            | Self::Unheld { .. }
            | Self::Unowned { .. }
            | Self::Stale { .. }
            | Self::Grown { .. } => crate::error::XtCode::InvariantRegistry,
        }
    }
}

/// The columns the registry's table has, in order: the decision, what it promises, a column per layer, and what the oracle cannot see.
const HEADER: [&str; 9] = [
    "Decision",
    "Invariant",
    "Types",
    "Self-check",
    "Oracle",
    "Plant",
    "Mutation",
    "States",
    "Blind",
];

/// The cells of one table line, trimmed, or nothing where the line is not a table line.
fn cells(line: &str) -> Option<Vec<&str>> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}

/// The names one layer cell holds, or that it is open.
fn cell(text: &str, decision: &str) -> Result<Cell, InvariantError> {
    if text == "none" {
        return Ok(Cell::Open);
    }
    let mut names = Vec::new();
    for part in text.split(", ") {
        let name = part
            .strip_prefix('`')
            .and_then(|rest| rest.strip_suffix('`'))
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .ok_or_else(|| InvariantError::Shape {
                source_name: "docs/invariants.md".to_owned(),
                detail: format!(
                    "{decision}: a layer cell is `none` or backticked item names joined by \
                     \", \", and {text:?} is neither"
                ),
            })?;
        names.push(name.to_owned());
    }
    Ok(Cell::Held(names))
}

/// Every row of the registry's table.
///
/// # Errors
/// The page has no table with the registry's columns, a line of it has another number of cells, a layer cell is neither `none` nor names, or a decision is listed twice.
pub fn rows(page: &str) -> Result<Vec<Row>, InvariantError> {
    let shape = |detail: String| InvariantError::Shape {
        source_name: "docs/invariants.md".to_owned(),
        detail,
    };
    let mut lines = page
        .lines()
        .skip_while(|line| cells(line).as_deref() != Some(&HEADER[..]));
    if lines.next().is_none() {
        return Err(shape(format!("no table heads its columns {HEADER:?}")));
    }
    if !lines.next().and_then(cells).is_some_and(|rule| {
        rule.len() == HEADER.len() && rule.iter().all(|dashes| dashes.chars().all(|c| c == '-'))
    }) {
        return Err(shape(
            "the table's header is not followed by its rule".to_owned(),
        ));
    }
    let mut rows: Vec<Row> = Vec::new();
    for line in lines.map_while(cells) {
        let [
            decision,
            _,
            types,
            check,
            oracle,
            plant,
            mutation,
            states,
            _,
        ] = line.as_slice()
        else {
            return Err(shape(format!(
                "a row has {} cells, and every row has {}",
                line.len(),
                HEADER.len()
            )));
        };
        if rows.iter().any(|row| row.decision == *decision) {
            return Err(shape(format!("{decision} is listed twice")));
        }
        let texts = [types, check, oracle, plant, mutation, states];
        let mut held = BTreeMap::new();
        for (layer, text) in Layer::ALL.into_iter().zip(texts) {
            held.insert(layer, cell(text, decision)?);
        }
        rows.push(Row {
            decision: (*decision).to_owned(),
            cells: held,
        });
    }
    Ok(rows)
}

/// Every hole the gaps ledger lists.
///
/// # Errors
/// A line is not `<decision> <layer> <owner>`, names a layer the registry does not have, or repeats another.
pub fn gaps(ledger: &str) -> Result<Vec<Gap>, InvariantError> {
    let mut gaps: Vec<Gap> = Vec::new();
    for line in ledger
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let shape = |detail: String| InvariantError::Shape {
            source_name: "xtask/invariant_gaps.txt".to_owned(),
            detail,
        };
        let words: Vec<&str> = line.split_whitespace().collect();
        let [decision, layer, owner] = words.as_slice() else {
            return Err(shape(format!(
                "{line:?} is not `<decision> <layer> <owner>`"
            )));
        };
        let layer = Layer::named(layer).ok_or_else(|| {
            shape(format!(
                "{line:?} names a layer the registry has no column for"
            ))
        })?;
        let gap = Gap {
            decision: (*decision).to_owned(),
            layer,
            owner: (*owner).to_owned(),
        };
        if gaps
            .iter()
            .any(|seen| seen.decision == gap.decision && seen.layer == gap.layer)
        {
            return Err(shape(format!("{line:?} is listed twice")));
        }
        gaps.push(gap);
    }
    Ok(gaps)
}

/// Every name `source` defines as an item: a function, type, constant, static, trait, module or macro.
#[must_use]
pub fn defined(source: &str) -> BTreeSet<String> {
    const INTRODUCERS: [&str; 9] = [
        "fn",
        "struct",
        "enum",
        "const",
        "static",
        "trait",
        "type",
        "mod",
        "macro_rules",
    ];
    let words = source
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty());
    let mut names = BTreeSet::new();
    let mut introduced = false;
    for word in words {
        if introduced {
            names.insert(word.to_owned());
        }
        introduced = INTRODUCERS.contains(&word);
    }
    names
}

/// Every way the registry, the gaps ledger and the tree disagree, or how many decisions and cells hold.
///
/// # Errors
/// Each cell naming what the tree does not define, each open cell nobody owns, each listed hole the registry does not have, and a ledger grown past its ceiling.
pub fn check(
    rows: &[Row],
    gaps: &[Gap],
    defined: &BTreeSet<String>,
    ceiling: usize,
) -> Result<(usize, usize), Vec<InvariantError>> {
    let mut refused = Vec::new();
    let mut held = 0_usize;
    for row in rows {
        for (layer, cell) in &row.cells {
            match cell {
                Cell::Held(names) => {
                    held = held.saturating_add(1);
                    refused.extend(names.iter().filter(|name| !defined.contains(*name)).map(
                        |name| InvariantError::Unheld {
                            decision: row.decision.clone(),
                            layer: layer.column(),
                            name: name.clone(),
                        },
                    ));
                }
                Cell::Open => {
                    if !gaps
                        .iter()
                        .any(|gap| gap.decision == row.decision && gap.layer == *layer)
                    {
                        refused.push(InvariantError::Unowned {
                            decision: row.decision.clone(),
                            layer: layer.column(),
                        });
                    }
                }
            }
        }
    }
    for gap in gaps {
        let open = rows.iter().any(|row| {
            row.decision == gap.decision && row.cells.get(&gap.layer) == Some(&Cell::Open)
        });
        if !open {
            refused.push(InvariantError::Stale {
                decision: gap.decision.clone(),
                layer: gap.layer.column(),
                owner: gap.owner.clone(),
            });
        }
    }
    if gaps.len() > ceiling {
        refused.push(InvariantError::Grown {
            count: gaps.len(),
            most: ceiling,
        });
    }
    if refused.is_empty() {
        Ok((rows.len(), held))
    } else {
        Err(refused)
    }
}
