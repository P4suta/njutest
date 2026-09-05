// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report as JUnit XML, for the continuous integration a team already has.

use std::fmt::Write as _;

use super::{Report, TargetStatus};

/// The report as one JUnit document.
#[must_use]
pub fn document(report: &Report) -> String {
    let targets = &report.accounting.targets;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _written = writeln!(
        out,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
        escape(&report.repository.root_name),
        targets.selected,
        targets.failed.saturating_add(targets.missing),
        targets.skipped,
        seconds(report.timing.duration_ms)
    );
    let _written = writeln!(
        out,
        "  <testsuite name=\"targets\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        targets.selected,
        targets.failed.saturating_add(targets.missing),
        targets.skipped
    );
    for target in &report.targets {
        write_target(&mut out, target);
    }
    let _written = writeln!(out, "  </testsuite>");
    write_findings(&mut out, report);
    let _written = writeln!(out, "  <properties>");
    for (name, value) in properties(report) {
        let _written = writeln!(
            out,
            "    <property name=\"{}\" value=\"{}\"/>",
            escape(&name),
            escape(&value)
        );
    }
    let _written = writeln!(out, "  </properties>");
    let _written = writeln!(out, "</testsuites>");
    out
}

fn write_target(out: &mut String, target: &super::TargetRecord) {
    let _written = write!(
        out,
        "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\"",
        escape(&target.package),
        escape(&target.name),
        seconds(target.duration_ms)
    );
    let message = target.message.as_deref().unwrap_or_default();
    match target.status {
        TargetStatus::Passed => {
            let _written = writeln!(out, "/>");
        }
        TargetStatus::Skipped => {
            let _written = writeln!(
                out,
                ">\n      <skipped message=\"{}\"/>\n    </testcase>",
                escape(message)
            );
        }
        TargetStatus::Failed | TargetStatus::Missing => {
            let _written = writeln!(
                out,
                ">\n      <failure message=\"{}\"/>\n    </testcase>",
                escape(message)
            );
        }
    }
}

fn write_findings(out: &mut String, report: &Report) {
    if report.findings.is_empty() {
        return;
    }
    let _written = writeln!(
        out,
        "  <testsuite name=\"findings\" tests=\"{}\" failures=\"{}\">",
        report.findings.len(),
        report.findings.len()
    );
    for finding in &report.findings {
        let _written = writeln!(
            out,
            "    <testcase classname=\"{}\" name=\"{}\">\n      \
             <failure message=\"{}\"/>\n    </testcase>",
            escape(&finding.kind_name()),
            escape(&finding.subject),
            escape(&finding.detail)
        );
    }
    let _written = writeln!(out, "  </testsuite>");
}

fn properties(report: &Report) -> Vec<(String, String)> {
    vec![
        ("schema".to_owned(), report.schema.clone()),
        ("run_id".to_owned(), report.run_id.clone()),
        ("verdict".to_owned(), format!("{:?}", report.verdict)),
        ("commit".to_owned(), report.repository.git.commit.clone()),
        ("rustc".to_owned(), report.toolchain.rustc.clone()),
        (
            "mutants_killed".to_owned(),
            report.accounting.mutants.killed.to_string(),
        ),
        (
            "mutants_survived".to_owned(),
            report.accounting.mutants.survived.to_string(),
        ),
    ]
}

fn seconds(milliseconds: u64) -> f64 {
    let whole = milliseconds / 1000;
    let rest = milliseconds % 1000;
    f64::from(u32::try_from(whole).unwrap_or(u32::MAX))
        + f64::from(u32::try_from(rest).unwrap_or_default()) / 1000.0
}

/// XML text with nothing in it that could close a tag.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other if other.is_control() && other != '\n' && other != '\t' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}
