// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says about the crashes a run put, read from the stream alone and decided again from it (ADR 0035).

use std::collections::BTreeMap;

use serde_json::Value;

/// One run of a test a crash was put to, in recording order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The target the test is in.
    pub target: String,
    /// The test.
    pub test: String,
    /// Which run: `crash`, `next` or `fresh`.
    pub stage: String,
    /// Whether it was a sealed instance rather than a process.
    pub sealed: bool,
    /// A process's exit status; nothing for a sealed instance.
    pub exit_code: Option<i64>,
    /// What the engine made of it: a native outcome, or for a sealed instance `halted` or what it came to against its control.
    pub outcome: String,
    /// Whether the runner says the runtime published the notice that it stopped at the call.
    pub noticed: bool,
    /// What the engine issued a `crash` run and read back, which the stop is decided on again; nothing on another run.
    pub issued: Option<Issued>,
    /// What a stopped run left.
    pub left: Vec<String>,
    /// The entry a stopped run left whose name is not text, where it left one, which leaves what it left unnamed.
    pub unnamed: Option<String>,
    /// What a next or fresh run failed.
    pub failed: Vec<String>,
}

/// What the engine issued one crashed run: the mutation, the catalog, the nonce, and the notice it read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    /// The mutation the run had active, in full.
    pub mutant: String,
    /// The catalog it was of.
    pub catalog: String,
    /// The nonce issued to this run alone.
    pub nonce: String,
    /// The notice as read, or nothing where none was published.
    pub read: Option<String>,
}

impl Issued {
    /// Whether the runtime published exactly the notice this run was issued: the schema, its own nonce, the catalog and the mutation.
    #[must_use]
    pub fn published(&self) -> bool {
        self.read.as_deref()
            == Some(
                format!(
                    "{NOTICE_SCHEMA}\t{}\t{}\t{}\n",
                    self.nonce, self.catalog, self.mutant
                )
                .as_str(),
            )
    }
}

/// The first field of every crash notice, written out again from the engine's contract rather than read from its code.
pub const NOTICE_SCHEMA: &str = "rust-mutants-crash-notice-v1";

/// One target a route asked, with its tests where the route names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The target.
    pub target: String,
    /// The tests that reach the call, or nothing where which of them does is not known.
    pub tests: Option<Vec<String>>,
}

/// One thing the runner recorded about a crash, in recording order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The compiler refused the crash.
    Rejected,
    /// An earlier stop wrote into the tree, so the crash was not run.
    Tainted,
    /// The targets and tests that reach the call, in the order they are asked.
    Route(Vec<Asked>),
    /// A stop of this crash wrote into the tree under measurement.
    Outside,
    /// One run of a test.
    Ran(Box<Run>),
    /// A step of a kind this audit does not know, which decides nothing it can check.
    Unread(String),
}

/// What a report says a crash came to, and what this audit decides it came to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Site {
    /// The crash a person types.
    pub crash: String,
    /// The decision's wire name.
    pub decision: String,
    /// The target and test it is about, where it names one.
    pub on: String,
    /// What the stop left, where it says.
    pub left: Vec<String>,
    /// What the next run failed, where it says.
    pub failed: Vec<String>,
    /// Whether the decision rests on at least one run and every run it rests on was a sealed instance.
    pub sealed: bool,
}

/// Every step a recording holds about the crashes, keyed by crash, in recording order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Crashed {
    /// Every step and the crash it is about, in recording order.
    pub steps: Vec<(String, Step)>,
}

/// A decision re-derived from a crash's steps, and whether a stop of it wrote into the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The decision.
    pub site: Site,
    /// Whether every later crash is left undecided because of this one.
    pub outside: bool,
}

/// A sequence of steps no run of the runner makes, which says the recording and the decision cannot be held to each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmade {
    /// What in the sequence no run makes.
    pub why: String,
}

/// The exit status of a test process a crash stopped, written out again from the engine's contract rather than read from its code.
pub const CRASH_EXIT: i64 = 93;

/// The rule every crash is put under, which an undecided crash names where no one test is to blame.
pub const RULE: &str = "crash-after-write";

