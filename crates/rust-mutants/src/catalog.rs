// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The catalog: candidates identified, deduplicated, ordered canonically,
//! and assigned the dense indices the generated runtime is built from.
//!
//! Catalog order is canonical rather than chronological: it is a pure
//! function of the candidate set, so two discovery passes over the same
//! workspace produce the same order, the same dense indices, and therefore
//! the same generated runtime array.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest as _, Sha256};

use crate::id::{
    DISPLAY_ID_LENGTH, ID_HEX_LENGTH, Identity, IdentityError, MIN_PREFIX_LENGTH, digest,
    is_lower_hex, write_length_prefixed,
};
use crate::rule::{Registry, Rule, RuleError};
use crate::span::Span;

/// The domain separator of the catalog digest.
pub const CATALOG_DOMAIN: &str = "rust-mutants-catalog-v1";

/// One proposed edit: replace the bytes of `span` in `path` with
/// `replacement`. The unit discovery produces and the catalog consumes.
///
/// `original` and `replacement` are bytes, not text: they are spliced
/// verbatim, so the original keeps whatever whitespace, comments, and line
/// endings the file had.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Candidate {
    /// The workspace-relative source path with forward slashes.
    pub path: String,
    /// The operator that proposed the edit.
    pub rule: Rule,
    /// The byte range being replaced.
    pub span: Span,
    /// Exactly the bytes `span` covers in the source file.
    pub original: Vec<u8>,
    /// What those bytes become. Empty for a deletion, and never equal to
    /// `original`: replacing bytes with themselves is not a mutation.
    pub replacement: Vec<u8>,
    /// The lowercase hex SHA-256 of the whole source file.
    pub source_digest: String,
}

/// A candidate the catalog refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CandidateError {
    /// The candidate's identity is not canonical.
    #[error(transparent)]
    Identity(#[from] IdentityError),
    /// The candidate's rule is not the registered one.
    #[error(transparent)]
    Rule(#[from] RuleError),
    /// The original text is not the length of the span: the span and the
    /// text were not taken from the same file at the same moment.
    #[error("{path} {span} covers {span_len} bytes, original text is {original_len} bytes")]
    OriginalLengthMismatch {
        /// The file.
        path: String,
        /// The span.
        span: Span,
        /// The span's length.
        span_len: u32,
        /// The original text's length.
        original_len: usize,
    },
    /// The replacement is byte-identical to the original.
    #[error("{path} {span} replaces its bytes with themselves")]
    NoOpReplacement {
        /// The file.
        path: String,
        /// The span.
        span: Span,
    },
    /// Two candidates claim different digests for the same file.
    #[error("{path} has conflicting source digests {first} and {second}")]
    SourceDigestConflict {
        /// The file.
        path: String,
        /// The digest first claimed.
        first: String,
        /// The digest claimed next.
        second: String,
    },
    /// Two candidates claim different original text for one span.
    #[error("{path} {span} has conflicting original text")]
    OriginalConflict {
        /// The file.
        path: String,
        /// The span.
        span: Span,
    },
}

impl Candidate {
    /// Whether the candidate is internally coherent.
    ///
    /// # Errors
    ///
    /// Returns the first incoherence: an invalid identity, an original text
    /// that is not the span's length, or a replacement identical to it.
    pub fn validate(&self) -> Result<(), CandidateError> {
        self.identity().validate()?;
        // The length check proves the span and the original text were taken
        // from the same file at the same moment: an off-by-one in a discovery
        // rule would otherwise mint a valid-looking ID for an edit that
        // splices garbage.
        if u64::from(self.span.len()) != u64::try_from(self.original.len()).unwrap_or(u64::MAX) {
            return Err(CandidateError::OriginalLengthMismatch {
                path: self.path.clone(),
                span: self.span,
                span_len: self.span.len(),
                original_len: self.original.len(),
            });
        }
        // The no-op check proves the edit is an edit at all: a replacement
        // identical to the original compiles by construction and survives
        // every test, inflating the denominator for a mutation that does not
        // exist. An empty span is still a legal insertion point, so what this
        // rejects is exactly the pair that changes nothing, deletions of
        // nothing included.
        if self.replacement == self.original {
            return Err(CandidateError::NoOpReplacement {
                path: self.path.clone(),
                span: self.span,
            });
        }
        Ok(())
    }

    /// The identity this candidate hashes to.
    #[must_use]
    pub fn identity(&self) -> Identity {
        Identity {
            path: self.path.clone(),
            rule_name: self.rule.name.to_owned(),
            rule_version: self.rule.version,
            span: self.span,
            source_digest: self.source_digest.clone(),
            original_digest: digest(&self.original),
            replacement_digest: digest(&self.replacement),
        }
    }

    /// The candidate's stable mutant ID.
    ///
    /// # Errors
    ///
    /// Returns the validation failure; an incoherent candidate never mints an ID.
    pub fn id(&self) -> Result<String, CandidateError> {
        self.validate()?;
        Ok(self.identity().id()?)
    }
}

/// A cataloged candidate: identified, deduplicated, and assigned its dense
/// runtime index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    /// The position in the generated runtime's activation array: the
    /// catalog's own order, densely assigned from zero.
    pub index: u32,
    /// The full 64 hex character stable identity.
    pub id: String,
    /// The short form, proven unique within this catalog.
    pub display_id: String,
    /// The edit itself.
    pub candidate: Candidate,
}

