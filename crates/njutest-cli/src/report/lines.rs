// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The record stream: one line per fact, tab-separated, the kind first.

use super::{Report, TargetStatus};

/// The record stream inside a run directory.
pub const FILE_NAME: &str = "njutest-assurance-report-v1.lines";

/// The whole report as records, each line terminated.
#[must_use]
pub fn stream(report: &Report) -> String {
    let mut out = String::new();
    identity(report, &mut out);
    accounting(report, &mut out);
    for target in &report.targets {
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
    for mutant in &report.mutants {
        record(
            &mut out,
            "MUTANT",
            &[
                &mutant.outcome,
                &mutant.display_id,
                &format!(
                    "{}:{}:{}",
                    mutant.path, mutant.position.line, mutant.position.column
                ),
                &mutant.rule,
            ],
        );
        if let Some(killed_by) = mutant.killed_by.as_deref() {
            append(&mut out, &format!("killed_by={killed_by}"));
        }
        if let Some(provenance) = mutant.source_run_id.as_deref() {
            append(&mut out, &format!("reused={provenance}"));
        }
        out.push('\n');
    }
    for finding in &report.findings {
        let kind = wire_name(&finding.kind);
        record(&mut out, "FINDING", &[kind.as_str(), &finding.subject]);
        append(&mut out, &finding.detail);
        if let Some(position) = finding.position {
            append(&mut out, &format!("{}:{}", position.line, position.column));
        }
        out.push('\n');
    }
    for limitation in &report.limitations {
        record(&mut out, "LIMITATION", &[&limitation.name]);
        append(&mut out, &limitation.detail);
        out.push('\n');
    }
    record(&mut out, "VERDICT", &[&verdict_name(report)]);
    out.push('\n');
    out
}

/// What the run was and what it ran on.
fn identity(report: &Report, out: &mut String) {
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
    record(
        out,
        "TOOLCHAIN",
        &[
            &format!("rustc={}", report.toolchain.rustc),
            &format!("cargo={}", report.toolchain.cargo),
            &format!("target={}", report.toolchain.target),
            &format!("os={}", report.toolchain.os),
            &format!("arch={}", report.toolchain.arch),
        ],
    );
    out.push('\n');
    record(
        out,
        "REPOSITORY",
        &[
            &format!("root={}", report.repository.root_name),
            &format!("packages={}", report.repository.packages.len()),
            &format!("commit={}", git.commit),
            &format!("branch={}", git.branch),
            &format!("dirty={}", git.dirty),
        ],
    );
    out.push('\n');
    record(
        out,
        "SCOPE",
        &[
            &format!("requested={}", report.scope.requested_packages.join(",")),
            &format!("resolved={}", report.scope.resolved_packages.join(",")),
            &format!("excluded={}", report.scope.excluded.join(",")),
        ],
    );
    out.push('\n');
    record(
        out,
        "TIMING",
        &[
            &format!("started={}", report.timing.started),
            &format!("finished={}", report.timing.finished),
            &format!("duration_ms={}", report.timing.duration_ms),
        ],
    );
    out.push('\n');
}

/// What the run counted.
fn accounting(report: &Report, out: &mut String) {
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
            &format!("killed={}", mutants.killed),
            &format!("survived={}", mutants.survived),
            &format!("timed_out={}", mutants.timed_out),
            &format!("unreached={}", mutants.unreached),
            &format!("accepted={}", mutants.accepted),
            &format!("reused_killed={}", mutants.reused_killed),
            &format!("reused_survived={}", mutants.reused_survived),
        ],
    );
    out.push('\n');
    let soundness = report.accounting.soundness;
    record(
        out,
        "SOUNDNESS",
        &[
            &format!("unsafe_items={}", soundness.unsafe_items),
            &format!("packages={}", soundness.packages_with_unsafe),
            &format!("executed={}", soundness.executed),
        ],
    );
    out.push('\n');
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

/// Adds a field that may not be there, and ends the record either way. A record never ends in an empty field: absent is absent.
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
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| super::UNAVAILABLE.to_owned())
}

fn run_kind_name(report: &Report) -> String {
    wire_name(&report.run_kind)
}

fn contract_name(report: &Report) -> String {
    wire_name(&report.contract)
}

fn verdict_name(report: &Report) -> String {
    wire_name(&report.verdict)
}
