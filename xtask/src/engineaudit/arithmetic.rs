// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to itself: every identity re-minted, every column re-tallied, every finding re-derived.
//!
//! Nothing here needs anything the run kept beside its report. A row that does
//! not mint the identity it carries, a column that is not the rows it
//! summarises, a score that is not the ratio it says, a survivor with no
//! finding naming it, an exit code that is not what the rows earned — each is
//! the document contradicting itself, and a document that contradicts itself
//! is one no evidence can rescue.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest as _, Sha256};

use super::{
    Audit, DISPLAY_ID_LENGTH, ERRORED_MUTANT, ID_DOMAIN, INCONCLUSIVE, INCONCLUSIVE_MUTANT, KILLED,
    Layer, MET, NOT_RUN, NOT_RUN_MUTANT, Notes, Report, Row, STALE, STALE_EXPECTATION, SURVIVED,
    SURVIVING_MUTANT, TIMED_OUT, UNMATCHED, UNMATCHED_EXPECTATION, UNREACHED, UNREACHED_MUTANT,
    count,
};

/// Every identity re-minted from the row that carries it.
pub(super) fn identity(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Identity);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in &report.mutants {
        if !seen.insert(&row.id) {
            notes.violated(
                row.label(),
                "two rows carry one identity; an identity that names two mutants names neither"
                    .to_owned(),
            );
        }
        if !row.complete() {
            notes.unaudited(
                row.label(),
                "the row omits what minting its identity takes, so whether it is the mutation \
                 it says it is cannot be re-derived"
                    .to_owned(),
            );
            continue;
        }
        let minted = mint(row);
        if minted != row.id {
            notes.violated(
                row.label(),
                format!(
                    "the row's own fields mint {minted} and it carries {}; a row that does not \
                     re-mint is not the mutation it says it is",
                    row.id
                ),
            );
        }
        if !row.id.starts_with(&row.display_id) || row.display_id.len() != DISPLAY_ID_LENGTH {
            notes.violated(
                row.label(),
                format!(
                    "the short identity is {} and the full one is {}; the short form is the \
                     head of the full form and nothing else",
                    row.display_id, row.id
                ),
            );
        }
    }
    dense(report, &mut notes);
}

/// The indices of the accepted and the refused together, which are the whole catalog.
fn dense(report: &Report, notes: &mut Notes<'_>) {
    let mut indices: Vec<u64> = Vec::new();
    let mut missing = false;
    for index in report
        .mutants
        .iter()
        .map(|row| row.index)
        .chain(report.rejections.iter().map(|one| one.index))
    {
        match index {
            Some(at) => indices.push(at),
            None => missing = true,
        }
    }
    if missing {
        notes.unaudited(
            "index",
            "a row carries no catalog index, so whether the catalog lost anything cannot be \
             re-derived"
                .to_owned(),
        );
        return;
    }
    indices.sort_unstable();
    let expected: Vec<u64> = (0..count(indices.len())).collect();
    if indices != expected {
        notes.violated(
            "index",
            format!(
                "the accepted and the refused carry {indices:?} where a catalog of {} runs \
                 from 0; an index that repeats or is missing means a row was lost or counted \
                 twice",
                indices.len()
            ),
        );
    }
}

