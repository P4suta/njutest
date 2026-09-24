// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A report, read as what a person is told about it.

use super::{
    Action, Diagnostic, Headline, Place, Severity, Site, Sources, Spot, Standing, Stated, Told,
    Unsettled,
};
use crate::report::{Conclusion, Decision, Finding, FindingKind, ProjectedMutant, Report};

impl Told {
    /// What `report` has to say, with the lines it is about taken from `sources`.
    ///
    /// # Errors
    /// Returns the checked projection error instead of shortening accounting.
    pub fn of(
        report: &Report,
        sources: &Sources,
        kept: &str,
    ) -> Result<Self, crate::report::CountError> {
        let conclusion = report.conclusion()?;
        Ok(Self {
            headline: Headline {
                verdict: conclusion.verdict,
                cataloged: conclusion.accounting.mutants.cataloged,
                killed: conclusion.accounting.mutants.killed,
                survived: conclusion.accounting.mutants.survived,
                unreached: conclusion.accounting.mutants.unreached,
                step_limit_reached: conclusion.accounting.mutants.step_limit_reached,
                waited: conclusion.accounting.mutants.waited,
                duration_ms: conclusion.timing.compute_total_ms(),
                kept: kept.to_owned(),
            },
            places: blind(&conclusion, sources),
            diagnostics: conclusion
                .findings
                .iter()
                .filter(|finding| !about_a_place(finding, &conclusion))
                .map(|finding| said(finding, &conclusion, sources))
                .collect(),
            limitations: conclusion
                .limitations
                .iter()
                .map(|limitation| Stated {
                    name: limitation.name.clone(),
                    detail: limitation.detail.clone(),
                })
                .collect(),
        })
    }
}

/// Whether a finding is one an item of the source is drawn for, rather than one of its own.
fn about_a_place(finding: &Finding, report: &Conclusion) -> bool {
    matches!(
        finding.kind,
        FindingKind::SurvivingMutant
            | FindingKind::Timeout
            | FindingKind::WaitedMutant
            | FindingKind::StepLimitReachedMutant
    ) && found(report, &finding.subject).is_some_and(|mutant| !mutant.item().is_empty())
}

/// The mutant a finding is about, when it is about one.
fn found<'a>(report: &'a Conclusion, subject: &str) -> Option<&'a ProjectedMutant> {
    report
        .mutants
        .iter()
        .find(|one| one.display_id() == subject || one.id() == subject)
}

/// Where the tests are blind, gathered by the item the source puts them in.
///
/// The order is the source's: a reader goes down a file, not down a list of identities, and two items of one file belong next to each other.
fn blind(report: &Conclusion, sources: &Sources) -> Vec<Place> {
    let mut by_item: std::collections::BTreeMap<(&str, &str), Vec<Spot>> =
        std::collections::BTreeMap::new();
    for finding in &report.findings {
        if !about_a_place(finding, report) {
            continue;
        }
        let Some(mutant) = found(report, &finding.subject) else {
            continue;
        };
        by_item
            .entry((mutant.path(), mutant.item()))
            .or_default()
            .push(spot(mutant, finding));
    }
    let mut places: Vec<Place> = by_item
        .into_iter()
        .map(|((path, item), mut spots)| {
            spots.sort_by_key(|one| (one.line, one.column));
            drawn(path, item, spots, sources)
        })
        .collect();
    places.sort_by(|left, right| {
        let line = |place: &Place| place.spots.first().map_or(0, |spot| spot.line);
        (left.path.clone(), line(left)).cmp(&(right.path.clone(), line(right)))
    });
    places
}

/// One place, with the lines that hold its blind spots and a line of room on each side, as the run measured them.
///
/// Whether the lines are drawn is the file's standing against the digest the run recorded, never a guess from what a line holds: a file edited after the run is `Edited` whatever its lines still say.
fn drawn(path: &str, item: &str, spots: Vec<Spot>, sources: &Sources) -> Place {
    let first = spots.iter().map(|one| one.line).min().unwrap_or(1);
    let last = spots.iter().map(|one| one.line).max().unwrap_or(1);
    let (excerpt, instead) =
        match sources.span(path, first.saturating_sub(1).max(1), last.saturating_add(1)) {
            Ok(lines) if lines.iter().any(|(line, _text)| *line == last) => (lines, None),
            Ok(_short) => (Vec::new(), Some(super::Missing::NoSuchLine)),
            Err(missing) => (Vec::new(), Some(missing)),
        };
    Place {
        item: item.to_owned(),
        path: path.to_owned(),
        excerpt,
        instead,
        spots,
    }
}

/// One blind spot: what the run changed, and what it established by changing it.
///
/// Read through `Blind`, so the only outcomes that reach a spot are the four that leave a hole and each of them arrives as itself.
/// An outcome that is not a hole has no spot to be, and one this layer could somehow be handed anyway says nothing was established rather than that the tests ran the line and missed it — which is the expensive way to be wrong, because somebody goes looking for the assertion they are missing and the run never got an answer at all (ADR 0023).
fn spot(mutant: &ProjectedMutant, _finding: &Finding) -> Spot {
    let standing = mutant
        .decision()
        .blind()
        .map_or(Standing::Unsettled(Unsettled::Errored), Standing::of);
    let across: Vec<super::Across> = mutant
        .blind_in()
        .iter()
        .map(|one| super::Across {
            build: one.build.as_str().to_owned(),
            standing: Standing::of(one.decision),
        })
        .collect();
    Spot {
        line: mutant.position().line,
        column: mutant.position().column,
        was: mutant.original().to_owned(),
        now: mutant.replacement().to_owned(),
        said: standing.worded(&across),
        standing,
        blind_in: across,
        locator: locator(mutant),
    }
}

