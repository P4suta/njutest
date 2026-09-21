// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The stable mutant identity recipe.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::ffi::OsString;

use crate::span::{Span, SpanError};

/// The domain separator hashed first for every mutant ID.
/// It carries the recipe version: a future recipe becomes `rust-mutants-id-v1` so that v1 identities can never be mistaken for v1 identities.
pub const ID_DOMAIN: &str = "rust-mutants-id-v1";

/// The length of a full mutant ID in lowercase hex characters (SHA-256).
pub const ID_HEX_LENGTH: usize = 64;

/// The length of the short ID shown in the console and accepted by `--mutant`. The catalog builder proves the prefix is unique within a run.
pub const DISPLAY_ID_LENGTH: usize = 20;

/// The shortest `--mutant` prefix the catalog resolves. Anything shorter is rejected as a typo rather than silently matching half the run.
pub const MIN_PREFIX_LENGTH: usize = 4;

/// A full lowercase SHA-256 value safe to use as one filesystem component.
///
/// Identities and cache keys share an encoding, but an arbitrary string never
/// reaches `Path::join`: construction proves the exact width and alphabet
/// first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexDigest(String);

/// A string that cannot be a canonical full digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{value:?} must be exactly 64 lowercase hex characters")]
pub struct HexDigestError {
    value: String,
}

impl HexDigest {
    /// The canonical lowercase spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Releases the canonical spelling at an explicit wire boundary.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Finishes a SHA-256 computation directly into the only canonical text
    /// representation this type admits.
    #[must_use]
    pub fn finish(hasher: Sha256) -> Self {
        Self(hex::encode(hasher.finalize()))
    }
}

impl std::fmt::Display for HexDigest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for HexDigest {
    type Error = HexDigestError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if is_digest(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(HexDigestError {
                value: value.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for HexDigest {
    type Error = HexDigestError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_digest(&value) {
            Ok(Self(value))
        } else {
            Err(HexDigestError { value })
        }
    }
}

/// The complete stable identity of one mutant.
///
/// Although mutant identities and arbitrary SHA-256 digests have the same
/// representation, they are deliberately different types.  A cache digest
/// must not accidentally select a mutant merely because both happen to be 64
/// lowercase hexadecimal characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutantId(HexDigest);

/// A string that cannot be a canonical mutant identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{value:?} must be exactly 64 lowercase hex characters")]
pub struct MutantIdError {
    value: String,
}

impl MutantId {
    /// The canonical lowercase spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The canonical short identity derived from this full identity.
    #[must_use]
    pub fn display(&self) -> DisplayId {
        DisplayId(self.as_str().chars().take(DISPLAY_ID_LENGTH).collect())
    }

    /// Releases the canonical spelling at an explicit wire boundary.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0.into_inner()
    }
}

impl std::fmt::Display for MutantId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for MutantId {
    type Error = MutantIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        HexDigest::try_from(value)
            .map(Self)
            .map_err(|_error| MutantIdError {
                value: value.to_owned(),
            })
    }
}

impl TryFrom<String> for MutantId {
    type Error = MutantIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        HexDigest::try_from(value.clone())
            .map(Self)
            .map_err(|_error| MutantIdError { value })
    }
}

/// The fixed-width identity shown to people and accepted as the canonical
/// short spelling of a mutant.
///
/// Construction proves the exact 20-character lowercase-hex representation.
/// A container that also carries a [`MutantId`] must additionally call
/// [`Self::belongs_to`] (or construct it with [`MutantId::display`]) so two
/// individually valid identities cannot be paired incorrectly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(String);

/// A string that cannot be a canonical display identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{value:?} must be exactly 20 lowercase hex characters")]
pub struct DisplayIdError {
    value: String,
}

impl DisplayId {
    /// The canonical lowercase spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Releases the canonical spelling at an explicit wire boundary.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Whether this is the unique canonical prefix derived from `full`.
    #[must_use]
    pub fn belongs_to(&self, full: &MutantId) -> bool {
        self.as_str() == full.display().as_str()
    }
}

