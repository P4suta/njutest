// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Syntactic discovery: the candidates one file yields, each with the guard site the instrumenter will use, and every place deliberately passed over, each with its reason.

mod annotate;
pub mod branch;
mod position;
mod rules;
mod shape;
mod walk;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::catalog::Candidate;
use crate::rule::{Registry, Rule, RuleError, Tier};
use crate::span::Span;
use crate::trace::{DiscoverFileRecord, SiteRecord, SkipCount};

pub use position::{LineIndex, Position, PositionError};
pub use rules::respell_int;

/// The name a reader writes for an `impl` block, which is the segment its items are named under.
pub(crate) fn implemented(block: &syn::ItemImpl) -> String {
    shape::implemented(block)
}

/// One of the four guard shapes the instrumenter composes a dormant mutant from; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Form {
    /// The boolean selector, for a syntactically boolean position.
    C,
    /// The expression selector, for any value position.
    E,
    /// The statement guard.
    S,
    /// The guard written onto a match arm that had none, which is the one shape that adds syntax rather than replacing it.
    M,
}

impl Form {
    /// The letter.
    #[must_use]
    pub const fn letter(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::E => "E",
            Self::S => "S",
            Self::M => "M",
        }
    }
}

impl fmt::Display for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.letter())
    }
}

/// The rewrite site the instrumenter has to use for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteHint {
    /// Which guard form.
    pub form: Form,
    /// The bytes the guard replaces: an expression for C and E, a statement for S. The candidate's edit lies inside it.
    pub site: Span,
    /// The bytes of `site`, verbatim.
    pub site_text: String,
    /// How many `super::` segments the site's call into the runtime module needs, which is none where every inline module around it glob-imports its parent.
    pub super_depth: u32,
}

/// Which inline modules between a site and its file root glob-import their parent, which decides how the site names the runtime module that lives at the root.
#[derive(Debug, Clone)]
pub(crate) struct ModuleScope {
    globs: Vec<bool>,
}

impl ModuleScope {
    /// The file root, inside no inline module.
    pub(crate) const fn root() -> Self {
        Self { globs: Vec::new() }
    }

    /// Walks into an inline module holding `items`.
    pub(crate) fn enter(&mut self, items: &[syn::Item]) {
        self.globs.push(globs_parent(items));
    }

    /// Walks back out of the innermost inline module, and says whether there was one.
    pub(crate) fn leave(&mut self) -> bool {
        self.globs.pop().is_some()
    }

    /// How many `super::` segments reach the runtime module: none where every module out to the file root glob-imports its parent, because the name is then already in scope and any qualification of it is one `unused_qualifications` refuses.
    #[must_use]
    pub(crate) fn supers(&self) -> Option<usize> {
        let reached = self.globs.iter().take_while(|globbed| **globbed).count();
        self.globs.len().checked_sub(reached)
    }
}

/// Whether `items` hold an unconditional `use super::*;`, the one import that brings everything the parent sees into scope.
fn globs_parent(items: &[syn::Item]) -> bool {
    items.iter().any(|item| match item {
        syn::Item::Use(used) => {
            used.attrs
                .iter()
                .all(|attribute| !attribute.path().is_ident("cfg"))
                && matches!(
                    &used.tree,
                    syn::UseTree::Path(path)
                        if path.ident == "super" && matches!(*path.tree, syn::UseTree::Glob(_))
                )
        }
        _ => false,
    })
}

/// One candidate plus where a human would look for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The edit, the whole truth for identity and instrumentation.
    pub candidate: Candidate,
    /// Where the edit starts.
    pub position: Position,
    /// The item the edit sits in, as a reader writes it: `mod::path::Type::method`, or `<Type as Trait>::method`.
    /// Empty at the top level of a file.
    pub item: String,
    /// The rewrite site.
    pub hint: SiteHint,
    /// What a branch proof about this edit would rest on, once the compiler has vouched for its witnesses.
    /// `None` where the syntax supports no proof.
    pub branch: Option<branch::Claim>,
    /// What the compiler must vouch for before this guard's two branches may be compared, so a run can record whether they ever differed.
    /// `None` where the syntax does not allow comparing them.
    pub comparable: Option<branch::Comparable>,
    /// What a probe of this edit would ask, when evaluating the expression a second time is not itself an event.
    /// `None` where no probe can be stated.
    pub probe: Option<crate::probe::Question>,
}