/// One identity, minted the way the engine mints one.
fn mint(row: &Row) -> String {
    let mut hasher = Sha256::new();
    let version = row.rule_version.unwrap_or_default().to_string();
    let start = row.start_byte.unwrap_or_default().to_string();
    let end = row.end_byte.unwrap_or_default().to_string();
    let original = digest(row.original.as_bytes());
    let replacement = digest(row.replacement.as_bytes());
    for field in [
        ID_DOMAIN,
        &row.path,
        &row.rule,
        &version,
        &start,
        &end,
        &row.source_digest,
        &original,
        &replacement,
    ] {
        let length = u32::try_from(field.len()).unwrap_or(u32::MAX);
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// The lowercase hex SHA-256 of `bytes`.
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Every column re-tallied from the rows, and every equation the contract states.
pub(super) fn accounting(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    for (name, derived) in [
        (KILLED, report.counted(KILLED)),
        (SURVIVED, report.counted(SURVIVED)),
        (TIMED_OUT, report.counted(TIMED_OUT)),
        (INCONCLUSIVE, report.counted(INCONCLUSIVE)),
        (NOT_RUN, report.counted(NOT_RUN)),
        ("cataloged", count(report.mutants.len())),
        ("refused", count(report.rejections.len())),
        (
            UNREACHED,
            count(report.mutants.iter().filter(|row| row.unreached).count()),
        ),
        (
            "expected",
            count(report.mutants.iter().filter(|row| row.expected).count()),
        ),
    ] {
        match report.column(name) {
            None => notes.unaudited(
                name,
                "the report omits this column, so the rows answer to nothing".to_owned(),
            ),
            Some(recorded) if recorded != derived => notes.violated(
                name,
                format!(
                    "the column says {recorded} and the rows come to {derived}; a report that \
                     contradicts itself is not evidence of anything"
                ),
            ),
            Some(_) => {}
        }
    }
    equations(report, &mut notes);
}

/// The equations the outcome columns stand in to each other.
fn equations(report: &Report, notes: &mut Notes<'_>) {
    let sum = [KILLED, SURVIVED, TIMED_OUT, INCONCLUSIVE, "errored"]
        .into_iter()
        .try_fold(0u64, |total, name| {
            report.column(name).map(|one| total.saturating_add(one))
        });
    match (sum, report.column("executed")) {
        (Some(outcomes), Some(executed)) if outcomes != executed => notes.violated(
            "executed",
            format!(
                "the outcome columns come to {outcomes} and the run says it executed \
                 {executed}; every execution ends in exactly one outcome"
            ),
        ),
        (None, _) | (_, None) => notes.unaudited(
            "executed",
            "the report omits a column this equation is over".to_owned(),
        ),
        _ => {}
    }
    match (
        report.column("executed"),
        report.column(NOT_RUN),
        report.column("cataloged"),
    ) {
        (Some(executed), Some(not_run), Some(cataloged))
            if executed.saturating_add(not_run) != cataloged =>
        {
            notes.violated(
                "cataloged",
                format!(
                    "{executed} executed and {not_run} not run come to \
                     {} where the catalog holds {cataloged}; every mutant is one or the other",
                    executed.saturating_add(not_run)
                ),
            );
        }
        (None, _, _) | (_, None, _) | (_, _, None) => notes.unaudited(
            "cataloged",
            "the report omits a column this equation is over".to_owned(),
        ),
        _ => {}
    }
    if let (Some(unreached), Some(not_run)) = (report.column(UNREACHED), report.column(NOT_RUN))
        && unreached > not_run
    {
        notes.violated(
            UNREACHED,
            format!(
                "{unreached} mutants are said to be unreached and only {not_run} were not run; \
                 a mutation nothing reaches is one nothing ran"
            ),
        );
    }
}

/// The score, against the columns it is a ratio over.
pub(super) fn score(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Score);
    let (Some(killed), Some(timed_out), Some(survived)) = (
        report.column(KILLED),
        report.column(TIMED_OUT),
        report.column(SURVIVED),
    ) else {
        notes.unaudited(
            "score",
            "the report omits a column the score is a ratio over".to_owned(),
        );
        return;
    };
    let detected = killed.saturating_add(timed_out);
    let decided = detected.saturating_add(survived);
    match (decided > 0, report.score) {
        (false, Some(_)) => notes.violated(
            "score",
            "the run decided nothing and carries a score anyway; a score of a run that decided \
             nothing is a number about no mutants"
                .to_owned(),
        ),
        (true, None) => notes.violated(
            "score",
            format!("the run decided {decided} mutants and carries no score"),
        ),
        (true, Some((was_detected, was_decided, value))) => {
            if was_detected != detected || was_decided != decided {
                notes.violated(
                    "score",
                    format!(
                        "the score is over {was_detected} of {was_decided} and the columns come \
                         to {detected} of {decided}"
                    ),
                );
            } else if (value - ratio(detected, decided)).abs() > f64::EPSILON {
                notes.violated(
                    "score",
                    format!(
                        "the score reads {value} where {detected} of {decided} is {}",
                        ratio(detected, decided)
                    ),
                );
            }
        }
        (false, None) => {}
    }
}

/// One ratio, as the report computes it.
fn ratio(detected: u64, decided: u64) -> f64 {
    if decided == 0 {
        return 0.0;
    }
    let (detected, decided) = (
        u32::try_from(detected).unwrap_or(u32::MAX),
        u32::try_from(decided).unwrap_or(u32::MAX),
    );
    f64::from(detected) / f64::from(decided)
}

/// The findings against the rows: every kind is a set equality in both directions.
pub(super) fn findings(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Findings);
    let interrupted = report.interrupted == Some(true);
    let raises = |kind: &str, row: &Row| match kind {
        SURVIVING_MUTANT => row.outcome == SURVIVED && !row.expected,
        UNREACHED_MUTANT => row.outcome == NOT_RUN && row.unreached,
        INCONCLUSIVE_MUTANT => row.outcome == INCONCLUSIVE,
        ERRORED_MUTANT => !matches!(
            row.outcome.as_str(),
            SURVIVED | INCONCLUSIVE | NOT_RUN | KILLED | TIMED_OUT
        ),
        NOT_RUN_MUTANT => row.outcome == NOT_RUN && !row.unreached && !interrupted,
        _ => false,
    };
    for kind in [
        SURVIVING_MUTANT,
        UNREACHED_MUTANT,
        INCONCLUSIVE_MUTANT,
        ERRORED_MUTANT,
        NOT_RUN_MUTANT,
    ] {
        let derived: BTreeSet<&str> = report
            .mutants
            .iter()
            .filter(|row| raises(kind, row))
            .map(Row::label)
            .collect();
        let reported: BTreeSet<&str> = report
            .findings
            .iter()
            .filter(|finding| finding.kind == kind)
            .map(|finding| {
                report
                    .row(&finding.mutant)
                    .map_or(finding.mutant.as_str(), Row::label)
            })
            .collect();
        if reported != derived {
            notes.violated(
                kind,
                format!(
                    "the rows say {derived:?} and the findings say {reported:?}; a run that \
                     does not report what it found is not a report"
                ),
            );
        }
    }
    for finding in &report.findings {
        if finding.kind == STALE_EXPECTATION || finding.kind == UNMATCHED_EXPECTATION {
            continue;
        }
        if report.row(&finding.mutant).is_none() {
            notes.violated(
                &finding.kind,
                format!(
                    "the finding names {} and no row of this run does; a finding about a \
                     mutant the run does not hold names nothing",
                    finding.mutant
                ),
            );
        }
    }
}

