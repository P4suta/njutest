// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What git says about the tree, and what it is never allowed to say by silence.
//!
//! Every answer here is optional, and nothing turns a question git could not be
//! asked into an empty answer: a run that could not see what changed must never
//! look like a run that saw nothing change.

use std::ffi::OsString;
use std::path::Path;

use crate::glob::Pattern;
use crate::runner::{Spec, Watch, run};

/// The revision a change set is computed against when the caller names none.
pub const DEFAULT_BASE: &str = "HEAD";

/// The pattern a run about an empty change set mutates within. No file is called this.
pub const NOTHING_CHANGED: &str = ".rust-mutants-nothing-changed";

/// Where to ask git, with what environment, and under whose watch.
#[derive(Debug)]
pub struct Asking<'a, W> {
    /// The directory the commands run in.
    pub root: &'a Path,
    /// The environment the commands run with.
    pub env: &'a [(OsString, OsString)],
    /// Directories whose contents are written by a run rather than verified by one, as workspace-relative prefixes.
    pub excluded: &'a [&'a str],
    /// What stops the commands, and who hears that they ran.
    pub watch: &'a W,
}

/// What the repository was when it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The commit `HEAD` resolves to.
    pub commit: String,
    /// The branch `HEAD` is on, or `HEAD` when it is detached.
    pub branch: String,
    /// Whether anything in the tree differs from that commit.
    pub dirty: bool,
}

/// What differs from a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The revision the change set was computed against.
    pub base: String,
    /// The commit both branches share, when git could name one.
    pub merge_base: Option<String>,
    /// Every file that differs, as workspace-relative paths, sorted. What is not committed counts too.
    pub files: Vec<String>,
}

/// What the repository at `root` was, or nothing when git could not be asked.
#[must_use]
pub fn facts<W: Watch>(asking: &Asking<'_, W>) -> Option<Facts> {
    let commit = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["rev-parse", "HEAD"],
    )?;
    let branch = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )?;
    let status = ask(
        asking,
        Empty::Accept,
        Shape::Verbatim,
        &["status", "--porcelain"],
    )?;
    Some(Facts {
        commit,
        branch,
        dirty: !status.trim().is_empty(),
    })
}

/// Every file that differs from `base`, committed and not, leaving out anything under an excluded directory.
///
/// Returns nothing when git could not be asked or does not know `base`.
#[must_use]
pub fn changed<W: Watch>(asking: &Asking<'_, W>, base: &str) -> Option<Change> {
    let merge_base = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["merge-base", base, "HEAD"],
    );
    let against = merge_base.as_deref().unwrap_or(base);
    let committed = ask(
        asking,
        Empty::Accept,
        Shape::Trimmed,
        &["diff", "--name-only", against],
    )?;
    let uncommitted = ask(
        asking,
        Empty::Accept,
        Shape::Verbatim,
        &["status", "--porcelain"],
    )?;
    let mut files: Vec<String> = committed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    files.extend(uncommitted.lines().filter_map(porcelain_path));
    files.retain(|path| !is_under(path, asking.excluded));
    files.sort();
    files.dedup();
    Some(Change {
        base: base.to_owned(),
        merge_base,
        files,
    })
}

/// The Rust files a change set names, as the patterns a run mutates within, keeping only what `include` already admits when it admits anything.
///
/// An empty list of patterns is every file, so a change set naming no Rust
/// file at all becomes the one pattern nothing matches: a run about nothing
/// changing must mutate nothing, not everything.
#[must_use]
pub fn within(change: &Change, include: &[Pattern]) -> Vec<Pattern> {
    let sources: Vec<&String> = change
        .files
        .iter()
        .filter(|path| Path::new(path).extension() == Some(std::ffi::OsStr::new("rs")))
        .filter(|path| include.is_empty() || include.iter().any(|pattern| pattern.matches(path)))
        .collect();
    if sources.is_empty() {
        return Pattern::compile(NOTHING_CHANGED)
            .map(|pattern| vec![pattern])
            .unwrap_or_default();
    }
    sources
        .into_iter()
        .filter_map(|path| Pattern::compile(path).ok())
        .collect()
}

/// Whether an empty answer is an answer. A `HEAD` that resolves to nothing is nothing; a diff that lists nothing is a diff that lists nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Empty {
    Refuse,
    Accept,
}

/// Whether the leading whitespace of an answer carries meaning. It does in `status --porcelain`, where the first two columns are the status and a space is one of the values they take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Trimmed,
    Verbatim,
}

/// Whether a path lies in one of the directories a run writes rather than one it is about.
fn is_under(path: &str, excluded: &[&str]) -> bool {
    excluded
        .iter()
        .any(|directory| path == *directory || path.starts_with(&format!("{directory}/")))
}

/// The path in one `git status --porcelain` line: two status characters, a space, then the path, with a rename written as `old -> new`.
fn porcelain_path(line: &str) -> Option<String> {
    let rest = line.get(3..)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    let path = rest.rsplit(" -> ").next().unwrap_or(rest);
    Some(path.trim_matches('"').to_owned())
}

/// The output of one git command, or nothing when it could not be run or did not succeed.
fn ask<W: Watch>(
    asking: &Asking<'_, W>,
    empty: Empty,
    shape: Shape,
    arguments: &[&str],
) -> Option<String> {
    let mut argv: Vec<OsString> = vec![OsString::from("git")];
    argv.extend(arguments.iter().map(OsString::from));
    let mut spec = Spec::new(argv);
    spec.dir = Some(asking.root.to_path_buf());
    spec.env = Some(asking.env.to_vec());
    spec.structured_stdout = Some(1 << 20);

    let asked = run(&spec, asking.watch.cancel());
    asking.watch.exec(&spec, &asked);
    if asked.error.is_some() || asked.exit_code != 0 {
        return None;
    }
    let raw = String::from_utf8_lossy(&asked.stdout);
    let answer = match shape {
        Shape::Trimmed => raw.trim().to_owned(),
        Shape::Verbatim => raw.trim_end_matches('\n').to_owned(),
    };
    if answer.is_empty() && empty == Empty::Refuse {
        return None;
    }
    Some(answer)
}