/// Everything the recording says about the crashes.
#[must_use]
pub fn read(recorded: &crate::route::Checked<crate::schemas::RunnerLines>) -> Crashed {
    let mut crashed = Crashed::default();
    for event in recorded.events() {
        match event.get("type").and_then(Value::as_str) {
            Some("crash-exec") => {
                let Some((crash, record)) = event
                    .get("crash")
                    .and_then(|record| Some((text(record, "crash")?, record)))
                else {
                    crashed
                        .steps
                        .push((String::new(), Step::Unread("crash-exec".to_owned())));
                    continue;
                };
                let issued = match issued(record) {
                    Said::Nothing => None,
                    Said::Whole(issued) => Some(issued),
                    Said::Unwhole => {
                        crashed.steps.push((
                            crash,
                            Step::Unread("crash-exec without a whole `issued`".to_owned()),
                        ));
                        continue;
                    }
                };
                let step = match ran(record, issued) {
                    Some(run) => Step::Ran(Box::new(run)),
                    None => Step::Unread("crash-exec without a field it requires".to_owned()),
                };
                crashed.steps.push((crash, step));
            }
            Some("crash-step") => {
                let Some((crash, taken)) = event
                    .get("step")
                    .and_then(|record| Some((text(record, "crash")?, record.get("taken")?)))
                else {
                    crashed
                        .steps
                        .push((String::new(), Step::Unread("crash-step".to_owned())));
                    continue;
                };
                crashed.steps.push((crash, step(taken)));
            }
            Some(_) | None => {}
        }
    }
    crashed
}

/// One run of a crash as the runner writes it, or nothing where a field the schema requires is not there.
fn ran(record: &Value, issued: Option<Issued>) -> Option<Run> {
    Some(Run {
        target: text(record, "target")?,
        test: text(record, "test")?,
        stage: text(record, "stage")?,
        sealed: record.get("sealed")?.as_bool()?,
        exit_code: match record.get("exit_code")? {
            Value::Null => None,
            Value::Number(code) => Some(code.as_i64()?),
            Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => return None,
        },
        outcome: text(record, "outcome")?,
        noticed: record.get("noticed")?.as_bool()?,
        issued,
        left: texts(record, "left")?,
        unnamed: match record.get("unnamed")? {
            Value::Null => None,
            Value::String(entry) => Some(entry.clone()),
            Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => return None,
        },
        failed: texts(record, "failed")?,
    })
}

/// One step as the runner writes it.
fn step(taken: &Value) -> Step {
    match taken.get("kind").and_then(Value::as_str) {
        Some("rejected") => Step::Rejected,
        Some("tainted") => Step::Tainted,
        Some("outside") => Step::Outside,
        Some("route") => match routed_as(taken) {
            Some(asked) => Step::Route(asked),
            None => Step::Unread("route".to_owned()),
        },
        Some(other) => Step::Unread(other.to_owned()),
        None => Step::Unread("a step that names no kind".to_owned()),
    }
}

/// The targets a route step asks, or nothing where the step does not say them all in full: a target by name, and its tests as a list of names or `null`.
fn routed_as(taken: &Value) -> Option<Vec<Asked>> {
    taken
        .get("asked")?
        .as_array()?
        .iter()
        .map(|one| {
            let tests = match one.get("tests")? {
                Value::Null => None,
                Value::Array(named) => Some(
                    named
                        .iter()
                        .map(|name| name.as_str().map(ToOwned::to_owned))
                        .collect::<Option<Vec<_>>>()?,
                ),
                Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
                    return None;
                }
            };
            Some(Asked {
                target: one.get("target")?.as_str()?.to_owned(),
                tests,
            })
        })
        .collect()
}

/// What a crash-exec record says the engine issued its run.
enum Said {
    /// It says `null`: the run was not a crashed one.
    Nothing,
    /// It says in full.
    Whole(Issued),
    /// The field is missing or not whole, which is read as nothing it can be held to.
    Unwhole,
}

/// What a crash-exec record says the engine issued its run.
fn issued(record: &Value) -> Said {
    let Some(said) = record.get("issued") else {
        return Said::Unwhole;
    };
    if said.is_null() {
        return Said::Nothing;
    }
    let field = |key: &str| said.get(key).and_then(Value::as_str).map(ToOwned::to_owned);
    let read = match said.get("read") {
        Some(Value::Null) => None,
        Some(Value::String(read)) => Some(read.clone()),
        Some(Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) | None => {
            return Said::Unwhole;
        }
    };
    match (said, field("mutant"), field("catalog"), field("nonce")) {
        (Value::Null, _, _, _) => Said::Nothing,
        (Value::Object(_), Some(mutant), Some(catalog), Some(nonce)) => Said::Whole(Issued {
            mutant,
            catalog,
            nonce,
            read,
        }),
        (
            Value::Object(_)
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Array(_),
            _,
            _,
            _,
        ) => Said::Unwhole,
    }
}

