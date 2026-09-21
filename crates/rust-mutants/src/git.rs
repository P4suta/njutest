// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What git says about the tree, and what it is never allowed to say by silence.

use std::ffi::OsString;
use std::path::Path;

use crate::glob::{GlobError, Pattern};
use crate::runner::{Bound, Spec, Watch, run};

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
#[must_use]
pub fn changed<W: Watch>(asking: &Asking<'_, W>, base: &str) -> Option<Change> {
    let merge_base = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["merge-base", base, "HEAD"],
    );
    let against = match merge_base.as_deref() {
        Some(merge_base) => merge_base,
        None => base,
    };
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
    let uncommitted_paths: Option<Vec<String>> = uncommitted.lines().map(porcelain_path).collect();
    files.extend(uncommitted_paths?);
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
/// # Errors
/// Refuses a changed path which cannot be represented by the mutation glob
/// language. Silently omitting such a path would make a partial change set
/// indistinguishable from the complete one the caller asked for.
pub fn within(change: &Change, include: &[Pattern]) -> Result<Vec<Pattern>, GlobError> {
    let sources: Vec<&String> = change
        .files
        .iter()
        .filter(|path| Path::new(path).extension() == Some(std::ffi::OsStr::new("rs")))
        .filter(|path| include.is_empty() || include.iter().any(|pattern| pattern.matches(path)))
        .collect();
    if sources.is_empty() {
        return Pattern::compile(NOTHING_CHANGED).map(|pattern| vec![pattern]);
    }
    sources
        .into_iter()
        .map(|path| Pattern::compile(path))
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
    let path = rest.rsplit(" -> ").next()?;
    Some(path.trim_matches('"').to_owned())
}

/// The output of one git command, or nothing when it could not be run or did not succeed.
/// Every variable that tells git to answer about a repository other than the one it is standing in.
///
/// A closed set, because leaving one out is the defect rather than a smaller
/// version of it: each of these is enough on its own to make git answer about
/// somewhere else.
const REDIRECTING: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// `env` with the variables that would point git somewhere else taken out.
///
/// A run names the repository it verified. `GIT_DIR` in the environment it
/// happened to be started with — which is what a git hook sets, and what any
/// wrapper may — makes git answer about that one instead, and the report then
/// names another repository's commit as the thing it established something
/// about. That is a conclusion drawn from how the run was invoked rather than
/// from what it looked at, so the invocation is not allowed to reach the
/// question.
fn about_the_tree(env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    env.iter()
        .filter(|(name, _value)| {
            !REDIRECTING
                .iter()
                .any(|pointed| crate::vars::same_name(name, std::ffi::OsStr::new(pointed)))
        })
        .cloned()
        .collect()
}

fn ask<W: Watch>(
    asking: &Asking<'_, W>,
    empty: Empty,
    shape: Shape,
    arguments: &[&str],
) -> Option<String> {
    let mut argv: Vec<OsString> = vec![OsString::from("git")];
    argv.extend(arguments.iter().map(OsString::from));
    let mut spec = Spec::new(argv, Bound::After(crate::runner::PROBE));
    spec.dir = Some(asking.root.to_path_buf());
    spec.env = Some(about_the_tree(asking.env));
    spec.structured_stdout = Some(1 << 20);

    let asked = run(&spec, asking.watch.cancel());
    asking.watch.exec(&spec, &asked);
    match asked.termination {
        crate::runner::Termination::Exited(crate::runner::ProcessExit::Code(0)) => {}
        crate::runner::Termination::NotStarted { .. }
        | crate::runner::Termination::Exited(_)
        | crate::runner::Termination::TimedOut
        | crate::runner::Termination::StoppedByMonitor
        | crate::runner::Termination::MonitorFailed { .. }
        | crate::runner::Termination::Cancelled { .. }
        | crate::runner::Termination::WaitFailed { .. } => return None,
    }
    let raw = match std::str::from_utf8(&asked.stdout) {
        Ok(raw) => raw,
        Err(_non_utf8_git_protocol) => return None,
    };
    let answer = match shape {
        Shape::Trimmed => raw.trim().to_owned(),
        Shape::Verbatim => raw.trim_end_matches('\n').to_owned(),
    };
    if answer.is_empty() && empty == Empty::Refuse {
        return None;
    }
    Some(answer)
}
