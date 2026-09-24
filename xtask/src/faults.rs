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

/// Every fault execution and every site decision a recording holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Faulted {
    /// Every execution, in recording order.
    pub execs: Vec<Exec>,
    /// Every control a confirmation asked, in recording order.
    pub controls: Vec<Control>,
    /// Every site decision, in recording order.
    pub sites: Vec<Site>,
}

/// Everything the recording says about the faults.
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Faulted, crate::route::ReadError> {
    let mut faulted = Faulted::default();
    for event in crate::route::events(recorded)? {
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
pub fn supports(site: &Site, execs: &[&Exec], controls: &[&Control]) -> Result<(), Contradiction> {
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
