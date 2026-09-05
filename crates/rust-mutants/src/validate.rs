// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deciding which candidates are real mutants, by compiling them.
//!
//! Discovery proposes an edit wherever a rule's token shape appears, and
//! nothing before this point type-checks anything: `String + &str` becomes a
//! `sub-to-add` candidate, `x * 0` becomes `x / 0`, and a function returning
//! a type with no `Default` gets a `return-default`. Which of those are
//! programs is a question the compiler answers for free while building the
//! tree that has to be built anyway.
//!
//! # Attribution, not bisection
//!
//! Every alternative's own text occupies a known byte range of the
//! instrumented file ([`crate::instrument::Branch`]), and rustc reports the
//! byte range of every diagnostic. An error whose primary span falls inside
//! a branch is therefore about exactly that mutant, and the whole round's
//! refusals can be condemned at once: one recompilation per round rather
//! than one per candidate.
//!
//! An error that falls outside every branch is not attributable. It is not
//! ignored: the suspects are isolated by bisection, halving the live set
//! until the offenders are named. That costs compilations, which is why the
//! branch table exists.
//!
//! # Fail closed
//!
//! A tree that does not compile with *no* mutant live is not the mutants'
//! fault, and the run stops with [`ValidateError::NotMutantInduced`] rather
//! than condemning candidates until the error goes away. A round loop that
//! does not settle within [`ValidateOptions::max_rounds`] falls back to
//! bisection rather than accepting a tree it never saw compile, and the
//! final state is always a tree that compiled.

use std::collections::{BTreeMap, BTreeSet};

use crate::cargo::{CargoError, Diagnostic, Message};
use crate::catalog::Catalog;
use crate::error::{self, ErrorCode};
use crate::instrument::{FileOutput, InstrumentError};
use crate::span::Span;
use crate::trace::Recorder;

/// How many rounds of "condemn what was attributed and recompile" are tried
/// before the remaining suspects are isolated by bisection.
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
}

/// Instrumenting the tree with a set of mutants left out, and compiling it.
///
/// The seam of this module: the loop below knows nothing about snapshots,
/// cargo, or the filesystem, so a test drives it with a script and a run
/// drives it with a real toolchain.
pub trait Compile {
    /// Instruments the tree leaving out `condemned`, compiles it, and
    /// reports what the compiler said.
    ///
    /// # Errors
    ///
    /// Whatever stopped the attempt from happening at all. A tree that
    /// merely fails to compile is a successful attempt with
    /// [`Attempt::success`] false.
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
    /// The tree does not compile with no mutant live, so nothing here is
    /// about the mutants.
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
    /// The attempt itself could not be made: the tree could not be written,
    /// or whatever else the [`Compile`] implementation needs to say.
    #[error("{}: validate: the attempt could not be made: {message}", error::VALIDATE_ATTEMPT_FAILED.code)]
    AttemptFailed {
        /// What went wrong.
        message: String,
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
            Self::AttemptFailed { .. } => error::VALIDATE_ATTEMPT_FAILED,
            Self::Instrument(error) => error.code(),
            Self::Cargo(error) => error.code(),
        }
    }
}

/// Reads one round's diagnostics against the files that were written.
///
/// An error is attributed to a mutant when its primary span lies inside
/// that mutant's branch in the file the diagnostic names. Warnings are not
/// rejections: the instrumented tree is allowed to be noisy.
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

/// The mutant whose branch holds this diagnostic's primary span.
fn locate(files: &[FileOutput], diagnostic: &Diagnostic) -> Option<u32> {
    let span = diagnostic.primary_span()?;
    let file = files
        .iter()
        .find(|file| ends_with_path(&span.file_name, &file.path))?;
    file.branches
        .iter()
        .filter(|branch| branch.span.start <= span.byte_start && span.byte_start < branch.span.end)
        // The innermost branch holding the offset: a nested site's branch
        // lies inside its parent's original branch, never inside one of the
        // parent's alternatives, so the narrowest is the one that is about
        // this error.
        .min_by_key(|branch| branch.span.len())
        .map(|branch| branch.index)
}

/// Whether the path a diagnostic names is the file that was written. rustc
/// reports a path relative to the directory it ran in, which is the
/// workspace root, and the engine names files the same way; a member's
/// nested path therefore matches exactly, and an absolute one by suffix.
fn ends_with_path(reported: &str, path: &str) -> bool {
    let reported = reported.replace('\\', "/");
    reported == path || reported.ends_with(&format!("/{path}"))
}

fn rendered(diagnostic: &Diagnostic) -> String {
    diagnostic
        .rendered
        .clone()
        .unwrap_or_else(|| diagnostic.message.clone())
}

/// The first error of a message stream, rendered, so a caller can say what
/// stopped a build without matching on the stream itself.
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

