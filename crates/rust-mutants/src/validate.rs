// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deciding which candidates are real mutants, by compiling them.

use std::collections::{BTreeMap, BTreeSet};

use crate::cargo::{CargoError, Diagnostic, Message};
use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::instrument::{FileOutput, InstrumentError};
use crate::runner::Cancel;
use crate::span::Span;
use crate::trace::{AttributionRecord, BisectRecord, Recorder, ValidateRoundRecord};

/// How many rounds of "condemn what was attributed and recompile" are tried before the remaining suspects are isolated by bisection.
pub const DEFAULT_MAX_ROUNDS: u32 = 6;

/// What one instrumented compilation produced.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// The files as they were written, with their branch tables.
    pub files: Vec<FileOutput>,
    /// Everything the compiler said.
    pub messages: Vec<Message>,
    /// Whether every unit compiled.
    pub success: bool,
    /// How many of the files this attempt had to write again, which is how many its condemnations changed.
    #[doc(alias = "rewritten")]
    pub written: u32,
}

/// Instrumenting the tree with a set of mutants left out, and compiling it.
pub trait Compile {
    /// Instruments the tree leaving out `condemned`, compiles it, and reports what the compiler said.
    ///
    /// # Errors
    /// Whatever stopped the attempt from happening at all.
    /// A tree that merely fails to compile is a successful attempt with [`Attempt::success`] false.
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError>;
}

/// Configures [`validate`].
#[derive(Debug, Clone, Copy)]
pub struct ValidateOptions {
    /// How many attribution rounds before falling back to bisection.
    pub max_rounds: u32,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            max_rounds: DEFAULT_MAX_ROUNDS,
        }
    }
}

/// One candidate the compiler refused, with its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The mutant's full identity.
    pub id: String,
    /// The mutant's short identity.
    pub display_id: String,
    /// The workspace-relative path of the file it lives in.
    pub path: String,
    /// The bytes the edit would have replaced.
    pub span: Span,
    /// The rule that proposed it.
    pub rule: String,
    /// The compiler's error code, when it had one.
    pub code: Option<String>,
    /// What the compiler said, rendered as it renders it for a person.
    pub diagnostic: String,
    /// Whether the compiler refused it on its own, rather than only alongside another mutant.
    pub isolated: bool,
}

/// What validation established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    /// The indices of the mutants that compile, ascending.
    pub accepted: Vec<u32>,
    /// The candidates the compiler refused, in catalog order.
    pub rejections: Vec<Rejection>,
    /// How many attribution rounds ran.
    pub rounds: u32,
    /// How many compilations bisection cost.
    pub bisections: u32,
}

/// What one round's diagnostics said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attributed {
    /// The mutants an error was inside of.
    pub condemned: BTreeSet<u32>,
    /// The errors no branch accounts for, rendered.
    pub unattributed: Vec<String>,
    /// What the compiler said about each condemned mutant.
    pub diagnostics: BTreeMap<u32, String>,
    /// The error code of each condemned mutant, when it had one.
    pub codes: BTreeMap<u32, Option<String>>,
}

/// Why validation could not finish.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ValidateError {
    /// The tree does not compile with no mutant live, so nothing here is about the mutants.
    #[error("{}: validate: the tree does not compile before any mutant is live: {first}", error::VALIDATE_NOT_MUTANT_INDUCED.code)]
    NotMutantInduced {
        /// The first error the compiler reported, rendered.
        first: String,
    },
    /// Bisection ran out of room: the suspects could not be narrowed.
    #[error("{}: validate: {suspects} suspects could not be isolated", error::VALIDATE_NOT_ISOLATED.code)]
    NotIsolated {
        /// How many were left.
        suspects: usize,
    },
    /// The attempt itself could not be made: the tree could not be written, or whatever else the [`Compile`] implementation needs to say.
    #[error("{}: validate: the attempt could not be made: {message}", error::VALIDATE_ATTEMPT_FAILED.code)]
    AttemptFailed {
        /// What went wrong.
        message: String,
    },
    /// The caller cancelled before validation finished, so nothing it saw says anything about a mutant.
    #[error("{}: validate: cancelled", error::INTERRUPTED.code)]
    Cancelled,
    /// Validation accounting exceeded the exact counter carried by its trace.
    #[error("{}: validate: {what} exceeded its exact u32 counter", error::VALIDATE_ATTEMPT_FAILED.code)]
    AccountingOverflow {
        /// The counter or arithmetic operation that overflowed.
        what: &'static str,
    },
    /// A file could not be instrumented.
    #[error(transparent)]
    Instrument(#[from] InstrumentError),
    /// The toolchain could not be driven.
    #[error(transparent)]
    Cargo(#[from] CargoError),
}

impl ValidateError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::NotMutantInduced { .. } => error::VALIDATE_NOT_MUTANT_INDUCED,
            Self::NotIsolated { .. } => error::VALIDATE_NOT_ISOLATED,
            Self::AttemptFailed { .. } | Self::AccountingOverflow { .. } => {
                error::VALIDATE_ATTEMPT_FAILED
            }
            Self::Cancelled => error::INTERRUPTED,
            Self::Instrument(error) => error.code(),
            Self::Cargo(error) => error.code(),
        }
    }
}

