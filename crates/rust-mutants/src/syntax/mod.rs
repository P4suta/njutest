// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Syntactic discovery: the candidates one file yields, each with the guard
//! site the instrumenter will use, and every place deliberately passed over,
//! each with its reason.
//!
//! Discovery is syntax-first ([ADR 0008]): `syn` parses the file, the walker
//! proposes an edit wherever an operator's token shape appears, and the
//! compiler settles later whether the edit type-checks. That is the whole
//! reason the phase needs no type information and runs without a toolchain
//! in the loop, and the reason it over-proposes: `String + &str` becomes a
//! `sub-to-add` candidate the validation phase rejects.
//!
//! # Sites and forms
//!
//! Every candidate carries a [`SiteHint`]: which guard form the instrumenter
//! composes the dormant mutant from, and over which bytes.
//!
//! - **Form C** wraps an expression in a syntactically boolean position — an
//!   `if` or `while` condition, an operand of `&&` or `||`, a match guard —
//!   as `(__rm::active(i) && (mutated) || !(__rm::active(i)) && (original))`.
//! - **Form E** wraps any other expression in value position as
//!   `(if __rm::active(i) { mutated } else { original })`; both branches
//!   unify to one type, and inference from the original settles what
//!   `Default::default()` means.
//! - **Form S** wraps a statement as `if __rm::active(i) { mutated } else
//!   { original }`, with the original bytes verbatim so lines are kept.
//!
//! The site is the candidate's own expression unless the edit changes that
//! expression's type: a range swap turns a `Range` into a `RangeInclusive`,
//! so its site is the enclosing statement, or the initializer of a `let`,
//! where the types meet again.
//!
//! # Skips, stated
//!
//! Nothing is dropped silently. A region the walker will not mutate — a
//! constant context, code behind a `cfg`, test code — is still walked, and
//! every candidate it would have produced is counted under the outermost
//! reason. A macro invocation counts once, because its body is tokens the
//! walker does not parse; a `macro_rules!` definition is not a place code
//! runs and is not counted. [`FileDiscovery::trace_record`] carries every
//! decision for the trace, and `rust-mutants why-skipped` tallies them.
//!
//! # Determinism
//!
//! Two discoveries over the same bytes produce identical results. Candidates
//! are ordered by (edit start, rule registry position) and skips by (reason
//! rank, path), compared byte-wise.
//!
//! [ADR 0008]: https://github.com/P4suta/mjutest/blob/main/docs/adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md

mod position;
mod rules;
mod walk;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::catalog::Candidate;
use crate::rule::{Registry, Rule, RuleError, Tier};
use crate::span::Span;
use crate::trace::{DiscoverFileRecord, SiteRecord, SkipCount};

pub use position::{LineIndex, Position};

/// One of the three guard shapes the instrumenter composes a dormant mutant
/// from; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Form {
    /// The boolean selector, for a syntactically boolean position.
    C,
    /// The expression selector, for any value position.
    E,
    /// The statement guard.
    S,
}

impl Form {
    /// The letter.
    #[must_use]
    pub const fn letter(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::E => "E",
            Self::S => "S",
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
    /// The bytes the guard replaces: an expression for C and E, a statement
    /// for S. The candidate's edit lies inside it.
    pub site: Span,
    /// The bytes of `site`, verbatim.
    pub site_text: String,
    /// How many `super::` segments separate the site's inline module from
    /// the file root, where the runtime module lives.
    pub super_depth: u32,
    /// The byte offset of the innermost enclosing `fn` item, where
    /// `#[allow(warnings)]` goes so a guard's own lint noise never trips a
    /// crate's deny policy. `None` outside any function.
    pub allow_at: Option<u32>,
}

/// One candidate plus where a human would look for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The edit, the whole truth for identity and instrumentation.
    pub candidate: Candidate,
    /// Where the edit starts.
    pub position: Position,
    /// The rewrite site.
    pub hint: SiteHint,
}

