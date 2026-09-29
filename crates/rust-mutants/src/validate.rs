// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deciding which candidates are real mutants, by compiling them.

use std::collections::{BTreeMap, BTreeSet};

use crate::cargo::{CargoError, Completion, Diagnostic, DiagnosticSpan, Message};
use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::instrument::{Deconst, FileOutput, InstrumentError};
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
    /// How the build came out, as its one final record and cargo's exit code established it together.
    pub completion: Completion,
    /// How many of the files this attempt had to write again, which is how many its condemnations changed.
    #[doc(alias = "rewritten")]
    pub written: u32,
}

/// Instrumenting the tree with a set of mutants left out, and compiling it.
pub trait Compile {
    /// Instruments the tree leaving out `condemned`, writing without their `const` the `const fn`s that hold a guard and the ones `constness` says carry one, compiles it, and reports what the compiler said.
    ///
    /// # Errors
    /// Whatever stopped the attempt from happening at all.
    /// A tree that merely fails to compile is a successful attempt whose [`Attempt::completion`] is [`Completion::Refused`].
    fn attempt(
        &mut self,
        condemned: &BTreeSet<u32>,
        constness: &Constness,
    ) -> Result<Attempt, ValidateError>;
}

/// Where a `const fn` is: its file, and the bytes its `const` covers in the pristine file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstFnAt {
    /// The workspace-relative path.
    pub path: String,
    /// The bytes its `const` covers in the pristine file.
    pub keyword: Span,
}

/// What validation has learned about which `const fn`s the tree may write without their `const` (ADR 0047).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Constness {
    /// Every `const fn` the compiler evaluates before the program runs, which keeps its `const` and so holds no guard.
    pub pinned: BTreeSet<ConstFnAt>,
    /// Every call the compiler refused from a `const fn` to one written without its `const`, as the caller and the callee: the caller goes without its `const` too wherever the callee does.
    pub calls: BTreeSet<(ConstFnAt, ConstFnAt)>,
}

impl Constness {
    /// Every `const fn` to write without its `const` although it holds no guard, by file, for the placements of each file that an attempt keeps: every caller the compiler named of a function holding one of their guards, and their callers in turn, except the ones it evaluates before the program runs.
    #[must_use]
    pub fn carriers_of<'p>(
        &self,
        kept: impl IntoIterator<Item = (&'p str, &'p [crate::instrument::Placement])>,
    ) -> BTreeMap<String, Vec<Span>> {
        let holding: BTreeSet<ConstFnAt> = kept
            .into_iter()
            .flat_map(|(path, placements)| {
                placements.iter().filter_map(move |placement| {
                    placement.hint.const_fn.as_ref().map(|function| ConstFnAt {
                        path: path.to_owned(),
                        keyword: function.keyword,
                    })
                })
            })
            .collect();
        self.carriers(&holding)
    }

    /// Every `const fn` to write without its `const` although it holds no guard, by file: every caller the compiler named of one of `holding`, and their callers in turn, except the ones it evaluates before the program runs.
    #[must_use]
    pub fn carriers(&self, holding: &BTreeSet<ConstFnAt>) -> BTreeMap<String, Vec<Span>> {
        let mut unconst: BTreeSet<&ConstFnAt> = holding.iter().collect();
        loop {
            let reached: Vec<&ConstFnAt> = self
                .calls
                .iter()
                .filter(|(caller, callee)| {
                    unconst.contains(callee)
                        && !unconst.contains(caller)
                        && !self.pinned.contains(caller)
                })
                .map(|(caller, _)| caller)
                .collect();
            if reached.is_empty() {
                break;
            }
            unconst.extend(reached);
        }
        let mut carriers: BTreeMap<String, Vec<Span>> = BTreeMap::new();
        for function in unconst.into_iter().filter(|one| !holding.contains(*one)) {
            carriers
                .entry(function.path.clone())
                .or_default()
                .push(function.keyword);
        }
        carriers
    }
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

/// Why validation left a candidate out of the tree.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Condemnation {
    /// The compiler refused the mutated program, which is a fact about the edit.
    CompilerRefused,
    /// The compiler evaluates the `const fn` the edit is in before the program runs, which is a fact about where the edit is (ADR 0047).
    EvaluatedBeforeRun,
}

