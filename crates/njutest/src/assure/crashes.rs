// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stopping the process just after each call that writes, and asking whether the next run starts over what it left (ADR 0035).

use std::collections::BTreeMap;

use rust_mutants::catalog::Mutant;
use rust_mutants::outcome::Outcome;
use rust_mutants::sealed::bench::{After, Bench, Crashing};
use rust_mutants::sealed::record::Came as SealedCame;
use rust_mutants::session::{
    Asked, Kept, Left, Observing, Request as ExecRequest, SealedKept, Session, Stop,
};

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
        Ok(Prepared::Nothing(kept)) => {
            report.limitations.push(Limitation::new(
                crate::limitation::Limitation::CrashNoSite,
                "no measured file calls anything that writes, so there was nothing to stop after",
            ));
            for path in kept {
                notes.note("kept", &path.display().to_string())?;
            }
            return Ok(());
        }
        Ok(Prepared::Guarded(session)) => *session,
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
    let runner = rust_mutants::run::sealed_runner(&session)?;
    let bench = match &runner {
        Some(runner) => {
            let sealing = watch.trace.phase("crash-seal");
            let bench = session.bench(runner, watch.cancel)?;
            sealing.end();
            Some(bench)
        }
        None => None,
    };
    let records = sites(
        &session,
        request,
        Putting {
            watch,
            bench: bench.as_ref(),
        },
    )?;
    drop(bench);
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

/// What a crash is put with: the run's watch, and the sealed bench where the run seals.
#[derive(Clone, Copy)]
struct Putting<'a> {
    watch: Watch<'a>,
    bench: Option<&'a Bench<'a>>,
}

/// Which kinds of run a crash's decision rests on so far.
#[derive(Debug, Clone, Copy, Default)]
struct Rested {
    /// A sealed instance ran.
    sealed: bool,
    /// A native process ran.
    native: bool,
}

impl Rested {
    /// Whether the decision rests on at least one run, and every one a sealed instance.
    const fn sealed(self) -> bool {
        self.sealed && !self.native
    }
}

/// What every call that writes of the part comes to, one at a time; a stop that wrote into the tree leaves every later one undecided.
fn sites(
    session: &Session,
    request: &Request,
    putting: Putting<'_>,
) -> Result<Vec<CrashRecord>, RunnerError> {
    let watch = putting.watch;
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
        let (decision, sealed) = match rejected.get(mutant.id.as_str()) {
            Some(diagnostic) => {
                stepped(watch, mutant, CrashStep::Rejected);
                let decision = CrashDecision::NotPut {
                    diagnostic: crate::assure::run::first_line(diagnostic).replace(&root, "."),
                };
                (decision, false)
            }
            None if tainted => {
                stepped(watch, mutant, CrashStep::Tainted);
                let decision = CrashDecision::Undecided {
                    on: RULE.to_owned(),
                    why: "an earlier stop wrote into the tree under measurement, so every later \
                          run starts over what it left there"
                        .to_owned(),
                };
                (decision, false)
            }
            None => {
                let (decision, rested) = decided(session, mutant, putting)?;
                if written(session)? == untouched {
                    (decision, rested.sealed())
                } else {
                    tainted = true;
                    stepped(watch, mutant, CrashStep::Outside);
                    let decision = CrashDecision::Undecided {
                        on: RULE.to_owned(),
                        why: "the stopped test wrote outside its scratch, into the tree under \
                              measurement, where no next run could be told to start over it"
                            .to_owned(),
                    };
                    (decision, rested.sealed())
                }
            }
        };
        let record = CrashRecord {
            catalog_index: CatalogIndex::new(mutant.index),
            id: mutant.id.to_string(),
            display_id: mutant.display_id.to_string(),
            path: mutant.candidate.path.clone(),
            item: match session.item_of(mutant.index) {
                Some(item) => item.to_owned(),
                None => String::new(),
            },
            position: session.position(mutant).map(|at| crate::report::Position {
                line: at.line,
                column: at.byte_column,
                character_column: at.char_column,
            }),
            decision,
            sealed,
        };
        watch.trace.crash(record.clone());
        records.push(record);
    }
    Ok(records)
}

