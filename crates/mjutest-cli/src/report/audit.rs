// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a durable report must satisfy, beyond being well-formed.
//!
//! A JSON Schema can say that a field is an integer. It cannot say that the
//! numbers add up, that the verdict is the one the accounting supports, or
//! that a fact recorded as unavailable is not also present. Those are the
//! invariants that make a report auditable, and every one of them is a
//! statement about this program rather than about the code under test: a
//! violation is a bug here, and a run that hit one must not write the
//! report and claim it.

use std::fmt;

use super::{Report, TargetStatus, UNAVAILABLE, Verdict};

/// One way a report failed to be a report.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    /// The target counts do not sum to the number selected.
    TargetsDoNotAddUp {
        /// What the report says it selected.
        selected: u32,
        /// What the terminal states add up to.
        accounted: u32,
    },
    /// The target records and the target accounting disagree.
    TargetRecordsDisagree {
        /// What the accounting says.
        counted: u32,
        /// How many records there are.
        recorded: usize,
    },
    /// The targets are not in the canonical order.
    TargetsOutOfOrder {
        /// The first record that is out of place.
        at: usize,
    },
    /// The verdict claims more than what ran supports.
    VerdictUnsupported {
        /// What the report claims.
        verdict: Verdict,
        /// Why the accounting does not support it.
        because: String,
    },
    /// The report claims an assurance without having observed anything.
    NothingObserved,
    /// A field that must always say something is empty.
    EmptyRequiredValue {
        /// Which field.
        field: String,
    },
    /// Something recorded as unavailable also carries facts.
    UnavailableWithFacts {
        /// Which field.
        field: String,
    },
    /// Something the report must state as a limitation is not stated.
    MissingLimitation {
        /// The limitation's name.
        name: String,
        /// Why it is required.
        because: String,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetsDoNotAddUp {
                selected,
                accounted,
            } => write!(
                f,
                "the report selected {selected} targets but accounts for {accounted}; \
                 every selected target has exactly one terminal state"
            ),
            Self::TargetRecordsDisagree { counted, recorded } => write!(
                f,
                "the accounting counts {counted} targets and the report carries {recorded} \
                 records; a reader must be able to check the counts against the list"
            ),
            Self::TargetsOutOfOrder { at } => write!(
                f,
                "the target at position {at} breaks the canonical order (slowest first, then \
                 by identity), so two runs of the same work would produce different documents"
            ),
            Self::VerdictUnsupported { verdict, because } => {
                write!(f, "the report claims {verdict:?} and {because}")
            }
            Self::NothingObserved => write!(
                f,
                "the report claims an assurance without having run a single target; nothing \
                 was observed, so nothing is assured"
            ),
            Self::EmptyRequiredValue { field } => write!(
                f,
                "{field} is empty; a fact that could not be established is the {UNAVAILABLE:?} \
                 sentinel, because an empty value reads as nothing to say"
            ),
            Self::UnavailableWithFacts { field } => write!(
                f,
                "{field} is recorded as unavailable and carries facts anyway; one of the two \
                 is wrong, and a reader cannot tell which"
            ),
            Self::MissingLimitation { name, because } => write!(
                f,
                "the report must state the limitation {name:?}, because {because}"
            ),
        }
    }
}

/// Everything wrong with a report that is about to be written, in the order
/// a reader would want to fix them.
///
/// An empty answer is the only one that may be persisted.
#[must_use]
pub fn validate_for_persistence(report: &Report) -> Vec<Violation> {
    let mut violations = Vec::new();
    check_required(report, &mut violations);
    check_targets(report, &mut violations);
    check_verdict(report, &mut violations);
    check_git(report, &mut violations);
    violations
}

