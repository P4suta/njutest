// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the repository was when the run started, in the report's own words.

use std::path::Path;

use rust_mutants::git::{Asking, Facts};

use crate::evidence::tree::Excluded;
use crate::report::{Git, UNAVAILABLE};
use crate::watch::Watch;

pub use rust_mutants::git::{Change, DEFAULT_BASE};

/// Where a run asks git, and what it leaves out.
#[derive(Debug)]
pub struct Asked<'a> {
    /// The tree the question is about.
    pub root: &'a Path,
    /// The environment git is run with.
    pub env: &'a rust_mutants::vars::Variables,
    /// Directories this project writes rather than verifies.
    pub excluded: &'a Excluded,
    /// What stops the commands, and who hears that they ran.
    pub watch: Watch<'a>,
}

/// Asks git about the tree.
#[must_use]
pub fn describe(asked: &Asked<'_>) -> Git {
    let names = asked.excluded.names();
    let Some(facts) = rust_mutants::git::facts(&asking(asked, &names)) else {
        return Git::Unavailable;
    };
    let Facts {
        commit,
        branch,
        dirty,
    } = facts;
    if commit == UNAVAILABLE || branch == UNAVAILABLE {
        return Git::Unavailable;
    }
    Git::Said(crate::report::Said {
        commit,
        branch,
        dirty,
        against: None,
    })
}

/// Every file that differs from `base`, committed and not.
#[must_use]
pub fn changed(asked: &Asked<'_>, base: &str) -> Option<Change> {
    let names = asked.excluded.names();
    rust_mutants::git::changed(&asking(asked, &names), base)
}

/// Where a run asks git, leaving out the directories a run writes rather than verifies.
const fn asking<'a>(asked: &'a Asked<'a>, excluded: &'a [&'a str]) -> Asking<'a, Watch<'a>> {
    Asking {
        root: asked.root,
        env: asked.env,
        excluded,
        watch: &asked.watch,
    }
}
