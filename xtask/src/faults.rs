// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says about the faults a run put, read from the stream alone (ADR 0032).

use serde_json::Value;

/// One fault run against one target.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exec {
    /// The fault a person types.
    pub fault: String,
    /// The target it ran against.
    pub target: String,
    /// What the execution established, as the engine names outcomes.
    pub outcome: String,
    /// Which execution it was: `first`, or the `confirmation` after a failure.
    pub role: String,
}

/// Whether the original code passed on one target when a fault's failure on it was confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Control {
    /// The fault whose detection this confirms.
    pub fault: String,
    /// The target.
    pub target: String,
    /// Whether the target passed on the original code.
    pub passed: bool,
}

/// What the run said one fault site came to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Site {
    /// The fault a person types.
    pub fault: String,
    /// The decision's wire name.
    pub decision: String,
    /// The target that noticed, where one did.
    pub by: Option<String>,
}

/// A survivor a target told apart only under a fault, as a report or a recording writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct Beside {
    /// The survivor a person types.
    pub mutant: String,
    /// The fault a person types.
    pub fault: String,
    /// The target that told them apart.
    pub target: String,
    /// Which of the two runs failed.
    pub failed: String,
}

/// Every fault execution, site decision and piece of evidence beside a fault a recording holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Faulted {
    /// Every execution, in recording order.
    pub execs: Vec<Exec>,
    /// Every control a confirmation asked, in recording order.
    pub controls: Vec<Control>,
    /// Which targets reach each fault the run routed, by fault.
    pub routes: Vec<(String, Vec<String>)>,
    /// What each target's faulted baseline reached, by the fault catalog's index, in recording order.
    pub baselines: Vec<Baseline>,
    /// Every fault the compiler refused.
    pub rejected: Vec<String>,
    /// Every fault a path the phase left written was tied to: run alone it wrote the path while its test passed, and its test alone without it did not.
    pub attributed: Vec<String>,
    /// Every path the phase left written that a fault was run alone to tie, with whether that run tied it, in recording order.
    pub writes: Vec<(String, bool)>,
    /// Every site decision, in recording order.
    pub sites: Vec<Site>,
    /// Every survivor told apart beside a fault, in recording order.
    pub besides: Vec<Beside>,
    /// Every pair of runs behind that evidence, in recording order.
    pub pairs: Vec<Pair>,
    /// Every run again of a fault every reaching test passed, in recording order.
    pub fates: Vec<Fate>,
    /// The fault records that lack a field their schema requires, by event type, which nothing is held to.
    pub unread: Vec<String>,
}

/// One fault every reaching test passed, run again on one target with its runtime recording what became of the failures it made.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fate {
    /// The fault a person types.
    pub fault: String,
    /// The target it ran against.
    pub target: String,
    /// What the execution established, as the engine names outcomes.
    pub outcome: String,
    /// The failures it made, read and dropped, where the run had a record it could read.
    pub counted: Option<(u64, u64, u64)>,
}

impl Fate {
    /// Whether this run bears out that the failure went nowhere: it passed, made a failure, and dropped every one it made without anything reading it.
    #[must_use]
    pub fn absorbing(&self) -> bool {
        self.outcome == "survived"
            && self
                .counted
                .is_some_and(|(made, read, dropped)| made > 0 && read == 0 && dropped == made)
    }
}

/// One pair of runs of a target: the fault alone, then the survivor beside it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pair {
    /// The survivor a person types.
    pub mutant: String,
    /// The fault a person types.
    pub fault: String,
    /// The target both were put to.
    pub target: String,
    /// What the fault alone came to.
    pub alone: String,
    /// What the survivor beside it came to.
    pub with: String,
}

/// Which run of a pair failed, where exactly one did and the other passed.
#[must_use]
pub fn told(pair: &Pair) -> Option<&'static str> {
    match (pair.alone.as_str(), pair.with.as_str()) {
        ("survived", "killed") => Some("beside"),
        ("killed", "survived") => Some("alone"),
        _ => None,
    }
}

/// The evidence the pairs of one survivor and fault support: the first target in name order on which every pair, and at least two, said the same one run failed.
#[must_use]
pub fn derived(pairs: &[&Pair]) -> Option<(String, &'static str)> {
    let mut targets: Vec<&str> = pairs.iter().map(|pair| pair.target.as_str()).collect();
    targets.sort_unstable();
    targets.dedup();
    targets.into_iter().find_map(|target| {
        let runs: Vec<&&Pair> = pairs.iter().filter(|pair| pair.target == target).collect();
        let first = told(runs.first()?)?;
        (runs.len() >= 2 && runs.iter().all(|pair| told(pair) == Some(first)))
            .then(|| (target.to_owned(), first))
    })
}

