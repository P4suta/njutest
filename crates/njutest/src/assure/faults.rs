// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failing every call a `?` asks about, one at a time, and asking the suite whether it noticed (ADR 0032).

use crate::assure::baseline::{self, Reporting};
use crate::assure::mutation::{self, Disposition, Judged, MutationOptions, Perturbing, Subject};
use crate::assure::run::Request;
use crate::cli::Environment;
use crate::error::RunnerError;
use crate::report::BuildReport;
use crate::report::faults::{
    BesideRecord, BesideRun, Failed, FaultAccounting, FaultDecision, FaultRecord,
};
use crate::report::{CatalogIndex, Finding, FindingKind};
use crate::ui::Notes;
use crate::watch::Watch;
use rust_mutants::outcome::Outcome;

/// The one rule a faulted session is discovered by.
pub const RULE: &str = "inject-error";

/// What the finding is about when the tree it faults would not give a baseline.
pub const NOT_MEASURED: &str = "fault-baseline-not-measured";

/// Puts every fault the tree holds to the tests, and writes what became of each into `report`.
///
/// The faults are asked in a session of their own, so no catalog, route, execution or control of the mutation phase ever holds one.
///
/// # Errors
/// Whatever stopped the phase from observing anything; a fault the compiler refuses and a test that fails are not errors.
pub fn put(
    request: &Request,
    environment: &Environment,
    report: &mut BuildReport,
    reporting: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let (notes, watch) = reporting;
    notes.phase("faults")?;
    watch.trace.stage("faults");
    let session = match prepared(request, environment, watch) {
        Ok(session) => session,
        Err(error) => {
            if baseline::refused(&error).is_none() {
                return Err(error);
            }
            report.findings.push(unmeasured());
            return Ok(());
        }
    };
    let measured = baseline::observe(&session, Reporting { notes, watch })?;
    if !crate::assure::run::measurable(&measured) {
        report.findings.push(unmeasured());
        session.close()?;
        return Ok(());
    }
    let before = written(&session)?;
    let judged = mutation::run_resuming(
        Subject {
            session: &session,
            baseline: &measured.targets,
            perturbing: Perturbing::Faults,
        },
        &MutationOptions {
            test_args: request.test_args.clone(),
            evidence: None,
            jobs: request.config.execution.jobs,
            exclusive: crate::assure::run::alone(&request.config),
            shard: request.shard,
        },
        &mut mutation::Resume {
            state: None,
            record: &mut |_judged| Ok(()),
        },
        Reporting { notes, watch },
    )?;
    let root = request.root.display().to_string();
    let records: Vec<FaultRecord> = judged
        .judged
        .iter()
        .map(|judged| recorded(judged, &root))
        .collect();
    for record in &records {
        watch.trace.fault(record.clone());
    }
    report.beside = beside(&session, report, watch)?;
    let after = written(&session)?;
    let broke: Vec<&String> = after.difference(&before).collect();
    if !broke.is_empty() {
        report.findings.push(Finding::new(
            FindingKind::BrokenUnderFault,
            RULE,
            &format!(
                "a test wrote into the tree it was measured in while calls it made were \
                 failing, where nothing had written before any failed: what the program does \
                 when a call fails reaches past the place it was asked to work in ({})",
                broke
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    report.accounting.faults = FaultAccounting::of(&records)?;
    report
        .findings
        .extend(crate::report::faults::found(&records));
    report
        .limitations
        .extend(crate::report::faults::limited(&records));
    report.faults = records;
    for path in session.close()? {
        notes.note("kept", &path.display().to_string())?;
    }
    Ok(())
}

/// Every error-propagation survivor of the report a target told apart only with the call at its own site failing, put again beside that fault (ADR 0032 decision 6).
///
/// Evidence and never a kill: the survivor stays one, and nothing here enters a count.
fn beside(
    session: &rust_mutants::session::Session,
    report: &BuildReport,
    watch: Watch<'_>,
) -> Result<Vec<BesideRecord>, RunnerError> {
    let mut found = Vec::new();
    for survivor in report
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome.outcome() == crate::report::Outcome::Survived)
    {
        let Ok(mutant) = session.resolve(&survivor.id) else {
            continue;
        };
        if mutant.candidate.rule.family != rust_mutants::rule::Family::ErrorPropagation {
            continue;
        }
        let Some(fault) = session.fault_beside(mutant) else {
            continue;
        };
        let mut reaching: Vec<String> = session
            .route(mutant)
            .reaching()
            .into_iter()
            .map(str::to_owned)
            .collect();
        reaching.sort();
        for target in reaching {
            if watch.cancel.is_cancelled() {
                return Err(RunnerError::Interrupted);
            }
            let pair = || -> Result<(Outcome, Outcome), RunnerError> {
                let asked = |request: rust_mutants::session::Request| {
                    session
                        .exec(&request.with_target(target.clone()), watch.cancel)
                        .map(|result| result.outcome())
                };
                let alone = asked(rust_mutants::session::Request::new(fault.id.to_string()))?;
                let with = asked(
                    rust_mutants::session::Request::new(mutant.id.to_string())
                        .with_fault(fault.id.to_string()),
                )?;
                watch.trace.beside_run(BesideRun {
                    mutant: survivor.display_id.clone(),
                    fault: fault.display_id.to_string(),
                    target: target.clone(),
                    alone: alone.name().to_owned(),
                    with: with.name().to_owned(),
                });
                Ok((alone, with))
            };
            let first = pair()?;
            let Some(failed) = told(first) else {
                continue;
            };
            if pair()? != first {
                continue;
            }
            let record = BesideRecord {
                mutant: survivor.display_id.clone(),
                fault: fault.display_id.to_string(),
                target,
                failed,
            };
            watch.trace.beside(record.clone());
            found.push(record);
            break;
        }
    }
    Ok(found)
}

/// Which run of a pair failed, where exactly one did and the other passed.
const fn told(pair: (Outcome, Outcome)) -> Option<Failed> {
    match pair {
        (Outcome::Survived, Outcome::Killed) => Some(Failed::Beside),
        (Outcome::Killed, Outcome::Survived) => Some(Failed::Alone),
        (
            Outcome::NotRun
            | Outcome::Killed
            | Outcome::Survived
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored,
            _,
        ) => None,
    }
}

/// What a run says when the tree it faults gave no baseline to put a fault against, which leaves a run asked for faults short of assured.
fn unmeasured() -> Finding {
    Finding::new(
        FindingKind::NotMeasured,
        NOT_MEASURED,
        "the tree with every fault site guarded did not build or passed no test with no fault \
         in place, so no fault was put and nothing is claimed about any failure",
    )
}

/// Every path of the tree under measurement that no longer matches what was instrumented.
fn written(
    session: &rust_mutants::session::Session,
) -> Result<std::collections::BTreeSet<String>, RunnerError> {
    Ok(session
        .changes()?
        .iter()
        .map(|drift| drift.rel_path().to_owned())
        .collect())
}

/// The session every fault site of the tree is guarded in, with its one run with nothing active.
fn prepared(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<rust_mutants::session::Session, RunnerError> {
    let workspace = rust_mutants::workspace::Workspace::open(
        &request.root,
        rust_mutants::workspace::OpenOptions {
            trace: rust_mutants::trace::Recorder::disabled(),
            ..crate::assure::run::opening(request, environment)
        },
        watch.cancel,
    )?;
    Ok(workspace.prepare(
        &rust_mutants::session::PrepareOptions {
            operators: std::iter::once(RULE.to_owned())
                .chain(
                    rust_mutants::rule::Registry::canonical()
                        .family_rules(rust_mutants::rule::Family::ErrorPropagation)
                        .iter()
                        .map(|rule| rule.name.to_owned()),
                )
                .collect(),
            ..crate::assure::run::preparing(request)?
        },
        watch.cancel,
    )?)
}

/// What one judged fault site comes to, with the tree's own location taken out of anything the compiler said.
fn recorded(judged: &Judged, root: &str) -> FaultRecord {
    FaultRecord {
        catalog_index: CatalogIndex::new(judged.catalog_index),
        id: judged.id.clone(),
        display_id: judged.display_id.clone(),
        path: judged.path.clone(),
        item: judged.item.clone(),
        position: judged.position,
        decision: match decided(&judged.disposition) {
            FaultDecision::NotPut { diagnostic } => FaultDecision::NotPut {
                diagnostic: diagnostic.replace(root, "."),
            },
            other @ (FaultDecision::Noticed { .. }
            | FaultDecision::Unnoticed
            | FaultDecision::Unreached
            | FaultDecision::Waited { .. }
            | FaultDecision::Undecided { .. }) => other,
        },
    }
}

/// The decision a disposition of the shared judging comes to for a fault.
///
/// Two dispositions no fault's judging produces — a route a proof emptied, and an equivalence — are undecided rather than read as nothing noticing, so a change that made them reachable fails closed.
#[must_use]
pub fn decided(disposition: &Disposition) -> FaultDecision {
    match disposition {
        Disposition::Killed { by } => FaultDecision::Noticed { by: by.clone() },
        Disposition::Survived {
            route: rust_mutants::session::Route::Discharged { .. },
        } => FaultDecision::Undecided {
            on: RULE.to_owned(),
            why: "a proof removed every target that reached it, which no fault's route admits"
                .to_owned(),
        },
        Disposition::Survived { .. } => FaultDecision::Unnoticed,
        Disposition::Equivalent { .. } => FaultDecision::Undecided {
            on: RULE.to_owned(),
            why: "the equivalence layer answered, which it is never asked about a fault".to_owned(),
        },
        Disposition::Unreached => FaultDecision::Unreached,
        Disposition::Waited { on } | Disposition::StepLimitReached { on, .. } => {
            FaultDecision::Waited { on: on.clone() }
        }
        Disposition::Unconfirmed { on, why } => FaultDecision::Undecided {
            on: on.clone(),
            why: why.detail(),
        },
        Disposition::Errored { on, detail } => FaultDecision::Undecided {
            on: on.clone(),
            why: detail.clone(),
        },
        Disposition::Rejected { diagnostic } => FaultDecision::NotPut {
            diagnostic: crate::assure::run::first_line(diagnostic),
        },
    }
}
