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

/// What a finding is about when the tree was written while faults were put and no execution is tied to the write.
pub const UNATTRIBUTED: &str = "fault-write-unattributed";

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
    report.findings.extend(writes(
        &session,
        (&judged.judged, &before),
        &request.test_args,
        watch,
    )?);
    report.accounting.faults = FaultAccounting::of(&records)?;
    report
        .findings
        .extend(crate::report::faults::found(&records));
    report
        .limitations
        .extend(crate::report::faults::limited(&records));
    if records.is_empty() {
        report.limitations.push(crate::report::Limitation::new(
            crate::limitation::FAULT_NO_SITE,
            "no measured file has a `?`, so there was no call a fault could fail",
        ));
    }
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

/// What the tree says the faults wrote: a `broken-under-fault` finding for each path one fault is tied to, and one `not-measured` finding naming the rest.
fn writes(
    session: &rust_mutants::session::Session,
    (judged, before): (&[Judged], &std::collections::BTreeSet<String>),
    test_args: &[String],
    watch: Watch<'_>,
) -> Result<Vec<Finding>, RunnerError> {
    let mut found = Vec::new();
    let after = written(session)?;
    let broke: Vec<&String> = after.difference(before).collect();
    let added = added(session)?;
    let mut unattributed: Vec<&str> = Vec::new();
    let mut runs = 0_usize;
    for path in broke {
        let by = if added.contains(path) {
            attributed(session, (judged, path), test_args, (&mut runs, watch))?
        } else {
            None
        };
        match by {
            Some((fault, target)) => found.push(Finding::new(
                FindingKind::BrokenUnderFault,
                &fault,
                &format!(
                    "{target}, run alone with this fault failing a call, wrote {path} into the tree \
                     it was measured in, and run alone without it did not: what the program does \
                     when that call fails reaches past the place it was asked to work in"
                ),
            )),
            None => unattributed.push(path),
        }
    }
    if !unattributed.is_empty() {
        found.push(Finding::new(
            FindingKind::NotMeasured,
            UNATTRIBUTED,
            &format!(
                "a test wrote into the tree it was measured in while calls it made were \
                 failing, where nothing had written before any failed ({}); no fault run alone \
                 was seen to write it where its test without the fault did not, so which failed \
                 call wrote it, and whether the test's own failure did, is not established, and \
                 nothing is concluded about it",
                unattributed.join(", ")
            ),
        ));
    }
    Ok(found)
}

/// How many executions attribution may run, alone and one after another, before the paths it has not reached stay unattributed.
const ATTRIBUTION_RUNS: usize = 64;

/// The paths of the tree a test created, which are the ones attribution can remove and watch come back.
fn added(
    session: &rust_mutants::session::Session,
) -> Result<std::collections::BTreeSet<String>, RunnerError> {
    Ok(session
        .changes()?
        .iter()
        .filter(|drift| matches!(drift, rust_mutants::snapshot::Drift::Added { .. }))
        .map(|drift| drift.rel_path().to_owned())
        .collect())
}

/// Whether the path at `path` is absent now, removing the file first; nothing where it cannot tell.
fn cleared(path: &std::path::Path) -> Option<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Some(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(()),
        Err(_unremovable) => None,
    }
}

/// The fault and target that wrote `path`: a fault run alone that writes it where the same target run alone without it does not, recorded run by run, within [`ATTRIBUTION_RUNS`].
fn attributed(
    session: &rust_mutants::session::Session,
    (judged, path): (&[Judged], &str),
    args: &[String],
    (runs, watch): (&mut usize, Watch<'_>),
) -> Result<Option<(String, String)>, RunnerError> {
    let at = session.snapshot_root().join(path);
    for one in judged {
        let Some(routing) = one.routing.as_ref() else {
            continue;
        };
        for asked in &routing.answered {
            if *runs >= ATTRIBUTION_RUNS || watch.cancel.is_cancelled() || cleared(&at).is_none() {
                return Ok(None);
            }
            *runs = runs.saturating_add(1);
            let request = rust_mutants::session::Request::new(one.id.as_str())
                .with_args(args.to_vec())
                .with_target(asked.target.clone());
            let faulted = session.exec(&request, watch.cancel)?;
            recorded_run(
                watch,
                (one, &asked.target, &request),
                (&faulted, crate::trace::FaultRole::Attribution),
            )?;
            let Ok(wrote) = at.try_exists() else {
                return Ok(None);
            };
            let passed = faulted.outcome() == Outcome::Survived;
            let unfaulted = if wrote && passed {
                if cleared(&at).is_none() {
                    return Ok(None);
                }
                let control = session.control(
                    &request,
                    watch.cancel,
                    rust_mutants::session::Observing::Nothing,
                )?;
                recorded_run(
                    watch,
                    (one, &asked.target, &request),
                    (&control.result, crate::trace::FaultRole::AttributionControl),
                )?;
                let Ok(again) = at.try_exists() else {
                    return Ok(None);
                };
                if again {
                    crate::trace::Unfaulted::Wrote
                } else {
                    crate::trace::Unfaulted::DidNotWrite
                }
            } else {
                crate::trace::Unfaulted::NotAsked
            };
            watch
                .trace
                .fault_attribution(crate::trace::FaultAttributionRecord {
                    fault: one.display_id.clone(),
                    target: asked.target.clone(),
                    path: path.to_owned(),
                    faulted: wrote,
                    passed,
                    unfaulted,
                });
            if wrote && passed && unfaulted == crate::trace::Unfaulted::DidNotWrite {
                return Ok(Some((one.display_id.clone(), asked.target.clone())));
            }
        }
    }
    Ok(None)
}

/// Records one execution attribution ran, as the fault execution it is.
fn recorded_run(
    watch: Watch<'_>,
    (one, target, request): (&Judged, &str, &rust_mutants::session::Request),
    (result, role): (
        &rust_mutants::execute::MutantResult,
        crate::trace::FaultRole,
    ),
) -> Result<(), RunnerError> {
    let milliseconds = result.duration.as_millis();
    let duration_ms = u64::try_from(milliseconds).map_err(|_outside_wire_range| {
        crate::assure::run::RunInvariantError::MutationDurationOutsideWire { milliseconds }
    })?;
    watch.trace.fault_exec(crate::trace::FaultExecRecord {
        fault: one.display_id.clone(),
        role,
        target: target.to_owned(),
        args: request.args.clone(),
        outcome: result.outcome().name().to_owned(),
        duration_ms,
        alone: true,
    });
    Ok(())
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
