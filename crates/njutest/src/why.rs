// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything a recording says stands behind one claim, from what proposed it to what decided it.
//!
//! The value only.
//! What a person reads is a separate job, and the line between them is that this knows nothing about how it is shown and a page invents nothing this does not carry.

use crate::report::faults::FaultDecision;
use crate::report::{Decided, SeamDecision};
use crate::trace::{Event, Payload, Read};

/// What a run can be asked about.
///
/// Both halves of the product mint sixty-four hex characters in separate identity domains, so the name a person types does not say on its own which was meant.
/// Asking is how the caller says, and answering about the other one would be answering a question nobody put.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// One mutation of the source.
    Mutation(String),
    /// One question about one exchange on one seam.
    Seam(String),
    /// One call a `?` asks about, failed by a fault.
    Fault(String),
}

impl Claim {
    /// The identity, whichever kind this is.
    #[must_use]
    pub fn named(&self) -> &str {
        match self {
            Self::Mutation(id) | Self::Seam(id) | Self::Fault(id) => id,
        }
    }
}

/// What a recording says stands behind one claim.
///
/// Three answers rather than two, and the third is the reason this is an enum.
/// A run that kept no recording establishes nothing about why anything happened; a recording that does not name the claim establishes that the claim is not in it.
/// Telling a reader "nothing stands behind this" about the first would be this command concluding from how it was measured —
/// the defect it exists to explain, arriving in the explanation.
///
/// The two are acted on differently.
/// One is a missing `--trace=`.
/// The other is a name typed wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The run kept no recording, so it says nothing about anything.
    NotRecorded,
    /// The recording does not name this claim, and names this many others.
    Unknown {
        /// How many claims it does name, so a reader can tell a typo from an empty run.
        recorded: usize,
    },
    /// What the recording says happened to it, in the order it happened.
    Followed(Chain),
}

/// One claim's causal chain, with the answer it came to.
///
/// The claim and its decision travel in one value: a mutation cannot come to a seam's decision, and three fields where two of them can disagree is the shape this project spends its time removing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chain {
    /// A mutation of the source.
    Mutation {
        /// Its identity.
        id: String,
        /// What each stage of the run did with it.
        steps: Vec<Step>,
        /// What the run finally established.
        came_to: Decided,
    },
    /// A question about one exchange on one seam.
    Seam {
        /// Its identity.
        id: String,
        /// What each stage of the run did with it.
        steps: Vec<Step>,
        /// What the run finally established.
        came_to: SeamDecision,
    },
    /// A call a `?` asks about, failed by a fault.
    Fault {
        /// Its identity.
        id: String,
        /// What each target did with the call failing.
        steps: Vec<Step>,
        /// What the run finally established.
        came_to: FaultDecision,
    },
}

impl Chain {
    /// The steps, whichever kind this is.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        match self {
            Self::Mutation { steps, .. } | Self::Seam { steps, .. } | Self::Fault { steps, .. } => {
                steps
            }
        }
    }
}

/// One thing a run did on the way to deciding a claim.
///
/// A closed set with no timestamps in it.
/// *Why did this come out this way* and *why did this take four minutes* are two questions, and `njutest trace` owns the second — it names the slowest work, which is what somebody making a run shorter is after.
/// A page carrying durations would grow a column nobody reads on every chain where each step is instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// An exchange went past a seam, which is what licenses a question about it.
    Observed {
        /// The seam it went past.
        capability: String,
        /// Where it fell in the order.
        seq: u64,
        /// How much of it was read, and what that reading found.
        read: Read,
    },
    /// A routing decided who could have noticed it.
    Routed {
        /// How the route was decided.
        granularity: rust_mutants::session::Granularity,
        /// The targets that could have noticed it.
        reaching: Vec<String>,
        /// The targets a proof removed, each beside the proof.
        discharged: Vec<(String, String)>,
        /// What widened the route past what the measurement supported.
        fallback: Option<rust_mutants::session::Fallback>,
    },
    /// It was put to a target, and the target answered.
    Asked {
        /// The target.
        target: String,
        /// What the target answered.
        outcome: String,
    },
    /// It was put to one test alone, sealed, and the instance came to this.
    Sealed {
        /// The target whose sealed module ran the test.
        target: String,
        /// The test.
        test: String,
        /// What the execution came to.
        came_to: String,
    },
    /// A fault every reaching test passed was put to a target again, with the target's answer and what its runtime recorded became of the failures it made.
    Fated {
        /// The target.
        target: String,
        /// What the target answered.
        outcome: String,
        /// What the record counted, where there was one it could read.
        fate: Option<rust_mutants::fate::Fate>,
    },
    /// A target told it from the original only with the call at its own site failing beside it: evidence it is no equivalence, and never a kill (ADR 0032 decision 6).
    ObservableUnderFault {
        /// The fault at its own call.
        fault: String,
        /// The target that told it apart.
        target: String,
    },
    /// It was read back from an earlier run rather than established here.
    ReadBack {
        /// The run it came from.
        run: String,
    },
    /// A fault was put to the suite and the suite answered.
    Put {
        /// What the fault asked the seam to do.
        rule: crate::wire::rule::Rule,
        /// What the run established.
        decision: SeamDecision,
    },
}

