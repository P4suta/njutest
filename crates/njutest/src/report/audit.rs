// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The independent relation checker used by checked report constructors.
//!
//! A Rust field type proves a local value.
//! A private completed type plus a checked constructor can also prove arithmetic and relations across all of its fields once, then preserve that proof because callers cannot mutate the fields.
//! This module performs that construction-time proof and gives independent audit tools the same explicit invariants to re-derive.
//!
//! Refusals move out of here only when a narrower constructor or sum type makes the invalid relation unconstructible.
//! They are never dropped merely because the producer currently happens to emit matching values.

use std::collections::BTreeSet;
use std::fmt;

use super::{
    BuildLedger, BuildReport, Decision, Finding, FindingKind, LatticedReport, ModelDecision,
    ModelRecord, MutantAccounting, Outcome, ProjectedMutant, Provenance, Report, Repository,
    RunKind, Scope, TargetAccounting, TargetStatus, Tool, UNAVAILABLE, Verdict,
};

/// One way a report failed to be a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// The target counts do not sum to the number selected.
    TargetsDoNotAddUp {
        /// What the report says it selected.
        selected: u32,
        /// What the terminal states add up to.
        accounted: u32,
    },
    /// The ways a mutation can be decided do not cover the catalog exactly once.
    DecisionsDoNotAddUp {
        /// What the report says it catalogued.
        cataloged: u32,
        /// What the decisions add up to.
        decided: u32,
    },
    /// A finding about a seam names a question the report does not hold.
    SeamFindingNamesNothing {
        /// The question's identity, as the finding names it.
        id: String,
    },
    /// The target records and the target accounting disagree.
    TargetRecordsDisagree {
        /// What the accounting says.
        counted: u32,
        /// How many records there are.
        recorded: usize,
    },
    /// The target rows do not reproduce the target accounting exactly.
    TargetAccountingDisagrees {
        /// What the report claims.
        counted: TargetAccounting,
        /// What the target rows derive.
        derived: TargetAccounting,
    },
    /// Two target rows claim the same stable identity.
    DuplicateTarget {
        /// The repeated identity.
        id: String,
    },
    /// The targets are not in the canonical order.
    TargetsOutOfOrder {
        /// The first record that is out of place.
        at: usize,
    },
    /// The mutation rows do not reproduce the mutation accounting exactly.
    MutantAccountingDisagrees {
        /// What the report claims.
        counted: Box<MutantAccounting>,
        /// What the mutation rows derive.
        derived: Box<MutantAccounting>,
    },
    /// Two mutation rows claim the same full identity.
    DuplicateMutant {
        /// The repeated identity.
        id: String,
    },
    /// A row attaches acceptance or reuse provenance to an outcome that cannot carry it.
    MutantRowIncoherent {
        /// The mutation identity.
        id: String,
        /// The impossible relation.
        because: String,
    },
    /// A mutation or a finding names a file its part recorded no digest for, so nothing can tell whether that file is still the one the run read.
    SourceUnrecorded {
        /// The file.
        path: String,
    },
    /// Two parts or builds of one run recorded different digests for one file, so they did not read one tree.
    SourcesDisagree {
        /// The file.
        path: String,
    },
    /// A kill this run established is not the last answer its own route recorded, or not the only kill among them.
    KillNotItsLastAnswer {
        /// The mutation identity.
        id: String,
        /// The target the row says noticed.
        by: String,
    },
    /// A survivor this run established was not asked, once each, of exactly the targets its own route kept, or one of them noticed.
    SurvivorNotAskedOfItsRoute {
        /// The mutation identity.
        id: String,
    },
    /// A mutation this run says every test declined to measure was not asked, once each, of exactly the targets its own route kept, or one of them answered otherwise (ADR 0043).
    DeclineNotAskedOfItsRoute {
        /// The mutation identity.
        id: String,
    },
    /// A finding the part's own records decide is stored where they do not raise it, or missing where they do.
    DerivedFindingIncoherent {
        /// The kind.
        kind: &'static str,
        /// What the records and the report disagree about.
        because: String,
    },
    /// A mutation row and the actionable finding that should expose it disagree.
    MutantFindingIncoherent {
        /// The mutation identity.
        id: String,
        /// The impossible relation.
        because: String,
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
    /// A retained model answer does not correlate one-to-one with the mutation decision it supports.
    ModelEvidenceIncoherent {
        /// The mutation the model record names.
        mutant: String,
        /// The relation that did not hold.
        because: String,
    },
    /// The ordered configured-build ledger is missing or disagrees with the request.
    BuildLedgerIncoherent {
        /// The exact cross-build relation that failed.
        because: String,
    },
    /// One build's retained evidence fails the ordinary single-build audit.
    BuildEvidenceIncoherent {
        /// The configured build.
        build: String,
        /// The exact retained fact that failed.
        because: String,
    },
    /// An exact row-derived counter does not fit the report's wire type.
    CounterUnrepresentable {
        /// The checked arithmetic failure.
        because: String,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetsDoNotAddUp {
                selected,
                accounted,
            } => fmt_target_sum(f, *selected, *accounted),
            Self::DecisionsDoNotAddUp { cataloged, decided } => {
                fmt_decision_sum(f, *cataloged, *decided)
            }
            Self::SeamFindingNamesNothing { id } => write!(f, "{}", names_nothing(id)),
            Self::FindingsDisagree {
                verdict,
                findings,
                because,
            } => write!(
                f,
                "the report concludes {verdict:?} with {findings} findings; {because}"
            ),
            Self::TargetRecordsDisagree { counted, recorded } => {
                fmt_target_records(f, *counted, *recorded)
            }
            Self::TargetAccountingDisagrees { counted, derived } => {
                fmt_accounting(f, "target", counted, derived)
            }
            Self::DuplicateTarget { id } => {
                write!(f, "the target identity {id:?} occurs more than once")
            }
            Self::TargetsOutOfOrder { at } => fmt_target_order(f, *at),
            Self::MutantAccountingDisagrees { counted, derived } => {
                fmt_accounting(f, "mutation", counted, derived)
            }
            Self::DuplicateMutant { id } => {
                write!(f, "the mutation identity {id:?} occurs more than once")
            }
            Self::MutantRowIncoherent { id, because } => {
                write!(f, "the mutation row {id} is incoherent: {because}")
            }
            Self::SourceUnrecorded { path } => fmt_source_unrecorded(f, path),
            Self::SourcesDisagree { path } => fmt_sources_disagree(f, path),
            Self::KillNotItsLastAnswer { id, by } => fmt_kill_not_last(f, id, by),
            Self::SurvivorNotAskedOfItsRoute { id } => fmt_survivor_not_asked(f, id),
            Self::DeclineNotAskedOfItsRoute { id } => fmt_decline_not_asked(f, id),
            Self::DerivedFindingIncoherent { kind, because } => fmt_derived(f, kind, because),
            Self::MutantFindingIncoherent { id, because } => fmt_mutant_finding(f, id, because),
            Self::VerdictUnsupported { verdict, because } => {
                write!(f, "the report claims {verdict:?} and {because}")
            }
            Self::NothingObserved => fmt_nothing_observed(f),
            Self::EmptyRequiredValue { field } => fmt_empty_required(f, field),
            Self::ProvenanceIncoherent { because } => write!(
                f,
                "the report does not say where its facts came from: {because}"
            ),
            Self::MissingLimitation { name, because } => write!(
                f,
                "the report must state the limitation {name:?}, because {because}"
            ),
            Self::UnmatchedAcceptanceResolved { subject, mutant } => {
                fmt_resolved_acceptance(f, subject, mutant)
            }
            Self::ModelEvidenceIncoherent { mutant, because } => write!(
                f,
                "the retained model answer for {mutant} does not support the report: {because}"
            ),
            Self::BuildLedgerIncoherent { because } => {
                write!(f, "the configured-build ledger is incoherent: {because}")
            }
            Self::BuildEvidenceIncoherent { build, because } => write!(
                f,
                "configured build {build:?} carries contradictory evidence: {because}"
            ),
            Self::CounterUnrepresentable { because } => {
                write!(
                    f,
                    "the report cannot represent its exact accounting: {because}"
                )
            }
        }
    }
}