/// Reads one round's diagnostics against the files that were written.
#[must_use]
pub fn attribute(files: &[FileOutput], messages: &[Message]) -> Attributed {
    let mut attributed = Attributed::default();
    for message in messages {
        let Message::CompilerMessage(compiler) = message else {
            continue;
        };
        let diagnostic = &compiler.message;
        if !diagnostic.is_error() {
            continue;
        }
        match locate(files, diagnostic) {
            Some(index) => {
                attributed.condemned.insert(index);
                attributed
                    .diagnostics
                    .entry(index)
                    .or_insert_with(|| rendered(diagnostic));
                attributed
                    .codes
                    .entry(index)
                    .or_insert_with(|| diagnostic.code.clone());
            }
            None => attributed.unattributed.push(rendered(diagnostic)),
        }
    }
    attributed
}

/// The mutant whose branch holds a span of this diagnostic, primary first, then the rest, then its notes.
fn locate(files: &[FileOutput], diagnostic: &Diagnostic) -> Option<u32> {
    if let Some(index) = diagnostic
        .primary_span()
        .and_then(|span| branch_at(files, span))
    {
        return Some(index);
    }
    if let Some(index) = diagnostic
        .spans
        .iter()
        .find_map(|span| branch_at(files, span))
    {
        return Some(index);
    }
    diagnostic
        .children
        .iter()
        .find_map(|child| locate(files, child))
}

/// The narrowest branch of a written file that holds `span`'s first byte.
fn branch_at(files: &[FileOutput], span: &crate::cargo::DiagnosticSpan) -> Option<u32> {
    let file = files
        .iter()
        .find(|file| crate::cargo::names_file(&span.file_name, &file.path))?;
    file.branches
        .iter()
        .filter(|branch| branch.span.start <= span.byte_start && span.byte_start < branch.span.end)
        .min_by_key(|branch| branch.span.len())
        .map(|branch| branch.index)
}

fn rendered(diagnostic: &Diagnostic) -> String {
    diagnostic
        .rendered
        .clone()
        .unwrap_or_else(|| diagnostic.message.clone())
}

/// The first error of a message stream, rendered, so a caller can say what stopped a build without matching on the stream itself.
#[must_use]
pub fn first_error_of(messages: &[Message]) -> String {
    first_error(messages)
}

/// The first error of a round, rendered, for a message about the round.
fn first_error(messages: &[Message]) -> String {
    messages
        .iter()
        .find_map(|message| match message {
            Message::CompilerMessage(compiler) if compiler.message.is_error() => {
                Some(rendered(&compiler.message))
            }
            _ => None,
        })
        .unwrap_or_else(|| "the build failed without an error message".to_owned())
}

/// What one validation is bounded and watched by.
#[derive(Debug, Clone, Copy)]
pub struct Validating<'a> {
    /// How many rounds before falling back to bisection.
    pub options: ValidateOptions,
    /// Cooperative cancellation: a build nobody waited for says nothing about a mutant.
    pub cancel: &'a Cancel,
    /// Where each round and each bisection is recorded.
    pub trace: &'a Recorder,
}