/// What a crash at one call comes to: the first test that reaches it, target by target in name order, stopped there and run again over what it left; and which kinds of run that rests on.
fn decided(
    session: &Session,
    mutant: &Mutant,
    putting: Putting<'_>,
) -> Result<(CrashDecision, Rested), RunnerError> {
    let watch = putting.watch;
    let ran = std::cell::Cell::new(Rested::default());
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
                putting,
                ran: &ran,
            };
            if let Some(decision) = on.decided()? {
                return Ok((decision, ran.get()));
            }
        }
    }
    if unnamed.is_empty() {
        return Ok((CrashDecision::Unreached, ran.get()));
    }
    let decision = CrashDecision::Undecided {
        on: unnamed.join(", "),
        why: "which of their tests reaches the call is not known, so a stop would stop every \
              test at once and tear what the others were writing"
            .to_owned(),
    };
    Ok((decision, ran.get()))
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
    /// It stopped at the call, and this is the scratch it left and what it left there.
    Stopped(Kept, Left),
    /// It passed without reaching the call's stop, so another test is asked.
    Passed,
    /// A process it started stopped at the call, and its own process ended with this status.
    Elsewhere(i32),
    /// It came to something else, which decides nothing either way.
    Other(Outcome, i32),
}

/// What one run of a test with the crash active came to, before what it left is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Came {
    /// It stopped at the call.
    Stopped,
    /// It passed without reaching the call's stop.
    Passed,
    /// A process it started stopped at the call, and its own process ended with this status.
    Elsewhere(i32),
    /// It came to something else.
    Other(Outcome, i32),
}

/// What the runtime's notice says of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Noticed {
    /// The engine verified the notice and the stop's status: the run's own process stopped at the call.
    Stop,
    /// Some process of the run published its notice, and the run's own process did not end with the stop's status.
    Published,
    /// No process of the run published it.
    Nothing,
}

impl Noticed {
    /// What `stop` and whether the notice was `published` say.
    const fn of(stop: Stop, published: bool) -> Self {
        if stop.noticed() {
            Self::Stop
        } else if published {
            Self::Published
        } else {
            Self::Nothing
        }
    }
}

/// What a run whose notice says `noticed`, that ended with `exit_code` and came to `outcome`, came to.
const fn came_to(noticed: Noticed, exit_code: i32, outcome: Outcome) -> Came {
    match noticed {
        Noticed::Stop => return Came::Stopped,
        Noticed::Published => return Came::Elsewhere(exit_code),
        Noticed::Nothing => {}
    }
    match outcome {
        Outcome::Survived => Came::Passed,
        other @ (Outcome::NotRun
        | Outcome::Killed
        | Outcome::StepLimitReached
        | Outcome::Waited
        | Outcome::Inconclusive
        | Outcome::Errored) => Came::Other(other, exit_code),
    }
}

