// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rendering what the engine established, for a person and for a program.

pub mod doctor;
pub mod html;

pub use rust_mutants::report::catalog::{
    CatalogDocument, MutantDocument, PlatformDocument, RejectionDocument, SelectionDocument,
    SkipDocument, WorkspaceDocument, document, mutant_document, rejection_documents,
    selection_document, skip_documents, workspace_document,
};
/// The run as a document, as the engine writes it.
pub use rust_mutants::report::run;
pub mod stryker;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rust_mutants::catalog::Mutant;
use rust_mutants::discover::Discovery;
use rust_mutants::execute::MutantResult;
use rust_mutants::session::Session;
use rust_mutants::syntax::{Position, Skip};

/// `path:line:column`, the spelling every editor and every `::warning` consumer already understands.
#[must_use]
pub fn at(path: &str, position: Position) -> String {
    format!("{path}:{}:{}", position.line, position.byte_column)
}

/// The position of a mutant's edit, found by counting the lines of the file it came from.
#[must_use]
pub fn position_in(source: &str, offset: u32) -> Position {
    rust_mutants::syntax::LineIndex::new(source).position(source, offset)
}

/// One line per candidate: what it is, where it is, and what it does.
#[must_use]
pub fn list(discovery: &Discovery, sources: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for located in &discovery.candidates {
        let candidate = &located.found.candidate;
        let mutant = discovery
            .catalog
            .by_id(&candidate.id().unwrap_or_default())
            .map(|mutant| mutant.display_id.clone())
            .unwrap_or_default();
        let position = sources
            .get(&candidate.path)
            .map_or(located.found.position, |source| {
                position_in(source, candidate.span.start)
            });
        let written = writeln!(
            text,
            "{mutant}  {rule:<26}  {where_}  {original:?} => {replacement:?}",
            rule = candidate.rule.to_string(),
            where_ = at(&candidate.path, position),
            original = String::from_utf8_lossy(&candidate.original),
            replacement = String::from_utf8_lossy(&candidate.replacement),
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}

/// The skip tallies, widest reason first.
#[must_use]
pub fn why_skipped(skips: &[Skip]) -> String {
    let mut totals: BTreeMap<&str, (u32, String)> = BTreeMap::new();
    for skip in skips {
        let entry = totals
            .entry(skip.reason.name())
            .or_insert_with(|| (0, skip.reason.explanation().to_owned()));
        entry.0 = entry.0.saturating_add(skip.count);
    }
    let mut rows: Vec<(&str, u32, String)> = totals
        .into_iter()
        .map(|(reason, (count, why))| (reason, count, why))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let mut text = String::new();
    for (reason, count, why) in rows {
        let written = write!(text, "{count:>6}  {reason}\n          {why}\n");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    if text.is_empty() {
        text.push_str("nothing was passed over\n");
    }
    text
}

/// What preparation established, for a person.
#[must_use]
pub fn catalog(session: &Session) -> String {
    let mut text = String::new();
    let written = write!(
        text,
        "workspace {}\ncatalog   {}\n{} accepted, {} refused, {} skipped\n\n",
        session.workspace_digest(),
        session.catalog().digest(),
        session.accepted().len(),
        session.rejections().len(),
        session
            .skips()
            .iter()
            .map(|skip| u64::from(skip.count))
            .sum::<u64>(),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    for index in session.accepted() {
        if let Some(mutant) = session.catalog().by_index(*index) {
            let written = writeln!(text, "{}", one_line(mutant));
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if !session.rejections().is_empty() {
        text.push_str("\nrefused by the compiler:\n");
        for rejection in session.rejections() {
            let written = writeln!(
                text,
                "{}  {:<26}  {}  {}",
                rejection.display_id,
                rejection.rule,
                rejection.path,
                rejection
                    .diagnostic
                    .lines()
                    .next()
                    .unwrap_or("no diagnostic"),
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    text
}

fn one_line(mutant: &Mutant) -> String {
    format!(
        "{}  {:<26}  {}  {:?} => {:?}",
        mutant.display_id,
        mutant.candidate.rule.to_string(),
        mutant.candidate.path,
        String::from_utf8_lossy(&mutant.candidate.original),
        String::from_utf8_lossy(&mutant.candidate.replacement),
    )
}

/// Everything known about one mutant.
#[must_use]
pub fn explain(session: &Session, mutant: &Mutant, source: Option<&str>) -> String {
    let candidate = &mutant.candidate;
    let where_ = source.map_or_else(
        || format!("{}:{}", candidate.path, candidate.span),
        |source| at(&candidate.path, position_in(source, candidate.span.start)),
    );
    let mut text = String::new();
    for (label, value) in [
        ("mutant", mutant.id.clone()),
        ("short", mutant.display_id.clone()),
        ("index", mutant.index.to_string()),
        (
            "rule",
            format!("{} ({})", candidate.rule, candidate.rule.family.name()),
        ),
        ("where", where_),
        (
            "edit",
            format!(
                "{:?} => {:?}",
                String::from_utf8_lossy(&candidate.original),
                String::from_utf8_lossy(&candidate.replacement)
            ),
        ),
        ("file", candidate.source_digest.clone()),
    ] {
        let written = writeln!(text, "{label:<9} {value}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push_str(&verdict(session, mutant));
    text
}

/// What the compiler made of one mutant, and what would run it.
fn verdict(session: &Session, mutant: &Mutant) -> String {
    let mut text = String::new();
    if let Some(rejection) = session
        .rejections()
        .iter()
        .find(|rejection| rejection.id == mutant.id)
    {
        text.push_str("verdict   refused by the compiler\n");
        for line in rejection.diagnostic.lines() {
            let written = writeln!(text, "          {line}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        return text;
    }
    text.push_str("verdict   accepted; it compiles and can be executed\n");
    let targets: Vec<&str> = session
        .targets()
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    let written = writeln!(text, "targets   {}", targets.join(", "));
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    text
}

/// What one execution said.
#[must_use]
pub fn outcome(result: &MutantResult, mutant: &Mutant) -> String {
    let mut text = format!(
        "{} {}  {}  {}\n",
        mutant.display_id,
        mutant.candidate.rule,
        result.outcome.name(),
        result.target,
    );
    let written = write!(
        text,
        "exit {}  {} ms",
        result.exit_code,
        result.duration.as_millis()
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    if let Some(summary) = result.summary {
        let written = write!(
            text,
            "  {} passed, {} failed, {} ignored, {} filtered out",
            summary.passed, summary.failed, summary.ignored, summary.filtered_out
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push('\n');
    text
}

/// The exit code an outcome earns: zero when the tests noticed the mutant, one when they did not, and two when nothing was established.
#[must_use]
pub const fn exit_code(outcome: rust_mutants::outcome::Outcome) -> u8 {
    use rust_mutants::outcome::Outcome;
    match outcome {
        Outcome::Killed | Outcome::TimedOut => 0,
        Outcome::Survived => 1,
        _ => crate::EXIT_USAGE,
    }
}

/// The run as lines a person reads: the tally, the score, and every finding.
#[must_use]
pub fn lines(document: &run::RunDocument) -> String {
    let a = &document.accounting;
    let mut text = String::new();
    let written = write!(
        text,
        "run       {}\nworkspace {}\ncatalog   {}\n\n\
         MUTANTS   cataloged={} refused={} skipped={} executed={}\n\
         OUTCOMES  killed={} survived={} timed_out={} inconclusive={} errored={} not_run={} \
         unreached={} discharged={} expected={}\n",
        document.run.id,
        document.workspace.workspace_digest,
        document.workspace.catalog_digest,
        a.cataloged,
        a.refused,
        a.skipped,
        a.executed,
        a.killed,
        a.survived,
        a.timed_out,
        a.inconclusive,
        a.errored,
        a.not_run,
        a.unreached,
        a.discharged,
        a.expected,
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    match &document.score {
        Some(score) => {
            let percent = score.value * 100.0;
            let written = writeln!(
                text,
                "SCORE     {percent:.1}%  ({} detected of {} decided)",
                score.detected, score.decided
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        None => text.push_str("SCORE     none; the run decided nothing\n"),
    }
    if !document.findings.is_empty() {
        text.push('\n');
        for one in &document.findings {
            let written = writeln!(text, "{:<22} {}", one.kind, one.detail);
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if document.run.interrupted {
        text.push_str("\nINTERRUPTED  the run stopped before every mutant was executed\n");
    }
    text
}
