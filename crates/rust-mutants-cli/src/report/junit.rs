// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as the JUnit XML every continuous integration server already reads.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::run::{RunDocument, RunMutantDocument};

/// The run as one JUnit XML document.
#[must_use]
pub fn document(document: &RunDocument) -> String {
    let mut files: BTreeMap<&str, Vec<&RunMutantDocument>> = BTreeMap::new();
    for mutant in &document.mutants {
        files.entry(&mutant.path).or_default().push(mutant);
    }
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let counted = document.accounting;
    let _written = writeln!(
        out,
        "<testsuites name=\"rust-mutants\" tests=\"{tests}\" failures=\"{failures}\" \
         errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time}\">",
        tests = counted.cataloged,
        failures = counted
            .survived
            .count()
            .saturating_sub(counted.expected.count()),
        errors = counted
            .inconclusive
            .count()
            .saturating_add(counted.errored.count()),
        skipped = counted.not_run.count(),
        time = seconds(document.run.duration_ms),
    );
    for (path, mutants) in files {
        suite(&mut out, path, &mutants);
    }
    loose(&mut out, document);
    out.push_str("</testsuites>\n");
    out
}

/// The findings no mutant row carries, as a suite of their own.
fn loose(out: &mut String, document: &RunDocument) {
    let held: std::collections::BTreeSet<&str> = document
        .mutants
        .iter()
        .map(|mutant| mutant.id.as_str())
        .collect();
    let orphaned: Vec<&super::run::FindingDocument> = document
        .findings
        .iter()
        .filter(|finding| {
            !finding
                .mutant
                .as_deref()
                .is_some_and(|id| held.contains(id))
        })
        .collect();
    if orphaned.is_empty() {
        return;
    }
    let _written = writeln!(
        out,
        "  <testsuite name=\"findings\" tests=\"{count}\" failures=\"{count}\" errors=\"0\" \
         skipped=\"0\" time=\"0.000\">",
        count = orphaned.len(),
    );
    for finding in orphaned {
        let _written = write!(
            out,
            "    <testcase name=\"{kind}\" classname=\"findings\" time=\"0.000\">\n\
             \x20     <failure message=\"{kind}\" type=\"{kind}\">{detail}</failure>\n\
             \x20 </testcase>\n",
            kind = escape(&finding.kind),
            detail = escape(&finding.detail),
        );
    }
    out.push_str("  </testsuite>\n");
}

fn suite(out: &mut String, path: &str, mutants: &[&RunMutantDocument]) {
    let counted = |kinds: &[&str]| {
        mutants
            .iter()
            .filter(|mutant| kinds.contains(&mutant.outcome.as_str()))
            .count()
    };
    let elapsed: u64 = mutants
        .iter()
        .fold(0, |total, mutant| total.saturating_add(mutant.duration_ms));
    let _written = writeln!(
        out,
        "  <testsuite name=\"{name}\" tests=\"{tests}\" failures=\"{failures}\" \
         errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time}\">",
        name = escape(path),
        tests = mutants.len(),
        failures = mutants
            .iter()
            .filter(|mutant| mutant.outcome == "survived" && !mutant.expected)
            .count(),
        errors = counted(&["inconclusive", "errored"]),
        skipped = counted(&["not_run"]),
        time = seconds(elapsed),
    );
    for mutant in mutants {
        case(out, path, mutant);
    }
    out.push_str("  </testsuite>\n");
}

fn case(out: &mut String, path: &str, mutant: &RunMutantDocument) {
    let name = format!(
        "{rule} at {path}:{line}:{column} [{id}]",
        rule = mutant.rule,
        line = mutant.line,
        column = mutant.column,
        id = mutant.display_id,
    );
    let head = format!(
        "    <testcase name=\"{name}\" classname=\"{class}\" time=\"{time}\"",
        name = escape(&name),
        class = escape(path),
        time = seconds(mutant.duration_ms),
    );
    let change = format!(
        "{} became {}",
        rendered(&mutant.original),
        rendered(&mutant.replacement)
    );
    let body = match mutant.outcome.as_str() {
        "survived" if mutant.expected => Some(format!(
            "      <skipped message=\"{}\"/>\n",
            escape(&format!(
                "{change} at {path}:{}:{} survived, which a reviewer wrote down in advance",
                mutant.line, mutant.column
            ))
        )),
        "survived" => Some(format!(
            "      <failure message=\"survived\" type=\"surviving-mutant\">{}</failure>\n",
            escape(&format!(
                "the tests did not notice that {change} at {path}:{}:{}",
                mutant.line, mutant.column
            ))
        )),
        "inconclusive" | "errored" => Some(format!(
            "      <error message=\"{outcome}\" type=\"{outcome}-mutant\">{detail}</error>\n",
            outcome = escape(&mutant.outcome),
            detail = escape(&format!("{change}; the run established nothing about it")),
        )),
        "not_run" => Some(format!(
            "      <skipped message=\"{}\"/>\n",
            escape(mutant.not_run_reason.as_deref().unwrap_or("not run"))
        )),
        _ => None,
    };
    match body {
        Some(body) => {
            let _written = write!(out, "{head}>\n{body}    </testcase>\n");
        }
        None => {
            let _written = writeln!(out, "{head}/>");
        }
    }
}

/// Milliseconds as the seconds this format counts in.
fn seconds(millis: u64) -> String {
    let whole = millis.wrapping_div(1000);
    let thousandths = millis.wrapping_rem(1000);
    format!("{whole}.{thousandths:03}")
}

/// The bytes an edit replaces, as a reader can see them even when there are none.
fn rendered(text: &str) -> String {
    if text.is_empty() {
        "nothing".to_owned()
    } else {
        format!("`{text}`")
    }
}

/// The text as it may appear in an XML document, in an attribute or between tags.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            one if one.is_control() => out.push(' '),
            one => out.push(one),
        }
    }
    out
}
