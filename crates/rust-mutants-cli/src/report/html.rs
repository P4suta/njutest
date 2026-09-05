// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as one self-contained page: no script, no font, no stylesheet, nothing to fetch.

use std::fmt::Write as _;

use super::run::{RunDocument, RunMutantDocument};

/// The run as one HTML document with no external resource of any kind.
#[must_use]
pub fn document(document: &RunDocument) -> String {
    let mut out = String::new();
    let _written = write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>rust-mutants {run}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         <h1>{root}</h1>\n<p class=\"identity\">{run} · catalog {catalog}</p>\n",
        run = escape(&document.run.id),
        root = escape(&document.workspace.root_name),
        catalog = escape(document.workspace.catalog_digest.get(..12).unwrap_or("")),
    );
    section(&mut out, "Score", &score(document));
    section(&mut out, "Accounting", &accounting(document));
    section(&mut out, "Findings", &findings(document));
    section(&mut out, "Mutants", &mutants(&document.mutants));
    out.push_str("</body>\n</html>\n");
    out
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem auto;max-width:64rem;\
line-height:1.5}h1{margin:0}.identity{color:#666;margin-top:0}\
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:.25rem .5rem;\
border-bottom:1px solid #ddd;vertical-align:top}code{font-family:ui-monospace,monospace}\
td.survived,td.errored,td.inconclusive,td.not_run{color:#a30}\
td.killed,td.timed_out{color:#0a6}p.none{color:#666}\
.score{font-size:2rem;font-weight:600}";

fn section(out: &mut String, title: &str, body: &str) {
    let _written = write!(out, "<h2>{}</h2>\n{body}\n", escape(title));
}

fn score(document: &RunDocument) -> String {
    document.score.as_ref().map_or_else(
        || {
            "<p class=\"none\">The run decided nothing, which is not a score of zero.</p>"
                .to_owned()
        },
        |score| {
            format!(
                "<p class=\"score\">{:.1}%</p>\n<p>{} detected of {} decided</p>",
                score.value * 100.0,
                score.detected,
                score.decided
            )
        },
    )
}

fn accounting(document: &RunDocument) -> String {
    let a = &document.accounting;
    let rows = [
        ("cataloged", a.cataloged),
        ("refused", a.refused),
        ("skipped", a.skipped),
        ("executed", a.executed),
        ("killed", a.killed),
        ("survived", a.survived),
        ("timed out", a.timed_out),
        ("inconclusive", a.inconclusive),
        ("errored", a.errored),
        ("not run", a.not_run),
        ("unreached", a.unreached),
        ("expected", a.expected),
    ];
    let mut out = String::from("<table>\n");
    for (name, count) in rows {
        let _written = writeln!(out, "<tr><th>{name}</th><td>{count}</td></tr>");
    }
    out.push_str("</table>");
    out
}

fn findings(document: &RunDocument) -> String {
    if document.findings.is_empty() {
        return "<p class=\"none\">Nothing was found.</p>".to_owned();
    }
    let mut out = String::from("<table>\n<tr><th>kind</th><th>detail</th></tr>\n");
    for finding in &document.findings {
        let _written = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td></tr>",
            escape(&finding.kind),
            escape(&finding.detail)
        );
    }
    out.push_str("</table>");
    out
}

fn mutants(mutants: &[RunMutantDocument]) -> String {
    if mutants.is_empty() {
        return "<p class=\"none\">Nothing was cataloged.</p>".to_owned();
    }
    let mut out = String::from(
        "<table>\n<tr><th>id</th><th>outcome</th><th>where</th><th>rule</th>\
         <th>change</th></tr>\n",
    );
    for mutant in mutants {
        let _written = writeln!(
            out,
            "<tr><td><code>{id}</code></td><td class=\"{class}\">{outcome}</td>\
             <td><code>{path}:{line}:{column}</code></td><td>{rule}</td>\
             <td><code>{original}</code> → <code>{replacement}</code></td></tr>",
            id = escape(&mutant.display_id),
            class = escape(&mutant.outcome),
            outcome = escape(&mutant.outcome),
            path = escape(&mutant.path),
            line = mutant.line,
            column = mutant.column,
            rule = escape(&mutant.rule),
            original = escape(&mutant.original),
            replacement = escape(&mutant.replacement),
        );
    }
    out.push_str("</table>");
    out
}

/// The text as it may appear in an HTML document.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}
