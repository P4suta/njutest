// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest why`: everything a recording says stands behind one claim, drawn for a person.

use super::{Style, Telling, Terminal};
use crate::report::faults::FaultDecision;
use crate::report::{Decided, SeamDecision};
use crate::trace::Read;
use crate::why::{Chain, Claim, Step, Why};

/// What kind of thing a claim is, in the word a reader would use for a pile of them.
const fn kind(claim: &Claim) -> &'static str {
    match claim {
        Claim::Mutation(_) => "mutation",
        Claim::Seam(_) => "seam question",
        Claim::Fault(_) => "fault",
    }
}

/// What the run finally established about a mutation.
fn came_to(decided: &Decided) -> String {
    match decided {
        Decided::Killed { by } => format!("{by} noticed it"),
        Decided::Survived => "nothing noticed it".to_owned(),
        Decided::Unreached => "nothing executed it".to_owned(),
        Decided::Equivalent => "a proof says no test could tell the difference".to_owned(),
        Decided::ModelNoticed => "the model checker found a distinguishing input".to_owned(),
        Decided::ModelProved => {
            "the model checker proved equality throughout its closed domain".to_owned()
        }
        Decided::CompileRejected => "the compiler refused it".to_owned(),
        Decided::StepLimitReached { on, boundary } => format!(
            "{on} crossed its step allowance at {} without a control verdict",
            boundary.observed()
        ),
        Decided::Waited { on } => format!("this machine stopped waiting for {on}"),
        Decided::Unconfirmed { on } => format!("{on} did not answer the same way twice"),
        Decided::Errored { on } => format!("{on} could not be measured"),
        Decided::Declined { on } => format!(
            "every test that reached it declined to measure on this machine, those of {on} among \
             them"
        ),
    }
}

/// What the run finally established about a seam question.
fn settled(decision: &SeamDecision) -> String {
    match decision {
        SeamDecision::Tests { noticed_by } => format!("{noticed_by} noticed it"),
        SeamDecision::Proved { proof } => format!("no observer could have noticed, by {proof}"),
        SeamDecision::Unnoticed => "the suite ran with it in place and nothing noticed".to_owned(),
        SeamDecision::Unreached => {
            "the question could not be put, so the run established nothing".to_owned()
        }
    }
}

/// What the run finally established about a failed call.
fn failed(decision: &FaultDecision) -> String {
    match decision {
        FaultDecision::Noticed { by } => format!("{by} noticed the call failing"),
        FaultDecision::Unnoticed => {
            "every test that reached it passed with the call failing".to_owned()
        }
        FaultDecision::Unreached => "no test reached it".to_owned(),
        FaultDecision::Waited { on } => format!("this machine stopped waiting for {on}"),
        FaultDecision::Undecided { on, why } => format!("{on} could not be decided: {why}"),
        FaultDecision::NotPut { diagnostic } => {
            format!("the compiler refused the fault: {diagnostic}")
        }
    }
}

/// What was read of an exchange, when anything was.
fn reading(read: &Read) -> String {
    match read {
        Read::Raw => "counted as bytes".to_owned(),
        Read::Http {
            method,
            path,
            status,
        } => format!("{method} {path} -> {status}"),
    }
}