/// What one target's faulted baseline reached, as a recording writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baseline {
    /// The target.
    pub target: String,
    /// Whether the target is a documentation one, which a route puts at every fault of its package whatever its own guards said.
    pub doc: bool,
    /// Every fault site its baseline reached, by the fault catalog's index.
    pub reached: Vec<u32>,
}

/// Everything the recording says about the faults.
#[must_use]
pub fn read(recorded: &crate::route::Checked<crate::schemas::RunnerLines>) -> Faulted {
    let mut faulted = Faulted::default();
    for event in recorded.events() {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            faulted
                .unread
                .push("an event that names no type".to_owned());
            continue;
        };
        if !held(&mut faulted, kind, event) {
            faulted.unread.push(kind.to_owned());
        }
    }
    faulted
}

/// Holds `event`, of type `kind`, in `faulted` where it is a fault record; whether it was read, which it is not where a field its schema requires is missing.
fn held(faulted: &mut Faulted, kind: &str, event: &Value) -> bool {
    let read = match kind {
        "beside" => event
            .get("beside")
            .and_then(beside)
            .map(|one| faulted.besides.push(one)),
        "beside-run" => event
            .get("pair")
            .and_then(pair)
            .map(|one| faulted.pairs.push(one)),
        "fault-route" => event
            .get("route")
            .and_then(|record| Some((text(record, "fault")?, texts(record, "reaching")?)))
            .map(|one| faulted.routes.push(one)),
        "fault-baseline" => event
            .get("baseline")
            .and_then(|record| {
                Some(Baseline {
                    target: text(record, "target")?,
                    doc: record.get("doc")?.as_bool()?,
                    reached: numbers(record, "reached")?,
                })
            })
            .map(|one| faulted.baselines.push(one)),
        "fault-attribution" => event.get("attribution").and_then(|record| {
            let flag = |key: &str| record.get(key).and_then(Value::as_bool);
            let fault = text(record, "fault")?;
            let tied = flag("faulted")?
                && flag("passed")?
                && text(record, "unfaulted")? == "did-not-write";
            faulted.writes.push((text(record, "path")?, tied));
            if tied {
                faulted.attributed.push(fault);
            }
            Some(())
        }),
        "fault-fate" => event
            .get("fate")
            .and_then(fate)
            .map(|one| faulted.fates.push(one)),
        "fault-rejected" => event
            .get("rejected")
            .and_then(|record| text(record, "fault"))
            .map(|fault| faulted.rejected.push(fault)),
        "fault-control" => event
            .get("control")
            .and_then(|record| {
                Some(Control {
                    fault: text(record, "fault")?,
                    target: text(record, "target")?,
                    passed: record.get("passed")?.as_bool()?,
                })
            })
            .map(|one| faulted.controls.push(one)),
        "fault-exec" => event
            .get("fault")
            .and_then(|record| {
                Some(Exec {
                    fault: text(record, "fault")?,
                    target: text(record, "target")?,
                    outcome: text(record, "outcome")?,
                    role: text(record, "role")?,
                })
            })
            .map(|one| faulted.execs.push(one)),
        "fault" => event
            .get("fault")
            .and_then(site)
            .map(|one| faulted.sites.push(one)),
        _ => Some(()),
    };
    read.is_some()
}

/// One piece of evidence beside a fault as a report or a recording writes it, or nothing where a field it requires is missing.
#[must_use]
pub fn beside(record: &Value) -> Option<Beside> {
    Some(Beside {
        mutant: text(record, "mutant")?,
        fault: text(record, "fault")?,
        target: text(record, "target")?,
        failed: text(record, "failed")?,
    })
}

/// One run again as the recording writes it, or nothing where a field it requires is missing.
fn fate(record: &Value) -> Option<Fate> {
    let counted = match record.get("fate")? {
        Value::Null => None,
        counts => {
            let count = |key: &str| counts.get(key).and_then(Value::as_u64);
            Some((count("made")?, count("read")?, count("dropped")?))
        }
    };
    Some(Fate {
        fault: text(record, "fault")?,
        target: text(record, "target")?,
        outcome: text(record, "outcome")?,
        counted,
    })
}

