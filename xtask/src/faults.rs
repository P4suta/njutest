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
    /// Every fault the compiler refused.
    pub rejected: Vec<String>,
    /// Every fault a path the phase left written was tied to: run alone it wrote the path while its test passed, and its test alone without it did not.
    pub attributed: Vec<String>,
    /// Every site decision, in recording order.
    pub sites: Vec<Site>,
    /// Every survivor told apart beside a fault, in recording order.
    pub besides: Vec<Beside>,
    /// Every pair of runs behind that evidence, in recording order.
    pub pairs: Vec<Pair>,
    /// The fault records that lack a field their schema requires, by event type, which nothing is held to.
    pub unread: Vec<String>,
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
        "fault-attribution" => event.get("attribution").and_then(|record| {
            let flag = |key: &str| record.get(key).and_then(Value::as_bool);
            let fault = text(record, "fault")?;
            let tied = flag("faulted")?
                && flag("passed")?
                && text(record, "unfaulted")? == "did-not-write";
            if tied {
                faulted.attributed.push(fault);
            }
            Some(())
        }),
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
        }
    }
}

/// What the executions of a fault contradict about the decision the run gave it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Contradiction {
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

impl crate::error::Coded for Contradiction {
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
            | Self::Unknown { .. }
            | Self::NoticedByNoOne => crate::error::XtCode::FaultContradicted,
        }
    }
}

/// Whether the executions of one fault support the decision the run gave it.
///
/// # Errors
/// The [`Contradiction`] the executions hold.
pub fn supports(site: &Site, evidence: &Evidence<'_>) -> Result<(), Contradiction> {
    let (execs, controls) = (evidence.execs.as_slice(), evidence.controls.as_slice());
    let ran = !execs.is_empty();
    let failed = execs.iter().find(|one| one.outcome != "survived");
    let bounded = execs
        .iter()
        .any(|one| one.outcome == "waited" || one.outcome == "step_limit_reached");
    match site.decision.as_str() {
        "noticed" => {
            let Some(by) = site.by.clone() else {
                return Err(Contradiction::NoticedByNoOne);
            };
            let failed_as = |role: &str| {
                execs
                    .iter()
                    .any(|one| one.target == by && one.outcome == "killed" && one.role == role)
            };
            if !failed_as("first") {
                Err(Contradiction::NoticedWithoutFailure { by })
            } else if !controls.iter().any(|one| one.target == by && one.passed) {
                Err(Contradiction::NoticedUnconfirmed {
                    by,
                    missing: "the original code passing on that target",
                })
            } else if !failed_as("confirmation") {
                Err(Contradiction::NoticedUnconfirmed {
                    by,
                    missing: "the failure repeating when it was asked again",
                })
            } else {
                Ok(())
            }
        }
        "unnoticed" if !ran => Err(Contradiction::NothingRan {
            decision: site.decision.clone(),
        }),
        "unnoticed" => match failed {
            None => Ok(()),
            Some(one) => Err(Contradiction::NotAllPassed {
                outcome: one.outcome.clone(),
            }),
        },
        "undecided" if ran && failed.is_none() => {
            Err(Contradiction::UndecidedThoughPassed { runs: execs.len() })
        }
        "unreached" | "not-put" if ran => Err(Contradiction::RanThough {
            decision: site.decision.clone(),
            runs: execs.len(),
        }),
        "unreached"
            if evidence
                .reaching
                .is_none_or(|reaching| !reaching.is_empty()) =>
        {
            Err(Contradiction::UnreachedWithoutRoute)
        }
        "not-put" if !evidence.rejected => Err(Contradiction::NotPutWithoutRefusal),
        "waited" if !bounded => Err(Contradiction::WaitedWithoutBound),
        "unreached" | "not-put" | "waited" | "undecided" => Ok(()),
        other => Err(Contradiction::Unknown {
            decision: other.to_owned(),
        }),
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