fn fmt_source_unrecorded(f: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(
        f,
        "{path} is named by a mutation or a finding and its part recorded no digest for it, so \
         nothing can tell whether the file is still the one the run read"
    )
}

fn fmt_sources_disagree(f: &mut fmt::Formatter<'_>, path: &str) -> fmt::Result {
    write!(
        f,
        "two parts of this run recorded different digests for {path}, so they did not read one tree"
    )
}

fn fmt_kill_not_last(f: &mut fmt::Formatter<'_>, id: &str, by: &str) -> fmt::Result {
    write!(
        f,
        "mutation {id} was killed by {by} in this run, and its own route's answers do not end \
         with {by} noticing it: the mutation phase stops at the first target that notices, so a \
         kill is the last answer a run records and the only kill among them"
    )
}

fn fmt_survivor_not_asked(f: &mut fmt::Formatter<'_>, id: &str) -> fmt::Result {
    write!(
        f,
        "mutation {id} survived in this run, and its own route's answers are not one survival \
         from each target the route kept: a survivor is a mutation every target that could \
         notice ran it and did not, so a kept target never asked, one asked twice, or one that \
         noticed says something else happened"
    )
}

fn fmt_decline_not_asked(f: &mut fmt::Formatter<'_>, id: &str) -> fmt::Result {
    write!(
        f,
        "mutation {id} is declined in this run, and its own route's answers are not one decline \
         from each target the route kept: a mutation is declined only where every target that \
         could notice ran it and every test of each declined to measure"
    )
}

fn fmt_target_sum(f: &mut fmt::Formatter<'_>, selected: u32, accounted: u32) -> fmt::Result {
    write!(
        f,
        "the report selected {selected} targets but accounts for {accounted}; \
         every selected target has exactly one terminal state"
    )
}

fn fmt_decision_sum(f: &mut fmt::Formatter<'_>, cataloged: u32, decided: u32) -> fmt::Result {
    write!(
        f,
        "the report catalogued {cataloged} mutations and names who decided {decided}; every \
         catalogued mutation was decided by exactly one of the type system, a test, a proof, \
         nothing that ran, nothing that could run, or nobody"
    )
}

fn fmt_target_records(f: &mut fmt::Formatter<'_>, counted: u32, recorded: usize) -> fmt::Result {
    write!(
        f,
        "the accounting counts {counted} targets and the report carries {recorded} records; \
         a reader must be able to check the counts against the list"
    )
}

fn fmt_accounting<T: fmt::Debug>(
    f: &mut fmt::Formatter<'_>,
    subject: &str,
    counted: &T,
    derived: &T,
) -> fmt::Result {
    write!(
        f,
        "the {subject} accounting {counted:?} differs from the counts derived from its rows \
         {derived:?}"
    )
}

fn fmt_target_order(f: &mut fmt::Formatter<'_>, at: usize) -> fmt::Result {
    write!(
        f,
        "the target at position {at} breaks the canonical order (slowest first, then by \
         identity), so two runs of the same work would produce different documents"
    )
}

fn fmt_nothing_observed(f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(
        "the report claims an assurance without having run a single target; nothing was \
         observed, so nothing is assured",
    )
}

fn fmt_empty_required(f: &mut fmt::Formatter<'_>, field: &str) -> fmt::Result {
    write!(
        f,
        "{field} is empty; a fact that could not be established is the {UNAVAILABLE:?} \
         sentinel, because an empty value reads as nothing to say"
    )
}

fn fmt_resolved_acceptance(f: &mut fmt::Formatter<'_>, subject: &str, mutant: &str) -> fmt::Result {
    write!(
        f,
        "the report calls acceptance {subject:?} unmatched, but it uniquely resolves to \
         {mutant}; an unmatched-acceptance finding must suppress nothing"
    )
}

/// Everything wrong with a report that is about to be written, in the order a reader would want to fix them.
#[must_use]
pub fn validate_for_persistence(report: &Report) -> Vec<Violation> {
    let mut violations = validate_lattice_evidence(report);
    let conclusion = match report.conclusion() {
        Ok(conclusion) => conclusion,
        Err(error) => {
            violations.push(Violation::CounterUnrepresentable {
                because: error.to_string(),
            });
            return violations;
        }
    };
    validate_for_persistence_with_seed(report, &conclusion, violations)
}

/// Rechecks a completed report against the conclusion already derived by its checked constructor.
/// Keeping this seam private to the report layer makes the successful derivation part of construction instead of a discarded preflight result.
pub(crate) fn validate_for_persistence_with_conclusion(
    report: &Report,
    conclusion: &super::Conclusion,
) -> Vec<Violation> {
    let violations = validate_lattice_evidence(report);
    validate_for_persistence_with_seed(report, conclusion, violations)
}

