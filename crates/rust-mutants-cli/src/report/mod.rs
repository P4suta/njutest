// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rendering what the engine established, for a person and for a program.

pub mod annotations;
pub mod candidates;
pub mod doctor;
pub mod html;
pub mod junit;
pub mod markdown;
pub mod sarif;
pub mod sources;
pub mod tally;

pub(crate) use rust_mutants::report::catalog::{document, selection_document};
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

fn candidate_text(
    mutant: &str,
    field: &'static str,
    bytes: &[u8],
) -> Result<String, crate::error::CliError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|source| crate::error::CliError::CandidateTextNotUtf8 {
            mutant: mutant.to_owned(),
            field,
            source,
        })
}

/// `path:line:column`, the spelling every editor and every `::warning` consumer already understands.
#[must_use]
pub fn at(path: &str, position: Position) -> String {
    format!("{path}:{}:{}", position.line, position.byte_column)
}

/// The position of a mutant's edit, found by counting the lines of the file it came from.
///
/// # Errors
/// Returns the exact source-position invariant that prevents a lossless one-based position from being represented.
pub fn position_in(
    source: &str,
    offset: u32,
) -> Result<Position, rust_mutants::syntax::PositionError> {
    rust_mutants::syntax::LineIndex::new(source)?.position(offset)
}

/// Every candidate as one document, for a reader who wants them in a program rather than on a screen.
///
/// # Errors
/// Returns the candidate identity failure when any proposed mutation cannot be named exactly.
pub fn candidates(
    discovery: &Discovery,
    sources: &BTreeMap<String, String>,
    file: Option<&str>,
) -> Result<candidates::CandidatesDocument, crate::error::CliError> {
    let mut listed = Vec::new();
    for located in &discovery.candidates {
        let candidate = &located.found.candidate;
        if file.is_some_and(|wanted| candidate.path != wanted) {
            continue;
        }
        let id = candidate.id()?;
        let position = match sources.get(&candidate.path) {
            Some(source) => position_in(source, candidate.span.start)?,
            None => located.found.position,
        };
        let original = candidate_text(id.as_str(), "original", &candidate.original)?;
        let replacement = candidate_text(id.as_str(), "replacement", &candidate.replacement)?;
        listed.push(candidates::CandidateDocument {
            display_id: id.display().into_inner(),
            id: id.into_inner(),
            path: candidate.path.clone(),
            item: located.found.item.clone(),
            rule: candidate.rule.name.to_owned(),
            line: position.line,
            column: position.byte_column,
            original,
            replacement,
        });
    }
    Ok(candidates::CandidatesDocument {
        document_type: candidates::DOCUMENT_TYPE.to_owned(),
        schema_version: candidates::SCHEMA_VERSION,
        tool_version: rust_mutants::VERSION.to_owned(),
        count: listed.len(),
        candidates: listed,
    })
}

/// One line per candidate: what it is, where it is, and what it does.
///
/// # Errors
/// Returns the candidate identity failure when any proposed mutation cannot be named exactly.
pub fn list(
    discovery: &Discovery,
    sources: &BTreeMap<String, String>,
    file: Option<&str>,
) -> Result<String, crate::error::CliError> {
    let mut text = String::new();
    let shown = discovery
        .candidates
        .iter()
        .filter(|located| file.is_none_or(|wanted| located.found.candidate.path == wanted))
        .count();
    for located in &discovery.candidates {
        let candidate = &located.found.candidate;
        if file.is_some_and(|wanted| candidate.path != wanted) {
            continue;
        }
        let candidate_id = candidate.id()?;
        let mutant = discovery.catalog.by_id(candidate_id.as_str()).map_or_else(
            || candidate_id.to_string(),
            |mutant| mutant.display_id.to_string(),
        );
        let position = match sources.get(&candidate.path) {
            Some(source) => position_in(source, candidate.span.start)?,
            None => located.found.position,
        };
        let written = writeln!(
            text,
            "{mutant}  {rule:<30}  {where_}  {original} => {replacement}",
            rule = candidate.rule.to_string(),
            where_ = at(&candidate.path, position),
            original = rust_mutants::telling::LosslessBytes::new(&candidate.original),
            replacement = rust_mutants::telling::LosslessBytes::new(&candidate.replacement),
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let written = writeln!(
        text,
        "\n{shown} candidates, which is what the rules propose. `catalog` says which of them \
         the compiler accepts, and `--json` here writes them as a document."
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    Ok(text)
}

/// The skip tallies, widest reason first.
///
/// # Errors
/// Returns when the complete skip count cannot be represented exactly.
pub fn why_skipped(skips: &[Skip]) -> Result<String, crate::error::CliError> {
    let mut totals: BTreeMap<&str, (u128, String)> = BTreeMap::new();
    for skip in skips {
        let entry = totals
            .entry(skip.reason.name())
            .or_insert_with(|| (0, skip.reason.explanation().to_owned()));
        entry.0 = entry.0.checked_add(u128::from(skip.count)).ok_or(
            crate::error::CliError::ProjectionOverflow {
                projection: "why-skipped",
                field: "the total skipped places",
            },
        )?;
    }
    let mut rows: Vec<(&str, u128, String)> = totals
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
    Ok(text)
}

/// Every decision the walk took in one file, in source order.
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
            "{}:{}  {:<30}  {what}{note}",
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
            match rejection.id.get(..20) {
                Some(short) => short,
                None => &rejection.id,
            },
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
                "{}  {:<30}  {}  {}",
                rejection.display_id,
                rejection.rule,
                rejection.path,
                match rejection.diagnostic.lines().next() {
                    Some(line) => line,
                    None => "no diagnostic",
                },
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    text
}

fn one_line(mutant: &Mutant) -> String {
    format!(
        "{}  {:<30}  {}  {} => {}",
        mutant.display_id,
        mutant.candidate.rule.to_string(),
        mutant.candidate.path,
        rust_mutants::telling::LosslessBytes::new(&mutant.candidate.original),
        rust_mutants::telling::LosslessBytes::new(&mutant.candidate.replacement),
    )
}

/// The block a reader pastes to record this mutation with a reason, under one label.
fn accepting(accept: &str, say: &mut impl FnMut(&str, &str)) {
    for (at, line) in accept.lines().enumerate() {
        say(if at == 0 { "ACCEPT" } else { "" }, line);
    }
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
    say("NAME", &rust_mutants::report::explain::names(one));
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
        for one in &route.discharged {
            say("PROVED", &format!("{}: {}", one.target, one.proof));
        }
    }
    say("REPRODUCE", &document.reproduce);
    accepting(&document.accept, &mut say);
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
        result.outcome().name(),
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
        Outcome::Killed => 0,
        Outcome::Survived => 1,
        Outcome::NotRun
        | Outcome::StepLimitReached
        | Outcome::Waited
        | Outcome::Inconclusive
        | Outcome::Errored => crate::EXIT_USAGE,
    }
}