/// Establishes which of a catalog's mutants compile.
///
/// # Errors
/// [`ValidateError::NotMutantInduced`] when the tree does not compile with nothing live, and whatever [`Compile::attempt`] reports.
pub fn validate(
    catalog: &Catalog,
    compile: &mut dyn Compile,
    validating: &Validating<'_>,
) -> Result<Validated, ValidateError> {
    let all: BTreeSet<u32> = catalog
        .mutants()
        .iter()
        .map(|mutant| mutant.index)
        .collect();
    validate_set(catalog, &all, compile, validating)
}

/// Establishes which mutants in `selected` compile, without making any claim about the rest of the catalog.
///
/// # Errors
/// The same failures as [`validate`].
pub fn validate_selected(
    catalog: &Catalog,
    selected: &BTreeSet<u32>,
    compile: &mut dyn Compile,
    validating: &Validating<'_>,
) -> Result<Validated, ValidateError> {
    let all: BTreeSet<u32> = selected
        .iter()
        .copied()
        .filter(|index| catalog.by_index(*index).is_some())
        .collect();
    validate_set(catalog, &all, compile, validating)
}

fn validate_set(
    catalog: &Catalog,
    all: &BTreeSet<u32>,
    compile: &mut dyn Compile,
    validating: &Validating<'_>,
) -> Result<Validated, ValidateError> {
    let Validating {
        options,
        cancel,
        trace,
    } = *validating;
    let mut condemned: BTreeSet<u32> = BTreeSet::new();
    let mut diagnostics: BTreeMap<u32, (Option<String>, String)> = BTreeMap::new();
    let mut interacting: BTreeSet<u32> = BTreeSet::new();
    let mut rounds: u32 = 0;
    let mut bisections: u32 = 0;

    loop {
        if cancel.is_cancelled() {
            return Err(ValidateError::Cancelled);
        }
        let attempt = cancelled_or(compile.attempt(&condemned), cancel)?;
        rounds = checked_add(rounds, 1, "validation rounds")?;
        if attempt.success {
            trace.validate_round(ValidateRoundRecord {
                round: rounds,
                condemned: exact_count(condemned.len(), "condemned mutants")?,
                success: true,
                attributed: Vec::new(),
                unattributed: Vec::new(),
                written: attempt.written,
            });
            break;
        }
        let attributed = attribute(&attempt.files, &attempt.messages);
        trace.validate_round(ValidateRoundRecord {
            round: rounds,
            condemned: exact_count(condemned.len(), "condemned mutants")?,
            success: false,
            written: attempt.written,
            attributed: attributions(&attributed),
            unattributed: attributed
                .unattributed
                .iter()
                .map(|said| first_line(said))
                .collect(),
        });
        record(&attributed, &mut diagnostics);
        let progressed = !attributed.condemned.is_subset(&condemned);
        condemned.extend(attributed.condemned.iter().copied());

        if progressed && rounds < options.max_rounds {
            continue;
        }
        let settled = cancelled_or(settle(compile, all, &condemned, validating), cancel)?;
        rounds = checked_add(rounds, settled.rounds, "validation rounds")?;
        bisections = checked_add(bisections, settled.attempts, "bisection attempts")?;
        for (index, entry) in settled.diagnostics {
            diagnostics.entry(index).or_insert(entry);
        }
        interacting.extend(settled.interacting.iter().copied());
        if settled.unsettled {
            return Err(ValidateError::NotIsolated {
                suspects: all.difference(&condemned).count(),
            });
        }
        condemned.extend(settled.condemned);
        break;
    }

    Ok(Validated {
        accepted: all.difference(&condemned).copied().collect(),
        rejections: refusals(catalog, diagnostics, &interacting),
        rounds,
        bisections,
    })
}

/// What each condemned mutant is in a report: its identity, its place, and what the compiler said.
fn refusals(
    catalog: &Catalog,
    diagnostics: BTreeMap<u32, (Option<String>, String)>,
    interacting: &BTreeSet<u32>,
) -> Vec<Rejection> {
    diagnostics
        .into_iter()
        .filter_map(|(index, (code, diagnostic))| {
            let mutant = catalog.by_index(index)?;
            Some(Rejection {
                index,
                id: mutant.id.to_string(),
                display_id: mutant.display_id.to_string(),
                path: mutant.candidate.path.clone(),
                span: mutant.candidate.span,
                rule: mutant.candidate.rule.name.to_owned(),
                code,
                diagnostic,
                isolated: !interacting.contains(&index),
            })
        })
        .collect()
}