/// Why a place produced no candidate.
/// Declared in rank order, which is the order skips are reported in.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// A constant context: a `const` or `static` initializer, a `const fn` body, a `const` block, an array length, an enum discriminant.
    ConstContext,
    /// A macro invocation, whose body is tokens the walker does not parse.
    MacroInvocation,
    /// An item, statement, arm, or expression behind a `#[cfg(...)]`.
    CfgAttribute,
    /// A `#[test]` function or anything behind `#[cfg(test)]`.
    TestCode,
    /// A candidate none of the guard forms can express at that position.
    UnsupportedSite,
    /// A file the include and exclude patterns removed.
    Excluded,
    /// A file only a test unit compiles.
    TestOnlyFile,
    /// A file of a `#![no_std]` crate.
    NoStdCrate,
    /// A file another file pastes in at expression position, which is a fragment rather than a program.
    IncludedExpression,
    /// A file a build script wrote outside the tree, which the tree does not hold and a run cannot rewrite.
    GeneratedOutsideWorkspace,
    /// A file of a crate that forbids a lint the guards' own attribute turns off, which no guard could compile in.
    ForbiddenLints,
    /// The body of a `const fn`, whose every call the compiler may evaluate, where a runtime guard cannot live.
    ConstFnBody,
    /// A condition that binds with `let`, whose parts a guard cannot rearrange without moving the binding out of scope.
    LetCondition,
    /// A range with no end, which has no other form to become.
    OpenRange,
    /// A return type the syntax cannot say has a default: an `impl Trait`, a reference, a pointer, a function, a type a macro writes, or a generic parameter nothing bound to `Default`.
    UnstatedReturnType,
    /// A jump whose loop decides its value by what it breaks with, so the other jump has no value to carry.
    LoopValue,
    /// A marker in the source says to pass this place over, and why.
    Annotated,
    /// A `[[mutation.skip]]` entry in the configuration says to pass this place over, and why.
    Configured,
}

impl SkipReason {
    /// The kebab-case name used in reports and on the command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ConstContext => "const-context",
            Self::MacroInvocation => "macro-invocation",
            Self::CfgAttribute => "cfg-attribute",
            Self::TestCode => "test-code",
            Self::UnsupportedSite => "unsupported-site",
            Self::Excluded => "excluded",
            Self::TestOnlyFile => "test-only-file",
            Self::NoStdCrate => "no-std-crate",
            Self::IncludedExpression => "included-expression",
            Self::GeneratedOutsideWorkspace => "generated-outside-workspace",
            Self::ForbiddenLints => "forbidden-lints",
            Self::ConstFnBody => "const-fn-body",
            Self::LetCondition => "let-condition",
            Self::OpenRange => "open-range",
            Self::UnstatedReturnType => "unstated-return-type",
            Self::LoopValue => "loop-value",
            Self::Annotated => "annotated",
            Self::Configured => "configured",
        }
    }

    /// One sentence for a person asking why there is no mutant here.
    #[must_use]
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::ConstContext => {
                "the expression is evaluated by the compiler (a const or static initializer, a const fn, a const block, an array length, a discriminant), where a runtime guard cannot live"
            }
            Self::MacroInvocation => {
                "the code is inside a macro invocation, whose body is tokens the walker does not parse; each invocation counts once"
            }
            Self::CfgAttribute => {
                "the code is behind a #[cfg(...)] the walker does not evaluate; a mutant in a branch cfg removes would never compile and never die"
            }
            Self::TestCode => {
                "the code is a test: a #[test] function or anything behind #[cfg(test)], which measures itself"
            }
            Self::UnsupportedSite => {
                "none of the guard forms can express the edit at this position"
            }
            Self::Excluded => "the include and exclude patterns removed the file",
            Self::TestOnlyFile => {
                "only a test unit compiles the file, so no non-test binary could carry the mutant"
            }
            Self::NoStdCrate => {
                "the crate is #![no_std], and the v1 runtime needs std for the environment and the process exit"
            }
            Self::IncludedExpression => {
                "another file pastes this one in where an expression goes, so it is a fragment rather than a program: it cannot carry a runtime module and there is nothing to parse it as"
            }
            Self::GeneratedOutsideWorkspace => {
                "a build script wrote this file into the build directory rather than into the tree, so it is not a file a reviewer edits and the next build would write over any change to it"
            }
            Self::ForbiddenLints => {
                "the crate forbids a lint the guards' own attribute turns off, and forbid is the one level an allow cannot override, so no guard could compile here whatever it edited"
            }
            Self::ConstFnBody => {
                "the expression is in the body of a const fn, which the compiler may evaluate at any call, where a runtime guard cannot live"
            }
            Self::LetCondition => {
                "the condition binds with let, and what a guard would have to rearrange is what the binding is in scope for"
            }
            Self::OpenRange => "the range has no end, so there is no other form of it to write",
            Self::UnstatedReturnType => {
                "the return type is one the syntax cannot say has a default: an impl Trait, a reference, a pointer, a function, a type a macro writes, or a generic parameter nothing bound to Default"
            }
            Self::LoopValue => {
                "a loop decides its value by what its breaks carry, and the other jump carries none: a break the syntax writes into one has no value to give it, and a continue carries nothing away"
            }
            Self::Annotated => {
                "a rust-mutants: skip marker in the source says to pass this place over, and the reason its author wrote is reported beside it"
            }
            Self::Configured => {
                "a [[mutation.skip]] entry in the configuration says to pass this place over, and the reason its author wrote is reported beside it"
            }
        }
    }

    /// The reason named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.name() == name)
    }
}

