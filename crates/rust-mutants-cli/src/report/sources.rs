// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The files a stored run named, read back from a tree so a projection can show the mutation in place.
//!
//! A projection that shows source has to answer one question first: is this
//! the file the run measured? The report carries the digest of the bytes each
//! mutation was cut from, so the answer is a fact rather than a guess. A file
//! that changed is shown as changed and never as the source; a file that is
//! not there at all is `RM0012`, because a projection that quietly left it out
//! would lose every mutant of it without saying so.

use std::collections::BTreeMap;
use std::path::Path;

use rust_mutants::report::run::RunDocument;

use crate::error::CliError;

/// One file the report names, as this tree holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The bytes the run measured, which the digest settles.
    Measured(String),
    /// The file is here and is not the one the run measured.
    Changed,
}

impl Held {
    /// The source, when this is the file the run measured.
    #[must_use]
    pub const fn measured(&self) -> Option<&String> {
        match self {
            Self::Measured(text) => Some(text),
            Self::Changed => None,
        }
    }
}

/// Every file the report's mutants name, read from `root`.
///
/// # Errors
/// [`CliError::SourceUnreadable`] when a file the report names is not under
/// `root`, which is `RM0012`.
pub fn read(document: &RunDocument, root: &Path) -> Result<BTreeMap<String, Held>, CliError> {
    let mut held: BTreeMap<String, Held> = BTreeMap::new();
    for mutant in &document.mutants {
        if held.contains_key(&mutant.path) {
            continue;
        }
        let bytes = std::fs::read(root.join(&mutant.path))
            .map_err(|_error| CliError::absent(&mutant.path, root))?;
        let same = mutant.source_digest.is_empty()
            || rust_mutants::id::digest(&bytes) == mutant.source_digest;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        held.insert(
            mutant.path.clone(),
            if same {
                Held::Measured(text)
            } else {
                Held::Changed
            },
        );
    }
    Ok(held)
}