/// Why a candidate lost deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DuplicateReason {
    /// The same rule proposed the same edit twice; both carry one mutant ID.
    Identical,
    /// A different rule proposed the same byte edit at the same span, and the
    /// more local rule won.
    Shadowed,
}

impl fmt::Display for DuplicateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Identical => "identical-candidate",
            Self::Shadowed => "shadowed-by-more-local-rule",
        })
    }
}

/// A candidate the catalog dropped. Kept rather than discarded so `explain`
/// can answer "why is there no mutant for this rule here?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    /// Why the candidate lost.
    pub reason: DuplicateReason,
    /// The losing candidate.
    pub dropped: Candidate,
    /// The losing candidate's mutant ID. Equal to `winner_id` for an
    /// identical duplicate.
    pub dropped_id: String,
    /// The ID of the mutant that was kept.
    pub winner_id: String,
    /// The rule that won.
    pub winner_rule: Rule,
}

/// One short ID shared by several mutants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayCollision {
    /// The colliding short form.
    pub display_id: String,
    /// The colliding full IDs, sorted.
    pub ids: Vec<String>,
}

/// Truncating full IDs to the display length would produce an ambiguous
/// short form.
///
/// Returned, never panicked: a collision among 20 hex characters is
/// astronomically unlikely but not impossible, and the honest response is a
/// diagnosable error rather than a silently ambiguous selector.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct DisplayCollisionError {
    /// The display length that collided.
    pub length: usize,
    /// The colliding short forms, sorted by short form.
    pub collisions: Vec<DisplayCollision>,
}

impl fmt::Display for DisplayCollisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} display id collision(s) at {} hex characters",
            self.collisions.len(),
            self.length
        )?;
        for collision in &self.collisions {
            write!(
                f,
                "; {} shared by {}",
                collision.display_id,
                collision.ids.join(", ")
            )?;
        }
        Ok(())
    }
}

/// A catalog that could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// A candidate the catalog refuses.
    #[error(transparent)]
    Candidate(#[from] CandidateError),
    /// Short IDs would be ambiguous.
    #[error(transparent)]
    DisplayCollision(#[from] DisplayCollisionError),
    /// More candidates than the `u32` runtime index can address.
    #[error("{count} candidates exceed the u32 index space")]
    TooLarge {
        /// The candidate count.
        count: usize,
    },
}

/// A user-supplied ID prefix that resolves to no single mutant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PrefixError {
    /// Too short, too long, or not lowercase hex.
    #[error("{prefix:?} must be {min} to {max} lowercase hex characters")]
    Invalid {
        /// The prefix.
        prefix: String,
        /// The shortest accepted length.
        min: usize,
        /// The longest accepted length.
        max: usize,
    },
    /// No mutant matches.
    #[error("no mutant matches {prefix:?}")]
    NotFound {
        /// The prefix.
        prefix: String,
    },
    /// More than one mutant matches.
    #[error("{prefix:?} matches {} mutants: {}", matches.len(), matches.join(", "))]
    Ambiguous {
        /// The prefix.
        prefix: String,
        /// The display IDs of every match, in catalog order.
        matches: Vec<String>,
    },
}

