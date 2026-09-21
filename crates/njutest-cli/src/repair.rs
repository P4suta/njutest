// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a generation provider may offer, and what a run will take from it.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};

/// The protocol version this release speaks.
pub const VERSION: u32 = 1;

/// The most candidates one finding may be answered with.
pub const CANDIDATE_LIMIT: usize = 64;

/// The most a provider may write in one answer.
pub const OUTPUT_LIMIT: usize = 4 << 20;

/// Where a provider may write when its configuration names nowhere: the test files and the fuzz corpora, and nothing else.
pub const DEFAULT_ALLOWED: [&str; 2] = ["**/tests/**/*.rs", "**/fuzz/corpus/**"];

/// What a run asks a generation provider about one finding.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// The protocol version.
    pub version: u32,
    /// The finding to write a test for.
    pub finding: AskedFinding,
    /// Where the provider may write, as workspace-relative globs.
    pub allowed_paths: Vec<String>,
    /// What tree this is about.
    pub workspace: AskedWorkspace,
}

/// The finding a provider is asked to close.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AskedFinding {
    /// The finding's identity.
    pub id: String,
    /// What kind of hole it is.
    pub kind: String,
    /// The file the mutation is in.
    pub path: String,
    /// The line it is on.
    pub line: u32,
    /// One sentence a reader can act on.
    pub summary: String,
    /// The command that reproduces it.
    pub replay: String,
    /// The mutation itself, as a person reads it.
    pub mutant: String,
    /// The mutant's identity.
    pub mutant_id: String,
}

/// What tree the provider is writing for.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AskedWorkspace {
    /// The tree as one number.
    pub workspace_digest: String,
    /// The run that found the hole.
    pub run_id: String,
}

/// What a generation provider answered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offered {
    /// The protocol version, which must be the one asked in.
    pub version: u32,
    /// What it would write.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
}

/// One thing a provider would write.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// `patch` or `corpus`.
    pub kind: String,
    /// Where it would be written, workspace-relative.
    pub path: String,
    /// The SHA-256 of the file as the provider saw it, absent when it would create one.
    #[serde(default)]
    pub preimage_sha256: Option<String>,
    /// The whole new content, base64 with padding and nothing else.
    pub content_base64: String,
}

/// What a candidate would do to a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Kind {
    /// A test file.
    Patch,
    /// A fuzz corpus entry.
    Corpus,
}

impl Kind {
    /// The word the protocol uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Corpus => "corpus",
        }
    }

    /// The kind that word names.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "patch" => Some(Self::Patch),
            "corpus" => Some(Self::Corpus),
            _ => None,
        }
    }
}

/// One candidate a run has checked the shape of: the path is admissible, the content decoded, and the preimage is what it claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// What it would do.
    pub kind: Kind,
    /// Where it would be written, workspace-relative with forward slashes.
    pub path: String,
    /// The SHA-256 of the file the provider saw, absent when it would create one.
    pub preimage: Option<String>,
    /// The whole new content.
    pub content: Vec<u8>,
    /// The SHA-256 of that content, which is the candidate's own identity.
    pub digest: String,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum RepairErrorKind {
    /// A provider said something this version does not understand.
    Protocol,
    /// A provider would write somewhere it may not.
    PathRefused,
    /// The file a candidate patches is not the file the provider saw.
    PreimageMoved,
}

impl RepairErrorKind {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Protocol => error::GENERATION_PROTOCOL,
            Self::PathRefused => error::GENERATION_PATH_REFUSED,
            Self::PreimageMoved => error::GENERATION_PREIMAGE_MOVED,
        }
    }
}

/// Why a candidate could not be taken.
#[derive(Debug, thiserror::Error)]
#[error("{}: {message}", kind.code().code)]
pub struct RepairError {
    kind: RepairErrorKind,
    message: String,
}