/// One pair of runs as the recording writes it, or nothing where a field it requires is missing.
fn pair(record: &Value) -> Option<Pair> {
    Some(Pair {
        mutant: text(record, "mutant")?,
        fault: text(record, "fault")?,
        target: text(record, "target")?,
        alone: text(record, "alone")?,
        with: text(record, "with")?,
    })
}

/// One site as a report or a recording writes it, or nothing where it is not the shape a run writes.
///
/// Only a decision that a target noticed carries `by`, so a site holds its absence as no target named.
#[must_use]
pub fn site(record: &Value) -> Option<Site> {
    let decision = record.get("decision")?;
    let by = match decision.get("by") {
        None => None,
        Some(by) => Some(by.as_str()?.to_owned()),
    };
    Some(Site {
        fault: text(record, "display_id")?,
        decision: text(decision, "decision")?,
        by,
    })
}

/// Everything a recording holds about one fault.
#[derive(Debug, Clone, Default)]
pub struct Evidence<'a> {
    /// Every execution of it.
    pub execs: Vec<&'a Exec>,
    /// Every control its confirmations asked.
    pub controls: Vec<&'a Control>,
    /// The targets that reach it, where the run routed it.
    pub reaching: Option<&'a [String]>,
    /// Whether the compiler refused it.
    pub rejected: bool,
    /// Every run again of it with a record of where its failures went.
    pub fates: Vec<&'a Fate>,
}

impl Faulted {
    /// Everything this recording holds about `fault`.
    #[must_use]
    pub fn evidence(&self, fault: &str) -> Evidence<'_> {
        Evidence {
            execs: self.execs.iter().filter(|one| one.fault == fault).collect(),
            controls: self
                .controls
                .iter()
                .filter(|one| one.fault == fault)
                .collect(),
            reaching: self
                .routes
                .iter()
                .find(|(routed, _)| routed == fault)
                .map(|(_, reaching)| reaching.as_slice()),
            rejected: self.rejected.iter().any(|one| one == fault),
            fates: self.fates.iter().filter(|one| one.fault == fault).collect(),
        }
    }
}

/// What the executions of a fault contradict about the decision the run gave it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FaultContradictionError {
    /// The run says a target noticed it and names none.
    #[error("the run says a target noticed it, and names no target")]
    NoticedByNoOne,
    /// A target was named as noticing and no execution failed on it.
    #[error(
        "the run says {by} noticed it, and the recording holds no execution of it that failed on {by}"
    )]
    NoticedWithoutFailure {
        /// The target named.
        by: String,
    },
    /// A target was named as noticing and the failure was never confirmed: the original code was not seen to pass, or the failure did not repeat.
    #[error(
        "the run says {by} noticed it, and the recording does not hold what confirms that: {missing}"
    )]
    NoticedUnconfirmed {
        /// The target named.
        by: String,
        /// What is missing.
        missing: &'static str,
    },
    /// A decision that rests on executions has none.
    #[error("the run says it is {decision}, and the recording holds no execution of it")]
    NothingRan {
        /// The decision given.
        decision: String,
    },
    /// Nothing is said to have noticed a fault that an execution did not pass.
    #[error(
        "the run says every test that reached it passed, and an execution of it came to {outcome}"
    )]
    NotAllPassed {
        /// What the execution that did not pass came to.
        outcome: String,
    },
    /// A fault every execution of which passed is said to be undecided.
    #[error("the run says it could not decide it, and every one of its {runs} execution(s) passed")]
    UndecidedThoughPassed {
        /// How many executions the recording holds.
        runs: usize,
    },
    /// A fault said never to have run ran.
    #[error("the run says it was {decision}, and the recording holds {runs} execution(s) of it")]
    RanThough {
        /// The decision given.
        decision: String,
        /// How many executions the recording holds.
        runs: usize,
    },
    /// A bound is said to have expired and no execution was stopped by one.
    #[error(
        "the run says a bound expired on it, and no execution of it the recording holds was stopped by one"
    )]
    WaitedWithoutBound,
    /// A fault said to be reached by nothing whose route the recording does not hold as reaching nothing.
    #[error(
        "the run says nothing reached it, and the recording holds no route of it that reached nothing"
    )]
    UnreachedWithoutRoute,
    /// A fault said to have been absorbed where a target that reached it has no run again that bears that out.
    #[error(
        "the run says the failure it made went nowhere, and the recording holds no run again of it on {target} that passed dropping every failure it made unread"
    )]
    AbsorbedUnborne {
        /// The reaching target without such a run.
        target: String,
    },
    /// A fault said to be unnoticed whose every reaching target has a run again that bears out that it was absorbed.
    #[error(
        "the run says something read the failure it made, and every target that reached it has a run again that dropped every failure unread"
    )]
    UnnoticedThoughAbsorbed,
    /// A fault said to be unnoticed or absorbed that a target reaching it was never asked about.
    #[error(
        "the run says every test that reached it passed with the call failing, and the recording holds no run of it on {target}, which reaches it"
    )]
    UnnoticedUnasked {
        /// The first reaching target, in name order, with no run of it.
        target: String,
    },
    /// A fault said to be noticed by a target where the judging, which stops at the first target that notices in name order, would have stopped earlier or never asked one before it.
    #[error(
        "the run says {by} noticed it, and {first} comes before it in name order and noticed it too or was never asked"
    )]
    NoticedNotFirst {
        /// The target named.
        by: String,
        /// The target before it the judging would have stopped at or had to ask.
        first: String,
    },
    /// A fault said not to have been put that the recording holds no refusal of.
    #[error("the run says the compiler refused it, and the recording holds no refusal of it")]
    NotPutWithoutRefusal,
    /// A decision no fault can come to.
    #[error("{decision:?} is no decision a fault can come to")]
    Unknown {
        /// The decision given.
        decision: String,
    },
}

