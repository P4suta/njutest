// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as the JUnit XML every continuous integration server already reads.

use std::collections::BTreeMap;

use rust_mutants::outcome::Outcome;

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
    crate::text::line(
        &mut out,
        format_args!(
            "<testsuites name=\"rust-mutants\" tests=\"{tests}\" failures=\"{failures}\" \
         errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time}\">",
            tests = counted.cataloged,
            failures = document
                .mutants
                .iter()
                .filter(|mutant| mutant.outcome == Outcome::Survived && !mutant.expected)
                .count(),
            errors = document
                .mutants
                .iter()
                .filter(|mutant| matches!(
                    mutant.outcome,
                    Outcome::StepLimitReached
                        | Outcome::Waited
                        | Outcome::Inconclusive
                        | Outcome::Errored
                ))
                .count(),
            skipped = counted.not_run.count(),
            time = seconds(document.run.duration_ms),
        ),
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
    crate::text::line(
        out,
        format_args!(
            "  <testsuite name=\"findings\" tests=\"{count}\" failures=\"{count}\" errors=\"0\" \
         skipped=\"0\" time=\"0.000\">",
            count = orphaned.len(),
        ),
    );
    for finding in orphaned {
        crate::text::append(
            out,
            format_args!(
                "    <testcase name=\"{kind}\" classname=\"findings\" time=\"0.000\">\n\
             \x20     <failure message=\"{kind}\" type=\"{kind}\">{detail}</failure>\n\
             \x20 </testcase>\n",
                kind = escape(finding.kind.as_str()),
                detail = escape(&finding.detail),
            ),
        );
    }
    out.push_str("  </testsuite>\n");
}

fn suite(out: &mut String, path: &str, mutants: &[&RunMutantDocument]) {
    let counted = |kinds: &[Outcome]| {
        mutants
            .iter()
            .filter(|mutant| kinds.contains(&mutant.outcome))
            .count()
    };
    let elapsed: u128 = mutants
        .iter()
        .map(|mutant| u128::from(mutant.duration_ms))
        .sum();
    crate::text::line(
        out,
        format_args!(
            "  <testsuite name=\"{name}\" tests=\"{tests}\" failures=\"{failures}\" \
         errors=\"{errors}\" skipped=\"{skipped}\" time=\"{time}\">",
            name = escape(path),
            tests = mutants.len(),
            failures = mutants
                .iter()
                .filter(|mutant| mutant.outcome == Outcome::Survived && !mutant.expected)
                .count(),
            errors = counted(&[
                Outcome::StepLimitReached,
                Outcome::Waited,
                Outcome::Inconclusive,
                Outcome::Errored,
            ]),
            skipped = counted(&[Outcome::NotRun]),
            time = seconds(elapsed),
        ),
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
    let body = match mutant.outcome {
        Outcome::Survived if mutant.expected => Some(format!(
            "      <skipped message=\"{}\"/>\n",
            escape(&format!(
                "{change} at {path}:{}:{} survived, which a reviewer wrote down in advance",
                mutant.line, mutant.column
            ))
        )),
        Outcome::Survived => Some(format!(
            "      <failure message=\"survived\" type=\"surviving-mutant\">{}</failure>\n",
            escape(&format!(
                "the tests did not notice that {change} at {path}:{}:{}",
                mutant.line, mutant.column
            ))
        )),
        Outcome::StepLimitReached | Outcome::Waited | Outcome::Inconclusive | Outcome::Errored => {
            Some(format!(
                "      <error message=\"{outcome}\" type=\"{outcome}-mutant\">{detail}</error>\n",
                outcome = escape(mutant.outcome.as_str()),
                detail = escape(&format!("{change}; the run established nothing about it")),
            ))
        }
        Outcome::NotRun => Some(format!(
            "      <skipped message=\"{}\"/>\n",
            escape(
                mutant
                    .not_run_reason
                    .map_or("not run", |reason| reason.as_str())
            )
        )),
        Outcome::Killed => None,
    };
    match body {
        Some(body) => {
            crate::text::append(out, format_args!("{head}>\n{body}    </testcase>\n"));
        }
        None => {
            crate::text::line(out, format_args!("{head}/>"));
        }
    }
}

/// Milliseconds as the seconds this format counts in.
fn seconds(millis: impl Into<u128>) -> String {
    let millis = millis.into();
    let whole = millis / 1000;
    let thousandths = millis % 1000;
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
