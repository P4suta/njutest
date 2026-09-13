// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a durable report must satisfy, beyond being well-formed.

use std::fmt;

use super::{FindingKind, Report, TargetStatus, UNAVAILABLE, Verdict};

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
    /// A report says it was read back but does not say from where, or says it was not and does.
    ProvenanceIncoherent {
        /// Why the two do not agree.
        because: String,
    },
    /// A verdict and the findings do not agree.
    FindingsDisagree {
        /// What the report claims.
        verdict: Verdict,
        /// How many findings it carries.
        findings: usize,
        /// Why that cannot be.
        because: String,
    },
    /// Something the report must state as a limitation is not stated.
    MissingLimitation {
        /// The limitation's name.
        name: String,
        /// Why it is required.
        because: String,
    },
    /// A finding claims an acceptance is unmatched although it resolves uniquely.
    UnmatchedAcceptanceResolved {
        /// The prefix the finding names.
        subject: String,
        /// The full mutant identity it resolves to.
        mutant: String,
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
            Self::FindingsDisagree {
                verdict,
                findings,
                because,
            } => write!(
                f,
                "the report concludes {verdict:?} with {findings} findings; {because}"
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
            Self::ProvenanceIncoherent { because } => write!(
                f,
                "the report does not say where its facts came from: {because}"
            ),
            Self::MissingLimitation { name, because } => write!(
                f,
                "the report must state the limitation {name:?}, because {because}"
            ),
            Self::UnmatchedAcceptanceResolved { subject, mutant } => write!(
                f,
                "the report calls acceptance {subject:?} unmatched, but it uniquely resolves to \
                 {mutant}; an unmatched-acceptance finding must suppress nothing"
            ),
        }
    }
}

/// Everything wrong with a report that is about to be written, in the order a reader would want to fix them.
#[must_use]
pub fn validate_for_persistence(report: &Report) -> Vec<Violation> {
    let mut violations = Vec::new();
    check_required(report, &mut violations);
    check_targets(report, &mut violations);
    check_verdict(report, &mut violations);
    check_git(report, &mut violations);
    check_findings(report, &mut violations);
    check_acceptances(report, &mut violations);
    check_provenance(report, &mut violations);
    violations
}

/// Whether every unmatched-acceptance finding really fails to name exactly one catalog entry.
fn check_acceptances(report: &Report, violations: &mut Vec<Violation>) {
    if report.scope.shard.is_some() {
        return;
    }
    for finding in report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::UnmatchedAcceptance)
    {
        let subject = finding.subject.as_str();
        let valid = (rust_mutants::id::MIN_PREFIX_LENGTH..=rust_mutants::id::ID_HEX_LENGTH)
            .contains(&subject.len())
            && subject
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            continue;
        }
        let mut matches = report
            .mutants
            .iter()
            .filter(|mutant| mutant.id.starts_with(subject));
        let Some(mutant) = matches.next() else {
            continue;
        };
        if matches.next().is_none() {
            violations.push(Violation::UnmatchedAcceptanceResolved {
                subject: finding.subject.clone(),
                mutant: mutant.id.clone(),
            });
        }
    }
}

/// A report that was read back names the run that established it; one that was not names nobody.
fn check_provenance(report: &Report, violations: &mut Vec<Violation>) {
    let source = report.provenance.source_run_id.as_deref();
    match (report.provenance.cached, source) {
        (true, None) => violations.push(Violation::ProvenanceIncoherent {
            because: "a report read back from an earlier run names that run".to_owned(),
        }),
        (true, Some(run)) if run == report.run_id => {
            violations.push(Violation::ProvenanceIncoherent {
                because: "a run cannot have read its own answer back".to_owned(),
            });
        }
        (false, Some(run)) => violations.push(Violation::ProvenanceIncoherent {
            because: format!("this run established its own facts and also names {run:?}"),
        }),
        (true, Some(_)) | (false, None) => {}
    }
    if source.is_some_and(str::is_empty) {
        violations.push(Violation::ProvenanceIncoherent {
            because: "a source run with no name is no source at all".to_owned(),
        });
    }
}

/// A verdict and the findings say the same thing, or the report says two things at once.
fn check_findings(report: &Report, violations: &mut Vec<Violation>) {
    let findings = report.findings.len();
    if report.verdict.is_assurance() && findings > 0 {
        violations.push(Violation::FindingsDisagree {
            verdict: report.verdict,
            findings,
            because: "an assurance is the claim that nothing was found".to_owned(),
        });
    }
    if report.verdict == Verdict::Defect && findings == 0 {
        violations.push(Violation::FindingsDisagree {
            verdict: report.verdict,
            findings,
            because: "a defect a reader cannot see named is not a defect they can act on"
                .to_owned(),
        });
    }
}

/// The fields every report says something in.
fn check_required(report: &Report, violations: &mut Vec<Violation>) {
    let fields: [(&str, &str); 9] = [
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
        ("provenance.identity", &report.provenance.identity),
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
    let scoped = match report.run_kind {
        crate::report::RunKind::Full => Verdict::Assured,
        crate::report::RunKind::Changed => Verdict::ChangeAssured,
        crate::report::RunKind::Scoped => Verdict::ScopeAssured,
    };
    if report.verdict != scoped {
        violations.push(Violation::VerdictUnsupported {
            verdict: report.verdict,
            because: format!(
                "a {:?} run assures only what it looked at, which is {scoped:?}",
                report.run_kind
            ),
        });
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
    if report.accounting.mutants.executed == 0 {
        unsupported(
            "not one mutation was put to a test; an assurance is the claim that every \
             mutation was noticed, and a run that made none says nothing about the suite",
        );
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