/// How many candidates one reason suppressed in one file.
/// Ordered by (reason rank, path).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skip {
    /// The reason.
    pub reason: SkipReason,
    /// The workspace-relative path.
    pub path: String,
    /// How many candidates, or invocations for a macro.
    pub count: u32,
}

/// One decision the walker took, for the trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The byte offset of the edit.
    pub offset: u32,
    /// Its position.
    pub position: Position,
    /// The rule, or the reason's name for a site that is not a rule's.
    pub rule: String,
    /// The guard form of a candidate.
    pub form: Option<Form>,
    /// The reason of a skip.
    pub skip: Option<SkipReason>,
    /// What the walker has to say about this decision beyond its reason.
    pub note: Option<String>,
}

/// One `rust-mutants: skip` marker, and whether it hid anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// The 1-based line the marker sits on.
    pub line: u32,
    /// The reason its author wrote.
    pub reason: String,
    /// Whether a place a rule targets starts inside what it speaks about.
    /// A marker that hid nothing is one somebody should take out.
    pub matched: bool,
}

/// Everything discovery found in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiscovery {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// The lowercase hex SHA-256 of the file's bytes.
    pub source_digest: String,
    /// The candidates, in (edit start, rule position) order.
    pub candidates: Vec<Found>,
    /// The skip tallies, in reason order.
    pub skips: Vec<Skip>,
    /// Every decision, in source order.
    pub decisions: Vec<Decision>,
    /// Whether the file carries `#![no_std]`.
    pub no_std: bool,
    /// Every file this one pastes in with `include!`, in source order.
    pub includes: Vec<Include>,
    /// Every `rust-mutants: skip` marker the file carries, in source order.
    pub annotations: Vec<Claim>,
}

/// One file another file pastes in with `include!`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Include {
    /// The workspace-relative path of the included file, with forward slashes.
    pub path: String,
    /// Whether the paste happens where items go rather than where an expression goes.
    pub at_item: bool,
}

impl FileDiscovery {
    /// The trace record of this discovery.
    ///
    /// # Errors
    /// Returns the exact candidate count when it does not fit the trace's u32 counter; no saturated trace record is produced.
    pub fn trace_record(&self) -> Result<DiscoverFileRecord, TraceRecordError> {
        let candidates =
            u32::try_from(self.candidates.len()).map_err(|_overflow| TraceRecordError {
                path: self.path.clone(),
                count: self.candidates.len(),
            })?;
        Ok(DiscoverFileRecord {
            path: self.path.clone(),
            candidates,
            sites: self
                .decisions
                .iter()
                .map(|decision| SiteRecord {
                    line: decision.position.line,
                    column: decision.position.byte_column,
                    rule: decision.rule.clone(),
                    form: decision.form.map(|form| form.letter().to_owned()),
                    skip: decision.skip.map(|reason| reason.name().to_owned()),
                    note: decision.note.clone(),
                })
                .collect(),
            skips: self
                .skips
                .iter()
                .map(|skip| SkipCount {
                    reason: skip.reason.name().to_owned(),
                    count: skip.count,
                })
                .collect(),
        })
    }
}