impl Condemnation {
    /// The kebab-case name a report carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CompilerRefused => "compiler-refused",
            Self::EvaluatedBeforeRun => "evaluated-before-run",
        }
    }

    /// The reason a place passed over this way is counted under, where it is counted as one rather than as a refusal.
    #[must_use]
    pub const fn skipped(self) -> Option<crate::syntax::SkipReason> {
        match self {
            Self::CompilerRefused => None,
            Self::EvaluatedBeforeRun => Some(crate::syntax::SkipReason::EvaluatedBeforeRun),
        }
    }

    /// Whether the compiler refused the edit itself, which is what a report's `refused` counts.
    #[must_use]
    pub const fn refused(self) -> bool {
        self.skipped().is_none()
    }
}

impl std::fmt::Display for Condemnation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Every place `rejections` passed over rather than refused, counted by file under the reason it was passed over for, beside `skips`, in the order skips are reported in.
///
/// # Errors
/// [`ValidateError::AccountingOverflow`] when one file's count does not fit.
pub fn passed_over(
    mut skips: Vec<crate::syntax::Skip>,
    rejections: &[Rejection],
) -> Result<Vec<crate::syntax::Skip>, ValidateError> {
    let mut counted: BTreeMap<(crate::syntax::SkipReason, &str), u32> = BTreeMap::new();
    for rejection in rejections {
        let Some(reason) = rejection.reason.skipped() else {
            continue;
        };
        let count = counted
            .entry((reason, rejection.path.as_str()))
            .or_insert(0);
        *count = checked_add(*count, 1, "places passed over in one file")?;
    }
    skips.extend(
        counted
            .into_iter()
            .map(|((reason, path), count)| crate::syntax::Skip {
                reason,
                path: path.to_owned(),
                count,
            }),
    );
    skips.sort();
    Ok(skips)
}

/// The code the compiler refuses a call in a constant context with, whose message names the function it would have to evaluate before the program runs.
pub const EVALUATED_CODE: &str = "E0015";

/// One candidate validation left out, with the compiler's own words.
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
    /// Why it was left out.
    pub reason: Condemnation,
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
    /// The mutants an error was inside of, or whose `const fn` an error said the compiler evaluates.
    pub condemned: BTreeSet<u32>,
    /// The condemned whose first error said the compiler evaluates their `const fn` before the program runs, which the next round writes with its `const` again.
    pub evaluated: BTreeSet<u32>,
    /// Every `const fn` an error said the compiler evaluates before the program runs, which keeps its `const` from now on.
    pub pinned: BTreeSet<ConstFnAt>,
    /// Every call from a `const fn` an error refused because the callee was written without its `const`, as the caller and the callee.
    pub calls: BTreeSet<(ConstFnAt, ConstFnAt)>,
    /// The errors a caller written without its `const` from the next round on accounts for, rendered.
    pub carried: Vec<String>,
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

/// Reads one round's diagnostics against the files that were written, and what validation has learned about their `const fn`s.
///
/// An error that refuses a call to a `const fn` the files write without their `const` is about that function wherever it points (ADR 0047).
/// Where the call is in the body of a `const fn` that keeps its `const` only for want of a guard, the caller goes without its `const` too.
/// Anywhere else the compiler evaluates the callee before the program runs, so it keeps its `const` from now on and every mutant it holds is condemned.
#[must_use]
pub fn attribute(files: &[FileOutput], messages: &[Message], constness: &Constness) -> Attributed {
    let mut attributed = Attributed::default();
    for message in messages {
        let Message::CompilerMessage(compiler) = message else {
            continue;
        };
        let diagnostic = &compiler.message;
        if !diagnostic.is_error() {
            continue;
        }
        let callees = evaluated(files, diagnostic);
        if !callees.is_empty() {
            match caller(files, diagnostic).filter(|caller| !constness.pinned.contains(caller)) {
                Some(caller) => {
                    attributed.calls.extend(
                        callees
                            .iter()
                            .map(|callee| (caller.clone(), callee.at.clone())),
                    );
                    attributed.carried.push(rendered(diagnostic));
                }
                None => {
                    for callee in &callees {
                        attributed.pinned.insert(callee.at.clone());
                        for index in &callee.function.mutants {
                            condemn(&mut attributed, *index, diagnostic, true);
                        }
                    }
                }
            }
            continue;
        }
        match locate(files, diagnostic) {
            Some(index) => condemn(&mut attributed, index, diagnostic, false),
            None => attributed.unattributed.push(rendered(diagnostic)),
        }
    }
    attributed
}

/// A function written without its `const`, as an error named it: where it is, and what the text holds of it.
struct Named<'f> {
    at: ConstFnAt,
    function: &'f Deconst,
}

