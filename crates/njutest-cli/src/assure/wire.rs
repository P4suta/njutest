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
        let decision = crate::wire::prove::discharges(fault, measuring.observed).map_or_else(
            || settle(fault, &run(fault)).decision,
            |proof| SeamDecision::Proved {
                proof: proof.to_owned(),
            },
        );
        watch.trace.wire_exec(crate::trace::WireExecRecord {
            fault: fault.id.clone(),
            capability: fault.capability.clone(),
            seq: fault.seq,
            rule: fault.rule,
            decision: decision.clone(),
        });
        match &decision {
            SeamDecision::Tests { .. } | SeamDecision::Proved { .. } => {}
            SeamDecision::Unreached => unput = unput.saturating_add(1),
            SeamDecision::Unnoticed => {
                done.findings.push(unnoticed(fault, measuring.observed));
            }
        }
        asked_about(&mut done, measuring, (fault, decision));
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
    (fault, decision): (&Fault, SeamDecision),
) {
    let named = measuring
        .observed
        .iter()
        .find(|one| one.capability == fault.capability && one.seq == fault.seq);
    let (asked, answered) = named.map_or_else(|| (String::new(), None), |one| one.spoken.asked());
    done.seams.push(crate::report::SeamRecord {
        id: fault.id.clone(),
        capability: fault.capability.clone(),
        seq: fault.seq,
        asked,
        answered,
        rule: fault.rule,
        decision,
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
///
/// Every seam is drained before any fault is put, and only then are they
/// measured one at a time. Draining a seam after another seam's fault runs
/// would take the traffic those runs drove through it for the baseline, and
/// derive a catalogue from a program that was already being perturbed.
///
/// One seam at a time, so the seam a question is about is the one that
/// derived it rather than one looked up by name afterwards. A lookup can
/// fail, and a failed lookup returning no answers would report *the run could
/// not put this question* about a run that had lost track of its own seam —
/// two facts under one sentence, and the one a reader would act on is the
/// wrong one.
#[must_use]
pub fn asking<R>(seams: &Seams, baseline: &Baseline, mut run: R, watch: Watch<'_>) -> Measured
where
    R: FnMut() -> Vec<Answered>,
{
    let mut done = Measured::default();
    for (at, observed) in seams.watching.iter().zip(&baseline.per_seam) {
        for exchange in observed {
            watch.trace.wire_exchange(recorded(exchange));
        }
        let measured = measure(
            &Measuring { observed },
            |fault| {
                for one in &seams.watching {
                    one.interposer.putting(None);
                }
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
        done.executed |= measured.executed;
        done.findings.extend(measured.findings);
        done.limitations.extend(measured.limitations);
        done.seams.extend(measured.seams);
    }
    for one in &seams.watching {
        one.interposer.putting(None);
    }
    done
}

/// One exchange as the recording writes it down, which is what an audit re-derives the catalogue from.
fn recorded(exchange: &Exchange) -> crate::trace::WireExchangeRecord {
    let (read, request_bytes, response_bytes) = match &exchange.spoken {
        crate::wire::Spoken::Http {
            method,
            path,
            status,
            request_bytes,
            response_bytes,
            body_bytes: _,
            status_line: _,
        } => (
            crate::trace::Read::Http {
                method: method.clone(),
                path: path.clone(),
                status: *status,
            },
            *request_bytes,
            *response_bytes,
        ),
        crate::wire::Spoken::Raw {
            request_bytes,
            response_bytes,
        } => (crate::trace::Read::Raw, *request_bytes, *response_bytes),
    };
    crate::trace::WireExchangeRecord {
        capability: exchange.capability.clone(),
        seq: exchange.seq,
        during: exchange.during.clone(),
        duration_ms: exchange.duration_ms,
        read,
        request_bytes,
        response_bytes,
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

/// What went past every watched seam while the program was the one the tests are about.
///
/// `Seams::sealed` is the only thing that makes one, and it stops the seams
/// recording as it takes it. So there is no later moment at which a recording
/// with a perturbed program's traffic in it can be obtained: not after the
/// mutation phase, which runs the suite once per mutation, and not during the
/// seam phase, which runs it once per question with a fault in place. A
/// catalogue is a set of questions about a program, and every one of those
/// runs is a different program from the one a reader is being told about.
#[derive(Debug, Default)]
pub struct Baseline {
    /// One recording per seam, in the order the seams were started.
    per_seam: Vec<Vec<Exchange>>,
}

impl Baseline {
    /// Every exchange every seam saw, in the order the seams were started.
    #[must_use]
    pub fn all(&self) -> Vec<Exchange> {
        self.per_seam.concat()
    }
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
    /// Records one run of the suite and stops recording: the catalogue is what that run did.
    ///
    /// The run is made here rather than taken from whatever else happened to
    /// go past. A fault names an exchange by its place in the order and is put
    /// by running the suite once, so a catalogue has to come from one run of
    /// the suite to hold questions that one run can reach.
    ///
    /// What is being ruled out is a catalogue assembled from several
    /// programs. A phase that builds and verifies before it measures runs the
    /// suite more than once, and the second run's exchanges are the same
    /// exchanges counted again — a fault naming one of them is unreachable by
    /// construction, and the report states it as a question nobody put. The
    /// mutation phase is worse: it runs the suite once per mutation, so the
    /// catalogue grows a copy of every exchange per mutation, every one of
    /// them a hole the run invented by measuring. Recording stops when this
    /// returns, so neither can happen however late anything else reads.
    #[must_use]
    pub fn observing<R>(&self, mut run: R) -> Baseline
    where
        R: FnMut(),
    {
        for one in &self.watching {
            one.interposer.restart();
        }
        run();
        Baseline {
            per_seam: self
                .watching
                .iter()
                .map(|one| one.interposer.seal())
                .collect(),
        }
    }

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
            (resource.wire, resource.hold),
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