/// A discovery whose exact candidate count does not fit the trace schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path} yielded {count} candidates, beyond the trace counter")]
pub struct TraceRecordError {
    /// The source file.
    pub path: String,
    /// The exact in-memory count that was refused.
    pub count: usize,
}

/// The rules discovery applies, drawn from a registry.
#[derive(Debug, Clone)]
pub struct Selection<'r> {
    registry: &'r Registry,
    rules: Vec<Rule>,
}

impl<'r> Selection<'r> {
    /// Every rule of `tier` and below.
    #[must_use]
    pub fn tier(registry: &'r Registry, tier: Tier) -> Self {
        Self {
            registry,
            rules: registry.select_tier(tier),
        }
    }

    /// Exactly the named rules.
    ///
    /// # Errors
    /// [`RuleError::UnknownRule`] for a name the registry does not know.
    pub fn rules(registry: &'r Registry, names: &[&str]) -> Result<Self, RuleError> {
        let rules = names
            .iter()
            .map(|name| {
                registry.lookup(name).ok_or_else(|| RuleError::UnknownRule {
                    name: (*name).to_owned(),
                })
            })
            .collect::<Result<Vec<Rule>, RuleError>>()?;
        Ok(Self { registry, rules })
    }

    /// The registry the rules come from.
    #[must_use]
    pub const fn registry(&self) -> &'r Registry {
        self.registry
    }

    /// The selected rules.
    #[must_use]
    pub fn selected(&self) -> &[Rule] {
        &self.rules
    }

    fn rule(&self, name: &str) -> Option<Rule> {
        self.rules.iter().copied().find(|rule| rule.name == name)
    }
}

/// Why a file could not be discovered.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyntaxError {
    /// The file is not UTF-8, which Rust source must be.
    #[error("{path}: source is not valid UTF-8")]
    NotUtf8 {
        /// The path.
        path: String,
    },
    /// The file is larger than spans can address.
    #[error("{path}: source is larger than 4 GiB")]
    TooLarge {
        /// The path.
        path: String,
    },
    /// A marker names no reason.
    #[error("{path}:{line}: a rust-mutants: skip marker names no reason")]
    AnnotationWithoutReason {
        /// The path.
        path: String,
        /// The 1-based line.
        line: u32,
    },
    /// A marker names a directive this release does not know.
    #[error("{path}:{line}: rust-mutants: {directive} is not a directive this release knows")]
    UnknownAnnotation {
        /// The path.
        path: String,
        /// The 1-based line.
        line: u32,
        /// The directive as written.
        directive: String,
    },
    /// The file does not parse as Rust.
    #[error("{path}:{line}:{column}: {message}")]
    Parse {
        /// The path.
        path: String,
        /// The 1-based line of the first error.
        line: usize,
        /// The 1-based character column.
        column: usize,
        /// The parser's message.
        message: String,
    },
}

impl SyntaxError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::NotUtf8 { .. } | Self::TooLarge { .. } | Self::Parse { .. } => {
                crate::error::DISCOVER_PARSE_FAILED
            }
            Self::AnnotationWithoutReason { .. } => {
                crate::error::DISCOVER_ANNOTATION_WITHOUT_REASON
            }
            Self::UnknownAnnotation { .. } => crate::error::DISCOVER_UNKNOWN_ANNOTATION,
        }
    }
}