/// Reads a cancelled compilation as a cancellation rather than as whatever the half-finished command printed.
fn cancelled_or<T>(result: Result<T, ValidateError>, cancel: &Cancel) -> Result<T, ValidateError> {
    match result {
        Err(error) if cancel.is_cancelled() => {
            drop(error);
            Err(ValidateError::Cancelled)
        }
        other => other,
    }
}

/// What a round attributed, for the trace.
fn attributions(attributed: &Attributed) -> Vec<AttributionRecord> {
    attributed
        .condemned
        .iter()
        .map(|index| AttributionRecord {
            index: *index,
            code: attributed
                .codes
                .get(index)
                .cloned()
                .and_then(std::convert::identity),
            said: attributed
                .diagnostics
                .get(index)
                .map(|said| first_line(said))
                .unwrap_or_default(),
        })
        .collect()
}

/// The first line of a rendered diagnostic, which is the one that says what went wrong.
fn first_line(said: &str) -> String {
    said.lines().next().unwrap_or(said).to_owned()
}

/// Keeps the first thing the compiler said about each condemned mutant.
fn record(attributed: &Attributed, into: &mut BTreeMap<u32, (Option<String>, String)>) {
    for index in &attributed.condemned {
        let code = attributed
            .codes
            .get(index)
            .cloned()
            .and_then(std::convert::identity);
        let said = attributed
            .diagnostics
            .get(index)
            .cloned()
            .unwrap_or_default();
        into.entry(*index).or_insert((code, said));
    }
}

/// What settling by bisection established.
struct Settled {
    condemned: BTreeSet<u32>,
    diagnostics: BTreeMap<u32, (Option<String>, String)>,
    rounds: u32,
    attempts: u32,
    unsettled: bool,
    /// The offenders the compiler refused only alongside another, which is what bisection could not narrow further.
    interacting: BTreeSet<u32>,
}

/// Isolates the offenders among everything still live, and leaves a tree that compiled behind.
fn settle(
    compile: &mut dyn Compile,
    all: &BTreeSet<u32>,
    condemned: &BTreeSet<u32>,
    validating: &Validating<'_>,
) -> Result<Settled, ValidateError> {
    let Validating { cancel, trace, .. } = *validating;
    if cancel.is_cancelled() {
        return Err(ValidateError::Cancelled);
    }
    let live: Vec<u32> = all.difference(condemned).copied().collect();
    let pristine = compile.attempt(all)?;
    if !pristine.success {
        return Err(ValidateError::NotMutantInduced {
            first: first_error(&pristine.messages),
        });
    }
    let mut isolation = Isolation {
        compile,
        base: all,
        attempts: 1,
    };
    let offences = isolation.isolate(&live)?;
    let offenders: Vec<u32> = offences
        .iter()
        .flat_map(|indices| indices.iter())
        .copied()
        .collect();
    let mut settled = Settled {
        condemned: offenders.iter().copied().collect(),
        diagnostics: BTreeMap::new(),
        rounds: 1,
        attempts: isolation.attempts,
        unsettled: false,
        interacting: BTreeSet::new(),
    };
    let diagnosed = diagnose_offences(&mut isolation, &offences, &mut settled)?;
    let attempts = isolation.attempts;
    settled.attempts = attempts;
    trace.bisect(BisectRecord {
        suspects: exact_count(live.len(), "bisection suspects")?,
        offenders: offenders.clone(),
        attempts,
        diagnosed,
    });
    let mut finally = condemned.clone();
    finally.extend(offenders);
    let last = compile.attempt(&finally)?;
    settled.rounds = checked_add(settled.rounds, 1, "settling rounds")?;
    if last.success {
        return Ok(settled);
    }
    let again = attribute(&last.files, &last.messages);
    if again.condemned.is_subset(&finally) {
        settled.unsettled = true;
        return Ok(settled);
    }
    record(&again, &mut settled.diagnostics);
    settled.condemned.extend(again.condemned);
    Ok(settled)
}