/// What `events` say stands behind `claim`.
///
/// `recording` being empty is not the same as a recording that holds nothing:
/// the caller says which it has by handing over `None`, because a reader of this value cannot tell an absent file from an empty one and would be told the same thing either way.
#[must_use]
pub fn why(claim: &Claim, recording: Option<&[Event]>) -> Why {
    let Some(events) = recording else {
        return Why::NotRecorded;
    };
    match claim {
        Claim::Mutation(id) => mutation(id, events),
        Claim::Seam(id) => seam(id, events),
        Claim::Fault(id) => fault(id, events),
    }
}

/// The chain of one fault: every target it was put to, then what the run said it came to.
fn fault(id: &str, events: &[Event]) -> Why {
    let mut steps = Vec::new();
    let mut came_to = None;
    let mut recorded = std::collections::BTreeSet::new();
    for event in events {
        match &event.payload {
            Payload::FaultExec { fault } if fault.fault == id => steps.push(Step::Asked {
                target: fault.target.clone(),
                outcome: fault.outcome.clone(),
            }),
            Payload::FaultFate { fate } if fate.fault == id => steps.push(Step::Fated {
                target: fate.target.clone(),
                outcome: fate.outcome.clone(),
                fate: fate.fate,
            }),
            Payload::Fault { fault } => {
                recorded.insert(fault.display_id.as_str());
                if fault.display_id == id {
                    came_to = Some(fault.decision.clone());
                }
            }
            Payload::RunStart { .. }
            | Payload::FaultControl { .. }
            | Payload::FaultAttribution { .. }
            | Payload::FaultWrites { .. }
            | Payload::FaultFate { .. }
            | Payload::FaultRoute { .. }
            | Payload::FaultBaseline { .. }
            | Payload::FaultRejected { .. }
            | Payload::PhaseStart { .. }
            | Payload::PhaseEnd { .. }
            | Payload::Exec { .. }
            | Payload::Progress { .. }
            | Payload::Artifact { .. }
            | Payload::Route { .. }
            | Payload::MutantExec { .. }
            | Payload::SealedExec { .. }
            | Payload::FaultExec { .. }
            | Payload::Beside { .. }
            | Payload::Knob { .. }
            | Payload::BesideRun { .. }
            | Payload::CrashExec { .. }
            | Payload::CrashStep { .. }
            | Payload::Crash { .. }
            | Payload::ProbeExec { .. }
            | Payload::WireExchange { .. }
            | Payload::WireExec { .. }
            | Payload::Sentinel { .. }
            | Payload::Model { .. }
            | Payload::Drift { .. }
            | Payload::Control { .. }
            | Payload::Confirm { .. }
            | Payload::Resumed { .. }
            | Payload::Repair { .. }
            | Payload::Note { .. }
            | Payload::RunEnd { .. } => {}
        }
    }
    came_to.map_or_else(
        || Why::Unknown {
            recorded: recorded.len(),
        },
        |came_to| {
            Why::Followed(Chain::Fault {
                id: id.to_owned(),
                steps,
                came_to,
            })
        },
    )
}