fn validate_for_persistence_with_seed(
    report: &Report,
    conclusion: &super::Conclusion,
    mut violations: Vec<Violation>,
) -> Vec<Violation> {
    violations.extend(held(&Grounds::of_whole(report, conclusion)));
    check_models(
        &ModelAudit {
            contract: report.contract,
            models: report.models(),
            mutants: &conclusion.mutants,
            counts: conclusion.accounting.mutants,
        },
        &mut violations,
    );
    violations
}

/// Everything wrong with one shard's document that is about to be written.
#[must_use]
pub fn validate_shard_for_persistence(report: &super::ShardReport) -> Vec<Violation> {
    held(&Grounds::of_shard(report))
}

/// Everything wrong with a pre-model whole-catalog lattice.
///
/// This is the checked-constructor boundary for [`LatticedReport`]; model correlation is intentionally deferred to the consuming completion transition and then rechecked by [`validate_for_persistence`].
#[must_use]
pub(crate) fn validate_lattice(
    report: &LatticedReport,
    accounting: &super::ConclusionAccounting,
    projected_mutants: usize,
) -> Vec<Violation> {
    let mut violations = validate_lattice_evidence(report);
    match accounting.targets.accounted() {
        Ok(accounted) if accounted == accounting.targets.selected => {}
        Ok(accounted) => violations.push(Violation::CounterUnrepresentable {
            because: format!(
                "the projected target ledger selects {} rows but accounts for {accounted}",
                accounting.targets.selected
            ),
        }),
        Err(error) => violations.push(Violation::CounterUnrepresentable {
            because: error.to_string(),
        }),
    }
    match accounting.mutants.observers.total() {
        Ok(total) => match usize::try_from(total) {
            Ok(total) if total == projected_mutants => {}
            Ok(total) => violations.push(Violation::CounterUnrepresentable {
                because: format!(
                    "the projected mutant ledger has {projected_mutants} rows but accounts for {total}"
                ),
            }),
            Err(error) => violations.push(Violation::CounterUnrepresentable {
                because: format!("the projected mutant total is not addressable: {error}"),
            }),
        },
        Err(error) => violations.push(Violation::CounterUnrepresentable {
            because: error.to_string(),
        }),
    }
    if accounting.soundness_by_build.len() != report.builds().len() {
        violations.push(Violation::CounterUnrepresentable {
            because: format!(
                "the soundness ledger has {} rows for {} configured builds",
                accounting.soundness_by_build.len(),
                report.builds().len()
            ),
        });
    }
    violations
}

trait LatticeEvidence {
    fn schema(&self) -> &str;
    fn schema_version(&self) -> u32;
    fn run_kind(&self) -> RunKind;
    fn contract(&self) -> crate::config::Contract;
    fn tool(&self) -> &Tool;
    fn repository(&self) -> &Repository;
    fn provenance(&self) -> &Provenance;
    fn scope(&self) -> &Scope;
    fn builds(&self) -> &BuildLedger;
    fn global_findings(&self) -> &[Finding];
}

impl LatticeEvidence for Report {
    fn schema(&self) -> &str {
        &self.schema
    }
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn run_kind(&self) -> RunKind {
        self.run_kind
    }
    fn contract(&self) -> crate::config::Contract {
        self.contract
    }
    fn tool(&self) -> &Tool {
        &self.tool
    }
    fn repository(&self) -> &Repository {
        &self.repository
    }
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn scope(&self) -> &Scope {
        &self.scope
    }
    fn builds(&self) -> &BuildLedger {
        &self.builds
    }
    fn global_findings(&self) -> &[Finding] {
        &self.global_findings
    }
}

impl LatticeEvidence for LatticedReport {
    fn schema(&self) -> &str {
        &self.schema
    }
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn run_kind(&self) -> RunKind {
        self.run_kind
    }
    fn contract(&self) -> crate::config::Contract {
        self.contract
    }
    fn tool(&self) -> &Tool {
        &self.tool
    }
    fn repository(&self) -> &Repository {
        &self.repository
    }
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn scope(&self) -> &Scope {
        &self.scope
    }
    fn builds(&self) -> &BuildLedger {
        &self.builds
    }
    fn global_findings(&self) -> &[Finding] {
        &self.global_findings
    }
}

fn validate_lattice_evidence(report: &impl LatticeEvidence) -> Vec<Violation> {
    let mut violations = validate_build_ledger(report);
    if report.schema() != super::SCHEMA || report.schema_version() != super::SCHEMA_VERSION {
        violations.push(Violation::BuildLedgerIncoherent {
            because: format!(
                "schema identity is {:?} version {}, not {:?} version {}",
                report.schema(),
                report.schema_version(),
                super::SCHEMA,
                super::SCHEMA_VERSION
            ),
        });
    }
    if report.scope().shard.is_some() {
        violations.push(Violation::BuildLedgerIncoherent {
            because: "a complete report cannot retain a partial shard scope".to_owned(),
        });
    }
    violations
}

fn validate_flat(report: &BuildReport) -> Vec<Violation> {
    let mut violations = Vec::new();
    check_required(report, &mut violations);
    check_targets(report, &mut violations);
    check_decisions(report, &mut violations);
    check_seams(report, &mut violations);
    check_verdict(report, &mut violations);
    check_git(report, &mut violations);
    check_findings(report, &mut violations);
    check_derived(report, &mut violations);
    check_acceptances(report, &mut violations);
    check_provenance(report, &mut violations);
    check_sources(report, &mut violations);
    check_answers(report, &mut violations);
    violations
}

/// Whether every file a row or a finding names is one the part recorded a digest for, which is what lets a reader of the report tell the file the run read from the file there now.
fn check_sources(report: &BuildReport, violations: &mut Vec<Violation>) {
    let named: BTreeSet<&str> = report
        .mutants
        .iter()
        .map(|row| row.path.as_str())
        .chain(
            report
                .findings
                .iter()
                .filter_map(|finding| finding.path.as_deref()),
        )
        .collect();
    for path in named {
        if !report.sources.contains_key(path) {
            violations.push(Violation::SourceUnrecorded {
                path: path.to_owned(),
            });
        }
    }
}

/// Whether every part of every build recorded one digest for each file, which is the premise that they read one tree.
fn check_one_tree(report: &impl LatticeEvidence, violations: &mut Vec<Violation>) {
    let mut read: std::collections::BTreeMap<&str, &rust_mutants::id::HexDigest> =
        std::collections::BTreeMap::new();
    let mut disagreed: BTreeSet<&str> = BTreeSet::new();
    for build in report.builds().iter() {
        for part in build.parts.iter() {
            for (path, digest) in &part.sources {
                match read.get(path.as_str()) {
                    None => {
                        read.insert(path.as_str(), digest);
                    }
                    Some(first) if *first == digest => {}
                    Some(_) => {
                        disagreed.insert(path.as_str());
                    }
                }
            }
        }
    }
    violations.extend(
        disagreed
            .into_iter()
            .map(|path| Violation::SourcesDisagree {
                path: path.to_owned(),
            }),
    );
}

