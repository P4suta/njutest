// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report as one self-contained page.

use super::{Conclusion, Report, TargetStatus};

/// The report as one HTML document with no external resource of any kind.
/// # Errors
/// Returns the checked projection error retained by the completed report.
pub fn document(report: &Report) -> Result<String, super::CountError> {
    let conclusion = report.conclusion()?;
    let mut out = String::new();
    crate::text::append(
        &mut out,
        format_args!(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{verdict} — {root}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n",
            verdict = escape(&format!("{:?}", conclusion.verdict)),
            root = escape(&report.repository.root_name),
        ),
    );
    crate::text::append(
        &mut out,
        format_args!(
            "<h1 class=\"{class}\">{verdict}</h1>\n<p class=\"identity\">{root} · {run}</p>\n",
            class = if conclusion.verdict.is_assurance() {
                "assured"
            } else {
                "not-assured"
            },
            verdict = escape(&format!("{:?}", conclusion.verdict)),
            root = escape(&report.repository.root_name),
            run = escape(report.run_id.as_str()),
        ),
    );

    section(&mut out, "Findings", &findings(&conclusion));
    section(&mut out, "Accounting", &accounting(&conclusion));
    section(&mut out, "Targets", &targets(&conclusion));
    section(&mut out, "Mutants", &mutants(&conclusion));
    section(&mut out, "Limitations", &limitations(&conclusion));
    section(&mut out, "Identity", &identity(report, &conclusion));

    out.push_str("</body>\n</html>\n");
    Ok(out)
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem auto;max-width:60rem;\
line-height:1.5}h1{margin:0}h1.assured{color:#0a6}h1.not-assured{color:#a30}\
.identity{color:#666;margin-top:0}table{border-collapse:collapse;width:100%}\
th,td{text-align:left;padding:.25rem .5rem;border-bottom:1px solid #ddd;\
vertical-align:top}code{font-family:ui-monospace,monospace}\
td.survived,td.failed,td.missing{color:#a30}td.killed,td.passed{color:#0a6}\
p.none{color:#666}";

fn section(out: &mut String, title: &str, body: &str) {
    out.push_str("<h2>");
    out.push_str(&escape(title));
    out.push_str("</h2>\n");
    out.push_str(body);
    out.push('\n');
}

fn findings(report: &Conclusion) -> String {
    if report.findings.is_empty() {
        return "<p class=\"none\">Nothing was found.</p>".to_owned();
    }
    let rows = report.findings.iter().map(|finding| {
        let at = finding.position.map_or_else(String::new, |position| {
            format!("{}:{}", position.line, position.column)
        });
        format!(
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            escape(&finding.kind_name()),
            escape(&finding.subject),
            escape(&finding.detail),
            escape(&at)
        )
    });
    table(&["Kind", "Subject", "Detail", "At"], rows)
}

fn accounting(report: &Conclusion) -> String {
    let targets = report.accounting.targets;
    let mutants = report.accounting.mutants;
    let rows = [
        ("targets selected", targets.selected),
        ("targets passed", targets.passed),
        ("targets failed", targets.failed),
        ("targets skipped", targets.skipped),
        ("targets missing", targets.missing),
        ("mutants cataloged", mutants.cataloged),
        ("mutants rejected", mutants.rejected),
        ("mutants executed", mutants.executed),
        ("mutants killed", mutants.killed),
        ("mutants survived", mutants.survived),
        ("mutants unreached", mutants.unreached),
        ("mutants accepted", mutants.accepted),
    ]
    .into_iter()
    .map(|(name, count)| format!("<tr><td>{}</td><td>{count}</td></tr>", escape(name)));
    table(&["What", "How many"], rows)
}

fn targets(report: &Conclusion) -> String {
    let rows = report.targets.iter().map(|target| {
        format!(
            "<tr><td class=\"{status}\">{status}</td><td><code>{name}</code></td>\
             <td>{duration} ms</td><td>{message}</td></tr>",
            status = escape(status_name(target.status)),
            name = escape(&target.name),
            duration = target.duration_ms,
            message = escape(target.message.as_deref().unwrap_or_default()),
        )
    });
    table(&["Status", "Target", "Took", "Said"], rows)
}

fn mutants(report: &Conclusion) -> String {
    if report.mutants.is_empty() {
        return "<p class=\"none\">No mutation was measured.</p>".to_owned();
    }
    let rows = report.mutants.iter().map(|mutant| {
        let decided_by = mutant
            .by_build()
            .iter()
            .filter_map(|fact| {
                fact.outcome()
                    .decided_by()
                    .map(|target| format!("{}: {target}", fact.build()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "<tr><td class=\"{outcome}\">{outcome}</td><td><code>{id}</code></td>\
             <td><code>{path}:{line}</code></td><td>{rule}</td><td>{by}</td></tr>",
            outcome = escape(mutant.decision().name()),
            id = escape(mutant.display_id()),
            path = escape(mutant.path()),
            line = mutant.position().line,
            rule = escape(mutant.rule()),
            by = escape(&decided_by),
        )
    });
    table(&["Outcome", "Mutant", "Where", "Rule", "Noticed by"], rows)
}

fn limitations(report: &Conclusion) -> String {
    if report.limitations.is_empty() {
        return "<p class=\"none\">The report claims everything it measured.</p>".to_owned();
    }
    let rows = report.limitations.iter().map(|limitation| {
        format!(
            "<tr><td><code>{}</code></td><td>{}</td></tr>",
            escape(&limitation.name),
            escape(&limitation.detail)
        )
    });
    table(&["Name", "What is not claimed"], rows)
}

fn identity(report: &Report, conclusion: &Conclusion) -> String {
    let mut rows = vec![
        ("commit", report.repository.git.commit().to_owned()),
        ("branch", report.repository.git.branch().to_owned()),
        ("dirty", report.repository.git.dirty().to_string()),
        ("started", conclusion.timing.wall().started().to_owned()),
        (
            "compute total",
            format!("{} ms", conclusion.timing.compute_total_ms()),
        ),
        (
            "configuration",
            report.repository.configuration_digest.clone(),
        ),
    ];
    for build in report.builds() {
        let baseline = build.baseline();
        rows.push(("build", build.name.as_str().to_owned()));
        rows.push(("rustc", baseline.toolchain.rustc.clone()));
        rows.push(("target", baseline.toolchain.target.clone()));
    }
    let rows = rows.into_iter().map(|(name, value)| {
        format!(
            "<tr><td>{}</td><td><code>{}</code></td></tr>",
            escape(name),
            escape(&value)
        )
    });
    table(&["What", "Value"], rows)
}

fn table(headings: &[&str], rows: impl Iterator<Item = String>) -> String {
    let mut out = String::from("<table>\n<tr>");
    for heading in headings {
        out.push_str("<th>");
        out.push_str(&escape(heading));
        out.push_str("</th>");
    }
    out.push_str("</tr>\n");
    for row in rows {
        out.push_str(&row);
        out.push('\n');
    }
    out.push_str("</table>");
    out
}

const fn status_name(status: TargetStatus) -> &'static str {
    match status {
        TargetStatus::Passed => "passed",
        TargetStatus::Failed => "failed",
        TargetStatus::Skipped => "skipped",
        TargetStatus::Missing => "missing",
    }
}

/// HTML text with nothing in it a browser would read as markup.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other if other.is_control() && other != '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}
