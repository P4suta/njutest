// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the repository was when the run started, in the report's own words.

use std::ffi::OsString;
use std::path::Path;

use rust_mutants::git::{Asking, Facts};

use crate::evidence::tree::EXCLUDED_DIRECTORIES;
use crate::report::{Git, UNAVAILABLE};
use crate::watch::Watch;

pub use rust_mutants::git::{Change, DEFAULT_BASE};

/// Asks git about the tree at `root`.
#[must_use]
pub fn describe(root: &Path, env: &[(OsString, OsString)], watch: Watch<'_>) -> Git {
    let Some(facts) = rust_mutants::git::facts(&asking(root, env, &watch)) else {
        return Git::unavailable();
    };
    let Facts {
        commit,
        branch,
        dirty,
    } = facts;
    if commit == UNAVAILABLE || branch == UNAVAILABLE {
        return Git::unavailable();
    }
    Git {
        available: true,
        commit,
        branch,
        dirty,
        merge_base: None,
        changed_files: Vec::new(),
    }
}

/// Every file that differs from `base`, committed and not.
///
/// Returns nothing when git could not be asked or does not know `base`,
/// which the caller states as a limitation rather than reading as an empty
/// change set: a run that verified nothing because it could not see what
/// changed must never look like a run that verified everything that did.
#[must_use]
pub fn changed(
    root: &Path,
    env: &[(OsString, OsString)],
    base: &str,
    watch: Watch<'_>,
) -> Option<Change> {
    rust_mutants::git::changed(&asking(root, env, &watch), base)
}

/// Where a run asks git, leaving out the directories a run writes rather than verifies.
const fn asking<'a>(
    root: &'a Path,
    env: &'a [(OsString, OsString)],
    watch: &'a Watch<'a>,
) -> Asking<'a, Watch<'a>> {
    Asking {
        root,
        env,
        excluded: &EXCLUDED_DIRECTORIES,
        watch,
    }
}