fn diagnose_offences(
    isolation: &mut Isolation<'_>,
    offences: &[Vec<u32>],
    settled: &mut Settled,
) -> Result<u32, ValidateError> {
    let mut diagnosed: u32 = 0;
    for offence in offences {
        let alone = isolation.alone(offence)?;
        for index in offence {
            let said = alone
                .diagnostics
                .get(index)
                .cloned()
                .or_else(|| {
                    alone
                        .unattributed
                        .first()
                        .cloned()
                        .map(|first| (alone.code.clone(), first))
                })
                .filter(|(_, words)| !words.is_empty());
            let (code, words) = match said {
                Some(said) => {
                    diagnosed = checked_add(diagnosed, 1, "diagnosed offenders")?;
                    said
                }
                None => (None, String::new()),
            };
            if offence.len() > 1 {
                settled.interacting.extend(std::iter::once(*index));
            }
            settled
                .diagnostics
                .entry(*index)
                .or_insert_with(|| (code, told(offence, *index, &words)));
        }
    }
    Ok(diagnosed)
}

/// What a run says about an offender bisection named, with whatever the compiler said about it.
fn told(offence: &[u32], index: u32, words: &str) -> String {
    let others: Vec<String> = offence
        .iter()
        .filter(|one| **one != index)
        .map(ToString::to_string)
        .collect();
    let head = if others.is_empty() {
        String::new()
    } else {
        format!(
            "the compiler refused this mutant together with {}, and each of them compiles on its own\n",
            others.join(", ")
        )
    };
    if words.is_empty() {
        if head.is_empty() {
            return "the compiler refused this mutant, and no diagnostic named it".to_owned();
        }
        return head.trim_end().to_owned();
    }
    format!("{head}{words}")
}

/// Narrowing a set of suspects by halving.
struct Isolation<'a> {
    compile: &'a mut dyn Compile,
    /// Everything, so that "only these are live" is spelled as a condemned set the [`Compile`] seam understands.
    base: &'a BTreeSet<u32>,
    attempts: u32,
}

/// What one compilation of an offence alone said about it.
#[derive(Debug, Default)]
struct Alone {
    diagnostics: BTreeMap<u32, (Option<String>, String)>,
    unattributed: Vec<String>,
    code: Option<String>,
}