/// The chain of one mutation.
fn mutation(id: &str, events: &[Event]) -> Why {
    let mut steps = Vec::new();
    let mut came_to = None;
    for event in events {
        came_to = followed(id, &event.payload, (&mut steps, came_to));
    }
    match came_to {
        Some(came_to) if !steps.is_empty() => Why::Followed(Chain::Mutation {
            id: id.to_owned(),
            steps,
            came_to,
        }),
        Some(_) | None => Why::Unknown {
            recorded: mutations(events),
        },
    }
}

/// What `payload` adds to the chain of mutation `id`: the steps it pushes onto `steps`, and what the mutation came to after it, given `came_to` before it.
fn followed(
    id: &str,
    payload: &Payload,
    (steps, mut came_to): (&mut Vec<Step>, Option<Decided>),
) -> Option<Decided> {
    match payload {
        Payload::Route { route } if route.mutant == id => {
            if let Some(run) = route.reused.clone() {
                steps.push(Step::ReadBack { run });
            }
            steps.push(Step::Routed {
                granularity: route.granularity,
                reaching: route.reaching.clone(),
                discharged: route
                    .discharged
                    .iter()
                    .map(|one| (one.target.clone(), one.proof.clone()))
                    .collect(),
                fallback: route.fallback,
            });
        }
        Payload::MutantExec { mutant } if mutant.mutant == id => {
            steps.push(Step::Asked {
                target: mutant.target.clone(),
                outcome: mutant.outcome.clone(),
            });
            came_to = crate::report::Outcome::parse(&mutant.outcome)
                .and_then(|outcome| {
                    Decided::of(outcome, Some(mutant.target.clone()), mutant.step_boundary)
                        .or_else(|| Decided::of(outcome, None, mutant.step_boundary))
                })
                .or(came_to);
        }
        Payload::SealedExec { sealed } if sealed.mutant == id => {
            steps.push(Step::Sealed {
                target: sealed.target.clone(),
                test: sealed.test.clone(),
                came_to: sealed.came_to.clone(),
            });
            came_to = sealed_decision(steps);
        }
        Payload::Model { model } if model.mutant() == id => {
            came_to = model_decision(model).or(came_to);
        }
        Payload::Beside { beside } if beside.mutant == id => steps.push(observable(beside)),
        Payload::RunStart { .. }
        | Payload::FaultControl { .. }
        | Payload::FaultAttribution { .. }
        | Payload::FaultWrites { .. }
        | Payload::FaultFate { .. }
        | Payload::FaultRoute { .. }
        | Payload::FaultBaseline { .. }
        | Payload::FaultRejected { .. }
        | Payload::PhaseStart { .. }
        | Payload::PhaseEnd { .. }
        | Payload::Exec { .. }
        | Payload::Progress { .. }
        | Payload::Artifact { .. }
        | Payload::Route { .. }
        | Payload::MutantExec { .. }
        | Payload::SealedExec { .. }
        | Payload::ProbeExec { .. }
        | Payload::WireExchange { .. }
        | Payload::WireExec { .. }
        | Payload::FaultExec { .. }
        | Payload::Fault { .. }
        | Payload::Beside { .. }
        | Payload::BesideRun { .. }
        | Payload::CrashExec { .. }
        | Payload::CrashStep { .. }
        | Payload::Crash { .. }
        | Payload::Sentinel { .. }
        | Payload::Model { .. }
        | Payload::Drift { .. }
        | Payload::Control { .. }
        | Payload::Confirm { .. }
        | Payload::Resumed { .. }
        | Payload::Repair { .. }
        | Payload::Knob { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => {}
    }
    came_to
}

/// The step a survivor's evidence beside a fault is.
fn observable(beside: &crate::report::faults::BesideRecord) -> Step {
    Step::ObservableUnderFault {
        fault: beside.fault.clone(),
        target: beside.target.clone(),
    }
}

/// What the sealed executions among `steps` establish: a kill by the first that detected the mutant, or a survival where every one passed; nothing where one names what this release does not spell.
fn sealed_decision(steps: &[Step]) -> Option<Decided> {
    let mut passed = false;
    for step in steps {
        let Step::Sealed {
            target, came_to, ..
        } = step
        else {
            continue;
        };
        let came = rust_mutants::sealed::record::Came::named(came_to)?;
        if came.detected() {
            return Some(Decided::Killed { by: target.clone() });
        }
        passed = came == rust_mutants::sealed::record::Came::Passed;
        if !passed {
            return None;
        }
    }
    passed.then_some(Decided::Survived)
}

/// Projects the two affirmative model answers into the mutation explanation.
const fn model_decision(model: &crate::report::ModelRecord) -> Option<Decided> {
    match model.answer() {
        crate::report::ModelDecision::Noticed { .. } => Some(Decided::ModelNoticed),
        crate::report::ModelDecision::Proved { .. } => Some(Decided::ModelProved),
        crate::report::ModelDecision::Ineligible { .. }
        | crate::report::ModelDecision::Undecided { .. } => None,
    }
}

/// The chain of one seam question.
fn seam(id: &str, events: &[Event]) -> Why {
    let mut steps = Vec::new();
    let mut came_to = None;
    let mut about = None;
    for event in events {
        if let Payload::WireExec { wire: exec } = &event.payload
            && exec.fault == id
        {
            about = Some((exec.capability.clone(), exec.seq));
            steps.push(Step::Put {
                rule: exec.rule,
                decision: exec.decision.clone(),
            });
            came_to = Some(exec.decision.clone());
        }
    }
    let Some(came_to) = came_to else {
        return Why::Unknown {
            recorded: seams(events),
        };
    };
    let observed = about.and_then(|(capability, seq)| {
        events.iter().find_map(|event| match &event.payload {
            Payload::WireExchange { exchange }
                if exchange.capability == capability && exchange.seq == seq =>
            {
                Some(Step::Observed {
                    capability: exchange.capability.clone(),
                    seq: exchange.seq,
                    read: exchange.read.clone(),
                })
            }
            _ => None,
        })
    });
    Why::Followed(Chain::Seam {
        id: id.to_owned(),
        steps: observed.into_iter().chain(steps).collect(),
        came_to,
    })
}

/// How many mutations the recording names.
fn mutations(events: &[Event]) -> usize {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for event in events {
        match &event.payload {
            Payload::Route { route } => {
                seen.extend(std::iter::once(route.mutant.as_str()));
            }
            Payload::MutantExec { mutant } => {
                seen.extend(std::iter::once(mutant.mutant.as_str()));
            }
            Payload::SealedExec { sealed } => {
                seen.extend(std::iter::once(sealed.mutant.as_str()));
            }
            Payload::RunStart { .. }
            | Payload::FaultControl { .. }
            | Payload::FaultAttribution { .. }
            | Payload::FaultWrites { .. }
            | Payload::FaultFate { .. }
            | Payload::FaultRoute { .. }
            | Payload::FaultBaseline { .. }
            | Payload::FaultRejected { .. }
            | Payload::PhaseStart { .. }
            | Payload::PhaseEnd { .. }
            | Payload::Exec { .. }
            | Payload::Progress { .. }
            | Payload::Artifact { .. }
            | Payload::ProbeExec { .. }
            | Payload::WireExchange { .. }
            | Payload::WireExec { .. }
            | Payload::FaultExec { .. }
            | Payload::Fault { .. }
            | Payload::Beside { .. }
            | Payload::BesideRun { .. }
            | Payload::CrashExec { .. }
            | Payload::CrashStep { .. }
            | Payload::Crash { .. }
            | Payload::Sentinel { .. }
            | Payload::Model { .. }
            | Payload::Drift { .. }
            | Payload::Control { .. }
            | Payload::Confirm { .. }
            | Payload::Resumed { .. }
            | Payload::Repair { .. }
            | Payload::Knob { .. }
            | Payload::Note { .. }
            | Payload::RunEnd { .. } => {}
        }
    }
    seen.len()
}

/// How many seam questions the recording names.
fn seams(events: &[Event]) -> usize {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for event in events {
        if let Payload::WireExec { wire: exec } = &event.payload {
            seen.extend(std::iter::once(exec.fault.as_str()));
        }
    }
    seen.len()
}
