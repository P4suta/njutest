// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The record stream: one line per fact, tab-separated, the kind first.

use super::{Conclusion, Report, TargetStatus};

/// The record stream inside a run directory.
pub const FILE_NAME: &str = "njutest-assurance-report-v1.lines";

/// The whole report as records, each line terminated, and where the run's document is from the project's own root.
///
/// A reader who has the stream should not have to know the layout to find the document beside it: a `REPORT` record is how a script that read the verdict reads the rest, and guessing a path is how one stops working the day a project moves its report directory.
/// # Errors
/// Returns the checked projection error retained by the completed report.
pub fn kept(report: &Report, document: &str) -> Result<String, super::CountError> {
    written(report, Some(document))
}

/// The whole report as records, each line terminated.
/// # Errors
/// Returns the checked projection error retained by the completed report.
pub fn stream(report: &Report) -> Result<String, super::CountError> {
    written(report, None)
}

fn written(report: &Report, document: Option<&str>) -> Result<String, super::CountError> {
    let mut out = String::new();
    let conclusion = report.conclusion()?;
    identity(report, &conclusion, &mut out);
    for target in &conclusion.targets {
        record(
            &mut out,
            "TARGET",
            &[
                status_name(target.status),
                &format!("{}ms", target.duration_ms),
                &target.id,
                &target.name,
            ],
        );
        append_optional(&mut out, target.message.as_deref());
    }
    for mutant in &conclusion.mutants {
        record(
            &mut out,
            "MUTANT",
            &[
                mutant.decision().name(),
                mutant.display_id(),
                &format!(
                    "{}:{}:{}",
                    mutant.path(),
                    mutant.position().line,
                    mutant.position().column
                ),
                mutant.rule(),
            ],
        );
        for fact in mutant.by_build() {
            if let Some(killed_by) = fact.outcome().decided_by() {
                append(
                    &mut out,
                    &format!("build={} killed_by={killed_by}", fact.build()),
                );
            }
            if let Some(provenance) = fact.reuse().0.read_back() {
                append(
                    &mut out,
                    &format!("build={} reused={provenance}", fact.build()),
                );
            }
        }
        out.push('\n');
    }
    for finding in &conclusion.findings {
        let kind = wire_name(&finding.kind);
        record(&mut out, "FINDING", &[kind.as_str(), &finding.subject]);
        append(&mut out, &finding.detail);
        if let Some(position) = finding.position {
            append(&mut out, &format!("{}:{}", position.line, position.column));
        }
        out.push('\n');
    }
    for limitation in &conclusion.limitations {
        record(&mut out, "LIMITATION", &[&limitation.name]);
        append(&mut out, &limitation.detail);
        out.push('\n');
    }
    onward(&conclusion, &mut out);
    accounting(&conclusion, &mut out);
    if let Some(document) = document {
        record(&mut out, "REPORT", &[document]);
        out.push('\n');
    }
    record(&mut out, "VERDICT", &[&verdict_name(report)]);
    out.push('\n');
    Ok(out)
}

/// Where a reader goes from a wall of findings, which is the next thing they want.
///
/// A survivor is a decision to make, not a fact to file: either the tests have a gap or the code has a claim in it somebody should write down.
/// `explain` answers the first and `accept` records the second, and nothing on the way here named either.
/// How a reader names this mutation again, which has to hold after they have changed the file.
fn onward(report: &Conclusion, out: &mut String) {
    let Some(first) = report
        .mutants
        .iter()
        .find(|one| one.decision() == super::Decision::Unnoticed)
    else {
        return;
    };
    record(
        out,
        "NEXT",
        &[
            &format!("njutest explain {}", crate::naming::locator(first)),
            "says which tests reached it",
            &format!(
                "njutest accept {} --reason \"...\"",
                crate::naming::locator(first)
            ),
            "records why it is not a gap",
        ],
    );
    out.push('\n');
}