/// The fields every report says something in.
fn check_required(report: &Report, violations: &mut Vec<Violation>) {
    let fields: [(&str, &str); 8] = [
        ("schema", &report.schema),
        ("run_id", &report.run_id),
        ("repository.root_name", &report.repository.root_name),
        (
            "repository.workspace_digest",
            &report.repository.workspace_digest,
        ),
        (
            "repository.configuration_digest",
            &report.repository.configuration_digest,
        ),
        ("repository.git.commit", &report.repository.git.commit),
        ("repository.git.branch", &report.repository.git.branch),
        ("toolchain.rustc", &report.toolchain.rustc),
    ];
    for (name, value) in fields {
        if value.trim().is_empty() {
            violations.push(Violation::EmptyRequiredValue {
                field: name.to_owned(),
            });
        }
    }
}

/// The target counts, the records, and their order.
fn check_targets(report: &Report, violations: &mut Vec<Violation>) {
    let targets = report.accounting.targets;
    if targets.accounted() != targets.selected {
        violations.push(Violation::TargetsDoNotAddUp {
            selected: targets.selected,
            accounted: targets.accounted(),
        });
    }
    if usize::try_from(targets.selected).unwrap_or(usize::MAX) != report.targets.len() {
        violations.push(Violation::TargetRecordsDisagree {
            counted: targets.selected,
            recorded: report.targets.len(),
        });
    }
    for (at, pair) in report.targets.windows(2).enumerate() {
        let [before, after] = pair else {
            continue;
        };
        let ordered = before.duration_ms > after.duration_ms
            || (before.duration_ms == after.duration_ms && before.id <= after.id);
        if !ordered {
            violations.push(Violation::TargetsOutOfOrder {
                at: at.saturating_add(1),
            });
            break;
        }
    }
}

/// Whether the verdict is the one what ran supports.
fn check_verdict(report: &Report, violations: &mut Vec<Violation>) {
    if !report.verdict.is_assurance() {
        return;
    }
    let targets = report.accounting.targets;
    if targets.selected == 0 {
        violations.push(Violation::NothingObserved);
        return;
    }
    let mut unsupported = |because: &str| {
        violations.push(Violation::VerdictUnsupported {
            verdict: report.verdict,
            because: because.to_owned(),
        });
    };
    if targets.failed > 0 {
        unsupported(&format!(
            "{} of its targets failed; a failing test is not an assurance",
            targets.failed
        ));
    }
    if targets.missing > 0 {
        unsupported(&format!(
            "{} of its targets could not be found; a target that never ran cannot support a \
             claim about what it would have said",
            targets.missing
        ));
    }
    if report
        .targets
        .iter()
        .any(|target| matches!(target.status, TargetStatus::Failed | TargetStatus::Missing))
        && targets.failed == 0
        && targets.missing == 0
    {
        unsupported("a target record says it failed or was missing while the accounting does not");
    }
    if report.accounting.mutants.survived > report.accounting.mutants.accepted {
        unsupported(&format!(
            "{} mutants survived and {} were accepted with a reason; a surviving mutant nobody \
             accepted is a gap in the tests",
            report.accounting.mutants.survived, report.accounting.mutants.accepted
        ));
    }
}

/// Git is either available with its facts, or explicitly not and said so.
fn check_git(report: &Report, violations: &mut Vec<Violation>) {
    let git = &report.repository.git;
    if git.available {
        return;
    }
    for (field, present) in [
        ("repository.git.commit", git.commit != UNAVAILABLE),
        ("repository.git.branch", git.branch != UNAVAILABLE),
        ("repository.git.dirty", git.dirty),
        ("repository.git.merge_base", git.merge_base.is_some()),
        (
            "repository.git.changed_files",
            !git.changed_files.is_empty(),
        ),
    ] {
        if present {
            violations.push(Violation::UnavailableWithFacts {
                field: field.to_owned(),
            });
        }
    }
    if !report.states("git-metadata-unavailable") {
        violations.push(Violation::MissingLimitation {
            name: "git-metadata-unavailable".to_owned(),
            because: "git could not be asked, so the run cannot name the commit it verified"
                .to_owned(),
        });
    }
}
