// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the rules propose, before the compiler has ruled on any of it.

use serde::{Deserialize, Serialize};

/// Names the shape of the document.
pub const DOCUMENT_TYPE: &str = "rust-mutants/candidates";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// Every place the selected rules target, as the walk found them.
///
/// This is what `list` says and `catalog` does not: a candidate here has not
/// been compiled, so nothing in it claims the compiler would accept it. A
/// reader who wants the accepted set, with the refusals and their reasons,
/// wants `catalog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatesDocument {
    /// [`DOCUMENT_TYPE`].
    pub document_type: String,
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The engine that walked the tree.
    pub tool_version: String,
    /// How many candidates the document holds, which is the number `catalog` will rule on.
    pub count: u32,
    /// Every candidate, in the order the walk found them.
    pub candidates: Vec<CandidateDocument>,
}

/// One place a rule targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDocument {
    /// The identity the catalog would give it, when the walk could compute one.
    pub id: String,
    /// The short form of that identity.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The item it sits in, as a reader writes it.
    pub item: String,
    /// The rule that proposed it.
    pub rule: String,
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The bytes the edit replaces.
    pub original: String,
    /// What they become.
    pub replacement: String,
}