impl crate::error::Coded for FaultContradictionError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::NoticedWithoutFailure { .. }
            | Self::NoticedUnconfirmed { .. }
            | Self::NothingRan { .. }
            | Self::NotAllPassed { .. }
            | Self::UndecidedThoughPassed { .. }
            | Self::RanThough { .. }
            | Self::WaitedWithoutBound
            | Self::UnreachedWithoutRoute
            | Self::NotPutWithoutRefusal
            | Self::AbsorbedUnborne { .. }
            | Self::UnnoticedThoughAbsorbed
            | Self::UnnoticedUnasked { .. }
            | Self::NoticedNotFirst { .. }
            | Self::Unknown { .. }
            | Self::NoticedByNoOne => crate::error::XtCode::FaultContradicted,
        }
    }
}

/// Whether the executions of one fault support the decision the run gave it.
///
/// # Errors
/// The [`FaultContradictionError`] the executions hold.
pub fn supports(site: &Site, evidence: &Evidence<'_>) -> Result<(), FaultContradictionError> {
    let (execs, controls) = (evidence.execs.as_slice(), evidence.controls.as_slice());
    let asked: Vec<&&Exec> = execs.iter().filter(|one| one.role == FIRST).collect();
    let ran = !asked.is_empty();
    let failed = asked.iter().find(|one| one.outcome != "survived");
    let bounded = asked
        .iter()
        .any(|one| one.outcome == "waited" || one.outcome == "step_limit_reached");
    match site.decision.as_str() {
        "noticed" => {
            let Some(by) = site.by.clone() else {
                return Err(FaultContradictionError::NoticedByNoOne);
            };
            if let Some(first) = before(evidence, &by) {
                return Err(FaultContradictionError::NoticedNotFirst { by, first });
            }
            let failed_as = |role: &str| {
                execs
                    .iter()
                    .any(|one| one.target == by && one.outcome == "killed" && one.role == role)
            };
            if !failed_as("first") {
                Err(FaultContradictionError::NoticedWithoutFailure { by })
            } else if !controls.iter().any(|one| one.target == by && one.passed) {
                Err(FaultContradictionError::NoticedUnconfirmed {
                    by,
                    missing: "the original code passing on that target",
                })
            } else if !failed_as("confirmation") {
                Err(FaultContradictionError::NoticedUnconfirmed {
                    by,
                    missing: "the failure repeating when it was asked again",
                })
            } else {
                Ok(())
            }
        }
        "unnoticed" | "absorbed" if !ran => Err(FaultContradictionError::NothingRan {
            decision: site.decision.clone(),
        }),
        "unnoticed" | "absorbed" => match (failed, unasked(evidence), site.decision.as_str()) {
            (Some(one), _, _) => Err(FaultContradictionError::NotAllPassed {
                outcome: one.outcome.clone(),
            }),
            (None, Some(target), _) => Err(FaultContradictionError::UnnoticedUnasked { target }),
            (None, None, "absorbed") => match unborne(evidence) {
                Some(target) => Err(FaultContradictionError::AbsorbedUnborne { target }),
                None => Ok(()),
            },
            (None, None, _) if absorbed_everywhere(evidence) => {
                Err(FaultContradictionError::UnnoticedThoughAbsorbed)
            }
            (None, None, _) => Ok(()),
        },
        "undecided" if ran && failed.is_none() => {
            Err(FaultContradictionError::UndecidedThoughPassed { runs: execs.len() })
        }
        "unreached" | "not-put" if !execs.is_empty() => Err(FaultContradictionError::RanThough {
            decision: site.decision.clone(),
            runs: execs.len(),
        }),
        "unreached"
            if evidence
                .reaching
                .is_none_or(|reaching| !reaching.is_empty()) =>
        {
            Err(FaultContradictionError::UnreachedWithoutRoute)
        }
        "not-put" if !evidence.rejected => Err(FaultContradictionError::NotPutWithoutRefusal),
        "waited" if !bounded => Err(FaultContradictionError::WaitedWithoutBound),
        "unreached" | "not-put" | "waited" | "undecided" => Ok(()),
        other => Err(FaultContradictionError::Unknown {
            decision: other.to_owned(),
        }),
    }
}