/// Finds every candidate in one file.
///
/// # Errors
/// See [`SyntaxError`].
pub fn discover_file(
    path: &str,
    source: &[u8],
    selection: &Selection<'_>,
) -> Result<FileDiscovery, SyntaxError> {
    let text = std::str::from_utf8(source).map_err(|_invalid| SyntaxError::NotUtf8 {
        path: path.to_owned(),
    })?;
    if u32::try_from(text.len()).is_err() {
        return Err(SyntaxError::TooLarge {
            path: path.to_owned(),
        });
    }
    let source_digest = hex::encode(Sha256::digest(source));
    let (base, parsed) = strip_prefix(text).map_err(|_prefix| SyntaxError::TooLarge {
        path: path.to_owned(),
    })?;
    let file: syn::File = syn::parse_str(parsed).map_err(|error| parse_error(path, &error))?;
    let index = LineIndex::new(text).map_err(|_position| SyntaxError::TooLarge {
        path: path.to_owned(),
    })?;
    let stream: proc_macro2::TokenStream =
        syn::parse_str(parsed).map_err(|error| parse_error(path, &error))?;
    let markers = annotate::markers(text, base, &stream, &index).map_err(|error| match error {
        annotate::MarkerError::SourceBounds => SyntaxError::TooLarge {
            path: path.to_owned(),
        },
        annotate::MarkerError::WithoutReason { line } => SyntaxError::AnnotationWithoutReason {
            path: path.to_owned(),
            line,
        },
        annotate::MarkerError::Unknown { line, directive } => SyntaxError::UnknownAnnotation {
            path: path.to_owned(),
            line,
            directive,
        },
    })?;
    let input = walk::Input {
        text,
        base,
        path,
        digest: &source_digest,
    };
    let mut walker = walk::Walker::new(input, selection, index);
    walker.annotate(markers);
    let no_std = walker.walk_file(&file);
    let walk::Walked {
        found: mut candidates,
        skips,
        mut decisions,
        includes,
        annotations,
    } = walker.finish().map_err(|_bounds| SyntaxError::TooLarge {
        path: path.to_owned(),
    })?;

    let position = |name: &str| selection.registry().position(name).unwrap_or(usize::MAX);
    candidates.sort_by_key(|found| {
        (
            found.candidate.span.start,
            position(found.candidate.rule.name),
        )
    });
    decisions.sort_by_key(|decision| (decision.offset, position(&decision.rule)));
    let skips = tally(path, skips);
    Ok(FileDiscovery {
        path: path.to_owned(),
        includes,
        source_digest,
        candidates,
        skips,
        decisions,
        no_std,
        annotations,
    })
}

fn parse_error(path: &str, error: &syn::Error) -> SyntaxError {
    let start = error.span().start();
    let Some(column) = start.column.checked_add(1) else {
        return SyntaxError::TooLarge {
            path: path.to_owned(),
        };
    };
    SyntaxError::Parse {
        path: path.to_owned(),
        line: start.line,
        column,
        message: error.to_string(),
    }
}

/// The skip tallies of one file, in reason order.
fn tally(path: &str, counts: BTreeMap<SkipReason, u32>) -> Vec<Skip> {
    counts
        .into_iter()
        .map(|(reason, count)| Skip {
            reason,
            path: path.to_owned(),
            count,
        })
        .collect()
}

/// Strips what `syn::parse_file` would strip — a byte order mark and a shebang line — and returns the byte offset the remainder starts at, so every span can be made absolute.
/// The shebang's newline is kept, which keeps the parser's line numbers equal to the file's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a {bytes}-byte Rust source prefix does not fit its u32 offset")]
pub(crate) struct PrefixError {
    bytes: usize,
}

/// Removes the parser prefixes while retaining their exact byte offset.
///
/// # Errors
/// Returns an exact-size error if the prefix does not fit the span schema.
pub(crate) fn strip_prefix(text: &str) -> Result<(u32, &str), PrefixError> {
    const BOM: &str = "\u{feff}";
    let mut rest = match text.strip_prefix(BOM) {
        Some(rest) => rest,
        None => text,
    };
    if rest.starts_with("#!") && !rest.trim_start_matches("#!").trim_start().starts_with('[') {
        rest = match rest.find('\n') {
            Some(newline) => rest
                .get(newline..)
                .ok_or(PrefixError { bytes: text.len() })?,
            None => "",
        };
    }
    let base = text
        .len()
        .checked_sub(rest.len())
        .ok_or(PrefixError { bytes: text.len() })?;
    let base = u32::try_from(base).map_err(|_overflow| PrefixError { bytes: base })?;
    Ok((base, rest))
}

/// The workspace-relative path of `literal` read from beside `including`, with forward slashes, when it stays inside the tree.
pub(super) fn beside(including: &str, literal: &str) -> Option<String> {
    let directory = std::path::Path::new(including).parent()?;
    let mut segments: Vec<&str> = directory
        .to_str()?
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in literal.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}