/// Accumulates candidates and produces a catalog.
///
/// `build` is a pipeline, in this order: validate, identify, deduplicate,
/// sort canonically, assign dense indices, then check display IDs.
#[derive(Debug, Clone)]
pub struct Builder {
    registry: Registry,
    candidates: Vec<Candidate>,
    display_len: usize,
    /// One source digest per path, so a contradiction is caught where it is
    /// introduced instead of surfacing as an unexplainable ID.
    digests: BTreeMap<String, String>,
    /// One original text per (path, span), for the same reason.
    originals: BTreeMap<(String, Span), Vec<u8>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// A builder backed by the canonical registry.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_registry(Registry::canonical())
    }

    /// A builder backed by `registry`.
    #[must_use]
    pub const fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            candidates: Vec::new(),
            display_len: DISPLAY_ID_LENGTH,
            digests: BTreeMap::new(),
            originals: BTreeMap::new(),
        }
    }

    /// Overrides the display ID length. It exists so tests can force the
    /// collision path real SHA-256 output will not produce; no production
    /// caller changes it. An out-of-range value falls back to the default.
    #[must_use]
    pub const fn with_display_length(mut self, length: usize) -> Self {
        self.display_len = length;
        self
    }

    /// The number of candidates added so far, before deduplication.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Whether no candidate was added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Validates a candidate and queues it. Insertion order does not affect
    /// the resulting catalog.
    ///
    /// # Errors
    ///
    /// Returns the refusal: an incoherent candidate, an unregistered rule, or
    /// a contradiction with a candidate already queued.
    pub fn add(&mut self, candidate: Candidate) -> Result<(), CandidateError> {
        candidate.validate()?;
        self.registry.verify(candidate.rule)?;
        if let Some(previous) = self.digests.get(&candidate.path)
            && *previous != candidate.source_digest
        {
            return Err(CandidateError::SourceDigestConflict {
                path: candidate.path.clone(),
                first: previous.clone(),
                second: candidate.source_digest.clone(),
            });
        }
        self.digests
            .insert(candidate.path.clone(), candidate.source_digest.clone());
        let key = (candidate.path.clone(), candidate.span);
        if let Some(previous) = self.originals.get(&key)
            && *previous != candidate.original
        {
            return Err(CandidateError::OriginalConflict {
                path: candidate.path.clone(),
                span: candidate.span,
            });
        }
        self.originals.insert(key, candidate.original.clone());
        self.candidates.push(candidate);
        Ok(())
    }

    /// Adds candidates in order, stopping at the first refused one.
    ///
    /// # Errors
    ///
    /// See [`Builder::add`].
    pub fn add_all<I>(&mut self, candidates: I) -> Result<(), CandidateError>
    where
        I: IntoIterator<Item = Candidate>,
    {
        for candidate in candidates {
            self.add(candidate)?;
        }
        Ok(())
    }

    /// Produces the catalog.
    ///
    /// # Errors
    ///
    /// Returns the first candidate that cannot be identified, a display ID
    /// collision, or a candidate count the runtime index cannot address.
    pub fn build(self) -> Result<Catalog, BuildError> {
        let count = self.candidates.len();
        if u32::try_from(count).is_err() {
            return Err(BuildError::TooLarge { count });
        }
        let mut entries = Vec::with_capacity(count);
        for candidate in self.candidates {
            let id = candidate.id()?;
            let position = self
                .registry
                .position(candidate.rule.name)
                .ok_or_else(|| RuleError::UnknownRule {
                    name: candidate.rule.name.to_owned(),
                })
                .map_err(CandidateError::from)?;
            entries.push(Entry {
                candidate,
                id,
                position,
            });
        }
        // Canonical order first, so that everything downstream — deduplication,
        // dense indices, the catalog digest — is a pure function of the set of
        // candidates and never of the order they were discovered in. Paths are
        // compared byte-wise; no locale or Unicode collation is involved.
        entries.sort_by(|x, y| {
            x.candidate
                .path
                .as_bytes()
                .cmp(y.candidate.path.as_bytes())
                .then(x.candidate.span.cmp(&y.candidate.span))
                .then(x.position.cmp(&y.position))
                .then(x.candidate.replacement.cmp(&y.candidate.replacement))
                .then(x.id.cmp(&y.id))
        });
        let (kept, duplicates) = dedup(entries);

        let display_len = effective_display_length(self.display_len);
        let mut mutants = Vec::with_capacity(kept.len());
        for (position, entry) in kept.into_iter().enumerate() {
            let index =
                u32::try_from(position).map_err(|_overflow| BuildError::TooLarge { count })?;
            let display_id = entry.id.get(..display_len).unwrap_or(&entry.id).to_owned();
            mutants.push(Mutant {
                index,
                id: entry.id,
                display_id,
                candidate: entry.candidate,
            });
        }
        check_display_ids(&mutants, display_len)?;
        let digest = catalog_digest(&mutants)?;
        Ok(Catalog {
            mutants,
            duplicates,
            display_len,
            digest,
        })
    }
}