/// Whether the mutation phase goes on to the next target after a target answers `outcome`: it stops only at a kill it confirmed, and a target never answers what only a whole mutation can be.
const fn carried_on_past(outcome: Outcome) -> bool {
    match outcome {
        Outcome::Survived
        | Outcome::Unconfirmed
        | Outcome::Waited
        | Outcome::StepLimitReached
        | Outcome::Errored
        | Outcome::Declined => true,
        Outcome::Killed
        | Outcome::CompileRejected
        | Outcome::Equivalent
        | Outcome::ModelNoticed
        | Outcome::ModelProved
        | Outcome::Unreached => false,
    }
}

/// Whether `routing` asked every target its route kept exactly once, each answering `outcome` or declining to measure, and, where any answered, at least one answering `outcome` (ADR 0043).
///
/// A target whose every test declined measured nothing, so it neither makes nor unmakes the mutation's outcome; a route a proof emptied answers nothing at all.
fn asked_once_each(routing: &super::Routing, outcome: Outcome) -> bool {
    let asked: BTreeSet<&str> = routing
        .answered
        .iter()
        .map(|one| one.target.as_str())
        .collect();
    let kept: BTreeSet<&str> = routing.reaching.iter().map(String::as_str).collect();
    asked == kept
        && asked.len() == routing.answered.len()
        && routing
            .answered
            .iter()
            .all(|one| one.outcome == outcome || one.outcome == Outcome::Declined)
        && (routing.answered.is_empty()
            || routing.answered.iter().any(|one| one.outcome == outcome))
}

/// Whether each row this run decided by a route it asked agrees with that route's answers: a kill is the last of them and the only kill, and a survivor was asked once of every target the route kept and each survived.
///
/// A row read back from another run or inherited without a route was not asked here, so this run's record holds no answers to hold it to.
fn check_answers(report: &BuildReport, violations: &mut Vec<Violation>) {
    for row in &report.mutants {
        let (super::Established::Here, Some(routing)) = (&row.reuse.0, &row.routing) else {
            continue;
        };
        match &row.outcome {
            super::Decided::Killed { by } => {
                let stopped = routing
                    .answered
                    .split_last()
                    .is_some_and(|(last, earlier)| {
                        last.target == *by
                            && last.outcome == Outcome::Killed
                            && earlier.iter().all(|one| carried_on_past(one.outcome))
                    });
                if !stopped {
                    violations.push(Violation::KillNotItsLastAnswer {
                        id: row.id.clone(),
                        by: by.clone(),
                    });
                }
            }
            super::Decided::Survived => {
                if !asked_once_each(routing, Outcome::Survived) {
                    violations.push(Violation::SurvivorNotAskedOfItsRoute { id: row.id.clone() });
                }
            }
            super::Decided::Declined { .. } => {
                if !asked_once_each(routing, Outcome::Declined) {
                    violations.push(Violation::DeclineNotAskedOfItsRoute { id: row.id.clone() });
                }
            }
            super::Decided::CompileRejected
            | super::Decided::ModelNoticed
            | super::Decided::ModelProved
            | super::Decided::StepLimitReached { .. }
            | super::Decided::Waited { .. }
            | super::Decided::Unreached
            | super::Decided::Equivalent
            | super::Decided::Unconfirmed { .. }
            | super::Decided::Errored { .. } => {}
        }
    }
}

/// Validates the non-empty, ordered build ledger and audits every build as an ordinary report.
/// Cross-build projection is checked only after every source row has independently passed the same audit.
fn validate_build_ledger(report: &impl LatticeEvidence) -> Vec<Violation> {
    let mut violations = Vec::new();
    check_build_names(report, &mut violations);
    check_global_finding_origins(report, &mut violations);
    let mut unique_names = BTreeSet::new();
    let mut unique_runs = BTreeSet::new();
    for build in report.builds().iter() {
        if !unique_names.insert(build.name.as_str()) {
            violations.push(Violation::BuildLedgerIncoherent {
                because: format!(
                    "build name {:?} is empty or occurs more than once",
                    build.name
                ),
            });
        }
        validate_build_parts(report, build, &mut unique_runs, &mut violations);
    }
    check_one_tree(report, &mut violations);
    check_merged_acceptances(report, &mut violations);
    violations
}

fn check_build_names(report: &impl LatticeEvidence, violations: &mut Vec<Violation>) {
    let names: Vec<&str> = report
        .builds()
        .iter()
        .map(|build| build.name.as_str())
        .collect();
    let expected: Vec<&str> = report
        .scope()
        .configured_builds
        .iter()
        .map(String::as_str)
        .collect();
    if names != expected {
        violations.push(Violation::BuildLedgerIncoherent {
            because: format!(
                "ordered build names {names:?} differ from the configured builds {expected:?}"
            ),
        });
    }
    if names.first().copied() != Some(crate::config::DEFAULT_CONFIGURATION) {
        violations.push(Violation::BuildLedgerIncoherent {
            because: format!(
                "the first build is {:?}, not {:?}",
                names.first(),
                crate::config::DEFAULT_CONFIGURATION
            ),
        });
    }
}

fn check_global_finding_origins(report: &impl LatticeEvidence, violations: &mut Vec<Violation>) {
    for finding in report.global_findings() {
        if !matches!(finding.origin, super::FindingOrigin::Global) {
            violations.push(Violation::BuildLedgerIncoherent {
                because: format!(
                    "run-wide finding {} about {:?} claims source origin {:?}",
                    finding.kind.name(),
                    finding.subject,
                    finding.origin
                ),
            });
        }
    }
}