impl RepairError {
    /// A failure of `kind` with `message`.
    #[must_use]
    pub fn new(kind: RepairErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The failure mode.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn kind(&self) -> RepairErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

/// Reads what a provider answered, taking nothing it may not offer.
///
/// # Errors
/// [`RepairErrorKind::Protocol`] for an answer this version does not
/// understand, [`RepairErrorKind::PathRefused`] for a path outside what the
/// configuration allows, and [`RepairErrorKind::PreimageMoved`] when the file
/// a candidate patches is not the file the provider saw.
pub fn take(said: &str, root: &Path, allowed: &[String]) -> Result<Vec<Proposal>, RepairError> {
    if said.len() > OUTPUT_LIMIT {
        return Err(RepairError::new(
            RepairErrorKind::Protocol,
            format!("the provider wrote more than {OUTPUT_LIMIT} bytes"),
        ));
    }
    let offered: Offered = crate::strictjson::decode_str(said.trim()).map_err(|source| {
        RepairError::new(
            RepairErrorKind::Protocol,
            format!("the provider said something that is not this protocol: {source}"),
        )
    })?;
    if offered.version != VERSION {
        return Err(RepairError::new(
            RepairErrorKind::Protocol,
            format!(
                "the provider answered in version {} and this release speaks {VERSION}",
                offered.version
            ),
        ));
    }
    if offered.candidates.len() > CANDIDATE_LIMIT {
        return Err(RepairError::new(
            RepairErrorKind::Protocol,
            format!(
                "the provider offered {} candidates and at most {CANDIDATE_LIMIT} are read",
                offered.candidates.len()
            ),
        ));
    }
    let patterns = compiled(allowed);
    let mut taken = Vec::with_capacity(offered.candidates.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for candidate in &offered.candidates {
        let proposal = proposed(candidate, root, &patterns)?;
        if !seen.insert(proposal.path.clone()) {
            return Err(RepairError::new(
                RepairErrorKind::Protocol,
                format!("the provider offered {:?} twice", proposal.path),
            ));
        }
        taken.push(proposal);
    }
    Ok(taken)
}

/// Where a provider may write: what its configuration says, or the default when it says nothing.
#[must_use]
pub fn allowed(configured: &[String]) -> Vec<String> {
    if configured.is_empty() {
        DEFAULT_ALLOWED.map(str::to_owned).to_vec()
    } else {
        configured.to_vec()
    }
}

/// One candidate, checked.
fn proposed(
    candidate: &Candidate,
    root: &Path,
    allowed: &[rust_mutants::glob::Pattern],
) -> Result<Proposal, RepairError> {
    let kind = Kind::parse(&candidate.kind).ok_or_else(|| {
        RepairError::new(
            RepairErrorKind::Protocol,
            format!(
                "the provider offered a candidate of kind {:?}",
                candidate.kind
            ),
        )
    })?;
    let path = admissible(&candidate.path, allowed)?;
    let content = decode(&candidate.content_base64).map_err(|why| {
        RepairError::new(
            RepairErrorKind::Protocol,
            format!("the content of {path:?} is not base64: {why}"),
        )
    })?;
    if content.len() > OUTPUT_LIMIT {
        return Err(RepairError::new(
            RepairErrorKind::Protocol,
            format!("the content of {path:?} is more than {OUTPUT_LIMIT} bytes"),
        ));
    }
    match_preimage(root, &path, candidate.preimage_sha256.as_deref())?;
    Ok(Proposal {
        kind,
        digest: hex::encode(Sha256::digest(&content)),
        path,
        preimage: candidate.preimage_sha256.clone(),
        content,
    })
}

/// The path a candidate names, as a workspace-relative path a run may write.
///
/// # Errors
/// [`RepairErrorKind::PathRefused`] for anything absolute, anything that
/// climbs out of the tree, and anything the allowed patterns do not match.
pub fn admissible(
    path: &str,
    allowed: &[rust_mutants::glob::Pattern],
) -> Result<String, RepairError> {
    let refuse = |why: &str| {
        RepairError::new(
            RepairErrorKind::PathRefused,
            format!("the provider would write {path:?}, which {why}"),
        )
    };
    if path.is_empty() {
        return Err(refuse("names no file"));
    }
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return Err(refuse("is absolute"));
    }
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(refuse("climbs out of the tree or names a root")),
        }
    }
    let mut parts = Vec::new();
    for component in candidate.components() {
        let Component::Normal(part) = component else {
            return Err(refuse("climbs out of the tree or names a root"));
        };
        let text = part.to_str().ok_or_else(|| refuse("is not valid UTF-8"))?;
        parts.push(text);
    }
    let relative = parts.join("/");
    if !allowed.iter().any(|pattern| pattern.matches(&relative)) {
        return Err(refuse("is not one of the allowed paths"));
    }
    Ok(relative)
}

