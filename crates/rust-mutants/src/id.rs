// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The stable mutant identity recipe.

use sha2::{Digest as _, Sha256};

use crate::span::{Span, SpanError};

/// The domain separator hashed first for every mutant ID. It carries the recipe version: a future recipe becomes `rust-mutants-id-v2` so that v1 identities can never be mistaken for v2 identities.
pub const ID_DOMAIN: &str = "rust-mutants-id-v1";

/// The length of a full mutant ID in lowercase hex characters (SHA-256).
pub const ID_HEX_LENGTH: usize = 64;

/// The length of the short ID shown in the console and accepted by `--mutant`. The catalog builder proves the prefix is unique within a run.
pub const DISPLAY_ID_LENGTH: usize = 20;

/// The shortest `--mutant` prefix the catalog resolves. Anything shorter is rejected as a typo rather than silently matching half the run.
pub const MIN_PREFIX_LENGTH: usize = 4;

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

/// `path` as a reader sees it: the separators a catalog uses, and nothing else changed.
#[must_use]
pub fn slashed(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    pub fn id(&self) -> Result<String, IdentityError> {
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
        Ok(hex::encode(hasher.finalize()))
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