fn validate_build_parts<'a>(
    report: &impl LatticeEvidence,
    build: &'a super::BuildEvidence,
    unique_runs: &mut BTreeSet<&'a str>,
    violations: &mut Vec<Violation>,
) {
    for part in build.parts.iter() {
        if !unique_runs.insert(part.run_id.as_str()) {
            violations.push(Violation::BuildLedgerIncoherent {
                because: format!(
                    "build {:?} repeats run namespace {:?}",
                    build.name, part.run_id
                ),
            });
        }
        check_part_finding_origins(build, part, violations);
        let mut scope = report.scope().clone();
        scope.shard = part.part.shard();
        let mut flat = BuildReport {
            schema: report.schema().to_owned(),
            schema_version: report.schema_version(),
            run_id: part.run_id.to_string(),
            run_kind: report.run_kind(),
            contract: report.contract(),
            verdict: Verdict::Insufficient,
            tool: report.tool().clone(),
            toolchain: part.toolchain.clone(),
            repository: report.repository().clone(),
            provenance: report.provenance().clone(),
            scope,
            timing: part.timing.clone(),
            accounting: part.accounting,
            resources: part.resources.clone(),
            candidates: part.candidates.clone(),
            seams: part.seams.clone(),
            faults: part.faults.clone(),
            beside: part.beside.clone(),
            crashes: part.crashes.clone(),
            targets: part.targets.clone(),
            sources: part.sources.clone(),
            mutants: part.mutants.clone(),
            findings: part.findings.clone(),
            limitations: part.limitations.clone(),
            drift: part.drift.clone(),
            knobs: part.knobs.clone(),
            concurrency: part.concurrency.clone(),
        };
        flat.verdict = flat.concluded();
        for failure in validate_flat(&flat) {
            violations.push(Violation::BuildEvidenceIncoherent {
                build: build.name.as_str().to_owned(),
                because: failure.to_string(),
            });
        }
    }
}

fn check_part_finding_origins(
    build: &super::BuildEvidence,
    part: &super::BuildPartEvidence,
    violations: &mut Vec<Violation>,
) {
    for finding in &part.findings {
        let correct = match &finding.origin {
            super::FindingOrigin::Source {
                build: origin_build_name,
                run_id,
                part: source_part,
            } => {
                origin_build_name == &build.name
                    && run_id == &part.run_id
                    && source_part == &part.part
            }
            super::FindingOrigin::Global => false,
        };
        if !correct {
            violations.push(Violation::BuildEvidenceIncoherent {
                build: build.name.as_str().to_owned(),
                because: format!(
                    "finding {} about {:?} has origin {:?}",
                    finding.kind.name(),
                    finding.subject,
                    finding.origin
                ),
            });
        }
    }
}

/// Whether model evidence obeys the selected contract.
///
/// Only `verified-v1` admits model evidence.
/// Under that contract every survivor and every affirmative model outcome has exactly one retained answer, and every retained answer points back to exactly one matching row.
#[derive(Clone, Copy)]
struct ModelAudit<'a> {
    contract: crate::config::Contract,
    models: &'a [ModelRecord],
    mutants: &'a [ProjectedMutant],
    counts: MutantAccounting,
}

fn check_models(audit: &ModelAudit<'_>, violations: &mut Vec<Violation>) {
    if !audit.contract.proves_models() {
        reject_models_for_contract(audit.contract, audit.models, audit.mutants, violations);
        return;
    }
    check_verified_models(audit.models, audit.mutants, violations);
    let counted = audit.counts.observers;
    compare_model_count(
        "noticed",
        audit
            .models
            .iter()
            .filter(|model| matches!(model.answer(), ModelDecision::Noticed { .. })),
        counted.model_noticed,
        violations,
    );
    compare_model_count(
        "proved",
        audit
            .models
            .iter()
            .filter(|model| matches!(model.answer(), ModelDecision::Proved { .. })),
        counted.model_proved,
        violations,
    );
}

fn reject_models_for_contract(
    contract: crate::config::Contract,
    models: &[ModelRecord],
    mutants: &[ProjectedMutant],
    violations: &mut Vec<Violation>,
) {
    for model in models {
        violations.push(Violation::ModelEvidenceIncoherent {
            mutant: model.mutant().to_owned(),
            because: format!("contract {contract:?} does not admit retained model answers"),
        });
    }
    for row in mutants {
        if matches!(
            row.decision(),
            Decision::ModelNoticed | Decision::ModelProved
        ) {
            violations.push(Violation::ModelEvidenceIncoherent {
                mutant: row.id().to_owned(),
                because: format!(
                    "contract {contract:?} does not admit model-decided mutation outcomes"
                ),
            });
        }
    }
}

fn check_verified_models(
    models: &[ModelRecord],
    mutants: &[ProjectedMutant],
    violations: &mut Vec<Violation>,
) {
    let mut seen = BTreeSet::new();
    for model in models {
        let mutant = model.mutant();
        if !seen.insert(mutant) {
            violations.push(Violation::ModelEvidenceIncoherent {
                mutant: mutant.to_owned(),
                because: "the same mutation has more than one model answer".to_owned(),
            });
            continue;
        }
        let Some(row) = mutants.iter().find(|row| row.id() == mutant) else {
            violations.push(Violation::ModelEvidenceIncoherent {
                mutant: mutant.to_owned(),
                because: "no mutation row has that identity".to_owned(),
            });
            continue;
        };
        let expected = match model.answer() {
            ModelDecision::Noticed { .. } => Decision::ModelNoticed,
            ModelDecision::Proved { .. } => Decision::ModelProved,
            ModelDecision::Ineligible { .. } | ModelDecision::Undecided { .. } => {
                Decision::Unnoticed
            }
        };
        let actual = row.decision();
        if actual != expected {
            violations.push(Violation::ModelEvidenceIncoherent {
                mutant: mutant.to_owned(),
                because: format!(
                    "the answer requires decision {} but the row says {}",
                    expected.name(),
                    actual.name()
                ),
            });
        }
    }
    for row in mutants {
        if matches!(
            row.decision(),
            Decision::Unnoticed | Decision::ModelNoticed | Decision::ModelProved
        ) && !seen.contains(row.id())
        {
            violations.push(Violation::ModelEvidenceIncoherent {
                mutant: row.id().to_owned(),
                because: "a survivor or affirmative model outcome has no retained model answer"
                    .to_owned(),
            });
        }
    }
}

fn compare_model_count<'a>(
    name: &str,
    retained: impl Iterator<Item = &'a ModelRecord>,
    counted: u32,
    violations: &mut Vec<Violation>,
) {
    let retained = match super::count_of("retained model answers", retained.count()) {
        Ok(retained) => retained,
        Err(error) => {
            violations.push(Violation::CounterUnrepresentable {
                because: error.to_string(),
            });
            return;
        }
    };
    if retained != counted {
        violations.push(Violation::ModelEvidenceIncoherent {
            mutant: "accounting".to_owned(),
            because: format!(
                "{retained} retained {name} answers disagree with the {counted} counted"
            ),
        });
    }
}

