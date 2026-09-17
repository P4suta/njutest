// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting every question a seam licensed to the tests, and saying what nothing noticed.

use crate::report::SeamDecision;
use crate::report::{Finding, FindingKind, Limitation};
use crate::watch::Watch;
use crate::wire::Exchange;
use crate::wire::derive::{Fault, derive};
use crate::wire::settle::{Answered, settle};

/// The limitation a run states where it could not put a question it derived.
pub const NOT_PUT: &str = "wire-fault-not-put";

/// What the phase is asked about.
#[derive(Debug, Clone, Copy)]
pub struct Measuring<'a> {
    /// Every exchange the baseline saw go past a seam.
    pub observed: &'a [Exchange],
}

/// What it established about the seams.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    /// Whether the phase put anything to anything.
    pub executed: bool,
    /// The questions nothing noticed, and the ones nothing was asked.
    pub findings: Vec<Finding>,
    /// What it could not say.
    pub limitations: Vec<Limitation>,
    /// Every question the recording licensed, and what became of it, so a finding's name can be looked up.
    pub seams: Vec<crate::report::SeamRecord>,
}

/// Puts every fault the recording licensed to the tests, by whatever `run` does to run them.
///
/// `run` takes the behaviour that starts the seam again with one fault in
/// place and measures the suite, which is an argument rather than something
/// this reaches for, as ADR 0001 requires.
#[must_use]
pub fn measure<R>(measuring: &Measuring<'_>, mut run: R, watch: Watch<'_>) -> Measured
where
    R: FnMut(&Fault) -> Vec<Answered>,
{
    let faults = derive(measuring.observed);
    if faults.is_empty() {
        return Measured::default();
    }
    let mut done = Measured {
        executed: true,
        ..Measured::default()
    };
    let mut unput = 0_u32;
    for fault in &faults {
        if watch.cancel.is_cancelled() {
            break;
        }
        if let Some(proof) = crate::wire::prove::discharges(fault, measuring.observed) {
            asked_about(
                &mut done,
                measuring,
                (fault, SeamDecision::Proved, Some(proof.to_owned())),
            );
            watch.trace.wire_exec(crate::trace::WireExecRecord {
                fault: fault.id.clone(),
                capability: fault.capability.clone(),
                seq: fault.seq,
                rule: fault.rule.name().to_owned(),
                decision: SeamDecision::Proved.name().to_owned(),
                noticed_by: Some(proof.to_owned()),
            });
            continue;
        }
        let settled = settle(fault, &run(fault));
        asked_about(
            &mut done,
            measuring,
            (fault, settled.decision, settled.noticed_by.clone()),
        );
        watch.trace.wire_exec(crate::trace::WireExecRecord {
            fault: settled.fault.id.clone(),
            capability: settled.fault.capability.clone(),
            seq: settled.fault.seq,
            rule: settled.fault.rule.name().to_owned(),
            decision: settled.decision.name().to_owned(),
            noticed_by: settled.noticed_by.clone(),
        });
        match settled.decision {
            SeamDecision::Tests | SeamDecision::Proved => {}
            SeamDecision::Unreached => unput = unput.saturating_add(1),
            SeamDecision::Unnoticed => {
                done.findings.push(unnoticed(fault, measuring.observed));
            }
        }
    }
    if unput > 0 {
        done.findings.push(Finding::new(
            FindingKind::NotMeasured,
            NOT_PUT,
            &format!(
                "{unput} question(s) this run derived from what went past a seam were \
                 put to no test, so it says nothing about whether anything would have \
                 noticed them"
            ),
        ));
    }
    done
}

/// Writes down one question and what became of it, so a finding that names it can be looked up.
fn asked_about(
    done: &mut Measured,
    measuring: &Measuring<'_>,
    (fault, decision, noticed_by): (&Fault, SeamDecision, Option<String>),
) {
    let named = measuring
        .observed
        .iter()
        .find(|one| one.capability == fault.capability && one.seq == fault.seq);
    let (asked, answered) = named.map_or_else(
        || (String::new(), None),
        |one| match &one.spoken {
            crate::wire::Spoken::Http {
                method,
                path,
                status,
                ..
            } => (format!("{method} {path}"), Some(*status)),
            crate::wire::Spoken::Raw { .. } => (String::new(), None),
        },
    );
    done.seams.push(crate::report::SeamRecord {
        id: fault.id.clone(),
        capability: fault.capability.clone(),
        seq: fault.seq,
        asked,
        answered,
        rule: fault.rule,
        decision,
        noticed_by,
    });
}

/// What a reader is told about a question the suite carried on through.
///
/// The exchange is named by what was asked over it rather than by its place in
/// the order: a reader who has to count round trips to find out which one this
/// was has been handed an ordinal instead of an answer.
fn unnoticed(fault: &Fault, observed: &[Exchange]) -> Finding {
    let spoke = observed
        .iter()
        .find(|one| one.capability == fault.capability && one.seq == fault.seq)
        .and_then(|one| match &one.spoken {
            crate::wire::Spoken::Http { method, path, .. } => Some(format!("{method} {path}")),
            crate::wire::Spoken::Raw { .. } => None,
        })
        .map_or_else(
            || format!("exchange {} of the {} seam", fault.seq, fault.capability),
            |what| format!("{what} on the {} seam", fault.capability),
        );
    let mut finding = Finding::new(
        FindingKind::WireUnnoticed,
        &fault.id,
        &format!(
            "nothing noticed when the run was told to {}, answering {spoke}",
            fault.asks()
        ),
    );
    finding.path = None;
    finding
}