/// The claims a reviewer declared, as the run left them.
pub(super) fn expectations(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Expectations);
    let mut met: BTreeMap<String, usize> = BTreeMap::new();
    for claim in &report.expectations {
        match claim.standing.as_str() {
            MET => match claim.mutant.as_deref().and_then(|it| report.row(it)) {
                None => notes.violated(
                    &claim.id,
                    "the claim is met and names no row of this run; a claim about a mutant \
                     that is not here was not met by anything"
                        .to_owned(),
                ),
                Some(row) => {
                    if !row.expected {
                        notes.violated(
                            &claim.id,
                            format!(
                                "the claim is met and {} is not marked expected; a row a \
                                 reviewer accounted for says so",
                                row.label()
                            ),
                        );
                    }
                    let seen = met.entry(row.label().to_owned()).or_default();
                    *seen = seen.saturating_add(1);
                }
            },
            STALE => {
                if claim
                    .mutant
                    .as_deref()
                    .and_then(|it| report.row(it))
                    .is_none()
                {
                    notes.violated(
                        &claim.id,
                        "the claim is stale and names no row of this run; a claim the run \
                         contradicted is one the run held"
                            .to_owned(),
                    );
                }
            }
            UNMATCHED => {
                if claim.mutant.is_some() {
                    notes.violated(
                        &claim.id,
                        "the claim is unmatched and names a mutant; a claim that matched \
                         nothing names nothing"
                            .to_owned(),
                    );
                }
            }
            other => notes.unaudited(
                &claim.id,
                format!("the claim stands as {other:?}, which this audit does not know"),
            ),
        }
    }
    accounted(report, &met, &mut notes);
}

/// Every row a reviewer accepted, and every claim the run contradicted, against what the findings say.
fn accounted(report: &Report, met: &BTreeMap<String, usize>, notes: &mut Notes<'_>) {
    for row in report.mutants.iter().filter(|row| row.expected) {
        match met.get(row.label()).copied().unwrap_or_default() {
            1 => {}
            0 => notes.violated(
                row.label(),
                "the row is marked expected and no claim was met by it; a row nobody \
                 accounted for is a finding"
                    .to_owned(),
            ),
            several => notes.violated(
                row.label(),
                format!("{several} claims were met by one row; a mutant is accepted once"),
            ),
        }
    }
    let standing: BTreeSet<&str> = report
        .findings
        .iter()
        .filter(|finding| finding.kind == STALE_EXPECTATION)
        .map(|finding| finding.mutant.as_str())
        .collect();
    let stale: BTreeSet<&str> = report
        .expectations
        .iter()
        .filter(|claim| claim.standing == STALE)
        .map(|claim| claim.id.as_str())
        .collect();
    if !stale.is_empty() && standing.is_empty() {
        notes.violated(
            "stale",
            format!("{stale:?} were contradicted by the run and no finding says so"),
        );
    }
}

/// The exit code, against what the run found.
pub(super) fn exit(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Exit);
    let Some(recorded) = report.exit_code else {
        notes.unaudited(
            "exit_code",
            "the report omits the code the run exited with".to_owned(),
        );
        return;
    };
    let infrastructure = report
        .findings
        .iter()
        .any(|finding| finding.kind == ERRORED_MUTANT || finding.kind == NOT_RUN_MUTANT);
    let derived = if report.interrupted == Some(true) {
        130
    } else if infrastructure {
        2
    } else {
        u64::from(!report.findings.is_empty())
    };
    if recorded != derived {
        notes.violated(
            "exit_code",
            format!(
                "the run exited {recorded} and what it found earns {derived}; a script that \
                 acts on the code acts on the wrong thing"
            ),
        );
    }
}