/// The `const fn` written with its `const` whose own body holds the call an error refuses: the narrowest body holding the call, unless a constant inside that body holds it, which the compiler evaluates on its own.
fn caller(files: &[FileOutput], diagnostic: &Diagnostic) -> Option<ConstFnAt> {
    let span = diagnostic.primary_span()?;
    let at = span.byte_start();
    let holds = |outer: &Span| outer.start <= at && at < outer.end;
    files
        .iter()
        .filter(|file| crate::cargo::names_file(&span.file_name, &file.path))
        .flat_map(|file| file.constant.iter().map(move |held| (file, held)))
        .filter(|(_, held)| holds(&held.body))
        .min_by_key(|(_, held)| held.body.len())
        .filter(|(_, held)| !held.evaluated.iter().any(holds))
        .map(|(file, held)| ConstFnAt {
            path: file.path.clone(),
            keyword: held.origin,
        })
}

/// Condemns one mutant for one error, keeping the first error that condemned it and whether that one was about its function being evaluated.
fn condemn(attributed: &mut Attributed, index: u32, diagnostic: &Diagnostic, evaluated: bool) {
    attributed.condemned.insert(index);
    if let std::collections::btree_map::Entry::Vacant(first) = attributed.diagnostics.entry(index) {
        first.insert(rendered(diagnostic));
        attributed.codes.insert(index, diagnostic.code.clone());
        if evaluated {
            attributed.evaluated.insert(index);
        }
    }
}

/// The functions written without their `const` whose call an error refuses: the one a note of the error defines, or else every one its message could name.
fn evaluated<'f>(files: &'f [FileOutput], diagnostic: &Diagnostic) -> Vec<Named<'f>> {
    if diagnostic.code.as_deref() != Some(EVALUATED_CODE) {
        return Vec::new();
    }
    let defined: Vec<Named<'f>> = diagnostic
        .children
        .iter()
        .flat_map(|child| &child.spans)
        .flat_map(|span| defined_at(files, span))
        .collect();
    if !defined.is_empty() {
        return defined;
    }
    let Some(Callee { owner, name }) = callee(&diagnostic.message) else {
        return Vec::new();
    };
    let named: Vec<Named<'f>> = files
        .iter()
        .flat_map(|file| {
            file.deconst
                .iter()
                .map(move |function| named(file, function))
        })
        .filter(|one| one.function.name == name)
        .collect();
    if owner.is_some()
        && named
            .iter()
            .any(|one| one.function.owner.as_ref() == owner.as_ref())
    {
        return named
            .into_iter()
            .filter(|one| one.function.owner.as_ref() == owner.as_ref())
            .collect();
    }
    named
}

/// A function of `file` written without its `const`, with where it is.
fn named<'f>(file: &FileOutput, function: &'f Deconst) -> Named<'f> {
    Named {
        at: ConstFnAt {
            path: file.path.clone(),
            keyword: function.origin,
        },
        function,
    }
}

/// A function as an error's message names it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Callee {
    /// The path segment before its name, which for an associated function is the type it belongs to.
    owner: Option<String>,
    /// Its own name.
    name: String,
}

/// Every function written without its `const` whose keyword lies inside `span`, which is where the compiler points at a function's definition.
fn defined_at<'f>(files: &'f [FileOutput], span: &DiagnosticSpan) -> Vec<Named<'f>> {
    files
        .iter()
        .filter(|file| crate::cargo::names_file(&span.file_name, &file.path))
        .flat_map(|file| {
            file.deconst
                .iter()
                .map(move |function| named(file, function))
        })
        .filter(|one| {
            span.byte_start() <= one.function.keyword.start
                && one.function.keyword.start < span.byte_end()
        })
        .collect()
}

/// The function an error's message names between its first pair of backticks, with every generic argument left out.
fn callee(message: &str) -> Option<Callee> {
    let rest = message.split_once('`')?.1;
    let quoted = rest.split_once('`')?.0;
    let mut depth: u32 = 0;
    let mut plain = String::with_capacity(quoted.len());
    for character in quoted.chars() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => depth = depth.checked_sub(1)?,
            other if depth == 0 => plain.push(other),
            _nested => {}
        }
    }
    let segments: Vec<&str> = plain.split("::").filter(|one| !one.is_empty()).collect();
    let (name, before) = segments.split_last()?;
    Some(Callee {
        owner: before.last().map(|owner| (*owner).to_owned()),
        name: (*name).to_owned(),
    })
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
fn branch_at(files: &[FileOutput], span: &DiagnosticSpan) -> Option<u32> {
    let file = files
        .iter()
        .find(|file| crate::cargo::names_file(&span.file_name, &file.path))?;
    file.branches
        .iter()
        .filter(|branch| {
            branch.span.start <= span.byte_start() && span.byte_start() < branch.span.end
        })
        .min_by_key(|branch| branch.span.len())
        .map(|branch| branch.index)
}

