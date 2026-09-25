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
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Faulted, crate::route::ReadError> {
    let mut faulted = Faulted::default();
    for event in crate::route::events(recorded, crate::schemas::Producer::Runner)? {
        if let Some(record) = event.get("beside") {
            faulted.besides.push(beside(record));
        }
        if let Some(record) = event.get("pair") {
            faulted.pairs.push(Pair {
                mutant: text(record, "mutant"),
                fault: text(record, "fault"),
                target: text(record, "target"),
                alone: text(record, "alone"),
                with: text(record, "with"),
            });
        }
        let kind = event.get("type").and_then(Value::as_str);
        if kind == Some("fault-route")
            && let Some(record) = event.get("route")
        {
            let reaching = record
                .get("reaching")
                .and_then(Value::as_array)
                .map(|targets| {
                    targets
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            faulted.routes.push((text(record, "fault"), reaching));
            continue;
        }
        if kind == Some("fault-attribution")
            && let Some(record) = event.get("attribution")
        {
            let flag = |key: &str| record.get(key).and_then(Value::as_bool);
            if flag("faulted") == Some(true)
                && flag("passed") == Some(true)
                && record.get("unfaulted").and_then(Value::as_str) == Some("did-not-write")
            {
                faulted.attributed.push(text(record, "fault"));
            }
            continue;
        }
        if kind == Some("fault-rejected")
            && let Some(record) = event.get("rejected")
        {
            faulted.rejected.push(text(record, "fault"));
            continue;
        }
        if event.get("type").and_then(Value::as_str) == Some("fault-control")
            && let Some(record) = event.get("control")
        {
            faulted.controls.push(Control {
                fault: text(record, "fault"),
                target: text(record, "target"),
                passed: record.get("passed").and_then(Value::as_bool) == Some(true),
            });
            continue;
        }
        let Some(record) = event.get("fault") else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("fault-exec") => faulted.execs.push(Exec {
                fault: text(record, "fault"),
                target: text(record, "target"),
                outcome: text(record, "outcome"),
                role: text(record, "role"),
            }),
            Some("fault") => faulted.sites.push(site(record)),
            _ => {}
        }
    }
    Ok(faulted)
}

/// One piece of evidence beside a fault as a report or a recording writes it.
#[must_use]
pub fn beside(record: &Value) -> Beside {
    Beside {
        mutant: text(record, "mutant"),
        fault: text(record, "fault"),
        target: text(record, "target"),
        failed: text(record, "failed"),
    }
}

/// One site as a report or a recording writes it.
#[must_use]
pub fn site(record: &Value) -> Site {
    let decision = record.get("decision").cloned().unwrap_or_default();
    Site {
        fault: text(record, "display_id"),
        decision: text(&decision, "decision"),
        by: decision
            .get("by")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
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
            let by = site.by.clone().unwrap_or_default();
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
        "unnoticed" => failed.map_or(Ok(()), |one| {
            Err(Contradiction::NotAllPassed {
                outcome: one.outcome.clone(),
            })
        }),
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

/// One string field, or the empty string where the recording does not carry it.
fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}
