// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stopping the process just after each call that writes, and asking whether the next run starts over what it left (ADR 0035).

use std::collections::BTreeMap;

use rust_mutants::catalog::Mutant;
use rust_mutants::instrument::CRASH_EXIT;
use rust_mutants::outcome::Outcome;
use rust_mutants::session::{Asked, Kept, Observing, Request as ExecRequest, Session};

use crate::assure::baseline::{self, Reporting};
use crate::assure::run::Request;
use crate::cli::Environment;
use crate::error::RunnerError;
use crate::report::crashes::{CrashAccounting, CrashDecision, CrashRecord};
use crate::report::{BuildReport, CatalogIndex, Finding, FindingKind, Limitation};
use crate::ui::Notes;
use crate::watch::Watch;

/// The one rule a crashed session is discovered by.
pub const RULE: &str = "crash-after-write";

/// What the finding is about when the tree it crashes would not give a baseline.
pub const NOT_MEASURED: &str = "crash-baseline-not-measured";

/// Stops the process after every call that writes the tree holds, one at a time, and writes what the next run did into `report`.
///
/// The crashes are asked in a session of their own, like faults, so no catalog, route, execution or control of another phase ever holds one.
///
/// # Errors
/// Whatever stopped the phase from observing anything; a crash the compiler refuses and a next run that fails are not errors.
pub fn put(
    request: &Request,
    environment: &Environment,
    report: &mut BuildReport,
    reporting: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let (notes, watch) = reporting;
    notes.phase("crashes")?;
    watch.trace.stage("crashes");
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
    let rejected: BTreeMap<String, String> = session
        .rejections()
        .iter()
        .map(|rejection| (rejection.id.clone(), rejection.diagnostic.clone()))
        .collect();
    let root = request.root.display().to_string();
    let mut records = Vec::new();
    for mutant in session
        .catalog()
        .mutants()
        .iter()
        .filter(|mutant| request.shard.is_none_or(|shard| shard.holds(mutant.index)))
    {
        let decision = match rejected.get(mutant.id.as_str()) {
            Some(diagnostic) => CrashDecision::NotPut {
                diagnostic: crate::assure::run::first_line(diagnostic).replace(&root, "."),
            },
            None => decided(&session, mutant, watch)?,
        };
        let record = CrashRecord {
            catalog_index: CatalogIndex::new(mutant.index),
            id: mutant.id.to_string(),
            display_id: mutant.display_id.to_string(),
            path: mutant.candidate.path.clone(),
            item: session.item_of(mutant.index).unwrap_or_default().to_owned(),
            position: session.position(mutant).map(|at| crate::report::Position {
                line: at.line,
                column: at.byte_column,
                character_column: at.char_column,
            }),
            decision,
        };
        watch.trace.crash(record.clone());
        records.push(record);
    }
    if records.is_empty() {
        report.limitations.push(Limitation::new(
            crate::limitation::CRASH_NO_SITE,
            "no measured file calls anything that writes, so there was nothing to stop after",
        ));
    }
    report.accounting.crashes = CrashAccounting::of(&records)?;
    report
        .findings
        .extend(crate::report::crashes::found(&records));
    report
        .limitations
        .extend(crate::report::crashes::limited(&records));
    report.crashes = records;
    for path in session.close()? {
        notes.note("kept", &path.display().to_string())?;
    }
    Ok(())
}

/// What a crash at one call comes to: the first test that reaches it, target by target in name order, stopped there and run again over what it left.
fn decided(
    session: &Session,
    mutant: &Mutant,
    watch: Watch<'_>,
) -> Result<CrashDecision, RunnerError> {
    let mut asked = session.route(mutant).asked();
    asked.sort_by(|one, other| one.target.cmp(&other.target));
    for reaches in asked {
        let tests = match reaches.tests {
            Asked::Every => {
                return Ok(CrashDecision::Undecided {
                    on: reaches.target,
                    why: "which of its tests reaches the call is not known, so a stop would \
                          stop every test of it at once and tear what the others were writing"
                        .to_owned(),
                });
            }
            Asked::These(tests) => tests,
        };
        for test in tests {
            let on = Stopped {
                session,
                mutant,
                target: &reaches.target,
                test: &test,
                watch,
            };
            if let Some(decision) = on.decided()? {
                return Ok(decision);
            }
        }
    }
    Ok(CrashDecision::Unreached)
}