fn rendered(diagnostic: &Diagnostic) -> String {
    match &diagnostic.rendered {
        Some(rendered) => rendered.clone(),
        None => diagnostic.message.clone(),
    }
}

/// The first error of a message stream, rendered, so a caller can say what stopped a build without matching on the stream itself.
#[must_use]
pub fn first_error_of(messages: &[Message]) -> String {
    first_error(messages)
}

/// The first error of a round, rendered, for a message about the round.
fn first_error(messages: &[Message]) -> String {
    match messages.iter().find_map(|message| match message {
        Message::CompilerMessage(compiler) if compiler.message.is_error() => {
            Some(rendered(&compiler.message))
        }
        _ => None,
    }) {
        Some(first) => first,
        None => "the build failed without an error message".to_owned(),
    }
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
    let mut constness = Constness::default();
    let mut diagnostics: BTreeMap<u32, Said> = BTreeMap::new();
    let mut interacting: BTreeSet<u32> = BTreeSet::new();
    let mut rounds: u32 = 0;
    let mut bisections: u32 = 0;

    loop {
        if cancel.is_cancelled() {
            return Err(ValidateError::Cancelled);
        }
        let attempt = cancelled_or(compile.attempt(&condemned, &constness), cancel)?;
        rounds = checked_add(rounds, 1, "validation rounds")?;
        match attempt.completion {
            Completion::Built => {
                trace.validate_round(ValidateRoundRecord {
                    round: rounds,
                    condemned: exact_count(condemned.len(), "condemned mutants")?,
                    success: true,
                    attributed: Vec::new(),
                    carried: Vec::new(),
                    unattributed: Vec::new(),
                    written: attempt.written,
                });
                break;
            }
            Completion::Refused => {}
        }
        let attributed = attribute(&attempt.files, &attempt.messages, &constness);
        trace.validate_round(refused_round(
            (rounds, &condemned),
            attempt.written,
            &attributed,
        )?);
        record(&attributed, &mut diagnostics);
        let progressed = !attributed.condemned.is_subset(&condemned);
        let learned = !attributed.pinned.is_subset(&constness.pinned)
            || !attributed.calls.is_subset(&constness.calls);
        condemned.extend(attributed.condemned.iter().copied());
        constness.pinned.extend(attributed.pinned);
        constness.calls.extend(attributed.calls);

        if learned || (progressed && rounds < options.max_rounds) {
            continue;
        }
        let settled = cancelled_or(
            settle(compile, (all, &condemned, &constness), validating),
            cancel,
        )?;
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

/// What each condemned mutant is in a report: its identity, its place, what the compiler said, and why it was left out.
fn refusals(
    catalog: &Catalog,
    diagnostics: BTreeMap<u32, Said>,
    interacting: &BTreeSet<u32>,
) -> Vec<Rejection> {
    diagnostics
        .into_iter()
        .filter_map(|(index, said)| {
            let mutant = catalog.by_index(index)?;
            Some(Rejection {
                index,
                id: mutant.id.to_string(),
                display_id: mutant.display_id.to_string(),
                path: mutant.candidate.path.clone(),
                span: mutant.candidate.span,
                rule: mutant.candidate.rule.name.to_owned(),
                code: said.code,
                diagnostic: said.words,
                isolated: !interacting.contains(&index),
                reason: said.reason,
            })
        })
        .collect()
}

/// What validation keeps about one condemned mutant: the compiler's code and words, and why it was left out.
#[derive(Debug, Clone)]
struct Said {
    code: Option<String>,
    words: String,
    reason: Condemnation,
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

/// The trace's record of round `round`, which `condemned` went into and the compiler refused, and of what it attributed.
fn refused_round(
    (round, condemned): (u32, &BTreeSet<u32>),
    written: u32,
    attributed: &Attributed,
) -> Result<ValidateRoundRecord, ValidateError> {
    Ok(ValidateRoundRecord {
        round,
        condemned: exact_count(condemned.len(), "condemned mutants")?,
        success: false,
        written,
        attributed: attributions(attributed),
        carried: attributed
            .carried
            .iter()
            .map(|said| first_line(said))
            .collect(),
        unattributed: attributed
            .unattributed
            .iter()
            .map(|said| first_line(said))
            .collect(),
    })
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
            said: match attributed.diagnostics.get(index) {
                Some(said) => first_line(said),
                None => String::new(),
            },
        })
        .collect()
}

/// The first line of a rendered diagnostic, which is the one that says what went wrong.
fn first_line(said: &str) -> String {
    match said.lines().next() {
        Some(first) => first,
        None => said,
    }
    .to_owned()
}

/// Keeps the first thing the compiler said about each condemned mutant, and why it was left out.
fn record(attributed: &Attributed, into: &mut BTreeMap<u32, Said>) {
    for index in &attributed.condemned {
        let code = attributed
            .codes
            .get(index)
            .cloned()
            .and_then(std::convert::identity);
        let words = match attributed.diagnostics.get(index) {
            Some(said) => said.clone(),
            None => String::new(),
        };
        let reason = if attributed.evaluated.contains(index) {
            Condemnation::EvaluatedBeforeRun
        } else {
            Condemnation::CompilerRefused
        };
        into.entry(*index).or_insert(Said {
            code,
            words,
            reason,
        });
    }
}

/// What settling by bisection established.
struct Settled {
    condemned: BTreeSet<u32>,
    diagnostics: BTreeMap<u32, Said>,
    rounds: u32,
    attempts: u32,
    unsettled: bool,
    /// The offenders the compiler refused only alongside another, which is what bisection could not narrow further.
    interacting: BTreeSet<u32>,
}

/// Isolates the offenders among everything still live, and leaves a tree that compiled behind.
fn settle(
    compile: &mut dyn Compile,
    (all, condemned, constness): (&BTreeSet<u32>, &BTreeSet<u32>, &Constness),
    validating: &Validating<'_>,
) -> Result<Settled, ValidateError> {
    let Validating { cancel, trace, .. } = *validating;
    if cancel.is_cancelled() {
        return Err(ValidateError::Cancelled);
    }
    let live: Vec<u32> = all.difference(condemned).copied().collect();
    let pristine = compile.attempt(all, constness)?;
    match pristine.completion {
        Completion::Built => {}
        Completion::Refused => {
            return Err(ValidateError::NotMutantInduced {
                first: first_error(&pristine.messages),
            });
        }
    }
    let mut isolation = Isolation {
        compile,
        base: all,
        constness,
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
    let last = compile.attempt(&finally, constness)?;
    settled.rounds = checked_add(settled.rounds, 1, "settling rounds")?;
    match last.completion {
        Completion::Built => return Ok(settled),
        Completion::Refused => {}
    }
    let again = attribute(&last.files, &last.messages, constness);
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
                    alone.unattributed.first().cloned().map(|first| Said {
                        code: alone.code.clone(),
                        words: first,
                        reason: Condemnation::CompilerRefused,
                    })
                })
                .filter(|said| !said.words.is_empty());
            let Said {
                code,
                words,
                reason,
            } = match said {
                Some(said) => {
                    diagnosed = checked_add(diagnosed, 1, "diagnosed offenders")?;
                    said
                }
                None => Said {
                    code: None,
                    words: String::new(),
                    reason: Condemnation::CompilerRefused,
                },
            };
            if offence.len() > 1 {
                settled.interacting.extend(std::iter::once(*index));
            }
            settled.diagnostics.entry(*index).or_insert_with(|| Said {
                code,
                words: told(offence, *index, &words),
                reason,
            });
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
    /// What the rounds before bisection learned about which `const fn`s go without their `const`.
    constness: &'a Constness,
    attempts: u32,
}

/// What one compilation of an offence alone said about it.
#[derive(Debug, Default)]
struct Alone {
    diagnostics: BTreeMap<u32, Said>,
    unattributed: Vec<String>,
    code: Option<String>,
}

impl Isolation<'_> {
    /// Whether a tree holding only `live` fails to compile.
    fn fails(&mut self, live: &[u32]) -> Result<bool, ValidateError> {
        Ok(match self.only(live)?.completion {
            Completion::Built => false,
            Completion::Refused => true,
        })
    }

    /// One compilation with only `live` in the tree.
    fn only(&mut self, live: &[u32]) -> Result<Attempt, ValidateError> {
        let mut condemned = self.base.clone();
        for index in live {
            condemned.remove(index);
        }
        self.attempts = checked_add(self.attempts, 1, "bisection attempts")?;
        self.compile.attempt(&condemned, self.constness)
    }

    /// Compiles one offence on its own so its own diagnostic can be kept.
    fn alone(&mut self, offence: &[u32]) -> Result<Alone, ValidateError> {
        let attempt = self.only(offence)?;
        match attempt.completion {
            Completion::Built => return Ok(Alone::default()),
            Completion::Refused => {}
        }
        let attributed = attribute(&attempt.files, &attempt.messages, self.constness);
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
