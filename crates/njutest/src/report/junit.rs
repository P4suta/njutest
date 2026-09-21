// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report as JUnit XML, for the continuous integration a team already has.

use std::fmt::Write as _;

use super::{Report, TargetStatus};

/// Why a complete report could not be projected to JUnit without changing a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JunitError {
    /// The report's exact accounting does not fit its durable counter type.
    #[error(transparent)]
    Count(#[from] super::CountError),
    /// Formatting refused the destination.
    #[error("the JUnit formatter refused its string destination")]
    Format(#[from] std::fmt::Error),
}

/// The report as one JUnit document.
///
/// # Errors
/// Returns [`JunitError`] instead of clipping accounting or discarding a formatting failure.
pub fn document(report: &Report) -> Result<String, JunitError> {
    let conclusion = report.conclusion()?;
    let targets = &conclusion.accounting.targets;
    let findings = super::count_of("JUnit findings", conclusion.findings.len())?;
    let tests = super::add("JUnit tests", targets.selected, findings)?;
    let target_failures = super::add("JUnit target failures", targets.failed, targets.missing)?;
    let failures = super::add("JUnit failures", target_failures, findings)?;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    writeln!(
        out,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{}\">",
        escape(&report.repository.root_name),
        tests,
        failures,
        targets.skipped,
        seconds(conclusion.timing.compute_total_ms())
    )?;
    writeln!(
        out,
        "  <testsuite name=\"targets\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        targets.selected, target_failures, targets.skipped
    )?;
    for target in &conclusion.targets {
        write_target(&mut out, target)?;
    }
    writeln!(out, "  </testsuite>")?;
    write_findings(&mut out, &conclusion)?;
    writeln!(out, "  <properties>")?;
    for (name, value) in properties(report, &conclusion) {
        writeln!(
            out,
            "    <property name=\"{}\" value=\"{}\"/>",
            escape(&name),
            escape(&value)
        )?;
    }
    writeln!(out, "  </properties>")?;
    writeln!(out, "</testsuites>")?;
    Ok(out)
}

fn write_target(out: &mut String, target: &super::TargetRecord) -> Result<(), std::fmt::Error> {
    write!(
        out,
        "    <testcase classname=\"{}\" name=\"{}\" time=\"{}\"",
        escape(&target.package),
        escape(&target.name),
        seconds(target.duration_ms)
    )?;
    let message = target.message.as_deref().unwrap_or_default();
    match target.status {
        TargetStatus::Passed => {
            writeln!(out, "/>")?;
        }
        TargetStatus::Skipped => {
            writeln!(
                out,
                ">\n      <skipped message=\"{}\"/>\n    </testcase>",
                escape(message)
            )?;
        }
        TargetStatus::Failed | TargetStatus::Missing => {
            writeln!(
                out,
                ">\n      <failure message=\"{}\"/>\n    </testcase>",
                escape(message)
            )?;
        }
    }
    Ok(())
}

fn write_findings(out: &mut String, report: &super::Conclusion) -> Result<(), std::fmt::Error> {
    if report.findings.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "  <testsuite name=\"findings\" tests=\"{}\" failures=\"{}\">",
        report.findings.len(),
        report.findings.len()
    )?;
    for finding in &report.findings {
        writeln!(
            out,
            "    <testcase classname=\"{}\" name=\"{}\">\n      \
             <failure message=\"{}\"/>\n    </testcase>",
            escape(&finding.kind_name()),
            escape(&finding.subject),
            escape(&finding.detail)
        )?;
    }
    writeln!(out, "  </testsuite>")?;
    Ok(())
}

fn properties(report: &Report, conclusion: &super::Conclusion) -> Vec<(String, String)> {
    vec![
        ("schema".to_owned(), report.schema.clone()),
        ("run_id".to_owned(), report.run_id.to_string()),
        ("verdict".to_owned(), format!("{:?}", conclusion.verdict)),
        (
            "commit".to_owned(),
            report.repository.git.commit().to_owned(),
        ),
        (
            "builds".to_owned(),
            report
                .builds()
                .map(|build| build.name.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "mutants_killed".to_owned(),
            conclusion.accounting.mutants.killed.to_string(),
        ),
        (
            "mutants_survived".to_owned(),
            conclusion.accounting.mutants.survived.to_string(),
        ),
    ]
}

fn seconds(milliseconds: u64) -> String {
    let whole = milliseconds / 1000;
    let rest = milliseconds % 1000;
    format!("{whole}.{rest:03}")
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