/// One step, as the line a reader follows down the page.
///
/// Every name a reader might want to go and look at is on the line that mentions it.
/// A step saying a proof removed a target without naming the proof is a verb with no agent, and the reader is told something was removed and not what removed it, which is the one thing they would check.
fn stepped(step: &Step, telling: Telling) -> String {
    match step {
        Step::Observed {
            capability,
            seq,
            read,
        } => format!(
            "{} {} #{seq}  {}",
            telling.painted(Style::Marker, "observed"),
            telling.painted(Style::Subject, capability),
            telling.painted(Style::Frame, &reading(read))
        ),
        Step::Routed {
            granularity,
            reaching,
            discharged,
            fallback,
        } => {
            let mut parts = vec![format!(
                "{} by {}",
                telling.painted(Style::Marker, "routed"),
                telling.painted(Style::Keyword, granularity.name())
            )];
            if !reaching.is_empty() {
                parts.push(format!(
                    "reaching {}",
                    telling.painted(Style::Subject, &reaching.join(", "))
                ));
            }
            parts.extend(discharged.iter().map(|(target, proof)| {
                format!(
                    "{} {} by {}",
                    telling.painted(Style::Well, "discharged"),
                    telling.painted(Style::Subject, target),
                    telling.painted(Style::Keyword, proof)
                )
            }));
            if let Some(fallback) = fallback {
                parts.push(format!(
                    "{} {}  {}",
                    telling.painted(Style::Limitation, "widened back:"),
                    telling.painted(Style::Keyword, fallback.name()),
                    crate::assure::route::detail(*fallback)
                ));
            }
            parts.join("  ")
        }
        Step::Asked { target, outcome } => format!(
            "{} {}  {}",
            telling.painted(Style::Marker, "asked"),
            telling.painted(Style::Subject, target),
            telling.painted(Style::Keyword, outcome)
        ),
        Step::ReadBack { run } => format!(
            "{} {}",
            telling.painted(Style::Limitation, "read back from"),
            telling.painted(Style::Subject, run)
        ),
        Step::Put { rule, decision } => format!(
            "{} {}  {}",
            telling.painted(Style::Marker, "put"),
            telling.painted(Style::Keyword, rule.name()),
            telling.painted(Style::Frame, &settled(decision))
        ),
    }
}

/// What `why` says about `claim`, drawn for `terminal`.
///
/// A run that kept no recording and a recording that does not name the claim are two pages, never one with a number in it.
/// The first establishes nothing about this claim or any other, and telling a reader their identity is not in the recording would be this command concluding from how it was measured — the defect it exists to explain, arriving in the explanation (ADR 0023).
/// The second is a name typed wrong, and the count of what the recording does hold is how a reader tells that from a run that recorded nothing of the kind.
///
/// No step carries a duration.
/// *Why did this come out this way* and *why did this take four minutes* are two questions and `njutest trace` owns the second; a column blank on every chain where each step is instant is a column nobody reads.
#[must_use]
pub fn page(claim: &Claim, why: &Why, terminal: Terminal) -> String {
    let telling = Telling::of(terminal);
    let named = telling.painted(Style::Subject, claim.named());
    match why {
        Why::NotRecorded => format!(
            "{} {named}\n  {}\n  {}\n",
            telling.painted(Style::Limitation, "why"),
            telling.painted(
                Style::Limitation,
                "this run kept no recording, so it says nothing about this or anything else"
            ),
            telling.painted(
                Style::Frame,
                "run it again with --trace=<directory> and ask then"
            )
        ),
        Why::Unknown { recorded } => format!(
            "{} {named}\n  {}\n  {}\n",
            telling.painted(Style::Gap, "why"),
            telling.painted(
                Style::Gap,
                &format!("the recording does not hold this {}", kind(claim))
            ),
            telling.painted(Style::Frame, &held(*recorded, kind(claim)))
        ),
        Why::Followed(chain) => {
            let answer = match chain {
                Chain::Mutation { came_to: to, .. } => came_to(to),
                Chain::Seam { came_to: to, .. } => settled(to),
                Chain::Fault { came_to: to, .. } => failed(to),
            };
            let drawn = chain
                .steps()
                .iter()
                .map(|step| format!("  {}\n", stepped(step, telling)))
                .collect::<Vec<_>>()
                .concat();
            format!(
                "{} {named}\n  {}\n\n{drawn}",
                telling.painted(Style::Marker, "why"),
                telling.painted(Style::Changed, &answer)
            )
        }
    }
}

/// What the recording does hold, which is what tells a name typed wrong from a run that recorded none of them.
fn held(recorded: usize, kind: &str) -> String {
    if recorded == 0 {
        return format!("it holds no {kind} at all, so nothing of this kind was recorded");
    }
    let plural = if recorded == 1 { "" } else { "s" };
    format!("it holds {recorded} other {kind}{plural}, so check the identity you typed")
}
