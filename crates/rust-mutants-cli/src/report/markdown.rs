// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as the paragraph a person puts in a pull request or a job summary.

use std::fmt::Write as _;

use super::run::{RunDocument, RunMutantDocument};

/// The run as one Markdown document.
#[must_use]
pub fn document(document: &RunDocument) -> String {
    let mut out = String::new();
    let _written = writeln!(
        out,
        "# Mutation report — {} {}\n",
        document.workspace.root_name, document.run.id
    );
    score(&mut out, document);
    accounting(&mut out, document);
    findings(&mut out, document);
    survivors(&mut out, document);
    out
}

fn score(out: &mut String, document: &RunDocument) {
    let said = document.score.as_ref().map_or_else(
        || "The run decided nothing, which is not a score of zero.".to_owned(),
        |score| {
            format!(
                "**{:.1}%** — {} detected of {} decided",
                score.value * 100.0,
                score.detected,
                score.decided
            )
        },
    );
    let _written = writeln!(out, "{said}\n");
}

fn accounting(out: &mut String, document: &RunDocument) {
    let counted = &document.accounting;
    out.push_str("| what | how many |\n| --- | --- |\n");
    for (name, count) in [
        ("cataloged", counted.cataloged),
        ("killed", counted.killed),
        ("survived", counted.survived),
        ("timed out", counted.timed_out),
        ("inconclusive", counted.inconclusive),
        ("errored", counted.errored),
        ("not run", counted.not_run),
        ("unreached", counted.unreached),
        ("discharged", counted.discharged),
        ("expected", counted.expected),
        ("refused", counted.refused),
        ("skipped", counted.skipped),
    ] {
        let _written = writeln!(out, "| {name} | {count} |");
    }
    out.push('\n');
}

fn findings(out: &mut String, document: &RunDocument) {
    out.push_str("## Findings\n\n");
    if document.findings.is_empty() {
        out.push_str("Nothing was found.\n\n");
        return;
    }
    for finding in &document.findings {
        let _written = writeln!(out, "- **{}** — {}", finding.kind, cell(&finding.detail));
    }
    out.push('\n');
}

fn survivors(out: &mut String, document: &RunDocument) {
    let rows: Vec<&RunMutantDocument> = document
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome == "survived" && !mutant.expected)
        .collect();
    let _written = writeln!(out, "## Survivors ({})\n", rows.len());
    if rows.is_empty() {
        out.push_str("Every mutant the run decided, the tests noticed.\n");
        return;
    }
    out.push_str("| where | rule | change | id |\n| --- | --- | --- | --- |\n");
    for mutant in rows {
        let _written = writeln!(
            out,
            "| `{path}:{line}:{column}` | `{rule}` | {change} | `{id}` |",
            path = cell(&mutant.path),
            line = mutant.line,
            column = mutant.column,
            rule = cell(&mutant.rule),
            change = change(mutant),
            id = cell(&mutant.display_id),
        );
    }
    out.push_str(
        "\nEach of these is a gap in the tests or a claim to write down: `rust-mutants \
                  explain <id>` says what it is, and `[[mutation.expect]]` accepts one with a \
                  reason.\n",
    );
}

/// What the edit does, as a table cell.
fn change(mutant: &RunMutantDocument) -> String {
    let shown = |text: &str| {
        if text.is_empty() {
            "*nothing*".to_owned()
        } else {
            format!("`{}`", cell(text))
        }
    };
    format!(
        "{} → {}",
        shown(&mutant.original),
        shown(&mutant.replacement)
    )
}

/// The text as it may appear inside one cell of a table.
fn cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '|' => out.push_str("\\|"),
            '`' => out.push('\''),
            '\n' | '\r' | '\t' => out.push(' '),
            one => out.push(one),
        }
    }
    out
}