/// The role of the execution that asks a target about a fault, which is the only one a decision about the suite rests on: a confirmation asks again, and an attribution run ties a write rather than asking.
const FIRST: &str = "first";

/// The first target that reached a fault, in name order, that no execution asked, where the recording holds its route.
fn unasked(evidence: &Evidence<'_>) -> Option<String> {
    let mut targets: Vec<&String> = evidence.reaching?.iter().collect();
    targets.sort();
    targets
        .into_iter()
        .find(|target| !asked_on(evidence, target))
        .cloned()
}

/// The first target reaching a fault before `by` in name order that the judging would have stopped at, because it noticed the fault too, or had to ask and did not, where the recording holds the route.
fn before(evidence: &Evidence<'_>, by: &str) -> Option<String> {
    let mut targets: Vec<&String> = evidence
        .reaching?
        .iter()
        .filter(|target| target.as_str() < by)
        .collect();
    targets.sort();
    targets
        .into_iter()
        .find(|target| !asked_on(evidence, target) || noticed_on(evidence, target))
        .cloned()
}

/// Whether an execution asked `target` about the fault.
fn asked_on(evidence: &Evidence<'_>, target: &str) -> bool {
    evidence
        .execs
        .iter()
        .any(|one| one.role == FIRST && one.target == target)
}

/// Whether `target` noticed the fault: it failed under it, passed on the unchanged program, and failed under it again.
fn noticed_on(evidence: &Evidence<'_>, target: &str) -> bool {
    let failed_as = |role: &str| {
        evidence
            .execs
            .iter()
            .any(|one| one.target == target && one.outcome == "killed" && one.role == role)
    };
    failed_as(FIRST)
        && evidence
            .controls
            .iter()
            .any(|one| one.target == target && one.passed)
        && failed_as("confirmation")
}

/// The first target that reached a fault, in name order, with no run again that bears out that its failure went nowhere, or the empty name where the recording holds no route.
fn unborne(evidence: &Evidence<'_>) -> Option<String> {
    let Some(reaching) = evidence.reaching else {
        return Some(String::new());
    };
    if reaching.is_empty() {
        return Some(String::new());
    }
    let mut targets: Vec<&String> = reaching.iter().collect();
    targets.sort();
    targets
        .into_iter()
        .find(|target| {
            !evidence
                .fates
                .iter()
                .any(|fate| &fate.target == *target && fate.absorbing())
        })
        .cloned()
}

/// Whether every target that reached a fault has a run again that bears out that its failure went nowhere.
fn absorbed_everywhere(evidence: &Evidence<'_>) -> bool {
    unborne(evidence).is_none()
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

/// One list of whole numbers, or nothing where it is not there or holds something that is not one.
fn numbers(value: &Value, key: &str) -> Option<Vec<u32>> {
    value.get(key)?.as_array()?.iter().map(small).collect()
}

/// One whole number a fault catalog can index with, or nothing where it is not one.
#[must_use]
pub fn small(item: &Value) -> Option<u32> {
    match u32::try_from(item.as_u64()?) {
        Ok(small) => Some(small),
        Err(_too_large) => None,
    }
}