impl std::fmt::Display for DisplayId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for DisplayId {
    type Error = DisplayIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() == DISPLAY_ID_LENGTH && is_lower_hex(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(DisplayIdError {
                value: value.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for DisplayId {
    type Error = DisplayIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == DISPLAY_ID_LENGTH && is_lower_hex(&value) {
            Ok(Self(value))
        } else {
            Err(DisplayIdError { value })
        }
    }
}

/// A canonical name for a newly-written run directory.
///
/// The alphabet is deliberately narrower than what one operating system may
/// accept. Dots are excluded because Windows trims trailing dots, device names
/// are rejected even when the current host is Unix, and letters must be
/// lowercase so case-insensitive filesystems cannot alias two writable IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(String);

/// A string that cannot name a run directory on every supported filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
    "{value:?} is not a canonical writable run id: use 1 to 64 lowercase ASCII letters, digits, `_` or `-`, excluding Windows device names"
)]
pub struct RunIdError {
    value: String,
}

/// A path-safe run name read from a previously-published store.
///
/// Published v1 artifacts used uppercase `T` and `Z`, so readers retain that
/// spelling. This type deliberately has no conversion to [`RunId`]: historical
/// compatibility must never accidentally acquire write capability. New IDs
/// convert in the other direction because every canonical writable ID is also
/// safe to read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoredRunId(String);

/// A string that cannot safely name a historical run directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
    "{value:?} is not a path-safe stored run id: use 1 to 64 ASCII letters, digits, `_` or `-`, excluding Windows device names"
)]
pub struct StoredRunIdError {
    value: String,
}

impl RunId {
    /// The canonical single-component spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Releases the canonical writable spelling at an explicit wire boundary.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Borrows this new ID through the read-only stored-ID capability.
    #[must_use]
    pub fn stored(&self) -> StoredRunId {
        StoredRunId(self.0.clone())
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for RunId {
    type Error = RunIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if valid_run_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(RunIdError {
                value: value.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for RunId {
    type Error = RunIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if valid_run_id(&value) {
            Ok(Self(value))
        } else {
            Err(RunIdError { value })
        }
    }
}

impl StoredRunId {
    /// The exact spelling used by the stored directory.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Releases the validated historical spelling at an explicit wire boundary.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// The filesystem-independent comparison key used to detect aliases on
    /// case-insensitive stores before any run is selected or removed.
    #[must_use]
    pub fn case_folded(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl std::fmt::Display for StoredRunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for StoredRunId {
    type Error = StoredRunIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if valid_stored_run_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(StoredRunIdError {
                value: value.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for StoredRunId {
    type Error = StoredRunIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if valid_stored_run_id(&value) {
            Ok(Self(value))
        } else {
            Err(StoredRunIdError { value })
        }
    }
}

fn deserialize_checked<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    let value = String::deserialize(deserializer)?;
    T::try_from(value).map_err(serde::de::Error::custom)
}

impl Serialize for HexDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HexDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_checked(deserializer)
    }
}

impl Serialize for MutantId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MutantId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_checked(deserializer)
    }
}

impl Serialize for DisplayId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DisplayId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_checked(deserializer)
    }
}

impl Serialize for RunId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RunId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_checked(deserializer)
    }
}

impl Serialize for StoredRunId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StoredRunId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_checked(deserializer)
    }
}

impl From<RunId> for StoredRunId {
    fn from(value: RunId) -> Self {
        Self(value.0)
    }
}

impl From<&RunId> for StoredRunId {
    fn from(value: &RunId) -> Self {
        value.stored()
    }
}

fn valid_run_id(value: &str) -> bool {
    valid_stored_run_id(value)
        && value
            .bytes()
            .all(|one| !one.is_ascii_alphabetic() || one.is_ascii_lowercase())
}

fn valid_stored_run_id(value: &str) -> bool {
    let shaped = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|one| one.is_ascii_alphanumeric() || matches!(one, b'_' | b'-'));
    shaped && !windows_device_name(value)
}

fn windows_device_name(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    matches!(lowercase.as_str(), "con" | "prn" | "aux" | "nul")
        || lowercase
            .strip_prefix("com")
            .or_else(|| lowercase.strip_prefix("lpt"))
            .is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes().first(), Some(b'1'..=b'9'))
            })
}

/// A source path that cannot name a file inside the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum PathError {
    /// The path is empty.
    #[error("source path is empty")]
    Empty,
    /// The path contains a NUL byte.
    #[error("source path contains a NUL byte")]
    NulByte,
    /// The path is absolute.
    #[error("source path {path:?} is not workspace-relative")]
    Absolute {
        /// The offending path.
        path: String,
    },
    /// The path starts with a Windows volume name.
    #[error("source path {path:?} has a volume name")]
    VolumeName {
        /// The offending path.
        path: String,
    },
    /// The path climbs out of the workspace root.
    #[error("source path {path:?} escapes the workspace root")]
    Escaping {
        /// The offending path.
        path: String,
    },
}