/// Every place what the engine issued the crashed runs disagrees with the report or with itself: a run of a crash issued another mutation than the report's site names, or a nonce issued twice.
#[must_use]
pub fn issued_disagreements(
    ids: &BTreeMap<String, String>,
    crashed: &Crashed,
) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut nonces: BTreeMap<&str, &str> = BTreeMap::new();
    for (crash, step) in &crashed.steps {
        let Step::Ran(run) = step else {
            continue;
        };
        let Some(issued) = &run.issued else {
            continue;
        };
        if ids.get(crash).is_some_and(|id| *id != issued.mutant) {
            found.push((
                crash.clone(),
                "a run of this crash was issued another mutation than the report's site names"
                    .to_owned(),
            ));
        }
        if let Some(earlier) = nonces.insert(issued.nonce.as_str(), crash.as_str()) {
            found.push((
                crash.clone(),
                format!("a run of this crash carries the nonce already issued a run of {earlier}"),
            ));
        }
    }
    found
}

/// One site as a report writes it, or nothing where it is not the shape a run writes.
///
/// A decision says `on`, `left` and `failed` only where its kind carries them, and a site holds one it does not say as empty, as a site the steps decide does.
#[must_use]
pub fn site(record: &Value) -> Option<Site> {
    let decision = record.get("decision")?;
    Some(Site {
        crash: text(record, "display_id")?,
        decision: text(decision, "decision")?,
        on: said(decision, "on")?,
        left: said_list(decision, "left")?,
        failed: said_list(decision, "failed")?,
        sealed: record.get("sealed")?.as_bool()?,
    })
}

/// What the ordered steps of one crash decide, by the run's own steps and none of its code.
///
/// Every step must be one the runner takes at that point and the last must be where it decides, so a step dropped, added or reordered is refused rather than read around.
///
/// # Errors
/// The sequence is not one a run makes: `stained` is whether an earlier crash's stop wrote into the tree.
pub fn decided(crash: &str, steps: &[&Step], stained: bool) -> Result<Decided, Unmade> {
    let site = |decision: &str, on: &str| Site {
        crash: crash.to_owned(),
        decision: decision.to_owned(),
        on: on.to_owned(),
        ..Site::default()
    };
    let Some((first, rest)) = steps.split_first() else {
        return Err(unmade("no step of it was recorded"));
    };
    if stained {
        return match (first, rest) {
            (Step::Tainted, []) => Ok(Decided {
                site: site("undecided", RULE),
                outside: false,
            }),
            (Step::Rejected, []) => Ok(Decided {
                site: site("not-put", ""),
                outside: false,
            }),
            (
                Step::Tainted
                | Step::Rejected
                | Step::Route(_)
                | Step::Outside
                | Step::Ran(_)
                | Step::Unread(_),
                _,
            ) => Err(unmade(
                "an earlier stop wrote into the tree, and this crash was not left alone",
            )),
        };
    }
    let (decided, rest) = match first {
        Step::Rejected => (site("not-put", ""), rest),
        Step::Route(asked) => {
            let mut cursor = Cursor { rest };
            let decided = routed(crash, asked, &mut cursor)?;
            (decided, cursor.rest)
        }
        Step::Tainted => {
            return Err(unmade(
                "it was left undecided for an earlier stop and no earlier stop wrote into the tree",
            ));
        }
        Step::Outside | Step::Ran(_) | Step::Unread(_) => {
            return Err(unmade("its first step is neither a route nor a refusal"));
        }
    };
    let sealed = rests_on_sealed(steps);
    match rest {
        [] => Ok(Decided {
            site: Site { sealed, ..decided },
            outside: false,
        }),
        [Step::Outside] if decided.decision != "not-put" => Ok(Decided {
            site: Site {
                sealed,
                ..site("undecided", RULE)
            },
            outside: true,
        }),
        [..] => Err(unmade("a step was recorded after the one that decides it")),
    }
}

/// Whether `steps` hold at least one run and every run among them was a sealed instance, which is what a report's `sealed` says of the decision they rest on.
fn rests_on_sealed(steps: &[&Step]) -> bool {
    let runs: Vec<&Run> = steps
        .iter()
        .filter_map(|step| match step {
            Step::Ran(run) => Some(run.as_ref()),
            Step::Rejected | Step::Tainted | Step::Route(_) | Step::Outside | Step::Unread(_) => {
                None
            }
        })
        .collect();
    !runs.is_empty() && runs.iter().all(|run| run.sealed)
}

