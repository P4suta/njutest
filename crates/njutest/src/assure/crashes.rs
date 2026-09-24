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
use crate::trace::{CrashAsked, CrashStep};
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
    let records = sites(&session, request, watch)?;
    if session.catalog().mutants().is_empty() {
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

/// What every call that writes of the part comes to, one at a time; a stop that wrote into the tree leaves every later one undecided.
fn sites(
    session: &Session,
    request: &Request,
    watch: Watch<'_>,
) -> Result<Vec<CrashRecord>, RunnerError> {
    let rejected: BTreeMap<String, String> = session
        .rejections()
        .iter()
        .map(|rejection| (rejection.id.clone(), rejection.diagnostic.clone()))
        .collect();
    let root = request.root.display().to_string();
    let untouched = written(session)?;
    let mut tainted = false;
    let mut records = Vec::new();
    for mutant in session
        .catalog()
        .mutants()
        .iter()
        .filter(|mutant| request.shard.is_none_or(|shard| shard.holds(mutant.index)))
    {
        let decision = match rejected.get(mutant.id.as_str()) {
            Some(diagnostic) => {
                stepped(watch, mutant, CrashStep::Rejected);
                CrashDecision::NotPut {
                    diagnostic: crate::assure::run::first_line(diagnostic).replace(&root, "."),
                }
            }
            None if tainted => {
                stepped(watch, mutant, CrashStep::Tainted);
                CrashDecision::Undecided {
                    on: RULE.to_owned(),
                    why: "an earlier stop wrote into the tree under measurement, so every later \
                          run starts over what it left there"
                        .to_owned(),
                }
            }
            None => {
                let decision = decided(session, mutant, watch)?;
                if written(session)? == untouched {
                    decision
                } else {
                    tainted = true;
                    stepped(watch, mutant, CrashStep::Outside);
                    CrashDecision::Undecided {
                        on: RULE.to_owned(),
                        why: "the stopped test wrote outside its scratch, into the tree under \
                              measurement, where no next run could be told to start over it"
                            .to_owned(),
                    }
                }
            }
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
    Ok(records)
}

/// What a crash at one call comes to: the first test that reaches it, target by target in name order, stopped there and run again over what it left.
fn decided(
    session: &Session,
    mutant: &Mutant,
    watch: Watch<'_>,
) -> Result<CrashDecision, RunnerError> {
    let mut asked = session.route(mutant).asked();
    asked.sort_by(|one, other| one.target.cmp(&other.target));
    stepped(
        watch,
        mutant,
        CrashStep::Route {
            asked: asked
                .iter()
                .map(|reaches| CrashAsked {
                    target: reaches.target.clone(),
                    tests: match &reaches.tests {
                        Asked::Every => None,
                        Asked::These(tests) => Some(tests.clone()),
                    },
                })
                .collect(),
        },
    );
    let mut unnamed = Vec::new();
    for reaches in asked {
        let tests = match reaches.tests {
            Asked::Every => {
                unnamed.push(reaches.target);
                continue;
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
    if unnamed.is_empty() {
        return Ok(CrashDecision::Unreached);
    }
    Ok(CrashDecision::Undecided {
        on: unnamed.join(", "),
        why: "which of their tests reaches the call is not known, so a stop would stop every \
              test at once and tear what the others were writing"
            .to_owned(),
    })
}

/// Records one thing the run did about `mutant` besides running a test.
fn stepped(watch: Watch<'_>, mutant: &Mutant, taken: CrashStep) {
    watch.trace.crash_step(crate::trace::CrashStepRecord {
        crash: mutant.display_id.to_string(),
        taken,
    });
}

/// Every path of the tree under measurement that no longer matches what was instrumented.
fn written(session: &Session) -> Result<std::collections::BTreeSet<String>, RunnerError> {
    Ok(session
        .changes()?
        .iter()
        .map(|drift| drift.rel_path().to_owned())
        .collect())
}

/// What one run of a test with the crash active came to.
enum Ran {
    /// It stopped at the call, and this is the scratch it left.
    Stopped(Kept),
    /// It passed without reaching the call's stop, so another test is asked.
    Passed,
    /// It came to something else, which decides nothing either way.
    Other(Outcome, i32),
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

    /// What the test came to with the crash active.
    fn crashed(&self) -> Result<Ran, RunnerError> {
        let (result, kept) = self
            .session
            .exec_keeping(&self.request(self.mutant.id.as_str()), self.watch.cancel)?;
        if result.exit_code == CRASH_EXIT {
            let left = kept.left()?;
            self.recorded(Recorded {
                stage: "crash",
                exit_code: result.exit_code,
                outcome: result.outcome(),
                left: &left,
                failed: &[],
            });
            return Ok(Ran::Stopped(kept));
        }
        self.recorded(Recorded {
            stage: "crash",
            exit_code: result.exit_code,
            outcome: result.outcome(),
            left: &[],
            failed: &[],
        });
        Ok(match result.outcome() {
            Outcome::Survived => Ran::Passed,
            other @ (Outcome::NotRun
            | Outcome::Killed
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored) => Ran::Other(other, result.exit_code),
        })
    }

    /// What the test comes to after a stop at the call, or nothing where it did not stop there.
    fn decided(&self) -> Result<Option<CrashDecision>, RunnerError> {
        let on = self.on();
        let kept = match self.crashed()? {
            Ran::Stopped(kept) => kept,
            Ran::Passed => return Ok(None),
            Ran::Other(outcome, exit_code) => {
                return Ok(Some(CrashDecision::Undecided {
                    on,
                    why: format!(
                        "the run came to {} with status {exit_code} and did not stop at the call",
                        outcome.name()
                    ),
                }));
            }
        };
        let left = kept.left()?;
        if left.is_empty() {
            return Ok(Some(CrashDecision::Unshared { on }));
        }
        let next = self
            .session
            .control_in(&self.request(""), &kept, self.watch.cancel)?;
        self.recorded(Recorded {
            stage: "next",
            exit_code: next.exit_code,
            outcome: next.outcome(),
            left: &[],
            failed: &next.failed_tests,
        });
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

    /// A failing next run held to [`CONFIRMATIONS`] rounds, each a fresh run that passes and a second stop that leaves something and fails the next run over it the same way, with this test among the failures.
    fn confirmed(&self, on: String, failed: Vec<String>) -> Result<CrashDecision, RunnerError> {
        if !failed.iter().any(|one| one == self.test) {
            return Ok(CrashDecision::Undecided {
                on,
                why: "the next run failed other tests than this one".to_owned(),
            });
        }
        for () in std::iter::repeat_n((), CONFIRMATIONS) {
            if let Round::Not(why) = self.round(&failed)? {
                return Ok(CrashDecision::Undecided {
                    on,
                    why: why.to_owned(),
                });
            }
        }
        Ok(CrashDecision::Corrupt { on, failed })
    }

    /// One round of confirming that a stop, and not the test, fails the next run.
    fn round(&self, failed: &[String]) -> Result<Round, RunnerError> {
        let fresh = self
            .session
            .control(&self.request(""), self.watch.cancel, Observing::Nothing)?
            .result;
        self.recorded(Recorded {
            stage: "fresh",
            exit_code: fresh.exit_code,
            outcome: fresh.outcome(),
            left: &[],
            failed: &fresh.failed_tests,
        });
        if fresh.outcome() != Outcome::Survived {
            return Ok(Round::Not(
                "the test fails in a fresh scratch too, so the failure is not the stop's",
            ));
        }
        let Ran::Stopped(kept) = self.crashed()? else {
            return Ok(Round::Not("a later run did not stop at the call"));
        };
        if kept.left()?.is_empty() {
            return Ok(Round::Not(
                "a later stop at the call left nothing for the next run",
            ));
        }
        let again = self
            .session
            .control_in(&self.request(""), &kept, self.watch.cancel)?;
        self.recorded(Recorded {
            stage: "next",
            exit_code: again.exit_code,
            outcome: again.outcome(),
            left: &[],
            failed: &again.failed_tests,
        });
        Ok(
            if again.outcome() == Outcome::Killed && again.failed_tests == failed {
                Round::Reproduced
            } else {
                Round::Not("the next run did not fail the same way after a later stop")
            },
        )
    }

    /// One execution, as the recording holds it.
    fn recorded(&self, run: Recorded<'_>) {
        self.watch.trace.crash_exec(crate::trace::CrashExecRecord {
            crash: self.mutant.display_id.to_string(),
            target: self.target.to_owned(),
            test: self.test.to_owned(),
            stage: run.stage.to_owned(),
            exit_code: i64::from(run.exit_code),
            outcome: run.outcome.name().to_owned(),
            left: run.left.to_vec(),
            failed: run.failed.to_vec(),
        });
    }
}

/// How many times a failing next run is reproduced, each after a fresh run that passes, before the stop is called corrupt: a test that fails half its runs by itself passes all of them about once in 128.
pub const CONFIRMATIONS: usize = 3;

/// What one round of confirming a corrupt stop came to.
enum Round {
    /// The stop failed the next run the same way again.
    Reproduced,
    /// It did not, and why.
    Not(&'static str),
}

/// One execution of a test a crash was put to, as the recording is told it.
#[derive(Clone, Copy)]
struct Recorded<'a> {
    stage: &'a str,
    exit_code: i32,
    outcome: Outcome,
    left: &'a [String],
    failed: &'a [String],
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