/// One finding, as the thing a reader is shown about it.
fn said(finding: &Finding, report: &Conclusion, sources: &Sources) -> Diagnostic {
    let mutant = report
        .mutants
        .iter()
        .find(|one| one.display_id() == finding.subject || one.id() == finding.subject);
    let unreached = mutant.is_some_and(|one| one.decision() == Decision::Unreached);
    let (severity, code, title) = about(finding.kind, unreached);
    Diagnostic {
        severity,
        code,
        title: title.to_owned(),
        at: mutant.map(|one| site(one, sources)),
        notes: vec![beyond(&finding.detail, mutant)],
        actions: mutant.map(answering).unwrap_or_default(),
    }
}

/// What a detail says beyond what the drawing already shows.
///
/// A finding's detail is written for the record stream, where nothing else is on the line, so it opens by naming the rule and the place.
/// Both are already above it here, and a note that repeats the line above it is one a reader learns to skip.
fn beyond(detail: &str, mutant: Option<&ProjectedMutant>) -> String {
    let Some(mutant) = mutant else {
        return detail.to_owned();
    };
    let Some((before, after)) = detail.split_once(": ") else {
        return detail.to_owned();
    };
    if before.contains(mutant.path()) && before.contains(mutant.rule()) {
        return after.to_owned();
    }
    detail.to_owned()
}

/// Where a mutation is, and what the run did to the line it is on.
fn site(mutant: &ProjectedMutant, sources: &Sources) -> Site {
    let excerpt = sources.at(mutant.path(), mutant.position().line);
    Site {
        path: mutant.path().to_owned(),
        line: mutant.position().line,
        column: mutant.position().column,
        excerpt,
        label: labelled(mutant),
        width: mutant.original().chars().count().max(1),
    }
}

/// What to say under the mark: what the run changed, and what noticed.
fn labelled(mutant: &ProjectedMutant) -> String {
    super::projected_label(mutant.rule(), mutant.decision())
}

/// What a reader can do about one mutation, as commands that work when they are typed.
fn answering(mutant: &ProjectedMutant) -> Vec<Action> {
    let named = locator(mutant);
    vec![
        Action {
            said: "explain".to_owned(),
            command: format!("njutest explain {named}"),
        },
        Action {
            said: "accept".to_owned(),
            command: format!("njutest accept {named} --reason \"...\""),
        },
    ]
}

/// How a reader names this mutation again, which has to hold after they have changed the file.
fn locator(mutant: &ProjectedMutant) -> String {
    if mutant.item().is_empty() || mutant.path().is_empty() {
        return mutant.display_id().to_owned();
    }
    format!(
        "{}:{}:{}@{}",
        mutant.path(),
        mutant.item(),
        mutant.rule(),
        mutant.position().line
    )
}

/// What a reader is shown about one finding: how much attention it deserves, what it is called, and what it is.
///
/// A mutation nothing reached and a mutation every test ran past are one kind of finding in the report and two different things to be told, so the mutant's own outcome decides between them.
///
/// Matched without a catch-all, so a finding kind added later is one the compiler makes somebody decide how to show rather than one that quietly reads as "the run found something".
const fn about(kind: FindingKind, unreached: bool) -> (Severity, &'static str, &'static str) {
    match kind {
        FindingKind::WireUnnoticed => (
            Severity::Gap,
            "NJ-WIRE",
            "the suite carried on through what a seam was asked",
        ),
        FindingKind::HollowTarget => (
            Severity::Gap,
            "NJ-HOLLOW-TARGET",
            "this test target noticed none of the changes it was put to",
        ),
        FindingKind::UnstableBaseline => (
            Severity::Limitation,
            "NJ-UNSTABLE-BASELINE",
            "this test target reached different code on two runs of the same passing tests, \
             so every proof that removed a run because of what it reached is unfounded",
        ),
        FindingKind::SurvivingMutant if unreached => (
            Severity::Gap,
            "NJ-UNREACHED",
            "no test reaches this code at all",
        ),
        FindingKind::SurvivingMutant => (
            Severity::Gap,
            "NJ-SURVIVOR",
            "the suite passed with this change in place",
        ),
        FindingKind::BuildFailure => (
            Severity::Refusal,
            "NJ-BUILD",
            "the workspace does not compile",
        ),
        FindingKind::FailingTest => (
            Severity::Refusal,
            "NJ-FAILING-TEST",
            "a test of the workspace fails",
        ),
        FindingKind::TargetMissing => (
            Severity::Refusal,
            "NJ-TARGET-MISSING",
            "a test target could not be found, so nothing was observed about it",
        ),
        FindingKind::Timeout => (
            Severity::Gap,
            "NJ-TIMEOUT",
            "this target ran out of time rather than answering",
        ),
        FindingKind::WaitedMutant => (
            Severity::Limitation,
            "NJ-WAITED",
            "this machine stopped waiting, so the run established nothing about it",
        ),
        FindingKind::StepLimitReachedMutant => (
            Severity::Limitation,
            "NJ-STEP-LIMIT",
            "the step boundary was reached without a matched control verdict",
        ),
        FindingKind::NotMeasured => (
            Severity::Limitation,
            "NJ-NOT-MEASURED",
            "the run could not measure this, so it claims nothing about it",
        ),
        FindingKind::UnmatchedAcceptance => (
            Severity::Limitation,
            "NJ-UNMATCHED-ACCEPTANCE",
            "an acceptance names no mutation in this catalog",
        ),
        FindingKind::UndefinedBehaviour => (
            Severity::Refusal,
            "NJ-UNSOUND",
            "the interpreter found what the compiler cannot check",
        ),
    }
}