/// Whether somebody is named for every mutation the run catalogued, and nobody twice.
fn check_decisions(report: &BuildReport, violations: &mut Vec<Violation>) {
    let counts = report.accounting.mutants;
    match counts.observers.total() {
        Ok(decided) if decided != counts.cataloged => {
            violations.push(Violation::DecisionsDoNotAddUp {
                cataloged: counts.cataloged,
                decided,
            });
        }
        Ok(_exact) => {}
        Err(error) => violations.push(Violation::CounterUnrepresentable {
            because: error.to_string(),
        }),
    }
    if let Some(derived) = derive_mutant_accounting(report, violations)
        && derived != counts
    {
        violations.push(Violation::MutantAccountingDisagrees {
            counted: Box::new(counts),
            derived: Box::new(derived),
        });
    }
}

/// Rebuilds every mutation counter whose source is the mutation rows.
fn derive_mutant_accounting(
    report: &BuildReport,
    violations: &mut Vec<Violation>,
) -> Option<MutantAccounting> {
    let derived = match super::count_mutants(&report.mutants) {
        Ok(derived) => derived,
        Err(error) => {
            violations.push(Violation::CounterUnrepresentable {
                because: error.to_string(),
            });
            return None;
        }
    };
    let mut identities = BTreeSet::new();
    for row in &report.mutants {
        if !identities.insert(row.id.as_str()) {
            violations.push(Violation::DuplicateMutant { id: row.id.clone() });
        }
        let outcome = row.outcome.outcome();
        if row.accepted && !outcome.review_answerable() {
            violations.push(Violation::MutantRowIncoherent {
                id: row.id.clone(),
                because: format!(
                    "outcome {} cannot be answered by a review acceptance",
                    outcome.name()
                ),
            });
        }
        let reused = row.reuse.0.read_back().is_some();
        if reused && !matches!(outcome, Outcome::Killed | Outcome::Survived) {
            violations.push(Violation::MutantRowIncoherent {
                id: row.id.clone(),
                because: format!(
                    "outcome {} is not a reusable verdict but names a source run",
                    outcome.name()
                ),
            });
        }
    }
    Some(derived)
}

/// What a reader is told where a seam finding names a question the report does not hold.
fn names_nothing(id: &str) -> String {
    format!(
        "the report calls {id} a question nothing noticed and holds no such question; \
         a reader handed a name with nothing to look it up in has been told nothing \
         they can act on"
    )
}

/// Whether every finding about a seam names a question the report itself holds.
fn check_seams(report: &BuildReport, violations: &mut Vec<Violation>) {
    for finding in report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::WireUnnoticed)
    {
        if !report.seams.iter().any(|one| one.id == finding.subject) {
            violations.push(Violation::SeamFindingNamesNothing {
                id: finding.subject.clone(),
            });
        }
    }
}

/// Whether every unmatched-acceptance finding really fails to name exactly one catalog entry, which a part of a shard set cannot say alone.
fn check_acceptances(report: &BuildReport, violations: &mut Vec<Violation>) {
    if report.scope.shard.is_some() {
        return;
    }
    resolved(&report.findings, &report.mutants, violations);
}

/// The same question of a build measured in parts, asked of every part's rows together.
fn check_merged_acceptances(report: &impl LatticeEvidence, violations: &mut Vec<Violation>) {
    for build in report.builds().iter() {
        if !build
            .parts
            .iter()
            .any(|part| matches!(part.part, super::CatalogPart::Shard(_)))
        {
            continue;
        }
        let rows: Vec<super::MutantRecord> = build
            .parts
            .iter()
            .flat_map(|part| part.mutants.iter().cloned())
            .collect();
        resolved(report.global_findings(), &rows, violations);
    }
}

/// A violation for every unmatched acceptance `rows` resolve.
fn resolved(findings: &[Finding], rows: &[super::MutantRecord], violations: &mut Vec<Violation>) {
    for (subject, mutant) in super::acceptances_the_catalog_resolves(findings, rows) {
        violations.push(Violation::UnmatchedAcceptanceResolved { subject, mutant });
    }
}

/// The two things about where a report's facts came from that a type does not settle.
///
/// What this used to say and no longer needs to — read back from nobody, and established here and also somewhere else — is unwritable now that `report::Established` carries the pair rather than two fields that can disagree.
///
/// What is left is two constraints on the content of a name rather than on which fields go together, which is where making illegal states unrepresentable stops reaching.
/// Naming itself needs the run's own identity, and the value holds only the source's.
/// Naming nothing is refused where a document is read as well; the only thing that mints a run id is this tool, and a constructor that made every caller invent a behaviour for an id it cannot produce would be worse code than this line.
fn check_provenance(report: &BuildReport, violations: &mut Vec<Violation>) {
    let source = report.provenance.facts.read_back();
    if source == Some(&report.run_id) {
        violations.push(Violation::ProvenanceIncoherent {
            because: "a run cannot have read its own answer back".to_owned(),
        });
    }
    if source.is_some_and(String::is_empty) {
        violations.push(Violation::ProvenanceIncoherent {
            because: "a source run with no name is no source at all".to_owned(),
        });
    }
}

/// Every finding the part's own records decide is one the report holds, and a kind the records decide wholly holds no other.
fn check_derived(report: &BuildReport, violations: &mut Vec<Violation>) {
    let derived = super::derived::findings(report);
    for kind in FindingKind::ALL {
        let subjects = |findings: &[Finding]| -> Vec<String> {
            let mut subjects: Vec<String> = findings
                .iter()
                .filter(|finding| finding.kind == kind)
                .map(|finding| finding.subject.clone())
                .collect();
            subjects.sort();
            subjects
        };
        let (decided, held) = (subjects(&derived), subjects(&report.findings));
        let missing: Vec<&String> = decided.iter().filter(|one| !held.contains(one)).collect();
        let because = match kind.derivation() {
            super::Derivation::Records if decided != held => Some(format!(
                "the records raise {decided:?} and the report holds {held:?}"
            )),
            super::Derivation::Shared if !missing.is_empty() => Some(format!(
                "the records raise {missing:?}, which the report does not hold"
            )),
            super::Derivation::Records
            | super::Derivation::Shared
            | super::Derivation::Row
            | super::Derivation::Observed => None,
        };
        if let Some(because) = because {
            violations.push(Violation::DerivedFindingIncoherent {
                kind: kind.name(),
                because,
            });
        }
    }
}

/// The sentence of a derived finding the records and the report disagree about.
fn fmt_derived(f: &mut fmt::Formatter<'_>, kind: &str, because: &str) -> fmt::Result {
    write!(
        f,
        "the {kind} findings disagree with the records: {because}"
    )
}