/// One test a crash is put to.
struct Stopped<'a> {
    session: &'a Session,
    mutant: &'a Mutant,
    target: &'a str,
    test: &'a str,
    putting: Putting<'a>,
    ran: &'a std::cell::Cell<Rested>,
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
        let (result, kept) = self.session.exec_keeping(
            &self.request(self.mutant.id.as_str()),
            self.putting.watch.cancel,
        )?;
        let came = came_to(
            Noticed::of(kept.stop(), kept.notice().published()),
            result.exit_code,
            result.outcome(),
        );
        let left = if came == Came::Stopped {
            kept.left()?
        } else {
            Left::Named(Vec::new())
        };
        self.recorded(Recorded {
            stage: "crash",
            sealed: false,
            exit_code: Some(result.exit_code),
            outcome: result.outcome().name(),
            stop: kept.stop(),
            issued: Some(kept.notice()),
            left: &left,
            failed: &[],
        });
        Ok(match came {
            Came::Stopped => Ran::Stopped(kept, left),
            Came::Passed => Ran::Passed,
            Came::Elsewhere(exit_code) => Ran::Elsewhere(exit_code),
            Came::Other(outcome, exit_code) => Ran::Other(outcome, exit_code),
        })
    }

    /// What the test comes to after a stop at the call, or nothing where it did not stop there: in sealed instances, in one round, where the bench answers for the test at the call's guard (ADR 0046), and natively otherwise.
    fn decided(&self) -> Result<Option<CrashDecision>, RunnerError> {
        if let Some(bench) = self.putting.bench
            && let Some(kept) = self
                .session
                .crash_sealed(bench, &self.request(self.mutant.id.as_str()))?
        {
            return self.sealed(bench, &kept);
        }
        let on = self.on();
        let (kept, left) = match self.crashed()? {
            Ran::Stopped(kept, left) => (kept, left),
            Ran::Passed => return Ok(None),
            Ran::Elsewhere(exit_code) => {
                return Ok(Some(CrashDecision::Undecided {
                    on,
                    why: format!(
                        "a process the test started stopped at the call and the test's own \
                         process {}, so the stop is not one the next run's test made",
                        ended_with(exit_code)
                    ),
                }));
            }
            Ran::Other(outcome, exit_code) => {
                return Ok(Some(CrashDecision::Undecided {
                    on,
                    why: format!(
                        "the run came to {}, its process {}, and did not stop at the call",
                        outcome.name(),
                        ended_with(exit_code)
                    ),
                }));
            }
        };
        let left = match left {
            Left::Named(left) => left,
            Left::Unnamed(entry) => {
                return Ok(Some(CrashDecision::Undecided {
                    on,
                    why: unnamed(&entry),
                }));
            }
        };
        if left.is_empty() {
            return Ok(Some(CrashDecision::Unshared { on }));
        }
        let next = self
            .session
            .control_in(&self.request(""), &kept, self.putting.watch.cancel)?;
        self.recorded(Recorded {
            stage: "next",
            sealed: false,
            exit_code: Some(next.exit_code),
            outcome: next.outcome().name(),
            stop: Stop::none(),
            issued: None,
            left: &Left::Named(Vec::new()),
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
            .control(
                &self.request(""),
                self.putting.watch.cancel,
                Observing::Nothing,
            )?
            .result;
        self.recorded(Recorded {
            stage: "fresh",
            sealed: false,
            exit_code: Some(fresh.exit_code),
            outcome: fresh.outcome().name(),
            stop: Stop::none(),
            issued: None,
            left: &Left::Named(Vec::new()),
            failed: &fresh.failed_tests,
        });
        if fresh.outcome() != Outcome::Survived {
            return Ok(Round::Not(
                "the test fails in a fresh scratch too, so the failure is not the stop's",
            ));
        }
        let Ran::Stopped(kept, left) = self.crashed()? else {
            return Ok(Round::Not("a later run did not stop at the call"));
        };
        match left {
            Left::Named(left) if !left.is_empty() => {}
            Left::Named(_) => {
                return Ok(Round::Not(
                    "a later stop at the call left nothing for the next run",
                ));
            }
            Left::Unnamed(_) => {
                return Ok(Round::Not(
                    "a later stop at the call left an entry whose name is not text",
                ));
            }
        }
        let again = self
            .session
            .control_in(&self.request(""), &kept, self.putting.watch.cancel)?;
        self.recorded(Recorded {
            stage: "next",
            sealed: false,
            exit_code: Some(again.exit_code),
            outcome: again.outcome().name(),
            stop: Stop::none(),
            issued: None,
            left: &Left::Named(Vec::new()),
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

    /// What the test comes to in sealed instances: the crash, halted by the host where the runtime published its notice, and the next instance started from what it left, judged against the test's control; one round decides, since the same instance comes out the same every time (ADR 0046).
    fn sealed(
        &self,
        bench: &Bench<'_>,
        kept: &SealedKept,
    ) -> Result<Option<CrashDecision>, RunnerError> {
        let on = self.on();
        let came = match kept.ended() {
            Crashing::Halted => None,
            Crashing::Judged(sealed) => Some(SealedCame::of(sealed)),
        };
        let stopped = kept.stop().noticed();
        let left: Vec<String> = if stopped {
            kept.left().to_vec()
        } else {
            Vec::new()
        };
        self.recorded(Recorded {
            stage: "crash",
            sealed: true,
            exit_code: None,
            outcome: match came {
                Some(came) => came.name(),
                None => HALTED,
            },
            stop: kept.stop(),
            issued: Some(kept.notice()),
            left: &Left::Named(left.clone()),
            failed: &[],
        });
        if !stopped {
            return Ok(match came {
                Some(SealedCame::Passed) if !kept.notice().published() => None,
                Some(came) => Some(CrashDecision::Undecided {
                    on,
                    why: format!(
                        "the sealed instance came to {} and did not halt at the call",
                        came.name()
                    ),
                }),
                None => Some(CrashDecision::Undecided {
                    on,
                    why: "the sealed instance halted where its notice goes without publishing \
                          the notice it was issued"
                        .to_owned(),
                }),
            });
        }
        if left.is_empty() {
            return Ok(Some(CrashDecision::Unshared { on }));
        }
        let next = match self.session.next_sealed(bench, &self.request(""), kept)? {
            Some(After::Came(next)) => SealedCame::of(next),
            Some(After::Unstartable(why)) => {
                return Ok(Some(self.unstartable(on, &why)));
            }
            None => {
                return Ok(Some(CrashDecision::Undecided {
                    on,
                    why: "the test had no control to judge the next instance against".to_owned(),
                }));
            }
        };
        let failed = if next.detected() {
            vec![self.test.to_owned()]
        } else {
            Vec::new()
        };
        self.recorded(Recorded {
            stage: "next",
            sealed: true,
            exit_code: None,
            outcome: next.name(),
            stop: Stop::none(),
            issued: None,
            left: &Left::Named(Vec::new()),
            failed: &failed,
        });
        Ok(Some(after_sealed(next, (on, left, failed))))
    }

    /// What a stop whose next instance cannot start over what it left decides, having recorded that it could not: nothing either way, since what the tree then holds says nothing of whether the program could start over it.
    fn unstartable(&self, on: String, why: &str) -> CrashDecision {
        self.recorded(Recorded {
            stage: "next",
            sealed: true,
            exit_code: None,
            outcome: UNSTARTABLE,
            stop: Stop::none(),
            issued: None,
            left: &Left::Named(Vec::new()),
            failed: &[],
        });
        CrashDecision::Undecided {
            on,
            why: format!("what the stop left is no state an instance of the test starts in: {why}"),
        }
    }

    /// One execution, as the recording holds it, and what the crash's decision rests on.
    fn recorded(&self, run: Recorded<'_>) {
        let mut rested = self.ran.get();
        if run.sealed {
            rested.sealed = true;
        } else {
            rested.native = true;
        }
        self.ran.set(rested);
        self.putting
            .watch
            .trace
            .crash_exec(crate::trace::CrashExecRecord {
                crash: self.mutant.display_id.to_string(),
                target: self.target.to_owned(),
                test: self.test.to_owned(),
                stage: run.stage.to_owned(),
                sealed: run.sealed,
                exit_code: run.exit_code.map(i64::from),
                outcome: run.outcome.to_owned(),
                noticed: run.stop.noticed(),
                issued: run.issued.map(|notice| crate::trace::CrashNoticeRecord {
                    mutant: notice.mutant.clone(),
                    catalog: notice.catalog.clone(),
                    nonce: notice.nonce.clone(),
                    read: notice.read.clone(),
                }),
                left: match run.left {
                    Left::Named(left) => left.clone(),
                    Left::Unnamed(_) => Vec::new(),
                },
                unnamed: match run.left {
                    Left::Named(_) => None,
                    Left::Unnamed(entry) => Some(entry.clone()),
                },
                failed: run.failed.to_vec(),
            });
    }
}

/// What a sealed crash comes to once the next instance, started from what it `left`, came to `next` on the test `on`, which then `failed` or not: passed is `restarted`, a detection is `corrupt`, and an instance that established nothing is `undecided`.
fn after_sealed(
    next: SealedCame,
    (on, left, failed): (String, Vec<String>, Vec<String>),
) -> CrashDecision {
    match next {
        SealedCame::Passed => CrashDecision::Restarted { on, left },
        SealedCame::Panicked
        | SealedCame::Failed
        | SealedCame::Trapped
        | SealedCame::FuelExceeded
        | SealedCame::MemoryExceeded
        | SealedCame::Declined => CrashDecision::Corrupt { on, failed },
        doubted @ (SealedCame::ExitedEarly
        | SealedCame::StackOverflow
        | SealedCame::Refused
        | SealedCame::Unaccounted
        | SealedCame::Unmatched
        | SealedCame::SetAside) => CrashDecision::Undecided {
            on,
            why: format!(
                "the next sealed instance came to {}, which says nothing either way",
                doubted.name()
            ),
        },
    }
}

/// How a process that ended with `exit_code` ended, where the engine gives no status for one it ended itself or one a signal ended.
fn ended_with(exit_code: i32) -> String {
    if exit_code == rust_mutants::runner::EXIT_CODE_UNAVAILABLE {
        "ended with no exit status of its own".to_owned()
    } else {
        format!("ended with status {exit_code}")
    }
}

/// What a sealed crash instance the host halted where its runtime publishes the notice is recorded as.
pub const HALTED: &str = "halted";

/// What a sealed next instance that could not start over what the stop left is recorded as.
pub const UNSTARTABLE: &str = "unstartable";

/// Why a stop that left an entry whose name is not text is undecided: what it left cannot be named to the next run, or to anyone reading the report.
fn unnamed(entry: &str) -> String {
    format!(
        "the stop left an entry whose name is not text, {entry}, so what it left cannot be named"
    )
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
    sealed: bool,
    exit_code: Option<i32>,
    outcome: &'a str,
    stop: Stop,
    issued: Option<&'a rust_mutants::session::Notice>,
    left: &'a Left,
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

/// What a crash phase is prepared as.
enum Prepared {
    /// Discovery found no call that writes, so nothing was built or run, and the closed workspace kept these paths.
    Nothing(Vec<std::path::PathBuf>),
    /// The session every call that writes is guarded in, with its one run with nothing active.
    Guarded(Box<Session>),
}

/// The session every call that writes is guarded in, or nothing where discovery alone finds no such call, before anything is built.
fn prepared(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<Prepared, RunnerError> {
    let workspace = rust_mutants::workspace::Workspace::open(
        &request.root,
        rust_mutants::workspace::OpenOptions {
            trace: rust_mutants::trace::Recorder::disabled(),
            ..crate::assure::run::opening(request, environment)
        },
        watch.cancel,
    )?;
    let options = rust_mutants::session::PrepareOptions {
        operators: vec![RULE.to_owned()],
        ..crate::assure::run::preparing(request)?
    };
    if workspace
        .discover(&options, watch.cancel)?
        .mutants()
        .is_empty()
    {
        return Ok(Prepared::Nothing(workspace.close()?));
    }
    Ok(Prepared::Guarded(Box::new(
        workspace.prepare(&options, watch.cancel)?,
    )))
}

#[cfg(test)]
mod tests {
    use super::{Came, CrashDecision, Noticed, Outcome, SealedCame, after_sealed, came_to};
    use rust_mutants::instrument::CRASH_EXIT;

    #[test]
    fn a_next_sealed_instance_decides_the_crash_in_one_round_by_what_it_came_to() {
        for next in SealedCame::ALL {
            let decided = after_sealed(
                next,
                (
                    "pkg/test/it::t".to_owned(),
                    vec!["count".to_owned()],
                    vec!["t".to_owned()],
                ),
            );
            let expected = if next == SealedCame::Passed {
                matches!(decided, CrashDecision::Restarted { .. })
            } else if next.detected() {
                matches!(decided, CrashDecision::Corrupt { .. })
            } else {
                matches!(decided, CrashDecision::Undecided { .. })
            };
            assert!(
                expected,
                "{}: a pass restarts, a detection is corrupt, and anything else establishes \
                 nothing: {decided:?}",
                next.name()
            );
        }
    }

    #[test]
    fn a_notice_published_by_a_process_the_test_started_is_a_stop_elsewhere() {
        assert_eq!(
            came_to(Noticed::Stop, CRASH_EXIT, Outcome::Killed),
            Came::Stopped,
            "the test's own process stopped at the call"
        );
        assert_eq!(
            came_to(Noticed::Published, 0, Outcome::Survived),
            Came::Elsewhere(0),
            "a child published this run's notice and its parent tolerated the child's status: \
             a stop happened where the next run's test did not make it, which is not a pass"
        );
        assert_eq!(
            came_to(Noticed::Published, 101, Outcome::Killed),
            Came::Elsewhere(101),
            "and a parent that failed for losing its child stopped elsewhere as well"
        );
        assert_eq!(
            came_to(Noticed::Nothing, 0, Outcome::Survived),
            Came::Passed,
            "a run that published nothing passed without reaching the stop"
        );
    }
}