/// What the run was and what it ran on.
fn identity(report: &Report, conclusion: &Conclusion, out: &mut String) {
    let git = &report.repository.git;
    record(
        out,
        "RUN",
        &[
            &format!("run={}", report.run_id),
            &format!("kind={}", run_kind_name(report)),
            &format!("contract={}", contract_name(report)),
        ],
    );
    out.push('\n');
    for build in report.builds() {
        let baseline = build.baseline();
        record(out, "BUILD", &[&format!("name={}", build.name)]);
        out.push('\n');
        record(
            out,
            "TOOLCHAIN",
            &[
                &format!("rustc={}", baseline.toolchain.rustc),
                &format!("cargo={}", baseline.toolchain.cargo),
                &format!("target={}", baseline.toolchain.target),
                &format!("os={}", baseline.toolchain.os),
                &format!("arch={}", baseline.toolchain.arch),
            ],
        );
        out.push('\n');
    }
    record(
        out,
        "REPOSITORY",
        &[
            &format!("root={}", report.repository.root_name),
            &format!("packages={}", report.repository.packages.len()),
            &format!("commit={}", git.commit()),
            &format!("branch={}", git.branch()),
            &format!("dirty={}", git.dirty()),
        ],
    );
    out.push('\n');
    record(
        out,
        "SCOPE",
        &[
            &format!("requested={}", named(&report.scope.requested_packages)),
            &format!("resolved={}", named(&report.scope.resolved_packages)),
            &format!("included={}", named(&report.scope.included)),
            &format!("excluded={}", named(&report.scope.excluded)),
            &format!(
                "from={}",
                if report.scope.configuration.is_empty() {
                    "(defaults)"
                } else {
                    &report.scope.configuration
                }
            ),
        ],
    );
    out.push('\n');
    record(
        out,
        "TIMING",
        &[
            &format!("started={}", conclusion.timing.wall().started()),
            &format!("finished={}", conclusion.timing.wall().finished()),
            &format!("compute_total_ms={}", conclusion.timing.compute_total_ms()),
        ],
    );
    out.push('\n');
}

/// A list as a reader sees it, saying that it is empty rather than being empty.
fn named(values: &[String]) -> String {
    if values.is_empty() {
        return String::from("(nothing named)");
    }
    values.join(",")
}

/// What the run counted.
fn accounting(report: &Conclusion, out: &mut String) {
    let targets = report.accounting.targets;
    record(
        out,
        "TARGETS",
        &[
            &format!("selected={}", targets.selected),
            &format!("passed={}", targets.passed),
            &format!("failed={}", targets.failed),
            &format!("skipped={}", targets.skipped),
            &format!("missing={}", targets.missing),
        ],
    );
    out.push('\n');
    let mutants = report.accounting.mutants;
    record(
        out,
        "MUTANTS",
        &[
            &format!("cataloged={}", mutants.cataloged),
            &format!("rejected={}", mutants.rejected),
            &format!("executed={}", mutants.executed),
            &format!("unreached={}", mutants.unreached),
        ],
    );
    out.push('\n');
    record(
        out,
        "OUTCOMES",
        &[
            &format!("killed={}", mutants.killed),
            &format!("survived={}", mutants.survived),
            &format!("step_limit_reached={}", mutants.step_limit_reached),
            &format!("waited={}", mutants.waited),
            &format!("equivalent={}", mutants.equivalent),
        ],
    );
    out.push('\n');
    record(
        out,
        "WITHIN_OUTCOMES",
        &[
            &format!("accepted_of_survived={}", mutants.accepted),
            &format!("reused_of_killed={}", mutants.reused_killed),
            &format!("reused_of_survived={}", mutants.reused_survived),
        ],
    );
    out.push('\n');
    dimensions(report, out);
    for measured in &report.accounting.soundness_by_build {
        let soundness = measured.accounting();
        record(
            out,
            "SOUNDNESS",
            &[
                &format!("build={}", measured.build()),
                &format!("unsafe_items={}", soundness.unsafe_items),
                &format!("packages_with_unsafe={}", soundness.packages_with_unsafe),
                &format!("was_executed={}", soundness.executed),
            ],
        );
        out.push('\n');
    }
}