/// Whether the file a candidate patches is still the file the provider saw.
fn match_preimage(root: &Path, path: &str, claimed: Option<&str>) -> Result<(), RepairError> {
    let found = preimage_of(root, path);
    if found.as_deref() == claimed {
        return Ok(());
    }
    Err(RepairError::new(
        RepairErrorKind::PreimageMoved,
        match (claimed, found) {
            (Some(_), None) => format!("{path:?} is not there, and a candidate patches it"),
            (None, Some(_)) => format!("{path:?} is already there, and a candidate creates it"),
            _ => format!("{path:?} is not the file the provider saw"),
        },
    ))
}

/// The SHA-256 of a file of the tree, or nothing when it is not there.
#[must_use]
pub fn preimage_of(root: &Path, path: &str) -> Option<String> {
    let bytes = match std::fs::read(root.join(path)) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    Some(hex::encode(Sha256::digest(&bytes)))
}

/// The allowed paths as patterns. A pattern that is not one allows nothing rather than everything.
fn compiled(allowed: &[String]) -> Vec<rust_mutants::glob::Pattern> {
    allowed
        .iter()
        .filter_map(
            |pattern| match rust_mutants::glob::Pattern::compile(pattern) {
                Ok(pattern) => Some(pattern),
                Err(_) => None,
            },
        )
        .collect()
}

/// Where a run keeps the candidates it was offered, workspace-relative.
pub const STORE: &str = ".njutest/candidates-v1";

/// Where one candidate's content is kept.
#[must_use]
pub fn stored_path(root: &Path, digest: &str) -> PathBuf {
    root.join(STORE).join(digest)
}

/// Keeps one candidate's content, named by its own digest, for a later `fix`.
///
/// # Errors
/// The operating system's, when the store cannot be written.
pub fn keep(root: &Path, proposal: &Proposal) -> Result<PathBuf, std::io::Error> {
    let path = stored_path(root, &proposal.digest);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &proposal.content)?;
    Ok(path)
}

/// The content of one kept candidate, when it is still there and still itself.
#[must_use]
pub fn load(root: &Path, digest: &str) -> Option<Vec<u8>> {
    let content = match std::fs::read(stored_path(root, digest)) {
        Ok(content) => content,
        Err(_) => return None,
    };
    (hex::encode(Sha256::digest(&content)) == digest).then_some(content)
}

/// Why provider text is not the strict base64 form the protocol accepts.
#[derive(Debug, thiserror::Error)]
enum DecodeError {
    /// Base64 is made of complete four-character quanta.
    #[error("its length is not a multiple of four")]
    Length,
    /// Padding appeared before the final two positions of the final quantum.
    #[error("it pads somewhere other than the end")]
    Padding,
    /// A non-padding character followed padding.
    #[error("it has a character after its padding")]
    AfterPadding,
    /// A character is outside the protocol alphabet.
    #[error("{found:?} is not a base64 character")]
    Character { found: char },
    /// The fixed protocol alphabet no longer fits the accumulator's index.
    #[error("its alphabet index cannot be represented")]
    AlphabetIndex,
}

/// The bytes a strict base64 text stands for.
fn decode(text: &str) -> Result<Vec<u8>, DecodeError> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(DecodeError::Length);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut chunks = bytes.chunks(4).peekable();
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        let mut value: u32 = 0;
        let mut padding = 0u32;
        for (position, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                if !last || position < 2 {
                    return Err(DecodeError::Padding);
                }
                padding = padding.saturating_add(1);
                value <<= 6;
                continue;
            }
            if padding > 0 {
                return Err(DecodeError::AfterPadding);
            }
            let index = ALPHABET
                .iter()
                .position(|allowed| allowed == byte)
                .ok_or_else(|| DecodeError::Character {
                    found: char::from(*byte),
                })?;
            let index = u32::try_from(index).map_err(|_overflow| DecodeError::AlphabetIndex)?;
            value = (value << 6) | index;
        }
        let [_, first, second, third] = value.to_be_bytes();
        out.push(first);
        if padding < 2 {
            out.push(second);
        }
        if padding < 1 {
            out.push(third);
        }
    }
    Ok(out)
}