/// What a sealed crash instance the host halted where its runtime publishes the notice is recorded as, written out again from the runner's contract rather than read from its code.
pub const HALTED: &str = "halted";

/// What a sealed next instance that could not start over what the stop left is recorded as, which decides nothing either way.
pub const UNSTARTABLE: &str = "unstartable";

/// What a sealed instance comes to judged against its control, and of those the ones that detect, written out again from the engine's contract rather than read from its code.
pub const SEALED_OUTCOMES: [&str; 13] = [
    "passed",
    "panicked",
    "failed",
    "trapped",
    "fuel-exceeded",
    "memory-exceeded",
    "declined",
    "exited-early",
    "stack-overflow",
    "refused",
    "unaccounted",
    "unmatched",
    "set-aside",
];

/// The sealed outcomes that detect: a next instance that came to one of them failed over what the stop left.
pub const SEALED_DETECTIONS: [&str; 6] = [
    "panicked",
    "failed",
    "trapped",
    "fuel-exceeded",
    "memory-exceeded",
    "declined",
];

/// What a native run comes to, written out again from the engine's contract rather than read from its code.
pub const NATIVE_OUTCOMES: [&str; 7] = [
    "not_run",
    "killed",
    "survived",
    "step_limit_reached",
    "waited",
    "inconclusive",
    "errored",
];

/// The steps of one crash not yet read.
struct Cursor<'a, 'b> {
    rest: &'a [&'b Step],
}

impl<'b> Cursor<'_, 'b> {
    /// The next step, which must be a run of `stage` of this test.
    fn run(&mut self, target: &str, test: &str, stage: &str) -> Result<&'b Run, Unmade> {
        let Some((next, rest)) = self.rest.split_first() else {
            return Err(unmade(&format!(
                "the {stage} run of {target}::{test} the decision needs was not recorded"
            )));
        };
        match next {
            Step::Ran(run) if run.target == target && run.test == test && run.stage == stage => {
                if run.noticed != stopped(run) {
                    return Err(unmade(&format!(
                        "a {stage} run of {target}::{test} says the runtime {} its notice and \
                         what the engine issued and read back says otherwise",
                        if run.noticed {
                            "published"
                        } else {
                            "did not publish"
                        }
                    )));
                }
                if (stage == "crash") != run.issued.is_some() {
                    return Err(unmade(&format!(
                        "a {stage} run of {target}::{test} {} what the engine issued it",
                        if run.issued.is_some() {
                            "carries"
                        } else {
                            "does not carry"
                        }
                    )));
                }
                if run.unnamed.is_some() && (!stopped(run) || !run.left.is_empty()) {
                    return Err(unmade(&format!(
                        "a {stage} run of {target}::{test} names an entry left unnamed, which \
                         only a stop that named nothing it left can"
                    )));
                }
                if let Some(why) = unkind(run) {
                    return Err(unmade(&format!("a {stage} run of {target}::{test} {why}")));
                }
                self.rest = rest;
                Ok(run.as_ref())
            }
            Step::Ran(_)
            | Step::Rejected
            | Step::Tainted
            | Step::Route(_)
            | Step::Outside
            | Step::Unread(_) => Err(unmade(&format!(
                "a {stage} run of {target}::{test} was due and something else was recorded"
            ))),
        }
    }

    /// The next step, which must be a run of `stage` of this test, sealed as `sealed` says: every run after a crash's is of the same kind as the crash's.
    fn run_as(
        &mut self,
        (target, test, stage): (&str, &str, &str),
        sealed: bool,
    ) -> Result<&'b Run, Unmade> {
        let run = self.run(target, test, stage)?;
        if run.sealed != sealed {
            return Err(unmade(&format!(
                "a {stage} run of {target}::{test} is {} where the stop it follows was {}",
                kind(run.sealed),
                kind(sealed)
            )));
        }
        Ok(run)
    }
}

/// How a run is named by its kind.
const fn kind(sealed: bool) -> &'static str {
    if sealed { "sealed" } else { "native" }
}

