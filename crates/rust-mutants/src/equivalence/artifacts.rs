// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a build produced, by name and by content, and what two such sets say about each other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The executables one build produced, by target identity, each with the lowercase hex SHA-256 of its bytes.
pub type Artifacts = BTreeMap<String, String>;

/// Why an executable a successful build named could not be compared.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArtifactError {
    /// The executable could not be read back.
    #[error("could not read executable {}: {source}", path.display())]
    Unreadable {
        /// The path the build named.
        path: PathBuf,
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
}

/// What comparing two builds' artifacts established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    /// Every executable of one build is byte for byte the executable of the other, so the two are the same programs.
    Identical,
    /// At least one executable differs, so the mutation is one the compiler renders.
    Differs,
    /// Nothing was established, and why.
    NotEstablished(&'static str),
}

impl Identity {
    /// The wire name a trace and a report use.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Identical => "identical",
            Self::Differs => "differs",
            Self::NotEstablished(_) => "not-established",
        }
    }
}

/// The reason an empty comparison establishes nothing, which is the reason a reader has to be able to tell from `Identical`.
pub const NOTHING_TO_COMPARE: &str =
    "the build produced no executable, and two empty sets are not two equal programs";

/// The reason a build that produced a different set of targets establishes nothing.
pub const DIFFERENT_TARGETS: &str =
    "the two builds produced different targets, so there is no pair of programs to compare";

/// The digest of each executable, by target identity.
///
/// # Errors
/// Returns the path of the first executable that could not be read: a build that said it produced a file this cannot read is not a build this run may draw a conclusion from.
pub fn digests<'a, I>(executables: I) -> Result<Artifacts, ArtifactError>
where
    I: IntoIterator<Item = (&'a str, &'a Path)>,
{
    let mut found = Artifacts::new();
    for (id, path) in executables {
        let bytes = std::fs::read(path).map_err(|source| ArtifactError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?;
        found.insert(id.to_owned(), hex::encode(Sha256::digest(&bytes)));
    }
    Ok(found)
}

/// Whether a build compiled the file it was asked about again, which is the only build a comparison may speak for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recompiled {
    /// Every unit that read the file was compiled, and at least one did.
    Every,
    /// Cargo reused the artifact of a unit that read the file, so that artifact was built from the bytes before the file changed.
    Reused,
    /// No unit of the build read the file, so nothing the build produced was compiled from it.
    Unread,
}

/// Whether every unit of `units` that read the file at `changed` was compiled rather than reused, a file named however its dep-info spells it.
///
/// `provenance` separates the two ways a build's products may have been established: an actual compiler process, whose own fresh bit is the honesty this check exists to hold, and the engine's verified record, whose key covers `changed`'s bytes and whose artifacts a real compiler run produced, so a reading unit of that record was compiled from those bytes however the record marks them.
#[must_use]
pub fn recompiled(
    units: &[crate::cargo::Unit],
    changed: &Path,
    provenance: crate::cargo::Provenance,
) -> Recompiled {
    let named = crate::cargo::resolved(changed);
    let verified_reuse = match provenance {
        crate::cargo::Provenance::VerifiedReuse => true,
        crate::cargo::Provenance::Compiler | crate::cargo::Provenance::SharedRefusal => false,
    };
    let mut read = false;
    for unit in units {
        let reads = unit.inputs.iter().any(|input| {
            input.file_name() == changed.file_name() && crate::cargo::resolved(input) == named
        });
        if !reads {
            continue;
        }
        if unit.fresh && !verified_reuse {
            return Recompiled::Reused;
        }
        read = true;
    }
    if read {
        Recompiled::Every
    } else {
        Recompiled::Unread
    }
}

/// What one build's artifacts say about another's.
#[must_use]
pub fn compare(original: &Artifacts, mutated: &Artifacts) -> Identity {
    if original.is_empty() || mutated.is_empty() {
        return Identity::NotEstablished(NOTHING_TO_COMPARE);
    }
    if original.len() != mutated.len() || !original.keys().eq(mutated.keys()) {
        return Identity::NotEstablished(DIFFERENT_TARGETS);
    }
    if original == mutated {
        Identity::Identical
    } else {
        Identity::Differs
    }
}
