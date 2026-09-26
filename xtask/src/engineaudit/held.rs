// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each claim was judged exactly where its `where` held, re-decided from the target a run recorded with an evaluator of this audit's own (ADR 0042).

use super::{ClaimStanding, Notes, Report};

/// How the engine says a claim was not judged because of its `cfg`, which is the fact this audit can re-decide.
const CFG_UNHELD: &str = "the target does not satisfy cfg(";

/// How the engine says a claim was not judged because of the tests' environment, which a report never carries.
const ENV_UNHELD: &str = "the tests are given ";

/// Every claim's standing, against whether its `cfg` holds of the recorded target.
pub(super) fn held(report: &Report, notes: &mut Notes<'_>) {
    for claim in &report.expectations {
        let said = |unheld: &str| {
            claim
                .why
                .as_deref()
                .is_some_and(|why| why.starts_with(unheld))
        };
        if claim.standing == ClaimStanding::Inapplicable && said(ENV_UNHELD) && !claim.env {
            notes.violated(
                &claim.id,
                "the claim was not judged for a value of the tests' environment, and its where \
                 names none"
                    .to_owned(),
            );
        } else if claim.standing == ClaimStanding::Inapplicable && said(ENV_UNHELD) {
            notes.unaudited(
                &claim.id,
                "the claim was not judged for a value of the tests' environment, which a report \
                 never carries, so whether it held is not re-decided"
                    .to_owned(),
            );
        }
        let Some(predicate) = &claim.cfg else {
            continue;
        };
        let Some(truth) = holds(predicate, &report.facts) else {
            notes.unaudited(
                &claim.id,
                format!("cfg({predicate}) is not a predicate this audit reads"),
            );
            continue;
        };
        let holds = truth == Truth::Holds;
        match claim.standing {
            ClaimStanding::Met | ClaimStanding::Stale | ClaimStanding::Unjudged if !holds => {
                notes.violated(
                    &claim.id,
                    format!(
                        "the claim was judged, and cfg({predicate}) does not hold of the target \
                         the run recorded"
                    ),
                );
            }
            ClaimStanding::Inapplicable if holds && said(CFG_UNHELD) => {
                notes.violated(
                    &claim.id,
                    format!(
                        "the claim was not judged for its cfg, and cfg({predicate}) holds of the \
                         target the run recorded"
                    ),
                );
            }
            ClaimStanding::Met
            | ClaimStanding::Stale
            | ClaimStanding::Unjudged
            | ClaimStanding::Inapplicable
            | ClaimStanding::Unmatched => {}
        }
    }
}

/// Whether a predicate holds of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truth {
    /// It holds.
    Holds,
    /// It does not.
    Fails,
}

impl Truth {
    const fn of(holds: bool) -> Self {
        if holds { Self::Holds } else { Self::Fails }
    }
}

/// Whether `predicate` holds of `facts`, each `name` or `name="value"`; nothing where it is not a predicate.
fn holds(predicate: &str, facts: &[String]) -> Option<Truth> {
    let mut reading = predicate.trim();
    let held = term(&mut reading, facts)?;
    reading.trim().is_empty().then_some(held)
}

fn term(reading: &mut &str, facts: &[String]) -> Option<Truth> {
    *reading = reading.trim_start();
    let end = match reading.find(|next: char| !(next.is_ascii_alphanumeric() || next == '_')) {
        Some(end) => end,
        None => reading.len(),
    };
    let (name, rest) = reading.split_at(end);
    *reading = rest.trim_start();
    match name {
        "" => None,
        "all" | "any" => {
            let every = list(reading, facts)?;
            Some(Truth::of(if name == "all" {
                every.iter().all(|one| *one == Truth::Holds)
            } else {
                every.contains(&Truth::Holds)
            }))
        }
        "not" => {
            *reading = reading.strip_prefix('(')?;
            let inner = term(reading, facts)?;
            *reading = reading.trim_start().strip_prefix(')')?;
            Some(Truth::of(inner == Truth::Fails))
        }
        _ => match reading.strip_prefix('=') {
            Some(after) => {
                let (value, rest) = after.trim_start().strip_prefix('"')?.split_once('"')?;
                *reading = rest;
                let wanted = format!("{name}=\"{value}\"");
                Some(Truth::of(facts.contains(&wanted)))
            }
            None => Some(Truth::of(facts.iter().any(|fact| fact == name))),
        },
    }
}

fn list(reading: &mut &str, facts: &[String]) -> Option<Vec<Truth>> {
    *reading = reading.strip_prefix('(')?;
    let mut every = Vec::new();
    loop {
        *reading = reading.trim_start();
        if let Some(rest) = reading.strip_prefix(')') {
            *reading = rest;
            return Some(every);
        }
        every.push(term(reading, facts)?);
        *reading = reading.trim_start();
        if let Some(rest) = reading.strip_prefix(',') {
            *reading = rest;
        } else {
            *reading = reading.strip_prefix(')')?;
            return Some(every);
        }
    }
}
