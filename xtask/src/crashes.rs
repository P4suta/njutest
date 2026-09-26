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
    /// The exit status.
    pub exit_code: i64,
    /// What the engine made of it.
    pub outcome: String,
    /// Whether the runner says the runtime published the notice that it stopped at the call.
    pub noticed: bool,
    /// What the engine issued a `crash` run and read back, which the stop is decided on again; nothing on another run.
    pub issued: Option<Issued>,
    /// What a stopped run left.
    pub left: Vec<String>,
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
    Ran(Run),
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
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Crashed, crate::route::ReadError> {
    let mut crashed = Crashed::default();
    for event in crate::route::events(recorded, crate::schemas::Producer::Runner)? {
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
                    Some(run) => Step::Ran(run),
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
    Ok(crashed)
}

/// One run of a crash as the runner writes it, or nothing where a field the schema requires is not there.
fn ran(record: &Value, issued: Option<Issued>) -> Option<Run> {
    Some(Run {
        target: text(record, "target")?,
        test: text(record, "test")?,
        stage: text(record, "stage")?,
        exit_code: record.get("exit_code")?.as_i64()?,
        outcome: text(record, "outcome")?,
        noticed: record.get("noticed")?.as_bool()?,
        issued,
        left: texts(record, "left")?,
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
        let Step::Ran(Run {
            issued: Some(issued),
            ..
        }) = step
        else {
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
    match rest {
        [] => Ok(Decided {
            site: decided,
            outside: false,
        }),
        [Step::Outside] if decided.decision != "not-put" => Ok(Decided {
            site: site("undecided", RULE),
            outside: true,
        }),
        [..] => Err(unmade("a step was recorded after the one that decides it")),
    }
}

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
                self.rest = rest;
                Ok(run)
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
                if stop.outcome == "survived" {
                    continue;
                }
                return Ok(site("undecided", &on));
            }
            if stop.left.is_empty() {
                return Ok(site("unshared", &on));
            }
            let next = cursor.run(&reaches.target, test, "next")?;
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

/// How many rounds confirm a corrupt stop, written out again from the runner's contract rather than read from its code.
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
        if cursor.run(target, test, "fresh")?.outcome != "survived" {
            return Ok(false);
        }
        let again = cursor.run(target, test, "crash")?;
        if !stopped(again) || again.left.is_empty() {
            return Ok(false);
        }
        let next = cursor.run(target, test, "next")?;
        if next.outcome != "killed" || next.failed != failed {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether a run stopped at the call: the stop's exit status, and the runtime's notice that it made it.
fn stopped(run: &Run) -> bool {
    run.exit_code == CRASH_EXIT && run.issued.as_ref().is_some_and(Issued::published)
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