/// The run as lines a person reads: the tally, the score, and every finding.
/// What a whole run would have started, what this one started, and what removed the rest.
fn work_line(document: &run::RunDocument) -> Result<String, rust_mutants::work::WorkError> {
    let work = rust_mutants::work::Work::of(document)?;
    if work.whole == 0 {
        return Ok(String::new());
    }
    let removed: Vec<String> = work
        .removed
        .iter()
        .map(|one| format!("{}={}", one.reason, one.pairs))
        .collect();
    let mut line = format!(
        "WORK      started={} of {} pairs across {} targets; {:.1}% removed",
        work.pairs(),
        work.whole,
        work.targets,
        work.saved()? * 100.0
    );
    if let Some(retried) = work
        .started
        .checked_sub(work.pairs())
        .filter(|count| *count > 0)
    {
        let written = write!(
            line,
            "\n          {retried} of those were started twice, because a timeout is confirmed alone \
             before it is believed",
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
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
    Ok(line)
}

/// The stored run as the lines a person reads.
///
/// # Errors
/// Returns the exact work-ledger refusal when the stored counts cannot be projected without truncation or overflow.
pub fn lines(document: &run::RunDocument) -> Result<String, rust_mutants::work::WorkError> {
    let mut text = String::new();
    let written = write!(
        text,
        "run       {}\nworkspace {}\ncatalog   {}\n",
        document.run.id, document.workspace.workspace_digest, document.workspace.catalog_digest,
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    if !document.findings.is_empty() {
        text.push('\n');
        for one in &document.findings {
            let written = writeln!(text, "{:<22} {}", one.kind, one.detail);
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    text.push_str(&survivors(document));
    text.push('\n');
    text.push_str(&totals(document)?);
    if document.run.interrupted {
        text.push_str("\nINTERRUPTED  the run stopped before every mutant was executed\n");
    }
    Ok(text)
}

/// How many separate gaps the survivors are, which is not how many survivors there are.
fn survivors(document: &run::RunDocument) -> String {
    let mut folded: BTreeMap<(&str, &str), Vec<&run::RunMutantDocument>> = BTreeMap::new();
    let mut alone: Vec<&run::RunMutantDocument> = Vec::new();
    for one in &document.mutants {
        if one.outcome != rust_mutants::outcome::Outcome::Survived || one.expected {
            continue;
        }
        if rust_mutants::rule::Registry::canonical()
            .lookup(&one.rule)
            .is_some_and(|rule| rule.survivor_names_an_unexecuted_path())
        {
            folded
                .entry((one.path.as_str(), one.rule.as_str()))
                .or_default()
                .push(one);
        } else {
            alone.push(one);
        }
    }
    let counted = folded.values().map(Vec::len).sum::<usize>();
    if counted == 0 && alone.is_empty() {
        return String::new();
    }
    let total = document
        .mutants
        .iter()
        .filter(|one| one.outcome == rust_mutants::outcome::Outcome::Survived && !one.expected)
        .count();
    let mut text = format!(
        "\nSURVIVORS    {total} survivors: {} each its own finding, and {counted} that are \
         {} unexercised paths, named once each below\n",
        alone.len(),
        folded.len()
    );
    for ((path, rule), grouped) in &folded {
        let written = writeln!(text, "  {:>4} x {rule:<26} {path}", grouped.len());
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}

/// What the run came to, in the four lines a reader takes away from it.
fn totals(document: &run::RunDocument) -> Result<String, rust_mutants::work::WorkError> {
    let tally = tally::Tally::of(document);
    let mut text = format!("MUTANTS   {}\n", tally.said());
    let outcomes = tally
        .parts
        .iter()
        .map(|(name, count)| format!("{}={count}", name.replace(' ', "_")))
        .collect::<Vec<String>>()
        .join(" ");
    let written = writeln!(text, "OUTCOMES  {outcomes}");
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    let written = writeln!(text, "OF THOSE  {}", tally.within_said());
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
    text.push_str(&work_line(document)?);
    Ok(text)
}