/// An identity that is incomplete or not canonical.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum IdentityError {
    /// The source path is invalid.
    #[error(transparent)]
    Path(#[from] PathError),
    /// The source path is not in its canonical form.
    #[error("source path {path:?} is not normalized; should be {normalized:?}")]
    UnnormalizedPath {
        /// The path as given.
        path: String,
        /// Its canonical form.
        normalized: String,
    },
    /// The rule name is empty or malformed.
    #[error("rule name {name:?} is invalid: {reason}")]
    InvalidRuleName {
        /// The offending name.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The rule version is below 1.
    #[error("rule version must be at least 1, found {version}")]
    InvalidRuleVersion {
        /// The offending version.
        version: u32,
    },
    /// The span is not well formed.
    #[error(transparent)]
    Span(#[from] SpanError),
    /// A digest is not 64 lowercase hex characters.
    #[error("{field} digest {value:?} must be 64 lowercase hex characters")]
    InvalidDigest {
        /// Which digest: `source`, `original`, or `replacement`.
        field: &'static str,
        /// The offending value.
        value: String,
    },
    /// A field's byte length overflows the 32-bit length prefix.
    #[error("identity field of {bytes} bytes exceeds the 32-bit length prefix")]
    FieldTooLong {
        /// The field's length.
        bytes: usize,
    },
    /// A value is not a full 64 hex character mutant ID.
    #[error("{value:?} is not a 64 hex character mutant id")]
    InvalidId {
        /// The offending value.
        value: String,
    },
}

/// The complete, canonical input to a stable mutant ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    /// The workspace-relative source path with forward slashes, as produced by [`normalize_path`].
    pub path: String,
    /// The operator rule name, for example `eq-to-neq`.
    pub rule_name: String,
    /// The rule's version. Bumping it re-mints every mutant the rule produces, which is how a behaviour change invalidates cached outcomes.
    pub rule_version: u32,
    /// The byte range of the original text being replaced.
    pub span: Span,
    /// The SHA-256 of the whole file, lowercase hex.
    pub source_digest: String,
    /// The SHA-256 of the original span bytes.
    pub original_digest: String,
    /// The SHA-256 of the replacement bytes. For a deletion, the digest of the empty string.
    pub replacement_digest: String,
}

/// The lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A path whose platform spelling cannot be represented by the UTF-8 catalog
/// format without changing its bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("path {path:?} is not valid UTF-8")]
pub struct SlashedPathError {
    path: OsString,
}

/// `path` as a reader sees it: the separators a catalog uses, and nothing else changed.
///
/// # Errors
/// Returns [`SlashedPathError`] when the platform spelling is not exact UTF-8.
pub fn slashed(path: &std::path::Path) -> Result<String, SlashedPathError> {
    path.as_os_str()
        .to_str()
        .map(|text| text.replace('\\', "/"))
        .ok_or_else(|| SlashedPathError {
            path: path.as_os_str().to_owned(),
        })
}

/// Converts a source path to the canonical workspace-relative form used in identities and reports: forward slashes, cleaned, no leading `./`.
///
/// # Errors
/// Returns the reason the path cannot name a workspace file.
pub fn normalize_path(path: &str) -> Result<String, PathError> {
    if path.is_empty() {
        return Err(PathError::Empty);
    }
    if path.contains('\0') {
        return Err(PathError::NulByte);
    }
    let slashed = path.replace('\\', "/");
    if slashed.starts_with('/') {
        return Err(PathError::Absolute {
            path: path.to_owned(),
        });
    }
    if has_volume_name(&slashed) {
        return Err(PathError::VolumeName {
            path: path.to_owned(),
        });
    }
    let cleaned = clean(&slashed);
    if has_volume_name(&cleaned) {
        return Err(PathError::VolumeName {
            path: path.to_owned(),
        });
    }
    if cleaned == "." || cleaned == ".." || cleaned.starts_with("../") {
        return Err(PathError::Escaping {
            path: path.to_owned(),
        });
    }
    Ok(cleaned)
}

/// A volume name is an ASCII letter followed by a colon. Both halves are checked: a colon after something that is not a letter (`1:`) is a directory whose name contains a colon, which POSIX allows.
fn has_volume_name(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!((bytes.first(), bytes.get(1)), (Some(letter), Some(b':')) if letter.is_ascii_alphabetic())
}

/// The relative-path half of Go's `path.Clean`: drop empty and `.` elements, resolve `..` against a preceding element, keep a leading `..`, and answer `.` for a path that cleans to nothing.
fn clean(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for element in path.split('/') {
        match element {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(last) if *last != "..") {
                    stack.pop();
                } else {
                    stack.push("..");
                }
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        ".".to_owned()
    } else {
        stack.join("/")
    }
}

impl Identity {
    /// Whether the identity is complete and canonical. Every field that feeds the hash is checked, because a malformed field would otherwise produce a plausible-looking ID for a mutant that cannot be resolved back to a source location.
    ///
    /// # Errors
    /// Returns the first field that is not canonical.
    pub fn validate(&self) -> Result<(), IdentityError> {
        let normalized = normalize_path(&self.path)?;
        if normalized != self.path {
            return Err(IdentityError::UnnormalizedPath {
                path: self.path.clone(),
                normalized,
            });
        }
        if self.rule_name.is_empty() {
            return Err(IdentityError::InvalidRuleName {
                name: self.rule_name.clone(),
                reason: "empty",
            });
        }
        if self.rule_name.contains([' ', '\t', '\r', '\n', '@']) {
            return Err(IdentityError::InvalidRuleName {
                name: self.rule_name.clone(),
                reason: "contains whitespace or '@'",
            });
        }
        if self.rule_version < 1 {
            return Err(IdentityError::InvalidRuleVersion {
                version: self.rule_version,
            });
        }
        self.span.validate()?;
        for (field, value) in [
            ("source", &self.source_digest),
            ("original", &self.original_digest),
            ("replacement", &self.replacement_digest),
        ] {
            if !is_digest(value) {
                return Err(IdentityError::InvalidDigest {
                    field,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }

    /// The stable full mutant ID.
    ///
    /// # Errors
    /// Returns the validation failure; an invalid identity never produces an ID.
    pub fn id(&self) -> Result<MutantId, IdentityError> {
        self.validate()?;
        let mut hasher = Sha256::new();
        let version = self.rule_version.to_string();
        let start = self.span.start.to_string();
        let end = self.span.end.to_string();
        for field in [
            ID_DOMAIN,
            &self.path,
            &self.rule_name,
            &version,
            &start,
            &end,
            &self.source_digest,
            &self.original_digest,
            &self.replacement_digest,
        ] {
            write_length_prefixed(&mut hasher, field)?;
        }
        Ok(MutantId(HexDigest::finish(hasher)))
    }
}

/// Appends `enc(s)` to `hasher`: a 4-byte big-endian byte length followed by the raw bytes. Every hash the engine builds from a list of fields uses this one encoding.
///
/// # Errors
/// Returns [`IdentityError::FieldTooLong`] when `s` does not fit the prefix.
pub fn write_length_prefixed(hasher: &mut Sha256, s: &str) -> Result<(), IdentityError> {
    let length = u32::try_from(s.len())
        .map_err(|_overflow| IdentityError::FieldTooLong { bytes: s.len() })?;
    hasher.update(length.to_be_bytes());
    hasher.update(s.as_bytes());
    Ok(())
}

/// The short form of a full mutant ID. Uniqueness of the short form is a property of a whole catalog, proven by the catalog builder; this only truncates.
///
/// # Errors
/// Returns [`IdentityError::InvalidId`] when `full` is not a full ID.
pub fn display_id_of(full: &str) -> Result<String, IdentityError> {
    if !is_id(full) {
        return Err(IdentityError::InvalidId {
            value: full.to_owned(),
        });
    }
    full.get(..DISPLAY_ID_LENGTH)
        .map(ToOwned::to_owned)
        .ok_or_else(|| IdentityError::InvalidId {
            value: full.to_owned(),
        })
}

/// Whether `s` is a full mutant ID: 64 lowercase hex characters.
#[must_use]
pub fn is_id(s: &str) -> bool {
    s.len() == ID_HEX_LENGTH && is_lower_hex(s)
}

/// Whether `s` is a lowercase hex SHA-256 digest.
#[must_use]
pub fn is_digest(s: &str) -> bool {
    s.len() == ID_HEX_LENGTH && is_lower_hex(s)
}

/// Whether every character of `s` is a lowercase hex digit. Uppercase is rejected rather than folded: identities are compared as strings, so exactly one spelling may exist.
#[must_use]
pub fn is_lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
