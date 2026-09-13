// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as one self-contained page: no font, no stylesheet, nothing to fetch, and one script of its own.
//!
//! A survivor is only worth reading where it is, so every file that holds one
//! is shown whole with its mutants on the lines they are on. A file whose
//! every mutation the tests noticed is counted rather than printed: a page
//! that shows a thousand lines nobody has to read is a page nobody opens.
//!
//! The page shows a file only when it is the one the run measured, which the
//! recorded digest settles; a file that changed since is named as changed
//! rather than shown, because showing the new bytes would be a lie about what
//! was measured.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::run::{RunDocument, RunMutantDocument};
use super::sources::Held;

/// The run as one HTML document with no external resource of any kind.
#[must_use]
pub fn document(document: &RunDocument, sources: &BTreeMap<String, Held>) -> String {
    let mut out = String::new();
    let _written = write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <title>rust-mutants {run}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         <h1>{root}</h1>\n<p class=\"identity\">{run} · catalog {catalog}</p>\n",
        run = escape(&document.run.id),
        root = escape(&document.workspace.root_name),
        catalog = escape(document.workspace.catalog_digest.get(..12).unwrap_or("")),
    );
    section(&mut out, "Score", &score(document));
    section(&mut out, "Accounting", &accounting(document));
    section(&mut out, "Findings", &findings(document));
    section(&mut out, "Mutants", &controls());
    out.push_str(&mutants(&document.mutants));
    section(&mut out, "Source", &files(document, sources));
    section(&mut out, "Refused", &rejections(document));
    section(&mut out, "Skipped", &skips(document));
    let _written = write!(out, "<script>{SCRIPT}</script>\n</body>\n</html>\n");
    out
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem auto;max-width:72rem;\
line-height:1.5;padding:0 1rem}h1{margin:0}.identity{color:#666;margin-top:0}\
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:.25rem .5rem;\
border-bottom:1px solid #ddd;vertical-align:top}code{font-family:ui-monospace,monospace}\
td.survived,td.errored,td.inconclusive,td.not_run{color:#a30}\
td.killed,td.timed_out{color:#0a6}p.none{color:#666}\
.score{font-size:2rem;font-weight:600}\
.controls{display:flex;gap:.5rem;flex-wrap:wrap;align-items:center;margin:.5rem 0}\
.controls input,.controls select{font:inherit;padding:.2rem .4rem}\
pre.source{background:#fbfbfb;border:1px solid #ddd;border-radius:4px;padding:.5rem;\
overflow-x:auto;font-family:ui-monospace,monospace;font-size:.85rem;line-height:1.45}\
pre.source .n{color:#999;user-select:none;display:inline-block;width:4ch;text-align:right;\
padding-right:1ch}pre.source .l{display:block}pre.source .l.has{background:#fff6f0}\
pre.source .l.killed{background:#f2fbf7}\
.mark{display:block;margin-left:5ch;font-size:.8rem;color:#a30}\
.mark.killed{color:#0a6}.changed{color:#a30}\
tr.hidden{display:none}";

const SCRIPT: &str = "(()=>{const t=document.getElementById('mutants');if(!t)return;\
const rows=[...t.tBodies[0].rows];const q=document.getElementById('q');\
const f=document.getElementById('outcome');const s=document.getElementById('sort');\
const apply=()=>{const text=(q.value||'').toLowerCase();const want=f.value;\
for(const r of rows){const hay=r.dataset.hay;const ok=(want==='all'||r.dataset.outcome===want)\
&&(text===''||hay.includes(text));r.classList.toggle('hidden',!ok);}\
const by=s.value;const sorted=[...rows].sort((a,b)=>{if(by==='where')\
return a.dataset.where.localeCompare(b.dataset.where);if(by==='rule')\
return a.dataset.rule.localeCompare(b.dataset.rule);\
return (a.dataset.rank-b.dataset.rank)||(a.dataset.index-b.dataset.index);});\
for(const r of sorted)t.tBodies[0].appendChild(r);};\
q.addEventListener('input',apply);f.addEventListener('change',apply);\
s.addEventListener('change',apply);apply();})();";

fn section(out: &mut String, title: &str, body: &str) {
    let _written = write!(out, "<h2>{}</h2>\n{body}\n", escape(title));
}

fn controls() -> String {
    "<div class=\"controls\">\
     <label>search <input id=\"q\" type=\"search\" placeholder=\"path, rule, id\"></label>\
     <label>outcome <select id=\"outcome\">\
     <option value=\"all\">all</option><option value=\"survived\">survived</option>\
     <option value=\"killed\">killed</option><option value=\"timed_out\">timed out</option>\
     <option value=\"inconclusive\">inconclusive</option><option value=\"errored\">errored</option>\
     <option value=\"not_run\">not run</option></select></label>\
     <label>order <select id=\"sort\">\
     <option value=\"finding\">findings first</option><option value=\"where\">where</option>\
     <option value=\"rule\">rule</option></select></label></div>"
        .to_owned()
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
    let counted = &document.accounting;
    let rows = [
        ("cataloged", counted.cataloged),
        ("refused", counted.refused),
        ("skipped", counted.skipped),
        ("executed", counted.executed),
        ("killed", counted.killed),
        ("survived", counted.survived),
        ("timed out", counted.timed_out),
        ("inconclusive", counted.inconclusive),
        ("errored", counted.errored),
        ("not run", counted.not_run),
        ("unreached", counted.unreached),
        ("discharged", counted.discharged),
        ("expected", counted.expected),
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
        "<table id=\"mutants\">\n<thead><tr><th>id</th><th>outcome</th><th>where</th>\
         <th>rule</th><th>change</th><th>noticed by</th></tr></thead>\n<tbody>\n",
    );
    for mutant in mutants {
        let where_ = format!("{}:{:0>6}:{:0>4}", mutant.path, mutant.line, mutant.column);
        let hay = format!(
            "{} {} {} {} {} {}",
            mutant.path, mutant.rule, mutant.family, mutant.display_id, mutant.id, mutant.outcome
        )
        .to_lowercase();
        let _written = writeln!(
            out,
            "<tr data-outcome=\"{outcome}\" data-rank=\"{rank}\" data-index=\"{index}\" \
             data-where=\"{sortable}\" data-rule=\"{rule}\" data-hay=\"{hay}\">\
             <td><code>{id}</code></td><td class=\"{outcome}\">{shown}</td>\
             <td><code>{path}:{line}:{column}</code></td><td>{rule}</td>\
             <td><code>{original}</code> → <code>{replacement}</code></td>\
             <td>{noticed}</td></tr>",
            outcome = escape(&mutant.outcome),
            rank = rank(mutant),
            index = mutant.index,
            sortable = escape(&where_),
            hay = escape(&hay),
            id = escape(&mutant.display_id),
            shown = escape(&mutant.outcome),
            path = escape(&mutant.path),
            line = mutant.line,
            column = mutant.column,
            rule = escape(&mutant.rule),
            original = escape(&mutant.original),
            replacement = escape(&mutant.replacement),
            noticed = escape(&mutant.killed_by.join(", ")),
        );
    }
    out.push_str("</tbody>\n</table>");
    out
}

/// Where one row sorts when the reader asks for findings first.
const fn rank(mutant: &RunMutantDocument) -> u8 {
    match mutant.outcome.as_bytes() {
        b"survived" if !mutant.expected => 0,
        b"errored" | b"inconclusive" => 1,
        b"not_run" => 2,
        b"survived" => 3,
        _ => 4,
    }
}

fn files(document: &RunDocument, sources: &BTreeMap<String, Held>) -> String {
    let mut by_file: BTreeMap<&str, Vec<&RunMutantDocument>> = BTreeMap::new();
    for mutant in &document.mutants {
        by_file.entry(&mutant.path).or_default().push(mutant);
    }
    if by_file.is_empty() {
        return "<p class=\"none\">Nothing was cataloged.</p>".to_owned();
    }
    let mut out = String::new();
    let mut clean: usize = 0;
    for (path, mutants) in by_file {
        if mutants.iter().all(|mutant| noticed(mutant)) {
            clean = clean.saturating_add(1);
            continue;
        }
        let _written = writeln!(out, "<h3><code>{}</code></h3>", escape(path));
        match sources.get(path).and_then(Held::measured) {
            Some(text) => out.push_str(&listing(text, &mutants)),
            None => {
                let _written = writeln!(
                    out,
                    "<p class=\"changed\">This file changed since the run, so what it holds now \
                     is not what was measured.</p>"
                );
            }
        }
    }
    if clean > 0 {
        let _written = writeln!(
            out,
            "<p class=\"none\">{clean} more file(s) the tests noticed every mutation in; the \
             table above lists them.</p>"
        );
    }
    if out.is_empty() {
        return "<p class=\"none\">The tests noticed every mutation.</p>".to_owned();
    }
    out
}

fn listing(text: &str, mutants: &[&RunMutantDocument]) -> String {
    let mut on_line: BTreeMap<u32, Vec<&RunMutantDocument>> = BTreeMap::new();
    for mutant in mutants {
        on_line.entry(mutant.line).or_default().push(mutant);
    }
    let mut out = String::from("<pre class=\"source\">");
    for (index, line) in text.lines().enumerate() {
        let number = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        let here = on_line.get(&number);
        let class = here.map_or("l", |rows| {
            if rows.iter().any(|one| noticed(one)) && rows.iter().all(|one| noticed(one)) {
                "l has killed"
            } else {
                "l has"
            }
        });
        let _written = write!(
            out,
            "<span class=\"{class}\"><span class=\"n\">{number}</span>{}</span>",
            escape(line)
        );
        for mutant in here.into_iter().flatten() {
            let _written = write!(
                out,
                "<span class=\"mark{}\">{} {} <code>{}</code> → <code>{}</code> [{}]</span>",
                if noticed(mutant) { " killed" } else { "" },
                escape(&mutant.outcome),
                escape(&mutant.rule),
                escape(&mutant.original),
                escape(&mutant.replacement),
                escape(&mutant.display_id),
            );
        }
    }
    out.push_str("</pre>\n");
    out
}

/// Whether the tests said anything about this mutant.
fn noticed(mutant: &RunMutantDocument) -> bool {
    matches!(mutant.outcome.as_str(), "killed" | "timed_out")
}

fn rejections(document: &RunDocument) -> String {
    if document.rejections.is_empty() {
        return "<p class=\"none\">The compiler accepted every candidate.</p>".to_owned();
    }
    let mut out = String::from(
        "<table>\n<tr><th>where</th><th>rule</th><th>code</th><th>alone</th>\
         <th>what the compiler said</th></tr>\n",
    );
    for rejection in &document.rejections {
        let _written = writeln!(
            out,
            "<tr><td><code>{path}</code></td><td>{rule}</td><td>{code}</td><td>{alone}</td>\
             <td>{diagnostic}</td></tr>",
            path = escape(&rejection.path),
            rule = escape(&rejection.rule),
            code = escape(rejection.code.as_deref().unwrap_or("—")),
            alone = if rejection.isolated { "yes" } else { "no" },
            diagnostic = escape(&rejection.diagnostic),
        );
    }
    out.push_str("</table>");
    out
}

fn skips(document: &RunDocument) -> String {
    if document.skips.is_empty() {
        return "<p class=\"none\">Discovery passed over nothing.</p>".to_owned();
    }
    let mut out = String::from(
        "<table>\n<tr><th>where</th><th>reason</th><th>how many</th><th>why</th></tr>\n",
    );
    for skip in &document.skips {
        let _written = writeln!(
            out,
            "<tr><td><code>{path}</code></td><td>{reason}</td><td>{count}</td><td>{why}</td></tr>",
            path = escape(&skip.path),
            reason = escape(&skip.reason),
            count = skip.count,
            why = escape(&skip.explanation),
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