/// The sentence of a mutation row whose finding is incoherent.
fn fmt_mutant_finding(f: &mut fmt::Formatter<'_>, id: &str, because: &str) -> fmt::Result {
    write!(
        f,
        "the finding for mutation row {id} is incoherent: {because}"
    )
}

/// Whether `kind` is one some outcome of a mutation row requires, read from the outcomes rather than listed beside them, so an outcome added later brings its finding here.
fn a_mutation_finding(kind: FindingKind) -> bool {
    Outcome::ALL.into_iter().any(|outcome| {
        [
            outcome.required_finding(false),
            outcome.required_finding(true),
        ]
        .contains(&Some(kind))
    })
}

/// Every mutation row is tied to exactly the finding its outcome requires.
fn check_findings(report: &BuildReport, violations: &mut Vec<Violation>) {
    for row in &report.mutants {
        let expected = row.outcome.outcome().required_finding(row.accepted);
        let tied: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.subject == row.id || finding.subject == row.display_id)
            .filter(|finding| a_mutation_finding(finding.kind))
            .collect();
        match expected {
            Some(kind) if matches!(tied.as_slice(), [one] if one.kind == kind) => {}
            Some(kind) => violations.push(Violation::MutantFindingIncoherent {
                id: row.id.clone(),
                because: format!(
                    "outcome {} requires exactly one {} finding, but the row has {} mutation finding(s)",
                    row.outcome.name(),
                    kind.name(),
                    tied.len()
                ),
            }),
            None if tied.is_empty() => {}
            None => violations.push(Violation::MutantFindingIncoherent {
                id: row.id.clone(),
                because: format!(
                    "outcome {} requires no mutation finding, but the row has {}",
                    row.outcome.name(),
                    tied.iter()
                        .map(|finding| finding.kind.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
        }
    }
}

/// The fields every report says something in.
fn check_required(report: &BuildReport, violations: &mut Vec<Violation>) {
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
        ("repository.git.commit", report.repository.git.commit()),
        ("repository.git.branch", report.repository.git.branch()),
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
fn check_targets(report: &BuildReport, violations: &mut Vec<Violation>) {
    let targets = report.accounting.targets;
    match targets.accounted() {
        Ok(accounted) if accounted != targets.selected => {
            violations.push(Violation::TargetsDoNotAddUp {
                selected: targets.selected,
                accounted,
            });
        }
        Ok(_exact) => {}
        Err(error) => violations.push(Violation::CounterUnrepresentable {
            because: error.to_string(),
        }),
    }
    if u64::from(targets.selected)
        != match u64::try_from(report.targets.len()) {
            Ok(recorded) => recorded,
            Err(_outside_u64) => {
                violations.push(Violation::CounterUnrepresentable {
                    because: format!(
                        "target row count {} is outside the audit counter range",
                        report.targets.len()
                    ),
                });
                return;
            }
        }
    {
        violations.push(Violation::TargetRecordsDisagree {
            counted: targets.selected,
            recorded: report.targets.len(),
        });
    }
    let derived = match super::count_targets(&report.targets) {
        Ok(derived) => derived,
        Err(error) => {
            violations.push(Violation::CounterUnrepresentable {
                because: error.to_string(),
            });
            return;
        }
    };
    let mut identities = BTreeSet::new();
    for target in &report.targets {
        if !identities.insert(target.id.as_str()) {
            violations.push(Violation::DuplicateTarget {
                id: target.id.clone(),
            });
        }
    }
    if derived != targets {
        violations.push(Violation::TargetAccountingDisagrees {
            counted: targets,
            derived,
        });
    }
    for (at, pair) in report.targets.windows(2).enumerate() {
        let [before, after] = pair else {
            continue;
        };
        let ordered = before.duration_ms > after.duration_ms
            || (before.duration_ms == after.duration_ms && before.id <= after.id);
        if !ordered {
            let Some(position) = at.checked_add(1) else {
                violations.push(Violation::CounterUnrepresentable {
                    because: "target order position exceeds usize".to_owned(),
                });
                break;
            };
            violations.push(Violation::TargetsOutOfOrder { at: position });
            break;
        }
    }
}

/// Whether the verdict is the one what ran supports.
fn check_verdict(report: &BuildReport, violations: &mut Vec<Violation>) {
    violations.extend(held(&Grounds::of_part(report)));
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
}

/// The facts a verdict rests on, as one view of a run holds them: one part, a shard's parts together, or the whole catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grounds {
    /// The verdict the view concludes.
    pub verdict: Verdict,
    /// How much of the workspace the run looked at.
    pub run_kind: RunKind,
    /// Which part of a divided catalog the view is, when it is one.
    pub shard: Option<String>,
    /// How many of its findings are defects in the code under test.
    pub defects: usize,
    /// How many findings it carries, defects included.
    pub findings: usize,
    /// Whether every build saw a target pass on the original tree.
    pub observed: bool,
    /// Whether every build put a mutation to a test.
    pub asked: bool,
    /// Why each mutation row that is not an answer is not one.
    pub unanswered: Vec<String>,
    /// Whether a reach moved or a knob shook where the findings about it are raised only once the parts are merged.
    pub unsettled: bool,
}

impl Grounds {
    /// What one part of one configured build holds.
    #[must_use]
    pub fn of_part(report: &BuildReport) -> Self {
        Self {
            verdict: report.verdict,
            run_kind: report.run_kind,
            shard: report.scope.shard.clone(),
            defects: report
                .findings
                .iter()
                .filter(|finding| finding.kind.is_defect())
                .count(),
            findings: report.findings.len(),
            observed: report.accounting.targets.passed > 0,
            asked: report.accounting.mutants.executed > 0,
            unanswered: report
                .mutants
                .iter()
                .filter_map(|row| unanswered(&row.id, row.outcome.outcome(), row.accepted))
                .collect(),
            unsettled: super::moved(&report.drift) || super::knobs::shaken(&report.knobs),
        }
    }
}

impl Grounds {
    /// Whether something the verdict rests on was not established.
    #[must_use]
    pub const fn short(&self) -> bool {
        !self.observed || !self.asked || !self.unanswered.is_empty() || self.unsettled
    }
}

impl Grounds {
    /// What the whole catalog holds, once every part's findings are raised over it.
    #[must_use]
    pub fn of_whole(report: &Report, conclusion: &super::Conclusion) -> Self {
        Self {
            verdict: conclusion.verdict,
            run_kind: report.run_kind,
            shard: report.scope.shard.clone(),
            defects: conclusion
                .findings
                .iter()
                .filter(|finding| finding.kind.is_defect())
                .count(),
            findings: conclusion.findings.len(),
            observed: report
                .builds
                .iter()
                .all(|build| build.baseline().accounting.targets.passed > 0),
            asked: report.builds.iter().all(|build| {
                build
                    .parts
                    .iter()
                    .any(|part| part.accounting.mutants.executed > 0)
            }),
            unanswered: conclusion
                .mutants
                .iter()
                .filter(|mutant| {
                    !matches!(
                        mutant.decision,
                        Decision::ModelNoticed | Decision::ModelProved
                    )
                })
                .flat_map(|mutant| {
                    mutant.by_build.iter().filter_map(|fact| {
                        unanswered(&mutant.id, fact.decision.outcome(), fact.accepted)
                    })
                })
                .collect(),
            unsettled: false,
        }
    }

    /// What one shard holds across its configured builds, whose drift and knobs are settled only by the merge.
    #[must_use]
    pub fn of_shard(report: &super::ShardReport) -> Self {
        let findings: Vec<&Finding> = report
            .global_findings
            .iter()
            .chain(
                report
                    .builds
                    .iter()
                    .flat_map(|build| build.source.findings.iter()),
            )
            .collect();
        Self {
            verdict: report.verdict(),
            run_kind: report.run_kind,
            shard: Some(format!("{}/{}", report.shard.index(), report.shard.of())),
            defects: findings
                .iter()
                .filter(|finding| finding.kind.is_defect())
                .count(),
            findings: findings.len(),
            observed: report
                .builds
                .iter()
                .all(|build| build.source.accounting.targets.passed > 0),
            asked: report
                .builds
                .iter()
                .all(|build| build.source.accounting.mutants.executed > 0),
            unanswered: report
                .builds
                .iter()
                .flat_map(|build| build.source.mutants.iter())
                .filter_map(|row| unanswered(&row.id, row.outcome.outcome(), row.accepted))
                .collect(),
            unsettled: report.builds.iter().any(|build| {
                super::moved(&build.source.drift) || super::knobs::shaken(&build.source.knobs)
            }),
        }
    }
}

/// Why a row that ended as `outcome` is not an answer, or nothing when it is one.
fn unanswered(id: &str, outcome: Outcome, accepted: bool) -> Option<String> {
    (!outcome.answered(accepted)).then(|| {
        format!(
            "mutation {id} ended as {}{}; that row is not an answer",
            outcome.name(),
            if outcome.review_answerable() {
                " without its own review acceptance"
            } else {
                ""
            }
        )
    })
}

/// Everything `grounds.verdict` claims that the rest of `grounds` does not support; whatever the rest holds, exactly one verdict draws nothing.
#[must_use]
pub fn held(grounds: &Grounds) -> Vec<Violation> {
    let mut violations = Vec::new();
    let verdict = grounds.verdict;
    let unsupported = |because: String| Violation::VerdictUnsupported { verdict, because };
    match (verdict, grounds.shard.as_deref()) {
        (Verdict::Assured | Verdict::ChangeAssured | Verdict::ScopeAssured, None) => {
            let scoped = match grounds.run_kind {
                RunKind::Full => Verdict::Assured,
                RunKind::Changed => Verdict::ChangeAssured,
                RunKind::Scoped => Verdict::ScopeAssured,
            };
            if verdict != scoped {
                violations.push(unsupported(format!(
                    "a {:?} run assures only what it looked at, which is {scoped:?}",
                    grounds.run_kind
                )));
            }
            established(grounds, &mut violations);
        }
        (Verdict::Assured | Verdict::ChangeAssured | Verdict::ScopeAssured, Some(shard)) => {
            violations.push(unsupported(format!(
                "part {shard} of a divided catalog assures nothing on its own"
            )));
        }
        (Verdict::Partial, Some(_)) => established(grounds, &mut violations),
        (Verdict::Partial, None) => violations.push(unsupported(
            "PARTIAL is what one part of a divided catalog concludes, and this records no shard"
                .to_owned(),
        )),
        (Verdict::Defect, _) if grounds.defects == 0 => {
            violations.push(Violation::FindingsDisagree {
                verdict,
                findings: grounds.findings,
                because: "a defect a reader cannot see named is not a defect they can act on"
                    .to_owned(),
            });
        }
        (Verdict::Insufficient, _) if grounds.defects > 0 => {
            violations.push(Violation::FindingsDisagree {
                verdict,
                findings: grounds.findings,
                because: "a run that found a defect says DEFECT, whole or in part".to_owned(),
            });
        }
        (Verdict::Insufficient, _) if grounds.findings == 0 && !grounds.short() => {
            violations.push(unsupported(
                "nothing was found, and every build observed, asked and answered everything \
                 it catalogued, so the run established more than it says"
                    .to_owned(),
            ));
        }
        (Verdict::Defect | Verdict::Insufficient, _) => {}
        (Verdict::Error, _) => violations.push(unsupported(
            "a report is what a run that came to a verdict writes, and ERROR is what one that \
             came to none says"
                .to_owned(),
        )),
    }
    violations
}

/// What an assurance and a part both claim: nothing was found, and every build observed, asked and answered everything it catalogued.
fn established(grounds: &Grounds, violations: &mut Vec<Violation>) {
    let verdict = grounds.verdict;
    let unsupported = |because: String| Violation::VerdictUnsupported { verdict, because };
    if grounds.findings > 0 {
        violations.push(Violation::FindingsDisagree {
            verdict,
            findings: grounds.findings,
            because: "an assurance is the claim that nothing was found, and a finding in a part \
                      is a finding"
                .to_owned(),
        });
    }
    if !grounds.observed {
        violations.push(unsupported(
            "a build saw no target pass on the original tree, so nothing was observed".to_owned(),
        ));
    }
    if !grounds.asked {
        violations.push(unsupported(
            "not one mutation was put to a test; an assurance is the claim that every mutation \
             was noticed, and a run that made none says nothing about the suite"
                .to_owned(),
        ));
    }
    violations.extend(grounds.unanswered.iter().cloned().map(unsupported));
    if grounds.unsettled {
        violations.push(unsupported(
            "a reach moved or a knob shook, and what that unsettles is decided only when the \
             parts are merged"
                .to_owned(),
        ));
    }
}

/// Git is either available with its facts, or explicitly not and said so.
fn check_git(report: &BuildReport, violations: &mut Vec<Violation>) {
    if report.repository.git.said().is_some() {
        return;
    }
    if !report.states("git-metadata-unavailable") {
        violations.push(Violation::MissingLimitation {
            name: "git-metadata-unavailable".to_owned(),
            because: "git could not be asked, so the run cannot name the commit it verified"
                .to_owned(),
        });
    }
}
