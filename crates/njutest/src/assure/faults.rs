// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failing every call a `?` asks about, one at a time, and asking the suite whether it noticed (ADR 0032).

use crate::assure::baseline::{self, Reporting};
use crate::assure::mutation::{self, Disposition, Judged, MutationOptions, Perturbing, Subject};
use crate::assure::run::Request;
use crate::cli::Environment;
use crate::error::RunnerError;
use crate::report::BuildReport;
use crate::report::faults::{FaultAccounting, FaultDecision, FaultRecord};
use crate::report::{CatalogIndex, Finding, FindingKind, Limitation};
use crate::ui::Notes;
use crate::watch::Watch;

/// The one rule a faulted session is discovered by.
pub const RULE: &str = "inject-error";

/// The limitation a run states when the tree it faults would not give a baseline.
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
            report.limitations.push(unmeasured());
            return Ok(());
        }
    };
    let measured = baseline::observe(&session, Reporting { notes, watch })?;
    if !crate::assure::run::measurable(&measured) {
        report.limitations.push(unmeasured());
        session.close()?;
        return Ok(());
    }
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
    let records: Vec<FaultRecord> = judged.judged.iter().map(recorded).collect();
    for record in &records {
        watch.trace.fault(record.clone());
    }
    if !session.changes()?.is_empty() {
        report.findings.push(Finding::new(
            FindingKind::BrokenUnderFault,
            RULE,
            "a test wrote into the tree it was measured in while a call it made was failing, \
             and did not while none was: what the program does when that call fails reaches \
             past the place it was asked to work in",
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

/// What a run says when the tree it faults gave no baseline to put a fault against.
fn unmeasured() -> Limitation {
    Limitation::new(
        NOT_MEASURED,
        "the tree with every fault site guarded did not build or passed no test with no fault \
         in place, so no fault was put and nothing is claimed about any failure",
    )
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
            operators: vec![RULE.to_owned()],
            ..crate::assure::run::preparing(request)?
        },
        watch.cancel,
    )?)
}

/// What one judged fault site comes to.
fn recorded(judged: &Judged) -> FaultRecord {
    FaultRecord {
        catalog_index: CatalogIndex::new(judged.catalog_index),
        id: judged.id.clone(),
        display_id: judged.display_id.clone(),
        path: judged.path.clone(),
        item: judged.item.clone(),
        position: judged.position,
        decision: decided(&judged.disposition),
    }
}

/// The decision a disposition of the shared judging comes to for a fault.
fn decided(disposition: &Disposition) -> FaultDecision {
    match disposition {
        Disposition::Killed { by } => FaultDecision::Noticed { by: by.clone() },
        Disposition::Survived { .. } | Disposition::Equivalent { .. } => FaultDecision::Unnoticed,
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
