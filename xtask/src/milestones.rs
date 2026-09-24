// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Milestone references in the documentation, held to the roadmap's registry.

use std::collections::BTreeSet;

/// Why the roadmap cannot serve as the milestone registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The same identifier has more than one row.
    #[error("the roadmap declares milestone {name} more than once")]
    Duplicate {
        /// The repeated milestone.
        name: String,
    },
    /// No row declares a milestone identifier.
    #[error("the roadmap table declares no milestones")]
    Empty,
}

impl crate::error::Coded for RegistryError {
    fn code(&self) -> crate::error::ErrorCode {
        crate::error::MILESTONE_REGISTRY
    }
}

/// The milestone identifiers declared by the roadmap table.
///
/// # Errors
/// The table declares the same identifier twice or declares none at all.
pub fn registry(roadmap: &str) -> Result<BTreeSet<String>, RegistryError> {
    let mut found = BTreeSet::new();
    for line in roadmap.lines().filter(|line| line.starts_with('|')) {
        let Some(cell) = line.get(1..).and_then(|rest| rest.split('|').next()) else {
            continue;
        };
        let Some(name) = cell
            .split_whitespace()
            .next()
            .filter(|name| milestone(name))
        else {
            continue;
        };
        if !found.insert(name.to_owned()) {
            return Err(RegistryError::Duplicate {
                name: name.to_owned(),
            });
        }
    }
    if found.is_empty() {
        return Err(RegistryError::Empty);
    }
    Ok(found)
}

/// Every milestone-shaped reference in prose.
#[must_use]
pub fn references(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| milestone(word))
        .map(ToOwned::to_owned)
        .collect()
}

/// Whether `word` has the deliberately narrow milestone spelling.
///
/// Compiler diagnostics such as `E0369` are intentionally outside it: a milestone has one or two decimal digits, and every identifier in the roadmap registry is therefore distinguishable from a Rust error code.
fn milestone(word: &str) -> bool {
    let mut characters = word.chars();
    let Some(prefix) = characters.next() else {
        return false;
    };
    if !matches!(prefix, 'M' | 'E' | 'K') {
        return false;
    }
    let digits: Vec<char> = characters.collect();
    matches!(digits.len(), 1 | 2) && digits.iter().all(char::is_ascii_digit)
}