/// Establishes which of a catalog's mutants compile.
///
/// Returns when a tree holding exactly the accepted mutants has compiled.
///
/// # Errors
///
/// [`ValidateError::NotMutantInduced`] when the tree does not compile with
/// nothing live, and whatever [`Compile::attempt`] reports.
pub fn validate(
    catalog: &Catalog,
    compile: &mut dyn Compile,
    options: ValidateOptions,
    trace: &Recorder,
) -> Result<Validated, ValidateError> {
    let all: BTreeSet<u32> = catalog
        .mutants()
        .iter()
        .map(|mutant| mutant.index)
        .collect();
    let mut condemned: BTreeSet<u32> = BTreeSet::new();
    let mut diagnostics: BTreeMap<u32, (Option<String>, String)> = BTreeMap::new();
    let mut rounds: u32 = 0;
    let mut bisections: u32 = 0;

    loop {
        let attempt = compile.attempt(&condemned)?;
        rounds = rounds.saturating_add(1);
        if attempt.success {
            trace.note(
                "validate-round",
                &format!(
                    "round {rounds}: the tree compiles with {} condemned",
                    condemned.len()
                ),
            );
            break;
        }
        let attributed = attribute(&attempt.files, &attempt.messages);
        trace.note(
            "validate-round",
            &format!(
                "round {rounds}: {} attributed, {} unattributed",
                attributed.condemned.len(),
                attributed.unattributed.len()
            ),
        );
        record(&attributed, &mut diagnostics);
        let progressed = !attributed.condemned.is_subset(&condemned);
        condemned.extend(attributed.condemned.iter().copied());

        if progressed && rounds < options.max_rounds {
            continue;
        }
        // Either nothing new was attributed or the rounds ran out. Anything
        // still live is a suspect, and the pristine tree settles whether
        // the mutants are to blame at all.
        let settled = settle(compile, &all, &condemned, trace)?;
        rounds = rounds.saturating_add(settled.rounds);
        bisections = bisections.saturating_add(settled.attempts);
        for (index, entry) in settled.diagnostics {
            diagnostics.entry(index).or_insert(entry);
        }
        if settled.unsettled {
            return Err(ValidateError::NotIsolated {
                suspects: all.difference(&condemned).count(),
            });
        }
        condemned.extend(settled.condemned);
        break;
    }

    let rejections = diagnostics
        .into_iter()
        .filter_map(|(index, (code, diagnostic))| {
            let mutant = catalog.by_index(index)?;
            Some(Rejection {
                index,
                id: mutant.id.clone(),
                display_id: mutant.display_id.clone(),
                path: mutant.candidate.path.clone(),
                span: mutant.candidate.span,
                rule: mutant.candidate.rule.name.to_owned(),
                code,
                diagnostic,
            })
        })
        .collect();
    Ok(Validated {
        accepted: all.difference(&condemned).copied().collect(),
        rejections,
        rounds,
        bisections,
    })
}

/// Keeps the first thing the compiler said about each condemned mutant.
fn record(attributed: &Attributed, into: &mut BTreeMap<u32, (Option<String>, String)>) {
    for index in &attributed.condemned {
        let code = attributed.codes.get(index).cloned().flatten();
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
}

/// Isolates the offenders among everything still live, and leaves a tree
/// that compiled behind.
///
/// The pristine tree comes first: a failure that survives with nothing live
/// is not the mutants' doing, and condemning candidates until it goes away
/// would blame the innocent.
fn settle(
    compile: &mut dyn Compile,
    all: &BTreeSet<u32>,
    condemned: &BTreeSet<u32>,
    trace: &Recorder,
) -> Result<Settled, ValidateError> {
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
    let offenders = isolation.isolate(&live)?;
    let attempts = isolation.attempts;
    trace.note(
        "validate-bisect",
        &format!(
            "isolated {} offenders in {attempts} attempts",
            offenders.len()
        ),
    );
    let mut settled = Settled {
        condemned: offenders.iter().copied().collect(),
        diagnostics: BTreeMap::new(),
        rounds: 1,
        attempts,
        unsettled: false,
    };
    for index in &offenders {
        settled.diagnostics.insert(
            *index,
            (
                None,
                "the compiler refused this mutant, and no diagnostic named it".to_owned(),
            ),
        );
    }
    // One last attempt, so that what is returned is a tree that compiled.
    let mut finally = condemned.clone();
    finally.extend(offenders);
    let last = compile.attempt(&finally)?;
    settled.rounds = settled.rounds.saturating_add(1);
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

/// Narrowing a set of suspects by halving.
struct Isolation<'a> {
    compile: &'a mut dyn Compile,
    /// Everything, so that "only these are live" is spelled as a condemned
    /// set the [`Compile`] seam understands.
    base: &'a BTreeSet<u32>,
    attempts: u32,
}

impl Isolation<'_> {
    /// Whether a tree holding only `live` fails to compile.
    fn fails(&mut self, live: &[u32]) -> Result<bool, ValidateError> {
        let mut condemned = self.base.clone();
        for index in live {
            condemned.remove(index);
        }
        self.attempts = self.attempts.saturating_add(1);
        Ok(!self.compile.attempt(&condemned)?.success)
    }

    /// The smallest sets of `suspects` that still fail, found by halving.
    ///
    /// When neither half fails on its own the failure needs both, and the
    /// whole set is condemned: an interaction between two mutants is not a
    /// mutant either half can be blamed for, and keeping any of them would
    /// leave a tree that does not compile.
    fn isolate(&mut self, suspects: &[u32]) -> Result<Vec<u32>, ValidateError> {
        if suspects.is_empty() || !self.fails(suspects)? {
            return Ok(Vec::new());
        }
        if suspects.len() == 1 {
            return Ok(suspects.to_vec());
        }
        let (left, right) = suspects.split_at(suspects.len() / 2);
        let mut offenders = self.isolate(left)?;
        let from_right = self.isolate(right)?;
        offenders.extend(from_right);
        if offenders.is_empty() {
            return Ok(suspects.to_vec());
        }
        Ok(offenders)
    }
}
