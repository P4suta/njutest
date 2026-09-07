// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rendering what the engine established, for a person and for a program.

pub mod doctor;
pub mod html;
pub mod junit;
pub mod markdown;
pub mod sarif;
pub mod sources;

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
pub fn list(
    discovery: &Discovery,
    sources: &BTreeMap<String, String>,
    file: Option<&str>,
) -> String {
    let mut text = String::new();
    for located in &discovery.candidates {
        let candidate = &located.found.candidate;
        if file.is_some_and(|wanted| candidate.path != wanted) {
            continue;
        }
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

/// Every decision the walk took in one file, in source order.
///
/// The tally says how much each reason hid; this says what each place was, so
/// a reader asking "why is there no mutant here" is answered about the place
/// rather than about the file.
#[must_use]
pub fn decisions(discovery: &Discovery, file: &str, line: Option<u32>) -> String {
    let mut text = String::new();
    let Some(report) = discovery.files.iter().find(|one| one.path == file) else {
        let written = writeln!(text, "{file} is not a file this run reads");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        return text;
    };
    if let Some(reason) = report.whole_file {
        let written = writeln!(
            text,
            "{file} was passed over whole: {}\n          {}",
            reason.name(),
            reason.explanation()
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        return text;
    }
    for decision in &discovery.decisions {
        if decision.path != file {
            continue;
        }
        if line.is_some_and(|wanted| decision.position.line != wanted) {
            continue;
        }
        let what = match (decision.form, decision.skip) {
            (Some(form), _) => form.letter().to_owned(),
            (None, Some(reason)) => reason.name().to_owned(),
            (None, None) => String::from("-"),
        };
        let note = decision
            .note
            .as_ref()
            .map(|note| format!("  {note:?}"))
            .unwrap_or_default();
        let written = writeln!(
            text,
            "{}:{}  {:<26}  {what}{note}",
            decision.position.line, decision.position.byte_column, decision.rule
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    if text.is_empty() {
        let written = writeln!(text, "no rule targets anything there");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}

/// What the compiler refused, with its own words.
#[must_use]
pub fn rejections(session: &Session) -> String {
    let mut text = String::new();
    for rejection in session.rejections() {
        let written = writeln!(
            text,
            "{} {}  {}\n          {}",
            rejection.id.get(..20).unwrap_or(&rejection.id),
            rejection.rule,
            rejection.path,
            rejection.diagnostic.lines().next().unwrap_or_default()
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    if text.is_empty() {
        text.push_str("the compiler refused nothing\n");
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

/// Everything one run established about one mutant, as the lines a person reads.
#[must_use]
pub fn explained(document: &rust_mutants::report::explain::ExplainDocument) -> String {
    let one = &document.mutant;
    let mut text = String::new();
    let mut say = |label: &str, value: &str| {
        let written = writeln!(text, "{label:<9} {value}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    };
    say("MUTANT", &one.id);
    say("SHORT", &one.display_id);
    say(
        "RULE",
        &format!("{}@{} ({})", one.rule, one.rule_version, one.family),
    );
    say(
        "WHERE",
        &format!("{}:{}:{}", one.path, one.line, one.column),
    );
    say(
        "EDIT",
        &format!("{:?} => {:?}", one.original, one.replacement),
    );
    if let Some(run) = &document.run_id {
        say("RUN", run);
    }
    match (&document.outcome, &document.refused) {
        (_, Some(diagnostic)) => {
            say("OUTCOME", "refused by the compiler");
            for line in diagnostic.lines() {
                say("", line);
            }
        }
        (Some(outcome), None) => {
            say("OUTCOME", outcome);
            if let Some(target) = &document.target {
                say("TARGET", target);
            }
            if !document.killed_by.is_empty() {
                say("KILLED BY", &document.killed_by.join(", "));
            }
            if let Some(milliseconds) = document.duration_ms {
                say(
                    "TIMING",
                    &format!(
                        "{milliseconds} ms{}",
                        if document.retried { ", retried" } else { "" }
                    ),
                );
            }
        }
        (None, None) => say("OUTCOME", "no stored run answers for it"),
    }
    if let Some(route) = &document.route {
        say(
            "ROUTE",
            &format!(
                "{} reaching [{}] executed [{}]",
                route.granularity,
                route.reaching.join(", "),
                route.executed.join(", ")
            ),
        );
        for (target, tests) in &route.tests {
            say("TESTS", &format!("{target}: {}", tests.join(", ")));
        }
    }
    say("REPRODUCE", &document.reproduce);
    match (&document.diff, &document.source) {
        (Some(diff), _) => {
            text.push('\n');
            text.push_str(diff);
        }
        (None, Some(why)) => say("DIFF", &format!("none: {why}")),
        (None, None) => {}
    }
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
/// What a whole run would have started, what this one started, and what removed the rest.
///
/// A report from before the target list was written carries no work line at
/// all: a share of nothing is not nought per cent, and a reader shown one
/// would read a run that measured everything as a run that measured nothing.
fn work_line(document: &run::RunDocument) -> String {
    let work = rust_mutants::work::Work::of(document);
    if work.whole == 0 {
        return String::new();
    }
    let removed: Vec<String> = work
        .removed
        .iter()
        .map(|one| format!("{}={}", one.reason, one.pairs))
        .collect();
    let mut line = format!(
        "WORK      started={} of {} pairs across {} targets; {:.1}% removed",
        work.started,
        work.whole,
        work.targets,
        work.saved() * 100.0
    );
    if !removed.is_empty() {
        let written = write!(line, " ({})", removed.join(" "));
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    if work.tests_whole > 0 {
        let written = write!(
            line,
            "\n          tests={} of {}; {:.1}% removed",
            work.tests_started,
            work.tests_whole,
            work.tests_saved() * 100.0
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        if work.established_tests() > 0 {
            let written = write!(
                line,
                " ({} of them establishing that a filtered set answers on its own)",
                work.established_tests()
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if !work.answers_for_the_whole() {
        line.push_str("\n          this run was asked for less than the whole catalog");
    }
    line.push('\n');
    line
}

/// The stored run as the lines a person reads.
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
    text.push_str(&work_line(document));
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