/// What the run counted along every dimension beside the mutations: the faults it put, and one record for each column of the matrix.
fn dimensions(report: &Conclusion, out: &mut String) {
    let faults = report.accounting.faults;
    if faults.sites > 0 {
        record(
            out,
            "FAULTS",
            &[
                &format!("sites={}", faults.sites),
                &format!("noticed={}", faults.noticed),
                &format!("unnoticed={}", faults.unnoticed),
                &format!("unreached={}", faults.unreached),
                &format!("waited={}", faults.waited),
                &format!("undecided={}", faults.undecided),
                &format!("not_put={}", faults.not_put),
            ],
        );
        out.push('\n');
    }
    for row in &report.matrix {
        dimension(row, out);
    }
}

/// One column of the matrix: the dimension, its state, and what it counted or why it counted nothing.
fn dimension(row: &super::matrix::Row, out: &mut String) {
    record(
        out,
        "DIMENSION",
        &[row.dimension.name(), row.column.state()],
    );
    match &row.column {
        super::matrix::Column::Measured {
            catalogued,
            answered,
            holes,
            speaks_not_about,
        } => {
            append(out, &format!("catalogued={catalogued}"));
            append(out, &format!("answered={answered}"));
            append(out, &format!("holes={holes}"));
            append_optional(
                out,
                (!speaks_not_about.is_empty())
                    .then(|| format!("speaks_not_about={}", speaks_not_about.join("; ")))
                    .as_deref(),
            );
        }
        super::matrix::Column::Unmeasured { why } | super::matrix::Column::NothingToAsk { why } => {
            append_optional(out, Some(why));
        }
        super::matrix::Column::NotAsked | super::matrix::Column::NotInThisRelease => {
            out.push('\n');
        }
    }
}

/// Writes `kind` and its fields, without terminating the record.
fn record(out: &mut String, kind: &str, fields: &[&str]) {
    out.push_str(kind);
    for field in fields {
        append(out, field);
    }
}

/// Adds one more field to the record being written.
fn append(out: &mut String, field: &str) {
    out.push('\t');
    out.push_str(&escape(field));
}

/// Adds a field that may not be there, and ends the record either way.
/// A record never ends in an empty field: absent is absent.
fn append_optional(out: &mut String, field: Option<&str>) {
    if let Some(field) = field {
        append(out, field);
    }
    out.push('\n');
}

/// The text of `value` with nothing in it that a terminal or a reader would act on: no record separator, no field separator, no cursor movement, no colour.
#[must_use]
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            other if other.is_control() => {
                let code = u32::from(other);
                out.push_str("\\u{");
                if code > 0xf {
                    out.extend(char::from_digit(code / 16, 16));
                }
                out.extend(char::from_digit(code % 16, 16));
                out.push('}');
            }
            other => out.push(other),
        }
    }
    out
}

/// The wire name of a target's terminal state.
const fn status_name(status: TargetStatus) -> &'static str {
    match status {
        TargetStatus::Passed => "passed",
        TargetStatus::Failed => "failed",
        TargetStatus::Skipped => "skipped",
        TargetStatus::Missing => "missing",
    }
}

/// The wire name of a value the model serializes, taken from the model so the two projections can never drift.
fn wire_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(_) | Err(_) => super::UNAVAILABLE.to_owned(),
    }
}

fn run_kind_name(report: &Report) -> String {
    wire_name(&report.run_kind)
}

fn contract_name(report: &Report) -> String {
    wire_name(&report.contract)
}

fn verdict_name(report: &Report) -> String {
    wire_name(&report.verdict())
}