/// A candidate with everything `build` needs to order it.
struct Entry {
    candidate: Candidate,
    id: String,
    position: usize,
}

/// Keeps one candidate per distinct edit — the same bytes replaced by the
/// same bytes in the same file; the rule is deliberately not part of the
/// key — and records the rest.
///
/// The winner is the candidate that comes first in canonical order, which
/// for one edit means the lowest registry position: the earlier row of the
/// operator table. Families are listed from the most local edit (a boolean
/// literal) to the least local (deleting a statement), so table position is
/// a usable, explainable, and above all stable proxy for locality.
fn dedup(sorted: Vec<Entry>) -> (Vec<Entry>, Vec<Duplicate>) {
    let mut winners: BTreeMap<(String, Span, Vec<u8>), (String, Rule)> = BTreeMap::new();
    let mut kept = Vec::with_capacity(sorted.len());
    let mut duplicates = Vec::new();
    for entry in sorted {
        let key = (
            entry.candidate.path.clone(),
            entry.candidate.span,
            entry.candidate.replacement.clone(),
        );
        match winners.get(&key) {
            None => {
                winners.insert(key, (entry.id.clone(), entry.candidate.rule));
                kept.push(entry);
            }
            Some((winner_id, winner_rule)) => {
                let reason = if *winner_rule == entry.candidate.rule {
                    DuplicateReason::Identical
                } else {
                    DuplicateReason::Shadowed
                };
                duplicates.push(Duplicate {
                    reason,
                    dropped: entry.candidate,
                    dropped_id: entry.id,
                    winner_id: winner_id.clone(),
                    winner_rule: *winner_rule,
                });
            }
        }
    }
    (kept, duplicates)
}

/// An out-of-range display length falls back to the default, and the catalog
/// records the length it really proved unique rather than the one asked for.
const fn effective_display_length(requested: usize) -> usize {
    if requested == 0 || requested > ID_HEX_LENGTH {
        DISPLAY_ID_LENGTH
    } else {
        requested
    }
}