/// One test a crash is put to.
struct Stopped<'a> {
    session: &'a Session,
    mutant: &'a Mutant,
    target: &'a str,
    test: &'a str,
    watch: Watch<'a>,
}

impl Stopped<'_> {
    /// The target and test, as a record names them.
    fn on(&self) -> String {
        format!("{}::{}", self.target, self.test)
    }

    /// The request that runs this test, with `mutant` active or with nothing where it is empty.
    fn request(&self, mutant: &str) -> ExecRequest {
        ExecRequest::new(mutant)
            .with_target(self.target)
            .test(Some(self.test.to_owned()))
    }

    /// The test stopped at the call, or nothing where it did not stop there.
    fn crashed(&self) -> Result<Option<Kept>, RunnerError> {
        let (result, kept) = self
            .session
            .exec_keeping(&self.request(self.mutant.id.as_str()), self.watch.cancel)?;
        self.recorded("crash", result.exit_code, result.outcome());
        Ok((result.exit_code == CRASH_EXIT).then_some(kept))
    }

    /// What the test comes to after a stop at the call, or nothing where it did not stop there.
    fn decided(&self) -> Result<Option<CrashDecision>, RunnerError> {
        let Some(kept) = self.crashed()? else {
            return Ok(None);
        };
        let on = self.on();
        let left = kept.left()?;
        if left.is_empty() {
            return Ok(Some(CrashDecision::Unshared { on }));
        }
        let next = self
            .session
            .control_in(&self.request(""), &kept, self.watch.cancel)?;
        self.recorded("next", next.exit_code, next.outcome());
        Ok(Some(match next.outcome() {
            Outcome::Survived => CrashDecision::Restarted { on, left },
            Outcome::Killed => self.confirmed(on, next.failed_tests)?,
            other @ (Outcome::NotRun
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored) => CrashDecision::Undecided {
                on,
                why: format!("the next run came to {}", other.name()),
            },
        }))
    }

    /// A failing next run held to a fresh run that passes and a second stop that fails the next run again.
    fn confirmed(&self, on: String, failed: Vec<String>) -> Result<CrashDecision, RunnerError> {
        let fresh = self
            .session
            .control(&self.request(""), self.watch.cancel, Observing::Nothing)?
            .result;
        self.recorded("fresh", fresh.exit_code, fresh.outcome());
        if fresh.outcome() != Outcome::Survived {
            return Ok(CrashDecision::Undecided {
                on,
                why: "the test fails in a fresh scratch too, so the failure is not the stop's"
                    .to_owned(),
            });
        }
        let Some(kept) = self.crashed()? else {
            return Ok(CrashDecision::Undecided {
                on,
                why: "a second run did not stop at the call".to_owned(),
            });
        };
        let again = self
            .session
            .control_in(&self.request(""), &kept, self.watch.cancel)?;
        self.recorded("next", again.exit_code, again.outcome());
        Ok(if again.outcome() == Outcome::Killed {
            CrashDecision::Corrupt { on, failed }
        } else {
            CrashDecision::Undecided {
                on,
                why: "the next run failed once and passed after a second stop".to_owned(),
            }
        })
    }

    /// One execution, as the recording holds it.
    fn recorded(&self, stage: &str, exit_code: i32, outcome: Outcome) {
        self.watch.trace.crash_exec(crate::trace::CrashExecRecord {
            crash: self.mutant.display_id.to_string(),
            target: self.target.to_owned(),
            test: self.test.to_owned(),
            stage: stage.to_owned(),
            exit_code: i64::from(exit_code),
            outcome: outcome.name().to_owned(),
        });
    }
}

/// What a run says when the tree it crashes gave no baseline, which leaves a run asked for crashes short of assured.
fn unmeasured() -> Finding {
    Finding::new(
        FindingKind::NotMeasured,
        NOT_MEASURED,
        "the tree with every call that writes guarded did not build or passed no test with \
         nothing active, so no crash was put and nothing is claimed about any stop",
    )
}

/// The session every call that writes is guarded in, with its one run with nothing active.
fn prepared(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<Session, RunnerError> {
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
