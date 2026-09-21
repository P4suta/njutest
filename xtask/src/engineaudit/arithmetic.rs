// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to itself: every identity re-minted, every column re-tallied, every finding re-derived.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest as _, Sha256};

use super::{
    Audit, DISCHARGED, DISCHARGED_MUTANT, DISPLAY_ID_LENGTH, ERRORED, ERRORED_MUTANT, ID_DOMAIN,
    INCONCLUSIVE, INCONCLUSIVE_MUTANT, KILLED, Layer, MET, NOT_RUN, NOT_RUN_MUTANT, Notes, Report,
    Row, STALE, STALE_EXPECTATION, STEP_LIMIT_REACHED, STEP_LIMIT_REACHED_MUTANT, STOPPED_EARLY,
    SURVIVED, SURVIVING_MUTANT, UNMATCHED, UNMATCHED_EXPECTATION, UNREACHED, UNREACHED_MUTANT,
    UNSELECTED, WAITED, WAITED_MUTANT, count,
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
        let Ok(minted) = mint(row) else {
            notes.violated(
                row.label(),
                "one identity field is too large for the four-byte identity framing".to_owned(),
            );
            continue;
        };
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
    let mut indices: Vec<u64> = report
        .mutants
        .iter()
        .map(|row| row.index)
        .chain(report.rejections.iter().map(|one| one.index))
        .collect();
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
fn mint(row: &Row) -> Result<String, IdentityWidthError> {
    let mut hasher = Sha256::new();
    let version = row.rule_version.to_string();
    let start = row.start_byte.to_string();
    let end = row.end_byte.to_string();
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
        let length = u32::try_from(field.len()).map_err(|_overflow| IdentityWidthError)?;
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IdentityWidthError;

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
        (STEP_LIMIT_REACHED, report.counted(STEP_LIMIT_REACHED)),
        (WAITED, report.counted(WAITED)),
        (INCONCLUSIVE, report.counted(INCONCLUSIVE)),
        (ERRORED, report.counted(ERRORED)),
        (NOT_RUN, report.counted(NOT_RUN)),
        ("cataloged", count(report.mutants.len())),
        ("refused", count(report.rejections.len())),
        (
            UNREACHED,
            count(report.mutants.iter().filter(|row| row.unreached).count()),
        ),
        (
            DISCHARGED,
            count(
                report
                    .mutants
                    .iter()
                    .filter(|row| row.not_run(DISCHARGED))
                    .count(),
            ),
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
    skipped(report, &mut notes);
    step_notices(report, &mut notes);
    equations(report, &mut notes);
}

/// The skipped-place column is the checked sum of every skip record.
fn skipped(report: &Report, notes: &mut Notes<'_>) {
    let mut derived = 0u64;
    for count in &report.skip_counts {
        let Some(next) = derived.checked_add(*count) else {
            notes.violated(
                "skipped",
                "the skip-record sum exceeds the report's integer width".to_owned(),
            );
            return;
        };
        derived = next;
    }
    match report.column("skipped") {
        Some(recorded) if recorded != derived => notes.violated(
            "skipped",
            format!(
                "the skip records come to {derived} and the accounting says {recorded}; every \
                 skipped place belongs to exactly one record"
            ),
        ),
        None => notes.unaudited(
            "skipped",
            "the report omits its skipped-place accounting column".to_owned(),
        ),
        Some(_) => {}
    }
}

/// A step-limit outcome is licensed only by a notice internally bound to this row and run.
fn step_notices(report: &Report, notes: &mut Notes<'_>) {
    for row in &report.mutants {
        match (row.outcome.as_str(), &row.step_notice) {
            (STEP_LIMIT_REACHED, None) => notes.violated(
                row.label(),
                "the row reached its step limit and carries no verified runtime notice".to_owned(),
            ),
            (STEP_LIMIT_REACHED, Some(notice)) => {
                if notice.nonce.len() != 32
                    || !notice
                        .nonce
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    notes.violated(
                        row.label(),
                        "the step notice nonce is not 16 lowercase hexadecimal bytes".to_owned(),
                    );
                }
                if notice.catalog != report.catalog_digest {
                    notes.violated(
                        row.label(),
                        "the step notice names another catalog".to_owned(),
                    );
                }
                if notice.mutant != row.id {
                    notes.violated(
                        row.label(),
                        "the step notice names another mutant".to_owned(),
                    );
                }
                if Some(notice.limit) != report.mutant_steps {
                    notes.violated(
                        row.label(),
                        "the step notice allowance is not the allowance this run selected"
                            .to_owned(),
                    );
                }
                if notice.limit.checked_add(1) != Some(notice.observed) {
                    notes.violated(
                        row.label(),
                        "the step notice observed count is not exactly one past its allowance"
                            .to_owned(),
                    );
                }
            }
            (_, Some(_)) => notes.violated(
                row.label(),
                "the row carries a step notice without a step-limit outcome".to_owned(),
            ),
            (_, None) => {}
        }
    }
}

/// The equations the outcome columns stand in to each other.
fn equations(report: &Report, notes: &mut Notes<'_>) {
    let sum = match column_sum(
        report,
        &[
            KILLED,
            SURVIVED,
            STEP_LIMIT_REACHED,
            WAITED,
            INCONCLUSIVE,
            ERRORED,
        ],
    ) {
        ColumnSum::Complete(total) => Some(total),
        ColumnSum::Missing => None,
        ColumnSum::Overflow => {
            notes.violated(
                "executed",
                "the outcome-column sum exceeds the report's integer width".to_owned(),
            );
            None
        }
    };
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
        (Some(executed), Some(not_run), Some(cataloged)) => match executed.checked_add(not_run) {
            None => notes.violated(
                "cataloged",
                "executed plus not-run exceeds the report's integer width".to_owned(),
            ),
            Some(total) if total != cataloged => notes.violated(
                "cataloged",
                format!(
                    "{executed} executed and {not_run} not run come to {total} where the catalog \
                     holds {cataloged}; every mutant is one or the other"
                ),
            ),
            Some(_) => {}
        },
        (None, _, _) | (_, None, _) | (_, _, None) => notes.unaudited(
            "cataloged",
            "the report omits a column this equation is over".to_owned(),
        ),
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

enum ColumnSum {
    Complete(u64),
    Missing,
    Overflow,
}

fn column_sum(report: &Report, names: &[&str]) -> ColumnSum {
    let mut total = 0u64;
    for name in names {
        let Some(value) = report.column(name) else {
            return ColumnSum::Missing;
        };
        let Some(next) = total.checked_add(value) else {
            return ColumnSum::Overflow;
        };
        total = next;
    }
    ColumnSum::Complete(total)
}

/// The score, against the columns it is a ratio over.
pub(super) fn score(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Score);
    let (Some(killed), Some(survived)) = (report.column(KILLED), report.column(SURVIVED)) else {
        notes.unaudited(
            "score",
            "the report omits a column the score is a ratio over".to_owned(),
        );
        return;
    };
    let detected = killed;
    let Some(decided) = killed.checked_add(survived) else {
        notes.violated(
            "score",
            "killed plus survived exceeds the report's integer width".to_owned(),
        );
        return;
    };
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
    widen(detected) / widen(decided)
}

fn widen(value: u64) -> f64 {
    let bytes = value.to_be_bytes();
    let high = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let low = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    f64::from(high).mul_add(4_294_967_296.0, f64::from(low))
}

/// The findings against the rows: every kind is a set equality in both directions.
pub(super) fn findings(report: &Report, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Findings);
    let interrupted = report.interrupted;
    let raises = |kind: &str, row: &Row| match kind {
        SURVIVING_MUTANT => row.outcome == SURVIVED && !row.expected,
        STEP_LIMIT_REACHED_MUTANT => row.outcome == STEP_LIMIT_REACHED,
        WAITED_MUTANT => row.outcome == WAITED,
        UNREACHED_MUTANT => row.outcome == NOT_RUN && row.unreached,
        DISCHARGED_MUTANT => row.outcome == NOT_RUN && row.not_run(DISCHARGED),
        INCONCLUSIVE_MUTANT => row.outcome == INCONCLUSIVE,
        ERRORED_MUTANT => row.outcome == ERRORED,
        NOT_RUN_MUTANT => {
            row.outcome == NOT_RUN
                && !row.unreached
                && !row.not_run(DISCHARGED)
                && !row.not_run(UNSELECTED)
                && !row.not_run(STOPPED_EARLY)
                && !interrupted
        }
        _ => false,
    };
    for kind in [
        SURVIVING_MUTANT,
        STEP_LIMIT_REACHED_MUTANT,
        WAITED_MUTANT,
        UNREACHED_MUTANT,
        DISCHARGED_MUTANT,
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
            .filter_map(|finding| {
                finding
                    .mutant
                    .as_deref()
                    .map(|mutant| report.row(mutant).map_or(mutant, Row::label))
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
        let Some(mutant) = finding.mutant.as_deref() else {
            notes.violated(
                finding.kind.as_str(),
                "this mutant finding names no mutant; a finding about nothing establishes \
                 nothing"
                    .to_owned(),
            );
            continue;
        };
        if report.row(mutant).is_none() {
            notes.violated(
                finding.kind.as_str(),
                format!(
                    "the finding names {mutant} and no row of this run does; a finding about a \
                     mutant the run does not hold names nothing"
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
                    let Some(next) = seen.checked_add(1) else {
                        notes.violated(
                            row.label(),
                            "the number of matching claims exceeds this platform's address \
                             space"
                                .to_owned(),
                        );
                        return;
                    };
                    *seen = next;
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
        .filter_map(|finding| finding.mutant.as_deref())
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
    let recorded = report.exit_code;
    let infrastructure = report.findings.iter().any(|finding| {
        matches!(
            finding.kind.as_str(),
            ERRORED_MUTANT | NOT_RUN_MUTANT | STEP_LIMIT_REACHED_MUTANT | WAITED_MUTANT
        )
    });
    let derived = if report.interrupted {
        130
    } else if infrastructure {
        2
    } else {
        u8::from(!report.findings.is_empty())
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