/// Puts every question a recording licensed back to `run`, seam by seam.
///
/// `putting` is what tells one seam to answer the way a fault says, and every
/// other seam is told to put nothing first: a run with two faults in place
/// would measure two and report one.
///
/// A run the exchange never came past answered nothing, whatever the tests
/// did. The suite may take a different path this time and pass throughout, and
/// reading that as nothing noticing would report a gap the tests could close
/// where nobody was asked anything.
#[must_use]
pub fn asking<R>(seams: &Seams, mut run: R, watch: Watch<'_>) -> Measured
where
    R: FnMut() -> Vec<Answered>,
{
    let observed: Vec<Exchange> = seams
        .watching
        .iter()
        .flat_map(|one| one.interposer.taken())
        .collect();
    for exchange in &observed {
        watch.trace.wire_exchange(recorded(exchange));
    }
    let measured = measure(
        &Measuring {
            observed: &observed,
        },
        |fault| {
            for one in &seams.watching {
                one.interposer.putting(None);
            }
            let Some(at) = seams
                .watching
                .iter()
                .find(|one| one.capability == fault.capability)
            else {
                return Vec::new();
            };
            at.interposer.putting(Some(fault.clone()));
            let answered = run();
            if at.interposer.was_put() {
                answered
            } else {
                Vec::new()
            }
        },
        watch,
    );
    for one in &seams.watching {
        one.interposer.putting(None);
    }
    measured
}

/// One exchange as the recording writes it down, which is what an audit re-derives the catalogue from.
fn recorded(exchange: &Exchange) -> crate::trace::WireExchangeRecord {
    let read = match &exchange.spoken {
        crate::wire::Spoken::Http {
            method,
            path,
            status,
            request_bytes,
            response_bytes,
            body_bytes: _,
            status_line: _,
        } => crate::trace::WireExchangeRecord {
            wire: "http".to_owned(),
            method: Some(method.clone()),
            path: Some(path.clone()),
            status: Some(*status),
            request_bytes: *request_bytes,
            response_bytes: *response_bytes,
            ..crate::trace::WireExchangeRecord::default()
        },
        crate::wire::Spoken::Raw {
            request_bytes,
            response_bytes,
        } => crate::trace::WireExchangeRecord {
            wire: "raw".to_owned(),
            request_bytes: *request_bytes,
            response_bytes: *response_bytes,
            ..crate::trace::WireExchangeRecord::default()
        },
    };
    crate::trace::WireExchangeRecord {
        capability: exchange.capability.clone(),
        seq: exchange.seq,
        during: exchange.during.clone(),
        duration_ms: exchange.duration_ms,
        ..read
    }
}

/// What a recording licensed a run to ask, where the run put none of it.
///
/// Recording a seam is the half of the phase a release has; putting the
/// questions back to the suite is the half it does not, and a run that said
/// nothing would leave a reader thinking a watched seam had been measured.
#[must_use]
pub fn licensing(observed: &[Exchange]) -> Option<Limitation> {
    let derived = derive(observed).len();
    if derived == 0 {
        return None;
    }
    Some(Limitation::new(
        NOT_PUT,
        &format!(
            "{} exchange(s) went past the seams this run watched, licensing {derived} \
             question(s) about them; this run records them and puts none of them back \
             to the tests",
            observed.len()
        ),
    ))
}

/// Every seam a run is watching, and what the tests are told instead.
#[derive(Debug, Default)]
pub struct Seams {
    /// The environment the tests are given, with every watched variable pointing at its interposer.
    pub environment: Vec<(String, String)>,
    /// The interposers, in the order the seams were started.
    pub watching: Vec<crate::wire::dialled::Watching>,
}

impl Seams {
    /// Stops every interposer and hands back everything that went past, seam by seam.
    #[must_use]
    pub fn recorded(self) -> Vec<Exchange> {
        self.watching
            .into_iter()
            .flat_map(|one| one.interposer.stop())
            .collect()
    }
}

/// Puts an interposer in front of every seam the configuration named, and says what the tests are told.
///
/// A lease the configuration did not name is handed over exactly as the
/// provider gave it: a run that rewrote one nobody named would send the tests
/// somewhere the configuration never chose, and record a seam it was not
/// asked to look at.
#[must_use]
pub fn watched(
    leases: &[&crate::resource::Lease],
    configured: &std::collections::BTreeMap<String, crate::config::Resource>,
) -> Seams {
    let mut seams = Seams::default();
    for lease in leases {
        let named = configured
            .get(&lease.capability)
            .filter(|resource| !resource.interpose.is_empty());
        let Some(resource) = named else {
            seams.environment.extend(lease.environment.iter().cloned());
            continue;
        };
        match crate::wire::dialled::interposed(
            lease,
            &resource.interpose,
            (resource.wire, crate::wire::interpose::HELD_UP),
        ) {
            Some(one) => {
                seams.environment.extend(one.environment.iter().cloned());
                seams.watching.push(one);
            }
            None => seams.environment.extend(lease.environment.iter().cloned()),
        }
    }
    seams
}
