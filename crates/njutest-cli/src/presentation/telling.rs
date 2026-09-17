// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A report, read as what a person is told about it.

use super::{
    Action, Blindness, Diagnostic, Headline, Place, Severity, Site, Sources, Spot, Stated, Told,
};
use crate::report::Outcome;
use crate::report::{Finding, FindingKind, MutantRecord, Report};

impl Told {
    /// What `report` has to say, with the lines it is about taken from `sources`.
    #[must_use]
    pub fn of(report: &Report, sources: &Sources, kept: &str) -> Self {
        Self {
            headline: Headline {
                verdict: report.verdict,
                project: report.repository.root_name.clone(),
                cataloged: report.accounting.mutants.cataloged,
                killed: report.accounting.mutants.killed,
                survived: report.accounting.mutants.survived,
                unreached: report.accounting.mutants.unreached,
                duration_ms: report.timing.duration_ms,
                kept: kept.to_owned(),
            },
            places: blind(report, sources),
            diagnostics: report
                .findings
                .iter()
                .filter(|finding| !about_a_place(finding, report))
                .map(|finding| said(finding, report, sources))
                .collect(),
            limitations: report
                .limitations
                .iter()
                .map(|limitation| Stated {
                    name: limitation.name.clone(),
                    detail: limitation.detail.clone(),
                })
                .collect(),
        }
    }
}

/// Whether a finding is one an item of the source is drawn for, rather than one of its own.
fn about_a_place(finding: &Finding, report: &Report) -> bool {
    matches!(
        finding.kind,
        FindingKind::SurvivingMutant | FindingKind::Timeout
    ) && found(report, &finding.subject).is_some_and(|mutant| !mutant.item.is_empty())
}

/// The mutant a finding is about, when it is about one.
fn found<'a>(report: &'a Report, subject: &str) -> Option<&'a MutantRecord> {
    report
        .mutants
        .iter()
        .find(|one| one.display_id == subject || one.id == subject)
}

/// Where the tests are blind, gathered by the item the source puts them in.
///
/// The order is the source's: a reader goes down a file, not down a list of
/// identities, and two items of one file belong next to each other.
fn blind(report: &Report, sources: &Sources) -> Vec<Place> {
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
            .entry((mutant.path.as_str(), mutant.item.as_str()))
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

/// One place, with the lines that hold its blind spots and a line of room on each side.
fn drawn(path: &str, item: &str, spots: Vec<Spot>, sources: &Sources) -> Place {
    let first = spots.iter().map(|one| one.line).min().unwrap_or(1);
    let last = spots.iter().map(|one| one.line).max().unwrap_or(1);
    let excerpt = sources.span(path, first.saturating_sub(1).max(1), last.saturating_add(1));
    let moved = spots.iter().any(|spot| {
        !spot.was.is_empty()
            && !excerpt
                .iter()
                .any(|(line, text)| *line == spot.line && text.contains(&spot.was))
    });
    let instead = if excerpt.is_empty() {
        Some(super::Excerpt::Unreadable)
    } else if moved {
        Some(super::Excerpt::Moved)
    } else {
        None
    };
    Place {
        item: item.to_owned(),
        path: path.to_owned(),
        excerpt: if instead.is_some() {
            Vec::new()
        } else {
            excerpt
        },
        instead,
        spots,
    }
}

/// One blind spot: what the run changed, and what it established by changing it.
fn spot(mutant: &MutantRecord, finding: &Finding) -> Spot {
    let blindness = match (mutant.outcome, finding.kind) {
        (_, FindingKind::Timeout) | (Outcome::TimedOut, _) => Blindness::Waited,
        (Outcome::Unreached, _) => Blindness::Never,
        (
            Outcome::CompileRejected
            | Outcome::Killed
            | Outcome::Survived
            | Outcome::Equivalent
            | Outcome::Unconfirmed
            | Outcome::Errored,
            _,
        ) => Blindness::Ran,
    };
    Spot {
        line: mutant.position.line,
        column: mutant.position.column,
        was: mutant.original.clone(),
        now: mutant.replacement.clone(),
        said: blindness.word().to_owned(),
        blindness,
        locator: locator(mutant),
    }
}

/// One finding, as the thing a reader is shown about it.
fn said(finding: &Finding, report: &Report, sources: &Sources) -> Diagnostic {
    let mutant = report
        .mutants
        .iter()
        .find(|one| one.display_id == finding.subject || one.id == finding.subject);
    let unreached = mutant.is_some_and(|one| one.outcome == Outcome::Unreached);
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
/// A finding's detail is written for the record stream, where nothing else is
/// on the line, so it opens by naming the rule and the place. Both are already
/// above it here, and a note that repeats the line above it is one a reader
/// learns to skip.
fn beyond(detail: &str, mutant: Option<&MutantRecord>) -> String {
    let Some(mutant) = mutant else {
        return detail.to_owned();
    };
    let Some((before, after)) = detail.split_once(": ") else {
        return detail.to_owned();
    };
    if before.contains(&mutant.path) && before.contains(&mutant.rule) {
        return after.to_owned();
    }
    detail.to_owned()
}

/// Where a mutation is, and what the run did to the line it is on.
fn site(mutant: &MutantRecord, sources: &Sources) -> Site {
    let excerpt = sources.at(&mutant.path, mutant.position.line, &mutant.original);
    Site {
        path: mutant.path.clone(),
        line: mutant.position.line,
        column: mutant.position.column,
        excerpt,
        label: labelled(mutant),
        width: mutant.original.chars().count().max(1),
    }
}

/// What to say under the mark: what the run changed, and what noticed.
fn labelled(mutant: &MutantRecord) -> String {
    let rule = &mutant.rule;
    match (mutant.outcome, mutant.killed_by.as_deref()) {
        (Outcome::Unreached, _) => format!("{rule} here, and nothing executed it"),
        (_, Some(target)) => format!("{rule} here, and {target} noticed"),
        (_, None) => format!("{rule} here, and nothing noticed"),
    }
}

/// What a reader can do about one mutation, as commands that work when they are typed.
fn answering(mutant: &MutantRecord) -> Vec<Action> {
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
fn locator(mutant: &MutantRecord) -> String {
    if mutant.item.is_empty() || mutant.path.is_empty() {
        return mutant.display_id.clone();
    }
    format!(
        "{}:{}:{}@{}",
        mutant.path, mutant.item, mutant.rule, mutant.position.line
    )
}

/// What a reader is shown about one finding: how much attention it deserves, what it is called, and what it is.
///
/// A mutation nothing reached and a mutation every test ran past are one kind
/// of finding in the report and two different things to be told, so the
/// mutant's own outcome decides between them.
///
/// Matched without a catch-all, so a finding kind added later is one the
/// compiler makes somebody decide how to show rather than one that quietly
/// reads as "the run found something".
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
            "this ran out of time rather than answering",
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