/// Why a place produced no candidate. Declared in rank order, which is the
/// order skips are reported in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// A constant context: a `const` or `static` initializer, a `const fn`
    /// body, a `const` block, an array length, an enum discriminant.
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
    /// A file of a `proc-macro` crate.
    ProcMacroCrate,
    /// A file of a `#![no_std]` crate.
    NoStdCrate,
}

impl SkipReason {
    /// Every reason, in rank order.
    pub const ALL: [Self; 9] = [
        Self::ConstContext,
        Self::MacroInvocation,
        Self::CfgAttribute,
        Self::TestCode,
        Self::UnsupportedSite,
        Self::Excluded,
        Self::TestOnlyFile,
        Self::ProcMacroCrate,
        Self::NoStdCrate,
    ];

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
            Self::ProcMacroCrate => "proc-macro-crate",
            Self::NoStdCrate => "no-std-crate",
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
            Self::ProcMacroCrate => {
                "the crate is a proc-macro crate, which runs inside the compiler rather than inside a test"
            }
            Self::NoStdCrate => {
                "the crate is #![no_std], and the v1 runtime needs std for the environment and the process exit"
            }
        }
    }

    /// The reason named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.name() == name)
    }
}

/// How many candidates one reason suppressed in one file. Ordered by
/// (reason rank, path).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
}

impl FileDiscovery {
    /// The trace record of this discovery.
    #[must_use]
    pub fn trace_record(&self) -> DiscoverFileRecord {
        DiscoverFileRecord {
            path: self.path.clone(),
            candidates: u32::try_from(self.candidates.len()).unwrap_or(u32::MAX),
            sites: self
                .decisions
                .iter()
                .map(|decision| SiteRecord {
                    line: decision.position.line,
                    column: decision.position.byte_column,
                    rule: decision.rule.clone(),
                    form: decision.form.map(|form| form.letter().to_owned()),
                    skip: decision.skip.map(|reason| reason.name().to_owned()),
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
        }
    }
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
    ///
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
    /// The file does not parse as Rust.
    #[error("{path}:{line}:{column}: {message}")]
    Parse {
        /// The path.
        path: String,
        /// The 1-based line of the first error.
        line: u32,
        /// The 1-based character column.
        column: u32,
        /// The parser's message.
        message: String,
    },
}

/// Finds every candidate in one file.
///
/// `path` is the workspace-relative path the candidates carry; `source` is
/// the file's bytes, digested as they are. The rules come from `selection`.
///
/// # Errors
///
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
    let (base, parsed) = strip_prefix(text);
    let file: syn::File = syn::parse_str(parsed).map_err(|error| {
        let start = error.span().start();
        SyntaxError::Parse {
            path: path.to_owned(),
            line: u32::try_from(start.line).unwrap_or(u32::MAX),
            column: u32::try_from(start.column.saturating_add(1)).unwrap_or(u32::MAX),
            message: error.to_string(),
        }
    })?;
    let index = LineIndex::new(text);
    let input = walk::Input {
        text,
        base,
        path,
        digest: &source_digest,
    };
    let mut walker = walk::Walker::new(input, selection, &index);
    let no_std = walker.walk_file(&file);
    let (mut candidates, skips, mut decisions) = walker.finish();

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
        source_digest,
        candidates,
        skips,
        decisions,
        no_std,
    })
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

/// Strips what `syn::parse_file` would strip — a byte order mark and a
/// shebang line — and returns the byte offset the remainder starts at, so
/// every span can be made absolute. The shebang's newline is kept, which
/// keeps the parser's line numbers equal to the file's.
fn strip_prefix(text: &str) -> (u32, &str) {
    const BOM: &str = "\u{feff}";
    let mut rest = text.strip_prefix(BOM).unwrap_or(text);
    if rest.starts_with("#!") && !rest.trim_start_matches("#!").trim_start().starts_with('[') {
        rest = rest
            .find('\n')
            .map_or("", |newline| rest.get(newline..).unwrap_or_default());
    }
    let base = text.len().saturating_sub(rest.len());
    (u32::try_from(base).unwrap_or(u32::MAX), rest)
}