/// What makes `run` no run of its kind the runner records, or nothing where it is one: a sealed instance has no exit status, no `fresh` run and one of the sealed outcomes, `halted` on a crash alone; a process has an exit status and a native outcome.
fn unkind(run: &Run) -> Option<&'static str> {
    if run.sealed {
        if run.exit_code.is_some() {
            return Some("is sealed and carries an exit status, which only a process has");
        }
        if run.stage == "fresh" {
            return Some("is a sealed fresh run, which one sealed round never makes");
        }
        let halted = run.outcome == HALTED && run.stage == "crash";
        let unstartable = run.outcome == UNSTARTABLE && run.stage == "next";
        if !halted && !unstartable && !SEALED_OUTCOMES.contains(&run.outcome.as_str()) {
            return Some("is sealed and names an outcome no sealed instance comes to");
        }
        return None;
    }
    if run.exit_code.is_none() {
        return Some("is native and carries no exit status");
    }
    if !NATIVE_OUTCOMES.contains(&run.outcome.as_str()) {
        return Some("is native and names an outcome no process comes to");
    }
    None
}

/// What the runs after a route decide: the first test that stopped at the call decides, a test that passed without stopping hands on to the next, and anything else is undecided.
fn routed(crash: &str, asked: &[Asked], cursor: &mut Cursor<'_, '_>) -> Result<Site, Unmade> {
    let site = |decision: &str, on: &str| Site {
        crash: crash.to_owned(),
        decision: decision.to_owned(),
        on: on.to_owned(),
        ..Site::default()
    };
    let mut unnamed = Vec::new();
    for reaches in asked {
        let Some(tests) = &reaches.tests else {
            unnamed.push(reaches.target.as_str());
            continue;
        };
        for test in tests {
            let on = format!("{}::{test}", reaches.target);
            let stop = cursor.run(&reaches.target, test, "crash")?;
            if !stopped(stop) {
                let passed = if stop.sealed { "passed" } else { "survived" };
                if stop.outcome == passed && !published(stop) {
                    continue;
                }
                return Ok(site("undecided", &on));
            }
            if stop.unnamed.is_some() {
                return Ok(site("undecided", &on));
            }
            if stop.left.is_empty() {
                return Ok(site("unshared", &on));
            }
            let next = cursor.run_as((&reaches.target, test, "next"), stop.sealed)?;
            if stop.sealed {
                return Ok(match sealed_next(next, test)? {
                    Next::Passed => Site {
                        left: stop.left.clone(),
                        ..site("restarted", &on)
                    },
                    Next::Detected => Site {
                        failed: next.failed.clone(),
                        ..site("corrupt", &on)
                    },
                    Next::Neither => site("undecided", &on),
                });
            }
            return Ok(match next.outcome.as_str() {
                "survived" => Site {
                    left: stop.left.clone(),
                    ..site("restarted", &on)
                },
                "killed" => {
                    if confirmed(cursor, &reaches.target, test, &next.failed)? {
                        Site {
                            failed: next.failed.clone(),
                            ..site("corrupt", &on)
                        }
                    } else {
                        site("undecided", &on)
                    }
                }
                _ => site("undecided", &on),
            });
        }
    }
    Ok(if unnamed.is_empty() {
        site("unreached", "")
    } else {
        site("undecided", &unnamed.join(", "))
    })
}

/// What a sealed next instance came to, which is all one sealed round needs.
enum Next {
    /// It passed over what the stop left.
    Passed,
    /// It detected something over it: the test failed.
    Detected,
    /// It established neither.
    Neither,
}

/// What the sealed next instance `next` of `test` came to, in the one round a sealed crash is decided in.
///
/// # Errors
/// A next instance whose failures are not exactly its test where it detected, or not none where it did not.
fn sealed_next(next: &Run, test: &str) -> Result<Next, Unmade> {
    let detected = SEALED_DETECTIONS.contains(&next.outcome.as_str());
    let failed: &[String] = if detected {
        std::slice::from_ref(&next.test)
    } else {
        &[]
    };
    if next.failed != failed || next.test != test {
        return Err(unmade(&format!(
            "a sealed next run of {test} came to {} and names {:?} as its failures",
            next.outcome, next.failed
        )));
    }
    Ok(if next.outcome == "passed" {
        Next::Passed
    } else if detected {
        Next::Detected
    } else {
        Next::Neither
    })
}

/// How many rounds confirm a corrupt native stop, written out again from the runner's contract rather than read from its code; a sealed stop needs none, since the same instance comes out the same every time.
pub const CONFIRMATIONS: usize = 3;