impl Isolation<'_> {
    /// Whether a tree holding only `live` fails to compile.
    fn fails(&mut self, live: &[u32]) -> Result<bool, ValidateError> {
        Ok(!self.only(live)?.success)
    }

    /// One compilation with only `live` in the tree.
    fn only(&mut self, live: &[u32]) -> Result<Attempt, ValidateError> {
        let mut condemned = self.base.clone();
        for index in live {
            condemned.remove(index);
        }
        self.attempts = checked_add(self.attempts, 1, "bisection attempts")?;
        self.compile.attempt(&condemned)
    }

    /// Compiles one offence on its own so its own diagnostic can be kept.
    fn alone(&mut self, offence: &[u32]) -> Result<Alone, ValidateError> {
        let attempt = self.only(offence)?;
        if attempt.success {
            return Ok(Alone::default());
        }
        let attributed = attribute(&attempt.files, &attempt.messages);
        let mut alone = Alone {
            diagnostics: BTreeMap::new(),
            unattributed: attributed.unattributed.clone(),
            code: None,
        };
        record(&attributed, &mut alone.diagnostics);
        if alone.unattributed.is_empty() {
            return Ok(alone);
        }
        alone.code = first_error_code(&attempt.messages);
        Ok(alone)
    }

    /// The offences among `suspects`, each a set the compiler refuses and no proper part of which it refuses.
    fn isolate(&mut self, suspects: &[u32]) -> Result<Vec<Vec<u32>>, ValidateError> {
        if suspects.is_empty() || !self.fails(suspects)? {
            return Ok(Vec::new());
        }
        if suspects.len() == 1 {
            return Ok(vec![suspects.to_vec()]);
        }
        let (left, right) = suspects.split_at(suspects.len() / 2);
        let mut offences = self.isolate(left)?;
        offences.extend(self.isolate(right)?);
        if offences.is_empty() {
            return Ok(vec![self.narrow(suspects.to_vec())?]);
        }
        Ok(offences)
    }

    /// The smallest part of `suspects` the compiler still refuses, when no half of it is refused alone.
    fn narrow(&mut self, suspects: Vec<u32>) -> Result<Vec<u32>, ValidateError> {
        let budget = checked_add(
            self.attempts,
            bisect_budget(suspects.len())?,
            "bisection budget",
        )?;
        let mut candidate = suspects;
        let mut parts = 4;
        while candidate.len() > 1 && self.attempts < budget {
            if parts > candidate.len() {
                break;
            }
            let chunks = chunks(&candidate, parts)?;
            if let Some(smaller) = self.first_failing(&chunks)? {
                candidate = smaller;
                parts = 2;
                continue;
            }
            let complements: Vec<Vec<u32>> = chunks
                .iter()
                .map(|chunk| without(&candidate, chunk))
                .collect();
            if let Some(smaller) = self.first_failing(&complements)? {
                parts = parts
                    .checked_sub(1)
                    .ok_or(ValidateError::AccountingOverflow {
                        what: "bisection partitions",
                    })?
                    .max(2);
                candidate = smaller;
                continue;
            }
            if parts >= candidate.len() {
                break;
            }
            parts = parts
                .checked_mul(2)
                .ok_or(ValidateError::AccountingOverflow {
                    what: "bisection partitions",
                })?
                .min(candidate.len());
        }
        Ok(candidate)
    }

    /// The first of `sets` the compiler still refuses, if any.
    fn first_failing(&mut self, sets: &[Vec<u32>]) -> Result<Option<Vec<u32>>, ValidateError> {
        for set in sets {
            if set.is_empty() || !self.fails(set)? {
                continue;
            }
            return Ok(Some(set.clone()));
        }
        Ok(None)
    }
}

/// How many compilations narrowing one interaction may cost.
fn bisect_budget(suspects: usize) -> Result<u32, ValidateError> {
    let attempts = suspects
        .checked_mul(8)
        .ok_or(ValidateError::AccountingOverflow {
            what: "bisection budget",
        })?;
    Ok(exact_count(attempts, "bisection budget")?.max(64))
}

/// `values` cut into `parts` pieces, the earlier ones one longer when it does not divide.
fn chunks(values: &[u32], parts: usize) -> Result<Vec<Vec<u32>>, ValidateError> {
    if parts == 0 {
        return Ok(Vec::new());
    }
    let size = values.len().div_euclid(parts);
    let extra = values.len().rem_euclid(parts);
    let mut found = Vec::new();
    let mut at: usize = 0;
    for part in 0..parts {
        let take = size.checked_add(usize::from(part < extra)).ok_or(
            ValidateError::AccountingOverflow {
                what: "bisection chunk size",
            },
        )?;
        let end = at
            .checked_add(take)
            .ok_or(ValidateError::AccountingOverflow {
                what: "bisection chunk boundary",
            })?
            .min(values.len());
        let slice = values
            .get(at..end)
            .ok_or(ValidateError::AccountingOverflow {
                what: "bisection chunk boundary",
            })?;
        found.push(slice.to_vec());
        at = end;
    }
    Ok(found)
}

fn checked_add(left: u32, right: u32, what: &'static str) -> Result<u32, ValidateError> {
    left.checked_add(right)
        .ok_or(ValidateError::AccountingOverflow { what })
}

fn exact_count(value: usize, what: &'static str) -> Result<u32, ValidateError> {
    u32::try_from(value).map_err(|_overflow| ValidateError::AccountingOverflow { what })
}

/// `values` without anything in `removed`.
fn without(values: &[u32], removed: &[u32]) -> Vec<u32> {
    values
        .iter()
        .filter(|one| !removed.contains(one))
        .copied()
        .collect()
}

/// The code of the first error of a stream, for a diagnostic that named no branch.
fn first_error_code(messages: &[Message]) -> Option<String> {
    messages.iter().find_map(|message| match message {
        Message::CompilerMessage(compiler) if compiler.message.is_error() => {
            compiler.message.code.clone()
        }
        _ => None,
    })
}