fn check_display_ids(mutants: &[Mutant], length: usize) -> Result<(), DisplayCollisionError> {
    let mut by_prefix: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for mutant in mutants {
        by_prefix
            .entry(mutant.display_id.as_str())
            .or_default()
            .push(mutant.id.clone());
    }
    let collisions: Vec<DisplayCollision> = by_prefix
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(display_id, mut ids)| {
            ids.sort_unstable();
            DisplayCollision {
                display_id: display_id.to_owned(),
                ids,
            }
        })
        .collect();
    if collisions.is_empty() {
        Ok(())
    } else {
        Err(DisplayCollisionError { length, collisions })
    }
}

/// The catalog's identity: the ordered list of mutant IDs, under the
/// catalog domain, with the count, all length-prefixed as in the ID recipe.
fn catalog_digest(mutants: &[Mutant]) -> Result<String, CandidateError> {
    let mut hasher = Sha256::new();
    write_length_prefixed(&mut hasher, CATALOG_DOMAIN)?;
    write_length_prefixed(&mut hasher, &mutants.len().to_string())?;
    for mutant in mutants {
        write_length_prefixed(&mut hasher, &mutant.id)?;
    }
    Ok(hex::encode(hasher.finalize()))
}

/// The immutable, ordered set of mutants for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    mutants: Vec<Mutant>,
    duplicates: Vec<Duplicate>,
    display_len: usize,
    digest: String,
}

impl Catalog {
    /// The number of mutants.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.mutants.len()
    }

    /// Whether the catalog holds no mutants.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.mutants.is_empty()
    }

    /// Every mutant in catalog order.
    #[must_use]
    pub fn mutants(&self) -> &[Mutant] {
        &self.mutants
    }

    /// The candidates deduplication dropped, in catalog order of the dropped
    /// candidate.
    #[must_use]
    pub fn duplicates(&self) -> &[Duplicate] {
        &self.duplicates
    }

    /// The display ID length this catalog proved unique.
    #[must_use]
    pub const fn display_length(&self) -> usize {
        self.display_len
    }

    /// The catalog digest: SHA-256 over the domain separator, the mutant
    /// count, and every mutant ID in order, all length-prefixed exactly as in
    /// the mutant ID recipe.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The mutant at a catalog position.
    #[must_use]
    pub fn at(&self, position: usize) -> Option<&Mutant> {
        self.mutants.get(position)
    }

    /// The mutant with the given dense runtime index.
    #[must_use]
    pub fn by_index(&self, index: u32) -> Option<&Mutant> {
        usize::try_from(index)
            .ok()
            .and_then(|position| self.mutants.get(position))
    }

    /// The mutant with the given full ID.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Mutant> {
        self.mutants.iter().find(|mutant| mutant.id == id)
    }

    /// The mutant with the given short ID.
    #[must_use]
    pub fn by_display_id(&self, display_id: &str) -> Option<&Mutant> {
        self.mutants
            .iter()
            .find(|mutant| mutant.display_id == display_id)
    }

    /// Resolves a user-supplied ID prefix, as `--mutant` accepts. It refuses
    /// to guess: a prefix matching two mutants is an error naming both.
    ///
    /// # Errors
    ///
    /// Returns an invalid, unmatched, or ambiguous prefix.
    pub fn resolve_prefix(&self, prefix: &str) -> Result<&Mutant, PrefixError> {
        if prefix.len() < MIN_PREFIX_LENGTH || prefix.len() > ID_HEX_LENGTH || !is_lower_hex(prefix)
        {
            return Err(PrefixError::Invalid {
                prefix: prefix.to_owned(),
                min: MIN_PREFIX_LENGTH,
                max: ID_HEX_LENGTH,
            });
        }
        let matches: Vec<&Mutant> = self
            .mutants
            .iter()
            .filter(|mutant| mutant.id.starts_with(prefix))
            .collect();
        match matches.as_slice() {
            [] => Err(PrefixError::NotFound {
                prefix: prefix.to_owned(),
            }),
            [only] => Ok(only),
            several => Err(PrefixError::Ambiguous {
                prefix: prefix.to_owned(),
                matches: several
                    .iter()
                    .map(|mutant| mutant.display_id.clone())
                    .collect(),
            }),
        }
    }
}