/// Whether a failing next run, with the stopped test among its failures, is held to [`CONFIRMATIONS`] rounds of a fresh run that passes and a later stop that leaves something and fails the next run over it the same way.
fn confirmed(
    cursor: &mut Cursor<'_, '_>,
    target: &str,
    test: &str,
    failed: &[String],
) -> Result<bool, Unmade> {
    if !failed.iter().any(|one| one == test) {
        return Ok(false);
    }
    for () in std::iter::repeat_n((), CONFIRMATIONS) {
        if cursor.run_as((target, test, "fresh"), false)?.outcome != "survived" {
            return Ok(false);
        }
        let again = cursor.run_as((target, test, "crash"), false)?;
        if !stopped(again) || again.unnamed.is_some() || again.left.is_empty() {
            return Ok(false);
        }
        let next = cursor.run_as((target, test, "next"), false)?;
        if next.outcome != "killed" || next.failed != failed {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether a run stopped at the call: for a process the stop's exit status, for a sealed instance the host's halt where the notice goes, and in either the runtime's notice that it made it.
fn stopped(run: &Run) -> bool {
    let ended = if run.sealed {
        run.outcome == HALTED
    } else {
        run.exit_code == Some(CRASH_EXIT)
    };
    ended && published(run)
}

/// Whether the runtime published the notice `run` was issued, in whatever process of the test reached the call.
fn published(run: &Run) -> bool {
    run.issued.as_ref().is_some_and(Issued::published)
}

/// Every place a report's crash sites and the recorded steps disagree, each with the crash it is about.
///
/// Each crash's steps decide it exactly, every recorded crash is a site of the report and every site is a recorded crash, and once a stop wrote into the tree every later crash is left undecided.
#[must_use]
pub fn disagreements(reported: &[Site], crashed: &Crashed) -> Vec<(String, String)> {
    let mut steps: Vec<(&str, Vec<&Step>)> = Vec::new();
    for (crash, step) in &crashed.steps {
        match steps.iter_mut().find(|(held, _)| *held == crash.as_str()) {
            Some((_, held)) => held.push(step),
            None => steps.push((crash.as_str(), vec![step])),
        }
    }
    let mut sites: BTreeMap<&str, &Site> = BTreeMap::new();
    let mut found = Vec::new();
    for site in reported {
        if sites.insert(site.crash.as_str(), site).is_some() {
            found.push((
                site.crash.clone(),
                "the report holds this crash twice".to_owned(),
            ));
        }
    }
    let mut stained = false;
    for (crash, held) in &steps {
        let derived = match decided(crash, held, stained) {
            Ok(derived) => derived,
            Err(unmade) => {
                found.push(((*crash).to_owned(), unmade.why));
                continue;
            }
        };
        stained |= derived.outside;
        match sites.remove(crash) {
            None => found.push((
                (*crash).to_owned(),
                format!(
                    "the recording decides {} and no site of the report holds it",
                    derived.site.decision
                ),
            )),
            Some(site) if *site != derived.site => found.push((
                (*crash).to_owned(),
                format!(
                    "the report says {} on {:?} and the recorded steps decide {} on {:?}",
                    site.decision, site.on, derived.site.decision, derived.site.on
                ),
            )),
            Some(_) => {}
        }
    }
    for crash in sites.keys() {
        found.push((
            (*crash).to_owned(),
            "the report holds this crash and the recording holds no step of it".to_owned(),
        ));
    }
    found
}

/// A sequence no run makes, and why.
fn unmade(why: &str) -> Unmade {
    Unmade {
        why: why.to_owned(),
    }
}

/// One string field, or nothing where it is not there or is not a string.
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

/// One list of strings, or nothing where it is not there or holds something that is not a string.
fn texts(value: &Value, key: &str) -> Option<Vec<String>> {
    value
        .get(key)?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToOwned::to_owned))
        .collect()
}

/// A string field only some decisions carry: absent is a decision that does not say it, held as empty; present and not a string is nothing.
fn said(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        None => Some(String::new()),
        Some(_) => text(value, key),
    }
}

/// A list field only some decisions carry: absent is a decision that does not say it, held as empty; present and not a list of strings is nothing.
fn said_list(value: &Value, key: &str) -> Option<Vec<String>> {
    match value.get(key) {
        None => Some(Vec::new()),
        Some(_) => texts(value, key),
    }
}
